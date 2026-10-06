//! 야간 재시작 — 매일 `rc.nightly.at` 에 `claude update` 뒤 구버전이면서 쉬는 서버만 다시 띄운다. 판정은
//! `rocky_core::rc`(`decide_nightly` …), 여기는 사실을 재고(설치 버전 · 기동 버전 기록 · 대화 기록) 순서를 짠다. 옛 CLI 의
//! 야간 모드를 옮겼다.
//!
//! 순서: update → 판정 → canary(고정 하나) → 나머지 → 못 뜬 것 회복(마감까지) → 바쁜 것 대기(마감까지, 풀리면 같은
//! 순서로). 내리기 직전마다 네트워크를 보고, 내리기 전에 되살림 표식을 찍는다 — 도중에 데몬이 죽어도 감시가 살린다.
//! 다시 띄울 대상은 처음에 한꺼번에 잠그고(`begin`), 못 뜬 것은 회복이 끝날 때까지 쥔다 — 그동안 감시가 같은 대상을
//! 띄우면 회복이 그 서버와 겹친다(옛 CLI 는 야간이 잠금을 쥔 동안 주기 실행이 돌지 않았다). 마감까지 못 띄운 것은
//! 표식을 남긴 채 놓아 감시에 맡기고, 마감까지 쉬지 않은 것은 다음 날로 넘긴다.
//!
//! 리허설(`nightly_preview`)은 update · 내리기 · 띄우기 없이 지금 설치 버전으로 판정만 돌려준다 — 기록도 남기지 않는다.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use chrono::NaiveDateTime;
use rocky_core::config::NightlyConfig;
use rocky_core::rc::{
    self, NightlyDecision, NightlyInfo, NightlyItem, NightlyOutcome, NightlyReason, NightlyReport,
    NightlyState, RcAction, ServerRow, Target,
};
use serde::{Deserialize, Serialize};

use super::{probe, RcCommand, RcController, RcNotifier, RcRefusal};

/// 일정 확인 간격 — 1분마다 "오늘 그 시각이 지났고 아직 안 돌았나" 를 본다.
pub const NIGHTLY_CHECK: Duration = Duration::from_secs(60);
/// `claude update` 한도.
const UPDATE_TIMEOUT: Duration = Duration::from_secs(5 * 60);
/// 업데이트 뒤 버전 읽기 — 새 바이너리의 첫 실행이 Gatekeeper 검사로 멎으면 이 간격으로 이만큼까지 다시 잰다.
const VERSION_RETRY_EVERY: Duration = Duration::from_secs(5);
const VERSION_RETRY_FOR: Duration = Duration::from_secs(90);
/// 내리기 직전 네트워크 확인 — 이 간격으로 이만큼까지. 새벽에 막 깬 맥은 네트워크가 늦게 붙는다.
const ONLINE_RETRY_EVERY: Duration = Duration::from_secs(10);
const ONLINE_RETRY_FOR: Duration = Duration::from_secs(3 * 60);
const ONLINE_TIMEOUT: Duration = Duration::from_secs(10);

/// 야간의 메모리 상태 — 일정이 켜졌나, 지금 도나, 마지막 결과.
#[derive(Default)]
pub(super) struct NightlyRun {
    scheduled: bool,
    running: bool,
    last: Option<NightlyReport>,
}

/// `rc/nightly.json` — 마지막으로 돈 날(맥이 자고 넘긴 날을 깬 뒤 따라잡는 근거)과 마지막 결과.
#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NightlyFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_run: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last: Option<NightlyReport>,
}

/// 다시 띄울 하나 — 잠근 대상과 판정.
struct Req {
    target: Target,
    decision: NightlyDecision,
}

/// 한 대상을 다시 띄운 결과.
enum Attempted {
    Up,
    /// 내렸는데 못 띄웠다 — 잠근 채 돌려받는다.
    Down(Req),
    /// 내리기 전에 멈췄다(현황을 못 읽음 · 로그아웃 · 정지 실패) — 옛 서버가 그대로 떠 있다. 이미 놓았다.
    Untouched,
}

