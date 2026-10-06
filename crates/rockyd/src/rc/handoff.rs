//! 핸드오프 서버 — 보드의 "새 세션 띄우기"(spawn)가 rc 가 켜진 기기에서 타는 길. 할 일의 워크트리를 만들고(없으면) 그
//! 안에서 단일 세션 rc 서버를 띄운 뒤, 서버가 만든 세션이 받은편지함을 등록하길 기다린다. 그 세션에 핸드오프를 넣고 깨우는
//! 일은 라우트가 지금의 핸드오프와 같은 길로 한다. `claude --bg` 세션은 로그인 세션 밖이라 ssh · 자격이 끊긴다.
//!
//! 이 서버는 대상(`rc.targets`)이 아니라 감시 · 야간이 건드리지 않는다(현황에는 대상 밖 서버로 보인다). 내리는 것은 사람이다 —
//! 세션이 끝났다고 알리면 사람이 닫는다. 판정은 `rocky_core::rc`(`handoff_server_argv` · `handoff_session` · `worktree_base`).

use std::path::Path;
use std::time::Duration;

use std::path::PathBuf;

use rocky_core::peer_inbox::InboxRegistration;
use rocky_core::rc::{self, AuthState, HandoffServerRecord, RcStatus, Registration};

use super::{argv, probe, RcController, RcRefusal};

/// git 명령 한도 — `worktree add` 는 큰 레포에서 수십 초. fetch 는 짧게(실패해도 받아 둔 origin 기준으로 딴다).
const GIT_TIMEOUT: Duration = Duration::from_secs(60);
const FETCH_TIMEOUT: Duration = Duration::from_secs(10);
/// 서버가 뜬 뒤 세션이 받은편지함을 등록하기까지(실측 6~7초) 기다리는 한도와 간격. `ps` 는 짧게 — 한도가 실제 시간이다.
pub const SESSION_WAIT: Duration = Duration::from_secs(60);
const SESSION_POLL: Duration = Duration::from_secs(1);
const PS_TIMEOUT: Duration = Duration::from_secs(3);

/// 세션 등록을 기다린 결과.
pub enum HandoffWait {
    Found(InboxRegistration),
    /// 한도까지 못 찾았다 — 서버는 떠 있다. `ps` 가 끝까지 실패했으면 그 사유.
    TimedOut {
        ps_error: Option<String>,
    },
    /// 기다리는 사이 서버가 내려갔다 — err 로그 끝줄.
    ServerGone(String),
}

impl RcController {
    /// 띄우기 전 점검 — 현황을 못 읽거나, 데몬 맥락의 claude 가 로그아웃이거나, 그 폴더에 rc 서버가 이미 있으면(대상이든
    /// 아니든 — 할 일당 서버 하나) 사유. 그때는 띄우지 않는다.
    pub async fn check_handoff_dir(&self, dir: &str) -> Result<(), String> {
        let status = probe(&self.runner, self.config.as_ref(), &self.home).await;
        if let Some(err) = status.probe_error {
            return Err(format!("rc 현황을 못 읽어 띄우지 않았다({dir}) — {err}"));
        }
        if status.auth == AuthState::Out {
            return Err(
                "데몬 맥락의 claude 가 로그아웃 상태다 — 띄워도 세션이 바로 내려간다. 로그인 뒤 다시"
                    .into(),
            );
        }
        let dir = dir.trim_end_matches('/');
        let running = status
            .servers
            .iter()
            .filter(|s| s.running && s.dir == dir)
            .find_map(|s| s.pid)
            .or_else(|| status.strays.iter().find(|s| s.dir == dir).map(|s| s.pid));
        match running {
            Some(pid) => Err(format!(
                "이 워크트리에 rc 서버가 이미 떠 있다(pid {pid}) — 그 세션이 아직 세션 목록에 없다. 폰 · 웹에서 열거나 rocky rc 로 본다: {dir}"
            )),
            None => Ok(()),
        }
    }

