//! 기본 브랜치 검증 잡 — `rocky.json` 의 `verify.targets[]` 마다 원격 브랜치를 보고, 새 커밋이면 전용 워크트리에서
//! 단계를 차례로 돈다. 판정은 `rocky_core::verify`(순수), 여기는 git·프로세스·파일 배선이다.
//!
//! - **감지**는 `git ls-remote` — GitHub API 예산(세션의 `gh` 와 같이 쓰는)을 쓰지 않는다.
//! - **장소**는 `<todo dir>/verify/<board>/<branch>/tree` — 보드 레포의 detached 워크트리. 사람·세션의 작업 트리와
//!   브랜치를 건드리지 않는다(메인 폴더에서 pull 하다 다른 세션의 브랜치와 부딪힌 일이 있다). 보드 레포의 git 훅은
//!   돌리지 않는다(`core.hooksPath=/dev/null`).
//! - **한 번에 하나**: 잡 하나가 대상을 차례로 돈다. 도는 사이 커밋이 몰리면 다음 바퀴에 최신 하나만 본다.
//! - **인프라 실패는 커밋 탓이 아니다**: fetch·워크트리 준비가 실패하면 그 커밋을 빨강으로 남기지 않고 다음 바퀴에 다시 한다.
//! - **단계는 자기 프로세스 그룹**으로 띄운다(cargo·bun 이 띄운 손자까지 한 번에 끝내려고). 그룹이 남지 않게 세 겹으로:
//!   시간 초과면 TERM → 유예 → KILL, 데몬이 정상 종료하며 작업을 버리면 가드가 KILL, 데몬이 죽어 남은 그룹은
//!   `running.pgid` 를 보고 다음 실행 전에 끝낸다(같은 트리에서 겹쳐 돌지 않게).

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use rocky_core::config::CommandBridge;
use rocky_core::verify::{
    dir_name, notification, parse_ls_remote, should_run, VerifyConfig, VerifyRecord, VerifyRun,
    VerifyState, VerifyTarget, DEFAULT_STEP_TIMEOUT_MS, MAX_ATTEMPTS,
};
use serde::Serialize;

use crate::runner::Runner;
use crate::server::ServerState;

/// git 명령 하나의 상한(fetch·worktree).
const GIT_TIMEOUT: Duration = Duration::from_secs(120);
/// 그룹에 TERM 을 보낸 뒤 KILL 까지의 유예.
const KILL_GRACE: Duration = Duration::from_secs(5);
/// 대상마다 남기는 로그 파일 수.
const KEEP_LOGS: usize = 10;
/// `signals.log`·`runs.jsonl` 이 이만큼 넘으면 `.1` 로 한 번 돌린다.
const APPEND_LOG_LIMIT: u64 = 256 * 1024;

/// 대상 하나의 지금 상태 — `GET /api/verify` 가 그대로 싣는다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyTargetStatus {
    pub board: String,
    pub branch: String,
    /// 마지막(또는 도는 중인) 검증.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record: Option<VerifyRecord>,
    /// 검증을 하지 못한 이유(보드에 path 없음·원격을 못 읽음·준비 실패) — 다음 바퀴에 다시 한다.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// `runs.jsonl` 에 마지막으로 남긴 이유 — 같은 이유를 또 쓰지 않으려고. 기동 때 그 파일의 마지막 줄에서 되살려,
    /// 데몬이 다시 떠도 이어지는 같은 이유를 다시 쓰지 않는다. 검증이 되면 비운다(다시 나면 다시 쓴다).
    #[serde(skip)]
    pub logged_error: Option<String>,
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

/// 대상마다 따로 — 같은 보드의 다른 브랜치가 기록·트리를 나눠 쓰지 않게.
pub fn target_dir(root: &Path, target: &VerifyTarget) -> PathBuf {
    root.join(dir_name(&target.board))
        .join(dir_name(&target.branch))
}

/// `last.json` — 마지막(또는 도는 중인) 기록. `finished.json` — 마지막으로 **끝난** 기록(알림의 비교 기준).
fn read_record(dir: &Path, file: &str) -> Option<VerifyRecord> {
    let raw = std::fs::read_to_string(dir.join(file)).ok()?;
    serde_json::from_str(&raw).ok()
}

