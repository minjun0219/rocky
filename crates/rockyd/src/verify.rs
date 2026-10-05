//! 기본 브랜치 검증 잡 — `rocky.json` 의 `verify.targets[]` 마다 원격 브랜치를 보고, 새 커밋이면 전용 워크트리에서
//! 단계를 차례로 돈다. 판정은 `rocky_core::verify`(순수), 여기는 git·프로세스·파일 배선이다.
//!
//! - **감지**는 `git ls-remote` — GitHub API 예산(세션의 `gh` 와 같이 쓰는)을 쓰지 않는다.
//! - **장소**는 `<todo dir>/verify/<board>/tree` — 보드 레포의 detached 워크트리. 사람·세션의 작업 트리와 브랜치를
//!   건드리지 않는다(메인 폴더에서 pull 하다 다른 세션의 브랜치와 부딪힌 일이 있다).
//! - **한 번에 하나**: 잡 하나가 대상을 차례로 돈다. 도는 사이 커밋이 몰리면 다음 바퀴에 최신 하나만 본다.
//! - 단계는 자기 프로세스 그룹으로 띄워, 시간 초과면 그룹째 끝낸다(cargo 가 띄운 자식까지).

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use rocky_core::config::CommandBridge;
use rocky_core::verify::{
    dir_name, notification, parse_ls_remote, should_run, VerifyConfig, VerifyRecord, VerifyState,
    VerifyTarget, DEFAULT_STEP_TIMEOUT_MS,
};
use serde::Serialize;

use crate::runner::Runner;
use crate::server::ServerState;

/// git 명령 하나의 상한(fetch·worktree).
const GIT_TIMEOUT: Duration = Duration::from_secs(120);

/// 대상 하나의 지금 상태 — `GET /api/verify` 가 그대로 싣는다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyTargetStatus {
    pub board: String,
    pub branch: String,
    /// 마지막(또는 도는 중인) 검증.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record: Option<VerifyRecord>,
    /// 검증을 시작하지도 못한 이유(보드에 path 없음·원격을 못 읽음).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// 사람에게 알리는 함수 — (제목, 본문). 기본은 osascript, 테스트는 붙잡는다.
pub type VerifyNotifier = Arc<dyn Fn(String, String) + Send + Sync>;

pub fn osascript_verify_notifier(runner: Runner) -> VerifyNotifier {
    Arc::new(move |title, body| {
        if !cfg!(target_os = "macos") {
            return;
        }
        let runner = runner.clone();
        tokio::spawn(async move {
            let _ = runner(
                rocky_core::prwatch::osascript_args(&title, &body),
                String::new(),
                Duration::from_secs(10),
            )
            .await;
        });
    })
}

fn iso_now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn target_dir(root: &Path, board: &str) -> PathBuf {
    root.join(dir_name(board))
}

fn read_last(dir: &Path) -> Option<VerifyRecord> {
    let raw = std::fs::read_to_string(dir.join("last.json")).ok()?;
    serde_json::from_str(&raw).ok()
}

fn write_last(dir: &Path, record: &VerifyRecord) {
    let _ = std::fs::create_dir_all(dir);
    if let Ok(text) = serde_json::to_string_pretty(record) {
        let tmp = dir.join("last.json.tmp");
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::rename(&tmp, dir.join("last.json"));
        }
    }
}

/// 기동 때 — 파일에 남은 마지막 기록으로 상태를 채운다(웹·CLI 가 첫 바퀴 전에도 본다).
pub fn load_statuses(cfg: &VerifyConfig, root: &Path) -> Vec<VerifyTargetStatus> {
    cfg.targets
        .iter()
        .map(|t| VerifyTargetStatus {
            board: t.board.clone(),
            branch: t.branch.clone(),
            record: read_last(&target_dir(root, &t.board)),
            error: None,
        })
        .collect()
}

fn set_status(state: &ServerState, status: VerifyTargetStatus) {
    let mut all = state.verify();
    match all
        .iter_mut()
        .find(|s| s.board == status.board && s.branch == status.branch)
    {
        Some(slot) => *slot = status,
        None => all.push(status),
    }
    state.set_verify(all);
}