impl RcController {
    fn nightly_config(&self) -> NightlyConfig {
        self.config
            .as_ref()
            .and_then(|c| c.nightly.clone())
            .unwrap_or_default()
    }

    /// 설치 버전 — `claude --version`, 못 재면 설치 경로.
    async fn installed_version(&self) -> Option<String> {
        match self.claude_version().await {
            Some(v) => Some(v),
            None => installed_version_fs(&self.home),
        }
    }

    /// 서버 하나의 사실을 재고 판정한다. 기록 · 대화 기록은 파일이고, 쉬는지는 실제 시각(파일 mtime 과 같은 시계)으로 잰다.
    fn judge_nightly(
        &self,
        row: &ServerRow,
        current: Option<&str>,
        cfg: &NightlyConfig,
    ) -> NightlyDecision {
        let state = NightlyState {
            current: current.map(str::to_string),
            recorded: std::fs::read_to_string(self.log_path(&row.label, "version")).ok(),
            live_session: row.sessions > 0,
            last_write: last_write(&self.home, &row.dir),
            pinned: row.pinned,
            offline: false,
        };
        rc::decide_nightly(&state, chrono::Utc::now().timestamp(), cfg.quiet)
    }

    fn nightly_path(&self) -> std::path::PathBuf {
        self.log_dir.join("nightly.json")
    }

    fn read_nightly_file(&self) -> NightlyFile {
        std::fs::read_to_string(self.nightly_path())
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default()
    }

    fn write_nightly_file(&self, file: &NightlyFile) {
        let _ = std::fs::create_dir_all(&self.log_dir);
        if let Ok(raw) = serde_json::to_string(file) {
            let _ = std::fs::write(self.nightly_path(), raw);
        }
    }

    /// 일정을 켠다 — 현황에 시각이 실리고, 마지막 결과를 파일에서 읽어 둔다.
    pub fn enable_nightly(&self) {
        let last = self.read_nightly_file().last;
        let mut run = self.nightly.lock().unwrap_or_else(|e| e.into_inner());
        run.scheduled = true;
        if run.last.is_none() {
            run.last = last;
        }
    }

    pub(super) fn nightly_info(&self) -> Option<NightlyInfo> {
        let run = self.nightly.lock().unwrap_or_else(|e| e.into_inner());
        let at = run
            .scheduled
            .then(|| self.nightly_config().at.format("%H:%M").to_string());
        (at.is_some() || run.running || run.last.is_some()).then(|| NightlyInfo {
            at,
            running: run.running,
            last: run.last.clone(),
        })
    }

    /// 야간을 시작한다(일정이든 손이든) — 이미 돌고 있으면 Busy, rc 가 꺼진 기기면 NotFound. 리허설은 받지 않아도 된다.
    pub fn begin_nightly(&self) -> Result<(), RcRefusal> {
        if self.config.is_none() {
            return Err(RcRefusal::NotFound("이 기기에서는 rc 가 꺼져 있다".into()));
        }
        let mut run = self.nightly.lock().unwrap_or_else(|e| e.into_inner());
        if run.running {
            return Err(RcRefusal::Busy("야간 재시작이 이미 돌고 있다".into()));
        }
        run.running = true;
        Ok(())
    }

    /// 오늘 돌 차례면 그 날을 먼저 남기고 true — 도중에 데몬이 다시 떠도 같은 날 두 번 돌지 않는다. 기록이 없으면(처음 켠
    /// 날) 이미 돈 것으로 칠 날을 남긴다(켜자마자 낮에 서버를 내리지 않게).
    pub fn claim_nightly(&self, now: NaiveDateTime) -> bool {
        let at = self.nightly_config().at;
        let mut file = self.read_nightly_file();
        let last = match file
            .last_run
            .as_deref()
            .and_then(|d| d.parse::<chrono::NaiveDate>().ok())
        {
            Some(d) => d,
            None => {
                let d = rc::nightly_first_mark(now, at);
                file.last_run = Some(d.to_string());
                self.write_nightly_file(&file);
                d
            }
        };
        if !rc::nightly_due(now, at, last) || self.begin_nightly().is_err() {
            return false;
        }
        file.last_run = Some(now.date().to_string());
        self.write_nightly_file(&file);
        true
    }