fn write_record(dir: &Path, file: &str, record: &VerifyRecord) {
    let _ = std::fs::create_dir_all(dir);
    if let Ok(text) = serde_json::to_string_pretty(record) {
        let tmp = dir.join(format!("{file}.tmp"));
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::rename(&tmp, dir.join(file));
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
            record: read_record(&target_dir(root, t), "last.json"),
            error: None,
            logged_error: last_logged_error(&target_dir(root, t)),
        })
        .collect()
}

/// `runs.jsonl` 의 마지막 줄이 검증을 못 한 이유면 그 이유.
fn last_logged_error(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join("runs.jsonl")).ok()?;
    let run: VerifyRun = serde_json::from_str(text.lines().last()?).ok()?;
    (run.event == rocky_core::verify::VerifyRunEvent::Error)
        .then_some(run.error)
        .flatten()
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

/// git 한 번 — 보드 레포의 훅은 돌리지 않는다(데몬 맥락에서 남의 post-checkout 이 돌지 않게).
async fn git(runner: &Runner, args: &[&str]) -> Result<String, String> {
    let mut argv = vec![
        "git".to_string(),
        "-c".to_string(),
        "core.hooksPath=/dev/null".to_string(),
    ];
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

/// 대상 디렉터리의 계속 쌓이는 파일에 한 줄 — 실행마다 갈리는 로그와 달리 데몬이 다시 떠도 남는다.
/// `APPEND_LOG_LIMIT` 를 넘으면 `<name>.1` 로 한 번 돌린다(그 전 것은 버린다).
fn append_line(dir: &Path, name: &str, line: &str) {
    use std::io::Write;
    // 실행 전 error 경로에서도 디렉터리가 생긴다 — 레포 경로·git 에러가 담기니 소유자만.
    if !dir.exists() {
        let _ = std::fs::create_dir_all(dir);
        let _ = std::fs::set_permissions(dir, std::os::unix::fs::PermissionsExt::from_mode(0o700));
    }
    let file = dir.join(name);
    if std::fs::metadata(&file)
        .map(|m| m.len() > APPEND_LOG_LIMIT)
        .unwrap_or(false)
    {
        let _ = std::fs::rename(&file, dir.join(format!("{name}.1")));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file)
    {
        let _ = writeln!(f, "{line}");
    }
}

/// 프로세스 그룹을 건드린 판단을 남긴다 — 데몬 로그와 `signals.log`. 그룹 id 재사용 판별이 맞았는지 나중에 이걸로 본다.
fn note_signal(dir: &Path, message: &str) {
    println!("rocky: verify 시그널 — {message}");
    append_line(dir, "signals.log", &format!("{} {message}", iso_now()));
}

/// 실행 이력 한 줄을 `runs.jsonl` 에 — 언제 어떤 커밋이 통과·실패·끊겼나, 검증을 못 한 이유.
fn note_run(dir: &Path, run: &VerifyRun) {
    if let Ok(line) = serde_json::to_string(run) {
        append_line(dir, "runs.jsonl", &line);
    }
}

fn sent(ok: bool) -> &'static str {
    if ok {
        "보냄"
    } else {
        "대상 없음"
    }
}

fn signal_group(pgid: i32, signal: i32) -> bool {
    // SAFETY: killpg 는 프로세스 그룹 id 와 시그널 번호만 받는다 — 메모리를 건드리지 않는다.
    pgid > 1 && unsafe { libc::killpg(pgid, signal) } == 0
}

/// 단계 그룹을 쥐고 있다가, 놓지 않은 채 버려지면(데몬이 정상 종료하며 작업을 drop) 그룹째 KILL 한다.
struct GroupGuard {
    dir: PathBuf,
    step: String,
    pgid: i32,
    pgid_file: PathBuf,
    armed: bool,
}

impl GroupGuard {
    fn release(mut self) {
        self.armed = false;
    }
}