    /// 할 일의 워크트리 — 있으면 그 폴더가 정말 git 워크트리인지 보고 그대로(`Ok(None)`), 없으면 만든다(`Ok(Some(기준))`, 기준이
    /// 없으면 메인 체크아웃의 HEAD). 기준은 `origin/HEAD` → `origin/main` → `origin/master`. Claude Code `--worktree <이름>` 과
    /// 같은 자리 · 같은 브랜치 이름(`worktree-<이름>`)이라 예전 spawn 의 워크트리와 이어지고, 브랜치만 남아 있으면 그 브랜치를
    /// 다시 꺼낸다. 만들다 실패하면 반쯤 만든 워크트리를 걷는다.
    pub async fn ensure_worktree(
        &self,
        repo: &str,
        path: &str,
        name: &str,
        label: &str,
    ) -> Result<Option<Option<String>>, String> {
        let git = |dir: &str, args: &[&str], timeout| {
            let mut v = vec!["git", "-C", dir];
            v.extend_from_slice(args);
            (self.runner)(argv(&v), String::new(), timeout)
        };
        if Path::new(path).exists() {
            // 일반 폴더면 세션의 git 이 위의 메인 레포를 잡는다 — 사람의 체크아웃 위에서 커밋하게 된다.
            let top = git(path, &["rev-parse", "--show-toplevel"], GIT_TIMEOUT).await;
            let top_dir = top.stdout.trim();
            if !top.ok() || top_dir.trim_end_matches('/') != path.trim_end_matches('/') {
                return Err(format!(
                    "{path} 가 있는데 git 워크트리가 아니다(최상위: {}) — 그 폴더를 치우고(git worktree remove · prune) 다시",
                    if top_dir.is_empty() { "없음" } else { top_dir }
                ));
            }
            return Ok(None);
        }
        let origin = git(
            repo,
            &[
                "symbolic-ref",
                "--quiet",
                "--short",
                "refs/remotes/origin/HEAD",
            ],
            GIT_TIMEOUT,
        )
        .await;
        let mut base = origin
            .ok()
            .then(|| rc::worktree_base(&origin.stdout))
            .flatten();
        if base.is_none() {
            // `origin/HEAD` 는 clone 할 때만 생긴다 — 없으면 흔한 이름으로 짐작한다.
            for guess in ["origin/main", "origin/master"] {
                let r = format!("refs/remotes/{guess}");
                if git(repo, &["rev-parse", "--verify", "--quiet", &r], GIT_TIMEOUT)
                    .await
                    .ok()
                {
                    base = Some(guess.to_string());
                    break;
                }
            }
        }
        if let Some(branch) = base.as_deref().and_then(|b| b.strip_prefix("origin/")) {
            // 실패해도 간다 — 그때는 마지막으로 받아 둔 origin 기준이다.
            let _ = git(repo, &["fetch", "--quiet", "origin", branch], FETCH_TIMEOUT).await;
        }
        let branch = rc::worktree_branch(name);
        let head = format!("refs/heads/{branch}");
        let kept = git(
            repo,
            &["rev-parse", "--verify", "--quiet", &head],
            GIT_TIMEOUT,
        )
        .await
        .ok();
        let out = if kept {
            git(repo, &["worktree", "add", path, &branch], GIT_TIMEOUT).await
        } else {
            let mut args = vec!["worktree", "add", "-b", &branch, path];
            if let Some(base) = &base {
                args.push(base);
            }
            git(repo, &args, GIT_TIMEOUT).await
        };
        if !out.ok() {
            // 시간 초과로 끊겼으면 반쯤 체크아웃된 워크트리가 등록된 채 남는다 — 다음 시도가 그걸 쓰지 않게 걷는다. 이 경로는
            // 방금 이 요청이 만들기 시작한 것이다(있었으면 위에서 돌아갔다).
            let cleanup = if Path::new(path).exists() {
                let removed =
                    git(repo, &["worktree", "remove", "--force", path], GIT_TIMEOUT).await;
                if removed.ok() {
                    " — 반쯤 만든 워크트리는 걷었다".to_string()
                } else {
                    format!(
                        " — 반쯤 만든 워크트리를 못 걷었다({}): git worktree remove --force 로 치운다",
                        removed.stderr.trim()
                    )
                }
            } else {
                String::new()
            };
            return Err(format!(
                "워크트리를 못 만든다({path}, 브랜치 {branch}, 기준 {}): {}{cleanup}",
                base.as_deref().unwrap_or("HEAD"),
                out.stderr.trim()
            ));
        }
        self.event(
            "handoff-worktree",
            label,
            serde_json::json!({ "path": path, "branch": branch, "base": base, "kept": kept }),
        );
        Ok(Some(base))
    }