    /// 야간 한 번 — `begin_nightly` 를 먼저 받아야 하고, 끝나면 그 표시를 푼다.
    pub async fn run_nightly(self: &Arc<Self>, notify: &RcNotifier) -> NightlyReport {
        let cfg = self.nightly_config();
        let mut report = NightlyReport {
            started_at: chrono::Utc::now().to_rfc3339(),
            ..Default::default()
        };
        let current = self.nightly_update(&mut report).await;
        report.version = current.clone();
        let status = probe(&self.runner, self.config.as_ref(), &self.home).await;
        if let Some(reason) = rc::nightly_blocked(&status) {
            report.blocked = Some(reason.into());
            self.event("nightly-skip", "", serde_json::json!({ "reason": reason }));
        } else {
            self.nightly_pass(&status, current.as_deref(), &cfg, &mut report)
                .await;
        }
        report.finished_at = Some(chrono::Utc::now().to_rfc3339());
        self.finish_nightly(&report, notify);
        report
    }

    /// `claude update` 뒤 판정에 쓸 설치 버전. 실패해도 그때 설치된 버전으로 판정한다 — 성공 여부는 문구가 아니라 종료
    /// 코드와 앞뒤 버전으로 가른다(문구는 버전마다 바뀐다).
    async fn nightly_update(&self, report: &mut NightlyReport) -> Option<String> {
        let before = self.claude_version().await;
        let out = (self.runner)(
            vec!["claude".into(), "update".into()],
            String::new(),
            UPDATE_TIMEOUT,
        )
        .await;
        let after = self.installed_after_update().await;
        let (result, summary) = if !out.ok() {
            (
                "failed",
                format!(
                    "실패(종료 코드 {}) · 설치 {} 기준으로 판정",
                    out.code,
                    or_unknown(after.as_deref())
                ),
            )
        } else if before != after {
            (
                "updated",
                format!(
                    "{} → {}",
                    or_unknown(before.as_deref()),
                    or_unknown(after.as_deref())
                ),
            )
        } else {
            (
                "unchanged",
                format!("변화 없음 · {}", or_unknown(after.as_deref())),
            )
        };
        self.event(
            "nightly-update",
            "",
            serde_json::json!({ "before": before, "after": after, "code": out.code, "result": result }),
        );
        report.update = summary;
        after
    }

    /// 업데이트 뒤 설치 버전 — `claude --version` 을 상한까지 다시 재고, 끝내 모르면 설치 경로로 읽는다.
    async fn installed_after_update(&self) -> Option<String> {
        let deadline = (self.ops.now)() + VERSION_RETRY_FOR;
        loop {
            if let Some(v) = self.claude_version().await {
                return Some(v);
            }
            if (self.ops.now)() >= deadline {
                break;
            }
            (self.ops.sleep)(VERSION_RETRY_EVERY).await;
        }
        let fallback = installed_version_fs(&self.home);
        self.event(
            "nightly-version",
            "",
            serde_json::json!({ "reason": "version-timeout", "fallback": fallback }),
        );
        fallback
    }

    /// 판정 → 다시 띄울 것을 잠가 canary 부터 → 바쁜 것은 마감까지 기다린다.
    async fn nightly_pass(
        self: &Arc<Self>,
        status: &rc::RcStatus,
        current: Option<&str>,
        cfg: &NightlyConfig,
        report: &mut NightlyReport,
    ) {
        let deadline = rc::busy_deadline((self.ops.now)(), cfg.busy_until);
        let until = cfg.busy_until.format("%H:%M").to_string();
        let mut reqs = Vec::new();
        let mut busy = Vec::new();
        for row in status.servers.iter().filter(|s| s.running) {
            let d = self.judge_nightly(row, current, cfg);
            self.event(
                "nightly-decide",
                &row.label,
                serde_json::json!({
                    "reason": d.reason, "from": d.from, "to": current, "mode": d.mode,
                    "idleSecs": d.idle_secs, "liveSession": row.sessions > 0, "pinned": row.pinned,
                }),
            );
            let (outcome, note) = match d.reason {
                NightlyReason::Restart => {
                    reqs.push((row.label.clone(), d));
                    continue;
                }
                NightlyReason::Busy => {
                    busy.push(row.label.clone());
                    continue;
                }
                _ => settled(&d),
            };
            report.items.push(NightlyItem {
                label: row.label.clone(),
                outcome,
                note,
            });
        }
        let reqs = self.lock_reqs(reqs, report);
        self.restart_round(reqs, current, deadline, &until, report)
            .await;
        self.wait_busy(busy, current, cfg, deadline, &until, report)
            .await;
    }

