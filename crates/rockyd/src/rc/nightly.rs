//! 야간 재시작 — 새벽에 `claude update` 뒤 구버전이면서 쉬는 서버만 다시 띄운다. 판정은 `rocky_core::rc`
//! (`decide_nightly` …), 여기는 사실을 재고(설치 버전 · 기동 버전 기록 · 대화 기록) 순서를 짠다. 옛 CLI 의 야간 모드를
//! 옮겼다.
//!
//! 리허설(`nightly_preview`)은 update · 내리기 · 띄우기 없이 지금 설치 버전으로 판정만 돌려준다 — 기록도 남기지 않는다.

use std::path::Path;

use rocky_core::config::NightlyConfig;
use rocky_core::rc::{
    self, NightlyDecision, NightlyItem, NightlyOutcome, NightlyReason, NightlyReport, NightlyState,
    ServerRow,
};

use super::{probe, RcController};

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
                    NightlyReason::Current => (
                        NightlyOutcome::Current,
                        or_unknown(d.from.as_deref()).into(),
                    ),
                    reason => (NightlyOutcome::Skipped, skip_note(reason).into()),
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

fn skip_note(reason: NightlyReason) -> &'static str {
    match reason {
        NightlyReason::NoRecord => "기동 버전 기록 없음 — 구버전인지 모른다",
        NightlyReason::VersionUnknown => "설치 버전을 못 쟀다",
        NightlyReason::NoNetwork => "네트워크 없음",
        _ => "",
    }
}