async fn git(runner: &Runner, args: &[&str]) -> Result<String, String> {
    let mut argv = vec!["git".to_string()];
    argv.extend(args.iter().map(|a| a.to_string()));
    let out = runner(argv, String::new(), GIT_TIMEOUT).await;
    if out.ok() {
        Ok(out.stdout)
    } else {
        let first = out
            .stderr
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("");
        Err(format!(
            "git {} 실패(코드 {}): {first}",
            args.join(" "),
            out.code
        ))
    }
}

/// 단계 하나 — 워크트리에서, 출력은 로그 파일로, 자기 프로세스 그룹으로.
async fn run_step(tree: &Path, step: &CommandBridge, log: &Path) -> Result<(), String> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
        .map_err(|e| format!("로그 파일을 열지 못했다({}): {e}", log.display()))?;
    let _ = writeln!(file, "\n### {} — $ {}", step.name, step.command.join(" "));
    let out = file.try_clone().map_err(|e| e.to_string())?;
    let mut cmd = tokio::process::Command::new(&step.command[0]);
    cmd.args(&step.command[1..])
        .current_dir(tree)
        .stdin(Stdio::null())
        .stdout(Stdio::from(out))
        .stderr(Stdio::from(file))
        .process_group(0)
        .kill_on_drop(true);
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("실행하지 못했다({}): {e}", step.command[0]))?;
    let limit = Duration::from_millis(step.timeout_ms.unwrap_or(DEFAULT_STEP_TIMEOUT_MS));
    match tokio::time::timeout(limit, child.wait()).await {
        Ok(Ok(status)) if status.success() => Ok(()),
        Ok(Ok(status)) => Err(match status.code() {
            Some(code) => format!("종료 코드 {code}"),
            None => "시그널로 끝났다".into(),
        }),
        Ok(Err(e)) => Err(format!("기다리지 못했다: {e}")),
        Err(_) => {
            // 그룹째 — cargo·bun 이 띄운 자식이 남지 않게. 그룹 id 는 자식 pid 다(process_group(0)).
            if let Some(pid) = child.id() {
                let _ = std::process::Command::new("kill")
                    .args(["-TERM", "--", &format!("-{pid}")])
                    .status();
            }
            let _ = child.kill().await;
            Err(format!("{}초 안에 끝나지 않았다", limit.as_secs()))
        }
    }
}

/// 워크트리를 그 커밋으로 — 없으면 만들고, 있으면 detached 로 옮긴다(이 트리는 데몬 것이라 로컬 변경은 버린다).
async fn prepare_tree(runner: &Runner, repo: &Path, tree: &Path, sha: &str) -> Result<(), String> {
    let repo_s = repo.to_string_lossy().to_string();
    let tree_s = tree.to_string_lossy().to_string();
    if tree.join(".git").exists() {
        git(
            runner,
            &[
                "-C", &tree_s, "checkout", "--quiet", "--force", "--detach", sha,
            ],
        )
        .await?;
    } else {
        let _ = git(runner, &["-C", &repo_s, "worktree", "prune"]).await;
        git(
            runner,
            &["-C", &repo_s, "worktree", "add", "--detach", &tree_s, sha],
        )
        .await?;
    }
    Ok(())
}