    /// 다시 띄울 대상을 한꺼번에 잠근다 — 사람이 그 대상에 무엇을 하고 있으면 건너뛴다.
    fn lock_reqs(
        &self,
        decided: Vec<(String, NightlyDecision)>,
        report: &mut NightlyReport,
    ) -> Vec<Req> {
        decided
            .into_iter()
            .filter_map(|(label, decision)| {
                match self.begin(&label, RcCommand::Restart { fresh: false }) {
                    Ok(target) => Some(Req { target, decision }),
                    Err(_) => {
                        report.items.push(NightlyItem {
                            label,
                            outcome: NightlyOutcome::Skipped,
                            note: "다른 일이 진행 중".into(),
                        });
                        None
                    }
                }
            })
            .collect()
    }

    /// 한 바퀴 — canary 를 먼저 내려 띄워 보고, 뜬 뒤에야 나머지를 내린다. 내리기 직전마다 네트워크를 본다. 못 뜬 것은
    /// 마감까지 회복한다.
    async fn restart_round(
        self: &Arc<Self>,
        reqs: Vec<Req>,
        current: Option<&str>,
        deadline: NaiveDateTime,
        until: &str,
        report: &mut NightlyReport,
    ) {
        if reqs.is_empty() {
            return;
        }
        let mut reqs = rc::canary_first(reqs, |r| r.target.pinned).into_iter();
        let canary = reqs.next().expect("비어 있지 않다");
        let rest: Vec<Req> = reqs.collect();
        if !self.wait_online("canary").await {
            for r in std::iter::once(canary).chain(rest) {
                self.skip_req(r, "네트워크 없음 — 내리면 다시 등록하지 못한다", report);
            }
            return;
        }
        let canary_label = canary.target.label.clone();
        let mut down = Vec::new();
        let canary_up = match self.restart_req(canary, current, report).await {
            Attempted::Up => true,
            Attempted::Down(r) => {
                down.push(r);
                false
            }
            Attempted::Untouched => false,
        };
        if !canary_up {
            report.canary_failed = true;
            for r in rest {
                let note = format!("먼저 시도한 {canary_label} 가 안 돼서 건드리지 않음");
                self.skip_req(r, &note, report);
            }
        } else if !rest.is_empty() {
            if self.wait_online("rest").await {
                let results = futures_join(rest.into_iter().map(|r| {
                    let this = self.clone();
                    let current = current.map(str::to_string);
                    async move {
                        let mut items = NightlyReport::default();
                        let res = this.restart_req(r, current.as_deref(), &mut items).await;
                        (res, items.items)
                    }
                }))
                .await;
                for (res, items) in results {
                    report.items.extend(items);
                    if let Attempted::Down(r) = res {
                        down.push(r);
                    }
                }
            } else {
                for r in rest {
                    self.skip_req(r, "네트워크 없음 — 내리면 다시 등록하지 못한다", report);
                }
            }
        }
        self.recover(down, current, deadline, until, report).await;
    }

    /// 잠근 대상을 손대지 않고 놓는다.
    fn skip_req(&self, r: Req, note: &str, report: &mut NightlyReport) {
        self.release(&r.target.label);
        report.items.push(NightlyItem {
            label: r.target.label,
            outcome: NightlyOutcome::Skipped,
            note: note.into(),
        });
    }

