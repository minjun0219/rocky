//! 핸드오프 서버 — 보드의 "새 세션 띄우기"(spawn)가 rc 가 켜진 기기에서 타는 길. 할 일의 워크트리를 만들고(없으면) 그
//! 안에서 단일 세션 rc 서버를 띄운 뒤, 서버가 만든 세션이 받은편지함을 등록하길 기다린다. 그 세션에 핸드오프를 넣고 깨우는
//! 일은 라우트가 지금의 핸드오프와 같은 길로 한다. `claude --bg` 세션은 로그인 세션 밖이라 ssh · 자격이 끊긴다.
//!
//! 이 서버는 대상(`rc.targets`)이 아니라 감시 · 야간이 건드리지 않는다(현황에는 대상 밖 서버로 보인다). 내리는 것은 사람이다 —
//! 세션이 끝났다고 알리면 사람이 닫는다. 판정은 `rocky_core::rc`(`handoff_server_argv` · `handoff_session` · `worktree_base`).

use std::path::Path;
use std::time::Duration;

use rocky_core::peer_inbox::InboxRegistration;
use rocky_core::rc::{self, AuthState, Registration};

use super::{argv, probe, RcController};

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
    pub async fn launch_handoff(&self, label: &str, dir: &str, name: &str) -> Result<u32, String> {
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
        match self.judge(label, pid).await {
            Registration::Connected => Ok(pid),
            // 등록 문구는 못 봤지만 떠 있다 — 세션 등록을 기다려 본다.
            Registration::Pending if (self.ops.signal)(pid, 0) => Ok(pid),
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