impl Drop for GroupGuard {
    fn drop(&mut self) {
        if self.armed {
            let ok = signal_group(self.pgid, libc::SIGKILL);
            note_signal(
                &self.dir,
                &format!(
                    "가드 step={} pgid={}: 데몬이 단계를 버림 → KILL {}",
                    self.step,
                    self.pgid,
                    sent(ok)
                ),
            );
        }
        let _ = std::fs::remove_file(&self.pgid_file);
    }
}

/// 프로세스의 시작 시각(`ps -o lstart=`) — pid 가 재사용됐는지 가르는 열쇠. 프로세스가 없으면 `None`.
async fn started_at(pid: i32) -> Option<String> {
    let out = tokio::process::Command::new("ps")
        .args(["-o", "lstart=", "-p", &pid.to_string()])
        .output()
        .await
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !text.is_empty()).then_some(text)
}

/// 데몬이 죽어(SIGKILL·크래시) 남은 단계 그룹을 끝낸다 — 같은 트리에서 새 실행과 겹치지 않게.
/// `running.pgid` 는 "그룹 id \t 리더 시작 시각" 이다. 리더가 살아 있는데 시작 시각이 다르면 그 번호를 다른 프로세스가
/// 재사용한 것이라 건드리지 않는다. 리더가 없으면 남은 멤버는 우리 것이다 — 그룹에 멤버가 있는 동안 그 id 는
/// 재사용되지 않는다(없으면 killpg 가 아무것도 하지 않는다).
async fn reap_orphan(dir: &Path) {
    let file = dir.join("running.pgid");
    let Ok(raw) = std::fs::read_to_string(&file) else {
        return;
    };
    let _ = std::fs::remove_file(&file);
    let (pgid, recorded) = raw.split_once('\t').unwrap_or((raw.as_str(), ""));
    let Ok(pgid) = pgid.trim().parse::<i32>() else {
        return;
    };
    let recorded = recorded.trim();
    let leader = match started_at(pgid).await {
        Some(now) if now != recorded => {
            // 다른 프로세스가 그 번호를 쓰고 있다 — 건드리지 않는다.
            note_signal(
                dir,
                &format!("정리 pgid={pgid}: 건너뜀(번호 재사용) — 기록 시작 '{recorded}', 지금 리더 시작 '{now}'"),
            );
            return;
        }
        Some(_) => "리더 일치",
        None => "리더 없음(남은 멤버만)",
    };
    let term = signal_group(pgid, libc::SIGTERM);
    let kill = if term {
        tokio::time::sleep(KILL_GRACE).await;
        Some(signal_group(pgid, libc::SIGKILL))
    } else {
        None
    };
    note_signal(
        dir,
        &format!(
            "정리 pgid={pgid}: {leader}, 기록 시작 '{recorded}' → TERM {}{}",
            sent(term),
            kill.map(|k| format!(", KILL {}", sent(k)))
                .unwrap_or_default()
        ),
    );
}