    /// 표식을 찍고 내려 띄운다. 뜨면 표식을 지우고 놓는다. 못 뜨면 잠근 채 돌려준다. 내리기도 전에 멈췄으면(옛 서버가 그대로
    /// 떠 있다) 놓고 건너뛴다 — 넘기면 다음 시도가 "이미 떠 있다" 를 재시작으로 센다.
    async fn restart_req(
        &self,
        r: Req,
        current: Option<&str>,
        report: &mut NightlyReport,
    ) -> Attempted {
        self.mark_revive(&r.target);
        let result = self
            .attempt(
                &r.target,
                RcCommand::Restart { fresh: false },
                &rc::NIGHTLY_REGISTRATION_BACKOFF,
                current,
            )
            .await;
        let label = r.target.label.clone();
        if result.ok {
            self.release(&label);
            self.clear_revive(&label, "started");
            report.items.push(NightlyItem {
                label,
                outcome: NightlyOutcome::Restarted,
                note: restart_note(&r.decision, current),
            });
            return Attempted::Up;
        }
        // 잠근 동안 띄울 수 있는 것은 이 기동기뿐이고, 등록에 실패한 서버는 기동기가 이미 내렸다 — 지금 떠 있으면 옛 서버다.
        let status = probe(&self.runner, self.config.as_ref(), &self.home).await;
        let still_up = status.probe_error.is_none()
            && status.servers.iter().any(|s| s.label == label && s.running);
        if still_up {
            self.release(&label);
            self.clear_revive(&label, "untouched");
            report.items.push(NightlyItem {
                label,
                outcome: NightlyOutcome::Skipped,
                note: format!("손대지 못했다 — {}", result.message),
            });
            return Attempted::Untouched;
        }
        self.set_action(&label, RcAction::Retrying);
        Attempted::Down(r)
    }