    /// 단일 세션 서버를 띄우고 등록(`· Connected ·`)까지 본다 — 서버 pid. `already served`(같은 폴더의 서버를 방금 내렸다)는
    /// 다시 해 보지 않는다 — 그 서버는 내리고 기다릴 시간을 알린다. 기동 로그는 `rc/<label>.out` · `.err`.
    pub async fn launch_handoff(
        &self,
        label: &str,
        dir: &str,
        name: &str,
        todo_ref: &str,
    ) -> Result<u32, String> {
        std::fs::create_dir_all(&self.log_dir)
            .map_err(|e| format!("로그 폴더를 못 만든다({}): {e}", self.log_dir.display()))?;
        let argv = rc::handoff_server_argv(name);
        let pid = (self.ops.spawn)(
            &argv,
            Path::new(dir),
            &self.log_path(label, "out"),
            &self.log_path(label, "err"),
        )
        .map_err(|e| format!("못 띄웠다({dir} 에서 {}): {e}", argv.join(" ")))?;
        self.event(
            "handoff-start",
            label,
            serde_json::json!({ "pid": pid, "dir": dir, "name": name }),
        );
        let up = |pid: u32| {
            // 현황이 대상 밖 서버에서 이 서버를 가려내고, 닫기가 이 pid 를 쓴다.
            self.write_handoff_record(&HandoffServerRecord {
                label: label.to_string(),
                name: name.to_string(),
                pid,
                dir: dir.to_string(),
                todo_ref: todo_ref.to_string(),
                started_at: chrono::Utc::now().to_rfc3339(),
            });
            Ok(pid)
        };
        match self.judge(label, pid).await {
            Registration::Connected => up(pid),
            // 등록 문구는 못 봤지만 떠 있다 — 세션 등록을 기다려 본다.
            Registration::Pending if (self.ops.signal)(pid, 0) => up(pid),
            Registration::Served => {
                let down = self.stop(label, pid).await;
                Err(format!(
                    "already served — 이 폴더({dir})의 서버 등록이 claude.ai 쪽에 남아 있다(서버를 내린 지 얼마 안 됐으면 3분쯤 뒤 다시). 새로 띄운 pid {pid} 는 {}",
                    if down { "내렸다" } else { "못 내렸다 — rocky rc 로 본다" }
                ))
            }
            Registration::Pending => Err(format!(
                "\"{name}\"(pid {pid}) 가 뜨자마자 내려갔다 — {}",
                self.err_tail(label)
            )),
        }
    }

    /// 그 서버가 만든 세션이 받은편지함을 등록하길 기다린다(`SESSION_WAIT` 까지, 기기 시계로) — 소켓 pid 의 부모가 그 서버이고
    /// `since`(유닉스 초) 뒤에 등록한 것(`rc::handoff_session`). 서버가 내려가면 그만 본다.
    pub async fn wait_handoff_session(
        &self,
        label: &str,
        server_pid: u32,
        since: i64,
        registrations: impl Fn() -> Vec<InboxRegistration>,
    ) -> HandoffWait {
        let deadline =
            (self.ops.now)() + chrono::Duration::from_std(SESSION_WAIT).unwrap_or_default();
        let mut ps_error: Option<String>;
        loop {
            let ps = (self.runner)(
                argv(&["ps", "-axww", "-o", "pid=,ppid=,etime=,args="]),
                String::new(),
                PS_TIMEOUT,
            )
            .await;
            if ps.ok() {
                ps_error = None;
                let rows = rc::parse_ps(&ps.stdout);
                let regs = registrations();
                if let Some(found) = rc::handoff_session(&regs, &rows, server_pid, since) {
                    return HandoffWait::Found(found.clone());
                }
            } else {
                ps_error = Some(super::probe_failure("ps", &ps));
            }
            if !(self.ops.signal)(server_pid, 0) {
                return HandoffWait::ServerGone(self.err_tail(label));
            }
            if (self.ops.now)() >= deadline {
                return HandoffWait::TimedOut { ps_error };
            }
            (self.ops.sleep)(SESSION_POLL).await;
        }
    }

    fn handoff_dir(&self) -> PathBuf {
        self.log_dir.join("handoff")
    }

    fn write_handoff_record(&self, record: &HandoffServerRecord) {
        let dir = self.handoff_dir();
        let written = std::fs::create_dir_all(&dir).and_then(|()| {
            let raw = serde_json::to_string(record).map_err(std::io::Error::other)?;
            std::fs::write(dir.join(format!("{}.json", record.label)), raw)
        });
        if let Err(e) = written {
            // 서버는 떴다 — 기록만 못 남겼다(현황에는 대상 밖으로 보이고, 닫기는 rocky rc 로 못 찾는다).
            self.event(
                "handoff-record",
                &record.label,
                serde_json::json!({ "dir": dir, "error": e.to_string() }),
            );
        }
    }

    /// 핸드오프 서버 기록을 지운다 — 서버가 내려갔다. 라벨은 데몬이 만든 안전한 이름(`handoff_log_label`)일 때만 이 길로.
    pub fn forget_handoff(&self, label: &str) {
        let _ = std::fs::remove_file(self.handoff_dir().join(format!("{label}.json")));
    }