/// 단계 하나 — 워크트리에서, 출력은 로그 파일로(파이프가 아니라 — 떨어져 나간 손자가 fd 를 물어도 `wait` 가 매달리지
/// 않는다), 자기 프로세스 그룹으로.
async fn run_step(dir: &Path, tree: &Path, step: &CommandBridge, log: &Path) -> Result<(), String> {
    use std::io::Write;
    let open_err = |e: std::io::Error| format!("로그 파일({}): {e}", log.display());
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
        .map_err(open_err)?;
    // argv 는 남기지 않는다 — 단계 인자에 토큰이 들어 있을 수 있다. 이름과 실행 파일만.
    let _ = writeln!(file, "\n### {} — {}", step.name, step.command[0]);
    let out = file.try_clone().map_err(open_err)?;
    let mut cmd = tokio::process::Command::new(&step.command[0]);
    cmd.args(&step.command[1..])
        .current_dir(tree)
        .stdin(Stdio::null())
        .stdout(Stdio::from(out))
        .stderr(Stdio::from(file))
        .process_group(0)
        .kill_on_drop(true);
    let mut child = cmd.spawn().map_err(|e| {
        format!(
            "실행하지 못했다({} in {}): {e}",
            step.command[0],
            tree.display()
        )
    })?;
    // 그룹 id 는 자식 pid 다(process_group(0)).
    let pgid = child.id().map(|p| p as i32).unwrap_or(0);
    let pgid_file = dir.join("running.pgid");
    let leader_start = started_at(pgid).await.unwrap_or_default();
    let _ = std::fs::write(&pgid_file, format!("{pgid}\t{leader_start}"));
    let guard = GroupGuard {
        dir: dir.to_path_buf(),
        step: step.name.clone(),
        pgid,
        pgid_file,
        armed: true,
    };
    let limit = Duration::from_millis(step.timeout_ms.unwrap_or(DEFAULT_STEP_TIMEOUT_MS));
    let result = match tokio::time::timeout(limit, child.wait()).await {
        Ok(Ok(status)) if status.success() => Ok(()),
        Ok(Ok(status)) => Err(match status.code() {
            Some(code) => format!("종료 코드 {code}"),
            None => "시그널로 끝났다".into(),
        }),
        Ok(Err(e)) => Err(format!("기다리지 못했다: {e}")),
        Err(_) => {
            // TERM 으로 정리할 틈을 주고, 남은 것은 KILL.
            let term = signal_group(pgid, libc::SIGTERM);
            let _ = tokio::time::timeout(KILL_GRACE, child.wait()).await;
            let kill = signal_group(pgid, libc::SIGKILL);
            let _ = child.kill().await;
            note_signal(
                dir,
                &format!(
                    "시간 초과 step={} pgid={pgid} ({limit:?}): TERM {}, KILL {}",
                    step.name,
                    sent(term),
                    sent(kill)
                ),
            );
            Err(format!("{limit:?} 안에 끝나지 않았다"))
        }
    };
    // 리더가 끝나도 그룹에 손자가 남았을 수 있다 — 단계가 끝났으면 그룹은 더 쓸 데가 없다.
    if signal_group(pgid, libc::SIGKILL) {
        note_signal(
            dir,
            &format!(
                "남은 손자 step={} pgid={pgid}: 리더가 끝난 뒤 그룹에 남아 KILL",
                step.name
            ),
        );
    }
    guard.release();
    result
}

/// 워크트리를 그 커밋으로 — 있으면 detached 로 옮기고(이 트리는 데몬 것이라 로컬 변경은 버린다), 안 되거나 없으면
/// 그 디렉터리만 지우고 `--force` 로 다시 만든다. 레포 전체 `worktree prune` 은 하지 않는다 — 사용자의 다른 워크트리
/// 등록까지 지울 수 있다.
async fn prepare_tree(runner: &Runner, repo: &Path, tree: &Path, sha: &str) -> Result<(), String> {
    let repo_s = repo.to_string_lossy().to_string();
    let tree_s = tree.to_string_lossy().to_string();
    if tree.join(".git").exists()
        && git(
            runner,
            &[
                "-C", &tree_s, "checkout", "--quiet", "--force", "--detach", sha,
            ],
        )
        .await
        .is_ok()
    {
        return Ok(());
    }
    if tree.exists() {
        std::fs::remove_dir_all(tree)
            .map_err(|e| format!("깨진 워크트리를 지우지 못했다({tree_s}): {e}"))?;
    }
    git(
        runner,
        &[
            "-C", &repo_s, "worktree", "add", "--force", "--detach", &tree_s, sha,
        ],
    )
    .await
    .map(|_| ())
}

/// 로그는 대상마다 최근 `KEEP_LOGS` 개만.
fn prune_logs(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut logs: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("log"))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    logs.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
    for (_, path) in logs.into_iter().skip(KEEP_LOGS) {
        let _ = std::fs::remove_file(path);
    }
}