    /// 못 띄운 서버를 누가 살리나 — 감시가 꺼져 있으면 표식만 남는다.
    fn down_hint(&self) -> &'static str {
        if self.config.as_ref().is_some_and(|c| c.supervise) {
            "감시가 되살린다"
        } else {
            "감시가 꺼져 있다 — rocky rc start 로 띄운다"
        }
    }

    /// 내리고 못 띄운 것을 마감까지 1 · 2 · 4 · 8 · 16분 간격으로 다시 띄운다 — 이어받기는 이미 실패했으니 새로, `already
    /// served` 를 만나도 그 자리에서 기다리지 않는다(이 간격이 곧 재시도다 — 기다리면 마감을 넘긴다). 그새 떠 있으면 다시
    /// 띄우지 않는다. 마감까지 안 뜬 것은 표식을 남긴 채 놓는다.
    async fn recover(
        &self,
        mut down: Vec<Req>,
        current: Option<&str>,
        deadline: NaiveDateTime,
        until: &str,
        report: &mut NightlyReport,
    ) {
        let mut attempt = 1;
        while !down.is_empty() {
            let now = (self.ops.now)();
            if now >= deadline {
                break;
            }
            let left = (deadline - now).to_std().unwrap_or(Duration::ZERO);
            (self.ops.sleep)(rc::recovery_wait(attempt).min(left)).await;
            attempt += 1;
            // 마지막 대기가 마감에 닿았으면(네트워크 확인도 3분까지 걸린다) 마감 뒤에 띄우지 않는다.
            if !self.wait_online("recover").await || (self.ops.now)() >= deadline {
                continue;
            }
            let status = probe(&self.runner, self.config.as_ref(), &self.home).await;
            let mut still = Vec::new();
            for r in down {
                let label = r.target.label.clone();
                let up = status.probe_error.is_none()
                    && status.servers.iter().any(|s| s.label == label && s.running);
                if up {
                    // 잠근 동안 rocky 밖에서 누가 띄웠다 — 어느 버전인지 모르니 재시작으로 세지 않는다.
                    self.release(&label);
                    self.clear_revive(&label, "already-running");
                    report.items.push(NightlyItem {
                        label,
                        outcome: NightlyOutcome::Skipped,
                        note: "그새 떠 있다 — 다시 띄우지 않았다".into(),
                    });
                    continue;
                }
                let mode = rc::retry_mode(r.target.pinned);
                let result = self
                    .attempt(&r.target, RcCommand::Revive(mode), &[], current)
                    .await;
                if result.ok {
                    self.release(&label);
                    self.clear_revive(&label, "started");
                    report.items.push(NightlyItem {
                        label,
                        outcome: NightlyOutcome::Restarted,
                        note: format!(
                            "{} ({}번째 재시도)",
                            restart_note(&r.decision, current),
                            attempt - 1
                        ),
                    });
                } else {
                    still.push(r);
                }
            }
            down = still;
        }
        for r in down {
            self.release(&r.target.label);
            self.event(
                "nightly-down",
                &r.target.label,
                serde_json::json!({ "until": until }),
            );
            report.items.push(NightlyItem {
                label: r.target.label,
                outcome: NightlyOutcome::Down,
                note: format!("내렸지만 {until} 까지 못 띄움 — {}", self.down_hint()),
            });
        }
    }

    /// 바쁜 서버 — 쉬게 될 때까지 5분마다 다시 보며 마감까지 기다린다. 풀린 것은 같은 순서(canary · 회복)로. 회복이 마감을
    /// 다 쓰면 다시 보지 못하고 넘긴다(옛 CLI 와 같은 순서).
    async fn wait_busy(
        self: &Arc<Self>,
        mut busy: Vec<String>,
        current: Option<&str>,
        cfg: &NightlyConfig,
        deadline: NaiveDateTime,
        until: &str,
        report: &mut NightlyReport,
    ) {
        let mut looked = false;
        while !busy.is_empty() {
            let now = (self.ops.now)();
            if now >= deadline {
                break;
            }
            let left = (deadline - now).to_std().unwrap_or(Duration::ZERO);
            (self.ops.sleep)(rc::NIGHTLY_BUSY_POLL.min(left)).await;
            // 마지막 대기가 마감에 닿았으면 마감 뒤에 재시작하지 않는다 — 다음 날로.
            if (self.ops.now)() >= deadline {
                break;
            }
            let status = probe(&self.runner, self.config.as_ref(), &self.home).await;
            if rc::nightly_blocked(&status).is_some() {
                continue;
            }
            looked = true;
            let mut free = Vec::new();
            let mut still = Vec::new();
            for label in busy {
                let Some(row) = status
                    .servers
                    .iter()
                    .find(|s| s.label == label && s.running)
                else {
                    report.items.push(NightlyItem {
                        label,
                        outcome: NightlyOutcome::Skipped,
                        note: "기다리는 사이 내려갔다".into(),
                    });
                    continue;
                };
                let d = self.judge_nightly(row, current, cfg);
                match d.reason {
                    NightlyReason::Restart => free.push((label, d)),
                    NightlyReason::Busy => still.push(label),
                    _ => {
                        let (outcome, note) = settled(&d);
                        report.items.push(NightlyItem {
                            label,
                            outcome,
                            note,
                        });
                    }
                }
            }
            let reqs = self.lock_reqs(free, report);
            self.restart_round(reqs, current, deadline, until, report)
                .await;
            busy = still;
        }
        let note = if looked {
            format!("작업 중 — {until} 까지 안 쉬어 다음 날로")
        } else {
            format!("작업 중 — {until} 까지 다시 볼 틈이 없어 다음 날로")
        };
        for label in busy {
            self.event(
                "nightly-skip",
                &label,
                serde_json::json!({ "reason": "busy-timeout", "until": until }),
            );
            report.items.push(NightlyItem {
                label,
                outcome: NightlyOutcome::Skipped,
                note: note.clone(),
            });
        }
    }

    /// 네트워크가 닿을 때까지 상한만큼 기다린다 — 서버를 내리기 직전에 부른다. `curl -4` 가 어떤 HTTP 응답이든 받으면
    /// 닿은 것이다(IPv6 만 붙은 순간을 닿았다고 보지 않게). 결과를 늘 남긴다.
    async fn wait_online(&self, why: &str) -> bool {
        let deadline = (self.ops.now)() + ONLINE_RETRY_FOR;
        let mut tries = 0;
        loop {
            tries += 1;
            let out = (self.runner)(
                [
                    "curl",
                    "-4",
                    "-sS",
                    "-o",
                    "/dev/null",
                    "--max-time",
                    "10",
                    "https://api.anthropic.com",
                ]
                .iter()
                .map(|s| s.to_string())
                .collect(),
                String::new(),
                ONLINE_TIMEOUT,
            )
            .await;
            let ok = out.ok();
            if ok || (self.ops.now)() >= deadline {
                self.event(
                    "wait-online",
                    "",
                    serde_json::json!({ "for": why, "ok": ok, "tries": tries }),
                );
                return ok;
            }
            (self.ops.sleep)(ONLINE_RETRY_EVERY).await;
        }
    }

    /// 결과를 남기고 표시를 푼다. 못 띄운 서버가 있거나 canary 가 실패했을 때만 배너 — 나머지는 현황과 기록으로 본다.
    fn finish_nightly(&self, report: &NightlyReport, notify: &RcNotifier) {
        let mut file = self.read_nightly_file();
        file.last = Some(report.clone());
        self.write_nightly_file(&file);
        self.event(
            "nightly-summary",
            "",
            serde_json::json!({
                "update": report.update, "version": report.version, "blocked": report.blocked,
                "restarted": report.count(NightlyOutcome::Restarted),
                "skipped": report.count(NightlyOutcome::Skipped),
                "down": report.count(NightlyOutcome::Down),
            }),
        );
        {
            let mut run = self.nightly.lock().unwrap_or_else(|e| e.into_inner());
            run.running = false;
            run.last = Some(report.clone());
        }
        let down = report.count(NightlyOutcome::Down);
        if down > 0 || report.canary_failed {
            notify(
                "rocky · rc 야간".into(),
                format!(
                    "재시작 {} · 못 띄움 {down} — 못 띄운 서버는 {}. rocky rc 로 본다",
                    report.count(NightlyOutcome::Restarted),
                    self.down_hint()
                ),
            );
        }
    }

    /// 리허설 — 지금 떠 있는 대상마다 야간이 무엇을 할지. 손대지 않고 기록도 남기지 않는다(옛 CLI `--nightly -n` 과
    /// 맞대 보는 자리). rc 가 꺼진 기기면 `blocked` 에 사유가 실린다.
    pub async fn nightly_preview(&self) -> NightlyReport {
        let cfg = self.nightly_config();
        // 둘은 서로 기다릴 이유가 없다 — 새 바이너리의 첫 실행이 멎어도 응답이 그만큼만 늦다.
        let (current, status) = tokio::join!(
            self.installed_version(),
            probe(&self.runner, self.config.as_ref(), &self.home)
        );
        let mut report = NightlyReport {
            started_at: chrono::Utc::now().to_rfc3339(),
            dry_run: true,
            update: format!("건너뜀(리허설) · 설치 {}", or_unknown(current.as_deref())),
            version: current.clone(),
            ..Default::default()
        };
        if let Some(reason) = rc::nightly_blocked(&status) {
            report.blocked = Some(reason.into());
        } else {
            let deadline = rc::busy_deadline((self.ops.now)(), cfg.busy_until);
            let until = cfg.busy_until.format("%H:%M");
            for row in status.servers.iter().filter(|s| s.running) {
                let d = self.judge_nightly(row, current.as_deref(), &cfg);
                let (outcome, note) = match d.reason {
                    NightlyReason::Restart => (
                        NightlyOutcome::WouldRestart,
                        restart_note(&d, current.as_deref()),
                    ),
                    NightlyReason::Busy if deadline > (self.ops.now)() => (
                        NightlyOutcome::WouldWait,
                        format!("작업 중 — {until} 까지 5분마다 다시 본다"),
                    ),
                    NightlyReason::Busy => (
                        NightlyOutcome::Skipped,
                        format!("작업 중 — {until} 이 지나 다음 날로"),
                    ),
                    _ => settled(&d),
                };
                report.items.push(NightlyItem {
                    label: row.label.clone(),
                    outcome,
                    note,
                });
            }
        }
        report.finished_at = Some(chrono::Utc::now().to_rfc3339());
        report
    }
}