/// 대상 하나 — 새 커밋이면 끝까지 돌고, 아니면 아무것도 안 한다.
pub async fn verify_target(
    state: &Arc<ServerState>,
    runner: &Runner,
    notifier: &VerifyNotifier,
    root: &Path,
    target: &VerifyTarget,
) {
    let fail_status = |error: String| VerifyTargetStatus {
        board: target.board.clone(),
        branch: target.branch.clone(),
        record: read_last(&target_dir(root, &target.board)),
        error: Some(error),
    };
    let repo = match state.store.list_boards(true) {
        Ok(boards) => match boards.into_iter().find(|b| b.key == target.board) {
            Some(b) => match b.path {
                Some(p) => PathBuf::from(p),
                None => {
                    set_status(
                        state,
                        fail_status(format!(
                            "보드 {} 에 path 가 없다 — rocky board path 로 레포를 잇는다",
                            target.board
                        )),
                    );
                    return;
                }
            },
            None => {
                set_status(state, fail_status(format!("보드가 없다: {}", target.board)));
                return;
            }
        },
        Err(e) => {
            set_status(state, fail_status(format!("보드 목록: {e}")));
            return;
        }
    };
    let repo_s = repo.to_string_lossy().to_string();
    let refspec = format!("refs/heads/{}", target.branch);
    let remote = match git(runner, &["-C", &repo_s, "ls-remote", "origin", &refspec]).await {
        Ok(out) => match parse_ls_remote(&out, &target.branch) {
            Some(sha) => sha,
            None => {
                set_status(
                    state,
                    fail_status(format!("origin 에 {} 브랜치가 없다", target.branch)),
                );
                return;
            }
        },
        Err(e) => {
            set_status(state, fail_status(e));
            return;
        }
    };
    let dir = target_dir(root, &target.board);
    let prev = read_last(&dir);
    if !should_run(prev.as_ref(), &remote) {
        set_status(
            state,
            VerifyTargetStatus {
                board: target.board.clone(),
                branch: target.branch.clone(),
                record: prev,
                error: None,
            },
        );
        return;
    }
    let _ = std::fs::create_dir_all(&dir);
    let log = dir.join(format!("{}.log", &remote[..remote.len().min(12)]));
    let _ = std::fs::write(&log, "");
    let mut record = VerifyRecord {
        board: target.board.clone(),
        branch: target.branch.clone(),
        sha: remote.clone(),
        subject: None,
        state: VerifyState::Running,
        failed_step: None,
        reason: None,
        started_at: iso_now(),
        finished_at: None,
        log: log.to_string_lossy().to_string(),
    };
    let publish = |record: &VerifyRecord| {
        write_last(&dir, record);
        set_status(
            state,
            VerifyTargetStatus {
                board: target.board.clone(),
                branch: target.branch.clone(),
                record: Some(record.clone()),
                error: None,
            },
        );
    };
    publish(&record);

    let tree = dir.join("tree");
    let prepared = async {
        git(
            runner,
            &["-C", &repo_s, "fetch", "--quiet", "origin", &target.branch],
        )
        .await
        .map_err(|e| ("fetch".to_string(), e))?;
        prepare_tree(runner, &repo, &tree, &remote)
            .await
            .map_err(|e| ("worktree".to_string(), e))
    }
    .await;
    let outcome = match prepared {
        Err(e) => Err(e),
        Ok(()) => {
            let tree_s = tree.to_string_lossy().to_string();
            record.subject = git(runner, &["-C", &tree_s, "log", "-1", "--format=%s"])
                .await
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            publish(&record);
            let mut result = Ok(());
            for step in &target.steps {
                if let Err(reason) = run_step(&tree, step, &log).await {
                    result = Err((step.name.clone(), reason));
                    break;
                }
            }
            result
        }
    };
    match outcome {
        Ok(()) => record.state = VerifyState::Passed,
        Err((step, reason)) => {
            record.state = VerifyState::Failed;
            record.failed_step = Some(step);
            record.reason = Some(reason);
        }
    }
    record.finished_at = Some(iso_now());
    publish(&record);
    if let Some((title, body)) = notification(prev.as_ref(), &record) {
        println!("rocky: {title} — {body}");
        notifier(title, body);
    }
}

/// 대상들을 차례로 — 하나가 끝나야 다음.
pub async fn tick(
    state: &Arc<ServerState>,
    runner: &Runner,
    notifier: &VerifyNotifier,
    root: &Path,
    cfg: &VerifyConfig,
) {
    for target in &cfg.targets {
        verify_target(state, runner, notifier, root, target).await;
    }
}

/// 기동 뒤 `first_after` 지나 처음, 이후 바퀴가 끝날 때마다 `interval_seconds` 쉬고 다시.
pub fn spawn_verifier(
    state: Arc<ServerState>,
    runner: Runner,
    notifier: VerifyNotifier,
    root: PathBuf,
    cfg: VerifyConfig,
    first_after: Duration,
) {
    if !cfg.active() {
        return;
    }
    state.set_verify(load_statuses(&cfg, &root));
    tokio::spawn(async move {
        tokio::time::sleep(first_after).await;
        loop {
            tick(&state, &runner, &notifier, &root, &cfg).await;
            tokio::time::sleep(Duration::from_secs(cfg.interval_seconds())).await;
        }
    });
}