/// 대상 하나 — 새 커밋이면 끝까지 돌고, 아니면 아무것도 안 한다.
pub async fn verify_target(
    state: &Arc<ServerState>,
    runner: &Runner,
    notifier: &VerifyNotifier,
    root: &Path,
    target: &VerifyTarget,
) {
    let dir = target_dir(root, target);
    let report = |record: Option<VerifyRecord>, error: Option<String>| {
        // 검증을 못 한 이유는 바뀌었을 때만 남긴다 — 원격이 안 닿으면 바퀴마다 같은 말이다.
        let before = state
            .verify()
            .into_iter()
            .find(|s| s.board == target.board && s.branch == target.branch)
            .and_then(|s| s.logged_error);
        if let Some(e) = error.as_ref().filter(|e| before.as_ref() != Some(*e)) {
            println!("rocky: verify {} {} — {e}", target.board, target.branch);
            note_run(&dir, &VerifyRun::error(iso_now(), e.clone()));
        }
        set_status(
            state,
            VerifyTargetStatus {
                board: target.board.clone(),
                branch: target.branch.clone(),
                record,
                logged_error: error.clone(),
                error,
            },
        );
    };
    let last = read_record(&dir, "last.json");
    // 보드는 별칭까지 푼다(보드 key 를 바꿔도 설정이 끊기지 않게).
    let repo = match state.store.get_board(&target.board) {
        Ok(Some(board)) => match board.path {
            Some(p) => PathBuf::from(p),
            None => {
                return report(
                    last,
                    Some(format!(
                        "보드 {} 에 path 가 없다 — rocky board path 로 레포를 잇는다",
                        target.board
                    )),
                )
            }
        },
        Ok(None) => return report(last, Some(format!("보드가 없다: {}", target.board))),
        Err(e) => return report(last, Some(format!("보드 {}: {e}", target.board))),
    };
    let repo_s = repo.to_string_lossy().to_string();
    let refspec = format!("refs/heads/{}", target.branch);
    let remote = match git(runner, &["-C", &repo_s, "ls-remote", "origin", &refspec]).await {
        Ok(out) => match parse_ls_remote(&out, &target.branch) {
            Some(sha) => sha,
            None => {
                return report(
                    last,
                    Some(format!(
                        "{repo_s}: origin 에 {} 브랜치가 없다",
                        target.branch
                    )),
                )
            }
        },
        Err(e) => return report(last, Some(format!("{repo_s}: {e}"))),
    };
    let rerun = state.verify_rerun_requested(&target.board, &target.branch);
    if !should_run(last.as_ref(), &remote, rerun) {
        return report(last, None);
    }
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::set_permissions(&dir, std::os::unix::fs::PermissionsExt::from_mode(0o700));
    reap_orphan(&dir).await;

    // 준비(fetch·워크트리)는 커밋을 빨강으로 만들지 않는다 — 실패하면 기록을 그대로 두고 다음 바퀴에 다시.
    let tree = dir.join("tree");
    let prepared = async {
        git(
            runner,
            &["-C", &repo_s, "fetch", "--quiet", "origin", &target.branch],
        )
        .await?;
        prepare_tree(runner, &repo, &tree, &remote).await
    }
    .await;
    if let Err(e) = prepared {
        return report(last, Some(format!("준비 실패(다음 바퀴에 다시) — {e}")));
    }
    // 여기서부터 이 커밋을 돈다 — 끝을 못 본 지난 실행을 이력에 남긴다.
    if let Some(prev) = last.filter(|r| r.state == VerifyState::Running) {
        note_run(&dir, &VerifyRun::of(iso_now(), prev));
    }
    let tree_s = tree.to_string_lossy().to_string();
    let log = dir.join(format!("{}.log", &remote[..remote.len().min(12)]));
    // 같은 커밋을 또 도는 것(다시 돌리기·끊긴 실행)이면 비우지 않고 이어 쓴다 — 이력의 앞 줄이 가리키는 출력이 남게.
    if std::fs::metadata(&log)
        .map(|m| m.len() > 0)
        .unwrap_or(false)
    {
        if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(&log) {
            use std::io::Write;
            let why = if rerun {
                "다시 돌리기 요청"
            } else {
                "끊긴 실행을 이어서"
            };
            let _ = writeln!(f, "\n### 같은 커밋을 다시 — {why} ({})", iso_now());
        }
    } else {
        let _ = std::fs::write(&log, "");
    }
    // 단계 출력이 쌓이는 곳 — 소유자만 읽는다.
    let _ = std::fs::set_permissions(&log, std::os::unix::fs::PermissionsExt::from_mode(0o600));
    let mut record = VerifyRecord {
        board: target.board.clone(),
        branch: target.branch.clone(),
        sha: remote.clone(),
        subject: git(runner, &["-C", &tree_s, "log", "-1", "--format=%s"])
            .await
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        state: VerifyState::Running,
        failed_step: None,
        reason: None,
        started_at: iso_now(),
        finished_at: None,
        log: log.to_string_lossy().to_string(),
        attempt: 1,
        rerun,
    };
    write_record(&dir, "last.json", &record);
    report(Some(record.clone()), None);
    // 도는 중으로 바꾸며 다시 돌리기 요청을 받은 것으로 친다(한 락 안에서) — 이 뒤의 요청은 "도는 중" 으로 거절된다.
    state.begin_verify(&target.board, &target.branch);

    // 실패하면 그 자리에서 다시 — 첫 실패는 이력에만 남기고 알리지 않는다. 출력은 같은 로그 파일에 이어 쌓인다.
    let outcome = loop {
        let mut outcome = Ok(());
        for step in &target.steps {
            if let Err(reason) = run_step(&dir, &tree, step, &log).await {
                outcome = Err((step.name.clone(), reason));
                break;
            }
        }
        let Err((step, reason)) = &outcome else {
            break outcome;
        };
        if record.attempt >= MAX_ATTEMPTS {
            break outcome;
        }
        let mut failed = record.clone();
        failed.state = VerifyState::Failed;
        failed.failed_step = Some(step.clone());
        failed.reason = Some(reason.clone());
        failed.finished_at = Some(iso_now());
        note_run(&dir, &VerifyRun::of(iso_now(), failed));
        println!(
            "rocky: verify {} {} {step} 실패({reason}) — 한 번 더 돈다",
            target.board, target.branch
        );
        // 로그는 `append_line` 으로 쓰지 않는다 — 상한에 걸려 돌려지면 첫 시도의 출력이 다른 이름으로 간다.
        if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(&log) {
            use std::io::Write;
            let _ = writeln!(f, "\n### 자동 재시도 — {step} 실패: {reason}");
        }
        record.attempt += 1;
        record.started_at = iso_now();
        write_record(&dir, "last.json", &record);
        report(Some(record.clone()), None);
    };
    state.end_verify();
    match outcome {
        Ok(()) => record.state = VerifyState::Passed,
        Err((step, reason)) => {
            record.state = VerifyState::Failed;
            record.failed_step = Some(step);
            record.reason = Some(reason);
        }
    }
    record.finished_at = Some(iso_now());
    // 알림은 마지막으로 **끝난** 기록과 비교한다 — 도중에 끊겨 다시 돈 실행이어도 복구를 놓치지 않게.
    let finished_before = read_record(&dir, "finished.json");
    write_record(&dir, "last.json", &record);
    write_record(&dir, "finished.json", &record);
    note_run(&dir, &VerifyRun::of(iso_now(), record.clone()));
    prune_logs(&dir);
    report(Some(record.clone()), None);
    // 구독한 세션에는 끝난 실행마다(통과도) — 배포를 맡은 세션이 폴링하지 않고 이것으로 움직인다.
    if let Some((kind, text)) =
        rocky_core::verify::session_notice(finished_before.as_ref(), &record)
    {
        crate::server::notify_verify_subscriber(state, &record, kind, &text).await;
    }
    if let Some((title, body)) = notification(finished_before.as_ref(), &record) {
        // 제목이 이미 `rocky:` 로 시작한다.
        println!("{title} — {body}");
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

/// 기동 뒤 `first_after` 지나 처음, 이후 바퀴가 끝날 때마다 `interval_seconds` 쉬고 다시. 다시 돌리기 요청이 오면
/// 쉬는 중이라도(기동 직후의 첫 대기 포함) 바로 다음 바퀴로 간다(`verify_wake`).
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
        tokio::select! {
            _ = tokio::time::sleep(first_after) => {}
            _ = state.verify_wake.notified() => {}
        }
        loop {
            tick(&state, &runner, &notifier, &root, &cfg).await;
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(cfg.interval_seconds())) => {}
                _ = state.verify_wake.notified() => {}
            }
        }
    });
}