/// 여러 미래를 같이 돌려 순서대로 결과를 모은다 — 대상마다 내리고 띄우는 데 1분이 넘을 수 있다.
async fn futures_join<F, T>(futures: impl IntoIterator<Item = F>) -> Vec<T>
where
    F: std::future::Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    let handles: Vec<_> = futures.into_iter().map(tokio::spawn).collect();
    let mut out = Vec::with_capacity(handles.len());
    for h in handles {
        if let Ok(v) = h.await {
            out.push(v);
        }
    }
    out
}

/// 그 폴더 대화 기록(`~/.claude/projects/<이름>/*.jsonl`) 중 가장 최근 mtime(unix 초). 없으면 None.
fn last_write(home: &str, dir: &str) -> Option<i64> {
    let projects = Path::new(home)
        .join(".claude/projects")
        .join(rc::project_dir_name(dir));
    std::fs::read_dir(projects)
        .ok()?
        .filter_map(|e| {
            let e = e.ok()?;
            if e.path().extension()? != "jsonl" {
                return None;
            }
            let modified = e.metadata().ok()?.modified().ok()?;
            Some(
                modified
                    .duration_since(std::time::UNIX_EPOCH)
                    .ok()?
                    .as_secs() as i64,
            )
        })
        .max()
}

/// 설치 경로로 읽은 버전 — `~/.local/bin/claude` 링크의 대상, 아니면 `versions/` 중 가장 높은 것. 새 바이너리의 첫
/// 실행이 Gatekeeper 검사로 멎어 `--version` 을 못 잴 때 쓴다.
fn installed_version_fs(home: &str) -> Option<String> {
    let home = Path::new(home);
    if let Ok(target) = std::fs::read_link(home.join(".local/bin/claude")) {
        if let Some(v) = rc::version_from_path(&target.to_string_lossy()) {
            return Some(v);
        }
    }
    let names: Vec<String> = std::fs::read_dir(home.join(".local/share/claude/versions"))
        .ok()?
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .collect();
    rc::newest_version(names.iter().map(String::as_str))
}