    /// 기록과 그 파일 경로 — 지울 때는 읽은 경로를 쓴다(손으로 고친 파일의 `label` 로 경로를 만들지 않게).
    fn handoff_records(&self) -> Vec<(PathBuf, HandoffServerRecord)> {
        let Ok(entries) = std::fs::read_dir(self.handoff_dir()) else {
            return Vec::new();
        };
        entries
            .filter_map(|e| {
                let path = e.ok()?.path();
                let raw = std::fs::read_to_string(&path).ok()?;
                Some((path, serde_json::from_str(&raw).ok()?))
            })
            .collect()
    }

    /// 현황에서 핸드오프 서버를 대상 밖 서버와 가른다. 현황은 몇 초 낡은 캐시일 수 있다 — 그 사이 끝난 대상 밖 서버는 빼고
    /// (닫은 직후), 기록은 프로브가 성공했고 **그 pid 가 정말 없을 때만** 지운다(막 띄운 서버가 낡은 스냅숏에 아직 없어도
    /// 기록을 지키게 — *never act on a stale or failed probe*).
    pub(super) fn decorate_handoffs(&self, status: &mut RcStatus) {
        if !status.configured {
            return;
        }
        status.strays.retain(|s| (self.ops.signal)(s.pid, 0));
        let records = self.handoff_records();
        if records.is_empty() {
            return;
        }
        let plain: Vec<HandoffServerRecord> = records.iter().map(|(_, r)| r.clone()).collect();
        let strays = std::mem::take(&mut status.strays);
        let (handoffs, strays, gone) = rc::split_handoffs(strays, &plain);
        status.handoffs = handoffs;
        status.strays = strays;
        if status.probe_error.is_some() {
            return;
        }
        for (path, record) in &records {
            if gone.contains(&record.label) && !(self.ops.signal)(record.pid, 0) {
                let _ = std::fs::remove_file(path);
            }
        }
    }

    /// 핸드오프 서버를 닫는다 — 라벨이나 할 일 참조로 고르고, 지금 그 폴더에서 그 pid 로 도는 rc 서버일 때만(`handoff_is_live`)
    /// pid 로 내린다(SIGTERM → 유예 → SIGKILL). 프로브가 실패하면 손대지 않는다. 워크트리는 남긴다(커밋 안 된 작업이 있을 수 있다).
    pub async fn stop_handoff(&self, key: &str) -> Result<(HandoffServerRecord, bool), RcRefusal> {
        let records = self.handoff_records();
        let plain: Vec<HandoffServerRecord> = records.iter().map(|(_, r)| r.clone()).collect();
        let Some(record) = rc::find_handoff(&plain, key).cloned() else {
            return Err(RcRefusal::NotFound(format!(
                "핸드오프 서버가 없다: {key} — rocky rc 로 본다"
            )));
        };
        let path = records
            .iter()
            .find(|(_, r)| r.label == record.label)
            .map(|(p, _)| p.clone());
        let forget = || {
            if let Some(path) = &path {
                let _ = std::fs::remove_file(path);
            }
        };
        let status = probe(&self.runner, self.config.as_ref(), &self.home).await;
        if let Some(err) = status.probe_error {
            return Err(RcRefusal::Busy(format!(
                "rc 현황을 못 읽어 닫지 않았다 — {err}"
            )));
        }
        if !status
            .strays
            .iter()
            .any(|s| rc::handoff_is_live(&record, s))
        {
            // 신호를 보내지 않는다 — 그 pid 가 살아 있어도 이제 그 폴더의 rc 서버가 아니다(재사용된 pid).
            forget();
            let why = if (self.ops.signal)(record.pid, 0) {
                "그 pid 는 이제 그 폴더의 rc 서버가 아니다 — 손대지 않고 기록만 지웠다"
            } else {
                "이미 내려가 있다 — 기록을 지웠다"
            };
            return Err(RcRefusal::NotFound(format!(
                "\"{}\"(pid {}) — {why}",
                record.name, record.pid
            )));
        }
        let down = self.stop(&record.label, record.pid).await;
        if down {
            forget();
        }
        self.event(
            "handoff-stop",
            &record.label,
            serde_json::json!({ "pid": record.pid, "down": down }),
        );
        Ok((record, down))
    }

    /// 기동 err 로그의 마지막 줄 — 비었으면 로그 경로를 댄다.
    fn err_tail(&self, label: &str) -> String {
        let err = self.read_log(label, "err");
        match err.lines().rev().find(|l| !l.trim().is_empty()) {
            Some(line) => line.trim().to_string(),
            None => format!(
                "err 로그가 비어 있다({})",
                self.log_path(label, "err").display()
            ),
        }
    }
}