fn or_unknown(v: Option<&str>) -> &str {
    v.unwrap_or("?")
}

fn restart_note(d: &NightlyDecision, current: Option<&str>) -> String {
    let mode = d.mode.map(|m| rc::mode_note(m, false)).unwrap_or("");
    format!(
        "{} → {} · {mode}",
        or_unknown(d.from.as_deref()),
        or_unknown(current)
    )
}

/// 손댈 일이 없는 판정의 결과 — 최신이거나 건너뛴 사유.
fn settled(d: &NightlyDecision) -> (NightlyOutcome, String) {
    let skip = |note: &str| (NightlyOutcome::Skipped, note.to_string());
    match d.reason {
        NightlyReason::Current => (
            NightlyOutcome::Current,
            or_unknown(d.from.as_deref()).into(),
        ),
        NightlyReason::NoRecord => skip("기동 버전 기록 없음 — 구버전인지 모른다"),
        NightlyReason::VersionUnknown => skip("설치 버전을 못 쟀다"),
        NightlyReason::NoNetwork => skip("네트워크 없음"),
        NightlyReason::Restart | NightlyReason::Busy => skip(""),
    }
}

/// 야간 일정 — 1분마다 차례인지 보고, 차례면 그 날을 남긴 뒤 한 번 돈다(사람이 손으로 돌리는 중이면 다음 확인으로).
pub fn spawn_rc_nightly(control: Arc<RcController>, notify: RcNotifier) {
    control.enable_nightly();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(NIGHTLY_CHECK).await;
            if control.claim_nightly((control.ops.now)()) {
                control.run_nightly(&notify).await;
            }
        }
    });
}
