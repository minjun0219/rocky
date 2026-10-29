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
//! - **배포**(`verify.targets[].deploy`): 통과한 커밋을 `<데몬 옆 rocky> update --main <sha>` 로 올린다. update 는 데몬을
//!   교체하므로 떨어진 프로세스 그룹으로 띄우고, 끝은 대개 새 데몬이 `deploy.json` 과 `main/.finished` 로 판정한다.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rocky_core::config::CommandBridge;
use rocky_core::verify::{
    deploy_base, deploy_gate, deploy_notification, deploy_plan, deploy_timed_out, dir_name,
    notification, parse_ls_remote, settle_deploy, should_run, DeployInput, DeployPlan,
    DeployRecord, DeployState, VerifyConfig, VerifyRecord, VerifyRun, VerifyRunEvent, VerifyState,
    VerifyTarget, DEFAULT_STEP_TIMEOUT_MS, MAX_ATTEMPTS,
};
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::runner::Runner;
use crate::server::ServerState;

/// git 명령 하나의 상한(fetch·worktree).
const GIT_TIMEOUT: Duration = Duration::from_secs(120);
/// 그룹에 TERM 을 보낸 뒤 KILL 까지의 유예.
const KILL_GRACE: Duration = Duration::from_secs(5);
/// 대상마다 남기는 로그 파일 수.
const KEEP_LOGS: usize = 10;
/// 대상마다 남기는 배포 로그(`deploy-<sha12>.log`) 수.
const KEEP_DEPLOY_LOGS: usize = 5;
/// `signals.log`·`runs.jsonl` 이 이만큼 넘으면 `.1` 로 한 번 돌린다.
const APPEND_LOG_LIMIT: u64 = 256 * 1024;

/// 대상 하나의 지금 상태 — `GET /api/verify` 가 그대로 싣는다.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
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
    /// 데몬이 띄운 마지막 배포(`deploy.json`) — `verify.targets[].deploy` 를 켠 대상만.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deploy: Option<DeployRecord>,
    /// 배포하지 않는 이유 중 손을 봐야 풀릴 수 있는 것(`DeploySkip::note`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deploy_note: Option<String>,
    /// 데몬 로그에 마지막으로 남긴 배포 건너뜀 이유 — 바뀔 때만 남기려고.
    #[serde(skip)]
    pub logged_deploy: Option<String>,
}

/// 검증 통과 커밋 자동 배포의 손 — main 빌드가 있는 곳과 update 를 실행할 CLI. 판정은 `rocky_core::verify::deploy_plan`.
#[derive(Clone)]
pub struct DeployEnv {
    /// `$XDG_DATA_HOME/rocky` — `main/`(빌드·`.sha`·`.finished`), 모드 링크 `current`, 잠금 `main-sync.lock`. update 도
    /// 여기서 띄운다.
    pub data_root: PathBuf,
    /// update 를 실행할 `rocky` — 데몬 옆의 것(main 빌드 모드면 `main/rocky`).
    pub rocky: PathBuf,
    /// 이 데몬이 띄운 update 의 끝(pid → 종료 코드, 시그널이면 None) — 기다리는 스레드가 적고 다음 바퀴의 판정이 읽는다.
    exits: Arc<Mutex<HashMap<u32, Option<i32>>>>,
    /// 이 기기의 현지 시각 — 배포 창(`deployWindow`)을 본다. 테스트는 바꿔 끼운다.
    clock: Arc<dyn Fn() -> chrono::NaiveTime + Send + Sync>,
}

impl DeployEnv {
    pub fn new(data_root: PathBuf, rocky: PathBuf) -> Self {
        Self {
            data_root,
            rocky,
            exits: Arc::default(),
            clock: Arc::new(|| chrono::Local::now().time()),
        }
    }

    /// 현지 시각을 바꿔 끼운다(테스트).
    pub fn with_clock(
        mut self,
        clock: impl Fn() -> chrono::NaiveTime + Send + Sync + 'static,
    ) -> Self {
        self.clock = Arc::new(clock);
        self
    }

    /// 데몬 옆 `rocky`(`rocky_beside`)와 데이터 홈(`rocky_core::config::data_home`).
    pub fn beside_daemon() -> Self {
        let rocky = std::env::current_exe()
            .map(|exe| rocky_beside(&exe))
            .unwrap_or_else(|_| PathBuf::from("rocky"));
        Self::new(rocky_core::config::data_home().join("rocky"), rocky)
    }

    fn main_mode(&self) -> bool {
        let link = std::fs::read_link(self.data_root.join("current")).ok();
        rocky_core::main_build::is_main_mode(link.as_deref().and_then(Path::to_str))
    }

    /// `main/` 의 표식 파일(`.sha`·`.finished`).
    fn main_file(&self, name: &str) -> Option<String> {
        std::fs::read_to_string(self.data_root.join("main").join(name)).ok()
    }

    fn busy(&self) -> bool {
        let lock = self.data_root.join("main-sync.lock");
        let pid = std::fs::read_to_string(lock.join("pid")).ok();
        rocky_core::main_build::lock_busy(lock.exists(), pid.as_deref(), pid_alive)
    }

    /// 이 데몬이 띄운 update 가 끝났으면 그 종료 코드(한 번만 꺼낸다).
    fn take_exit(&self, pid: u32) -> Option<Option<i32>> {
        self.exits.lock().ok()?.remove(&pid)
    }

    fn exited(&self, pid: u32) -> bool {
        self.exits.lock().is_ok_and(|e| e.contains_key(&pid))
    }
}

/// 데몬 실행 파일 옆의 `rocky` — 링크(`current/rockyd`)로 떴으면 실제 파일 옆이다(CLI 의 `sibling_binary` 와 같다).
pub fn rocky_beside(exe: &Path) -> PathBuf {
    let exe = std::fs::canonicalize(exe).unwrap_or_else(|_| exe.to_path_buf());
    exe.parent()
        .map(|dir| dir.join("rocky"))
        .unwrap_or_else(|| PathBuf::from("rocky"))
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

/// `last.json` — 마지막(또는 도는 중인) 기록. `finished.json` — 마지막으로 **끝난** 기록(알림의 비교 기준). `passed.json` —
/// 마지막으로 통과한 기록(배포 대상). `deploy.json` — 데몬이 띄운 마지막 배포.
fn read_record<T: DeserializeOwned>(dir: &Path, file: &str) -> Option<T> {
    let raw = std::fs::read_to_string(dir.join(file)).ok()?;
    serde_json::from_str(&raw).ok()
}

fn write_record<T: Serialize>(dir: &Path, file: &str, record: &T) {
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
            logged_error: last_logged_error(&target_dir(root, t)),
            // 스위치를 끈 대상도 도는 배포는 보인다(그동안 검증이 멈춘다).
            deploy: read_record::<DeployRecord>(&target_dir(root, t), "deploy.json")
                .filter(|d| t.deploy || d.state == DeployState::Running),
            ..Default::default()
        })
        .collect()
}

/// `runs.jsonl` 의 마지막 검증 줄이 검증을 못 한 이유면 그 이유. 배포 줄(`deployed`·`deploy-failed`)은 건너뛴다.
fn last_logged_error(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join("runs.jsonl")).ok()?;
    let run = text
        .lines()
        .rev()
        .filter_map(|line| serde_json::from_str::<VerifyRun>(line).ok())
        .find(|run| {
            !matches!(
                run.event,
                VerifyRunEvent::Deployed | VerifyRunEvent::DeployFailed
            )
        })?;
    (run.event == VerifyRunEvent::Error)
        .then_some(run.error)
        .flatten()
}

/// 대상 하나의 상태를 고친다(없으면 만든다) — 검증과 배포가 서로의 칸을 지우지 않게.
fn update_status(
    state: &ServerState,
    target: &VerifyTarget,
    f: impl FnOnce(&mut VerifyTargetStatus),
) {
    let mut all = state.verify();
    let at = match all
        .iter()
        .position(|s| s.board == target.board && s.branch == target.branch)
    {
        Some(at) => at,
        None => {
            all.push(VerifyTargetStatus {
                board: target.board.clone(),
                branch: target.branch.clone(),
                ..Default::default()
            });
            all.len() - 1
        }
    };
    f(&mut all[at]);
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

/// 프로세스의 시작 시각(`ps -o lstart=`) — pid 가 재사용됐는지 가르는 열쇠. 프로세스가 없으면 `None`. 글자가 로캘·시간대를
/// 따르므로 둘을 고정한다 — 적은 데몬과 견주는 데몬(교체된 새 데몬)의 환경이 달라도 같은 프로세스면 같은 값이 나오게.
async fn started_at(pid: i32) -> Option<String> {
    let out = tokio::process::Command::new("ps")
        .args(["-o", "lstart=", "-p", &pid.to_string()])
        .env("LC_ALL", "C")
        .env("TZ", "UTC0")
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

/// 로그는 대상마다 최근 몇 개만 — 검증 로그 `KEEP_LOGS` 개, 배포 로그(`deploy-*.log`, `deploy` 가 true) `KEEP_DEPLOY_LOGS` 개.
fn prune_logs(dir: &Path, deploy: bool) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let keep = if deploy { KEEP_DEPLOY_LOGS } else { KEEP_LOGS };
    let mut logs: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("log"))
        .filter(|e| e.file_name().to_string_lossy().starts_with("deploy-") == deploy)
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    logs.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
    for (_, path) in logs.into_iter().skip(keep) {
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
    verify_target_with(state, runner, notifier, root, target, None).await;
}

/// `verify_target` 에 배포의 손을 더한 것 — 데몬이 배포하는 대상이면 세션 알림이 세션에게 배포하지 말라고 한다.
pub async fn verify_target_with(
    state: &Arc<ServerState>,
    runner: &Runner,
    notifier: &VerifyNotifier,
    root: &Path,
    target: &VerifyTarget,
    deploy: Option<&DeployEnv>,
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
        update_status(state, target, |s| {
            s.record = record;
            s.logged_error = error.clone();
            s.error = error;
        });
    };
    let last = read_record(&dir, "last.json");
    // 보드는 별칭까지 푼다(보드 key 를 바꿔도 설정이 끊기지 않게).
    let (repo, repo_name) = match state.store.get_board(&target.board) {
        Ok(Some(board)) => match board.path {
            Some(p) => (PathBuf::from(p), board.repo),
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
    if record.state == VerifyState::Passed {
        // 배포 대상 — 뒤 커밋이 빨강이어도 마지막 초록은 남는다.
        write_record(&dir, "passed.json", &record);
    }
    note_run(&dir, &VerifyRun::of(iso_now(), record.clone()));
    prune_logs(&dir, false);
    report(Some(record.clone()), None);
    // 구독한 세션에는 끝난 실행마다(통과도) — 배포를 맡은 세션이 폴링하지 않고 이것으로 움직인다. 데몬이 배포하는 대상이면
    // 세션에게 배포하지 말라고 한다.
    let daemon_deploys = deploy.is_some_and(|env| {
        deploy_gate(
            target.deploy,
            &target.branch,
            repo_name.as_deref(),
            env.main_mode(),
        )
        .is_ok()
    });
    if let Some((kind, text)) =
        rocky_core::verify::session_notice(finished_before.as_ref(), &record, daemon_deploys)
    {
        crate::server::notify_verify_subscriber(state, &record, kind, &text).await;
    }
    if let Some((title, body)) = notification(finished_before.as_ref(), &record) {
        // 제목이 이미 `rocky:` 로 시작한다.
        println!("{title} — {body}");
        notifier(title, body);
    }
}

/// 그 pid 가 살아 있나(`kill(pid, 0)` — `rocky update --main` 의 잠금 확인과 같다).
fn pid_alive(pid: u32) -> bool {
    // SAFETY: 시그널 0 은 보내지 않고 있는지만 본다 — 메모리를 건드리지 않는다.
    libc::pid_t::try_from(pid).is_ok_and(|pid| pid > 0 && unsafe { libc::kill(pid, 0) } == 0)
}

fn short(sha: &str) -> &str {
    sha.get(..7).unwrap_or(sha)
}

/// 데몬이 띄운 배포가 아직 도나 — pid 와 그 시작 시각으로(재사용된 pid 를 도는 것으로 보지 않는다).
async fn deploy_running(env: &DeployEnv, record: &DeployRecord) -> bool {
    let Some(pid) = record.pid.filter(|_| record.state == DeployState::Running) else {
        return false;
    };
    if env.exited(pid) {
        return false;
    }
    let Ok(pid_i) = i32::try_from(pid) else {
        return false;
    };
    match started_at(pid_i).await {
        Some(now) => record.pid_started.as_deref().is_none_or(|at| at == now),
        None => false,
    }
}

/// `<rocky> update --main <sha>` 를 데몬과 떨어진 프로세스 그룹으로 띄운다 — update 가 이 데몬을 교체해도 끝까지 간다
/// (launchd 는 잡을 내릴 때 잡의 그룹만 정리한다, rc 서버와 같다). `--yes` 는 넘기지 않는다. 출력은 로그 파일로. 끝나면
/// 종료 코드를 남기고 검증 잡을 깨운다(이 데몬이 그때까지 살아 있으면).
fn spawn_update(
    env: &DeployEnv,
    sha: &str,
    log: &Path,
    state: &Arc<ServerState>,
) -> Result<u32, String> {
    use std::os::unix::fs::OpenOptionsExt;
    use std::os::unix::process::CommandExt;
    let open_err = |e: std::io::Error| format!("로그 파일({}): {e}", log.display());
    let out = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(log)
        .map_err(open_err)?;
    let err = out.try_clone().map_err(open_err)?;
    let mut child = std::process::Command::new(&env.rocky)
        .args(["update", "--main", sha])
        .current_dir(&env.data_root)
        .stdin(Stdio::null())
        .stdout(Stdio::from(out))
        .stderr(Stdio::from(err))
        // 데몬의 launchd 표식을 물려주지 않는다 — 자식이 자기를 launchd 잡으로 오판한다.
        .env_remove("XPC_SERVICE_NAME")
        .process_group(0)
        .spawn()
        .map_err(|e| format!("{} 를 실행하지 못했다: {e}", env.rocky.display()))?;
    let pid = child.id();
    let exits = env.exits.clone();
    let state = state.clone();
    std::thread::spawn(move || {
        let code = child.wait().ok().and_then(|s| s.code());
        if let Ok(mut exits) = exits.lock() {
            exits.insert(pid, code);
        }
        state.verify_wake.notify_one();
    });
    Ok(pid)
}

/// 끝난 배포를 남긴다 — `deploy.json`, 이력(`runs.jsonl`), 상태, 실패면 배너.
fn finish_deploy(
    state: &ServerState,
    notifier: &VerifyNotifier,
    dir: &Path,
    target: &VerifyTarget,
    record: DeployRecord,
) {
    write_record(dir, "deploy.json", &record);
    note_run(dir, &VerifyRun::deploy(iso_now(), record.clone()));
    prune_logs(dir, true);
    match deploy_notification(&target.board, &record) {
        Some((title, body)) => {
            println!("{title} — {body}");
            notifier(title, body);
        }
        None => println!(
            "rocky: verify {} {} 배포 {} 끝",
            target.board,
            target.branch,
            short(&record.sha)
        ),
    }
    update_status(state, target, |s| s.deploy = Some(record));
}

/// 끝난 배포를 판정한다(도는 것은 그대로). 아직 도는 배포가 있으면 true — 그동안 검증 잡은 새 실행을 시작하지 않는다
/// (update 가 데몬을 교체하면서 끊긴다). 스위치를 끈 대상도 본다 — 이미 띄운 update 는 스위치와 상관없이 데몬을 교체한다.
/// 한도(`DEPLOY_TIMEOUT_MINUTES`)를 넘긴 배포는 실패로 끝내고 더 기다리지 않는다(프로세스는 그대로).
async fn settle_deploys(
    state: &Arc<ServerState>,
    notifier: &VerifyNotifier,
    root: &Path,
    cfg: &VerifyConfig,
    env: &DeployEnv,
) -> bool {
    let mut running = false;
    for target in &cfg.targets {
        let dir = target_dir(root, target);
        let Some(record) = read_record::<DeployRecord>(&dir, "deploy.json") else {
            continue;
        };
        if record.state != DeployState::Running {
            continue;
        }
        if deploy_running(env, &record).await {
            match deploy_timed_out(&record, chrono::Utc::now()) {
                Some(given_up) => finish_deploy(state, notifier, &dir, target, given_up),
                None => running = true,
            }
            continue;
        }
        // 종료 코드는 그 update 를 띄운 데몬만 안다 — 교체된 새 데몬은 `main/.finished` 만 본다.
        let exit = record.pid.and_then(|pid| env.take_exit(pid)).flatten();
        let finished = env.main_file(".finished");
        if let Some(done) = settle_deploy(&record, finished.as_deref(), exit, iso_now()) {
            finish_deploy(state, notifier, &dir, target, done);
        }
    }
    running
}

/// `base` 가 `sha` 의 조상인가 — 보드 레포에서(검증이 그 커밋을 fetch 해 두었다). git 이 답하지 못하면(없음·시간 초과·모르는
/// 커밋) 그 이유. 러너는 띄우지 못함·시간 초과도 종료 코드 1 로 돌려주므로 "조상 아님"은 git 이 말없이 1 로 끝난 것만이다.
async fn is_ancestor(runner: &Runner, repo: &str, base: &str, sha: &str) -> Result<bool, String> {
    let argv = [
        "git",
        "-c",
        "core.hooksPath=/dev/null",
        "-C",
        repo,
        "merge-base",
        "--is-ancestor",
        base,
        sha,
    ]
    .map(String::from)
    .to_vec();
    let out = runner(argv, String::new(), GIT_TIMEOUT).await;
    match out.code {
        0 => Ok(true),
        1 if out.stderr.trim().is_empty() => Ok(false),
        code => Err(format!(
            "{repo} 에서 git merge-base 종료 코드 {code}: {}",
            out.stderr.lines().next().unwrap_or("").trim()
        )),
    }
}

/// 배포를 켠 대상 하나 — 통과한 커밋을 띄울지 판정하고, 띄웠으면 true. `deploying` 은 이 기기에서 도는 배포가 있는가.
async fn deploy_target(
    state: &Arc<ServerState>,
    runner: &Runner,
    notifier: &VerifyNotifier,
    root: &Path,
    target: &VerifyTarget,
    env: &DeployEnv,
    deploying: bool,
) -> bool {
    let dir = target_dir(root, target);
    let board = state.store.get_board(&target.board).ok().flatten();
    let passed: Option<VerifyRecord> = read_record(&dir, "passed.json").or_else(|| {
        read_record::<VerifyRecord>(&dir, "finished.json")
            .filter(|r| r.state == VerifyState::Passed)
    });
    let last: Option<DeployRecord> = read_record(&dir, "deploy.json");
    let finished_build = env.main_file(".finished");
    let built = env.main_file(".sha");
    let mut input = DeployInput {
        enabled: target.deploy,
        branch: &target.branch,
        repo: board.as_ref().and_then(|b| b.repo.as_deref()),
        main_mode: env.main_mode(),
        running: deploying,
        busy: false,
        passed: passed.as_ref(),
        last_deploy: last.as_ref(),
        finished_build: finished_build.as_deref(),
        built: built.as_deref(),
        ancestor: Some(true),
        ancestry_error: None,
        window: target.deploy_window,
        local_time: Some((env.clock)()),
    };
    // 조상 여부(git)와 잠금은 띄울 수 있을 때만 본다 — 평소(이미 깔림·이미 시도함)에는 git 을 부르지 않는다. 그 앞에서
    // 건너뛰는 판정은 둘과 상관없다.
    let mut ancestry_error = None;
    let plan = match deploy_plan(&input) {
        DeployPlan::Start(sha) => {
            input.ancestor = match (
                deploy_base(input.finished_build, input.built),
                board.as_ref().and_then(|b| b.path.as_deref()),
            ) {
                (Some(base), Some(repo)) if base != sha => {
                    match is_ancestor(runner, repo, base, &sha).await {
                        Ok(yes) => Some(yes),
                        Err(why) => {
                            ancestry_error = Some(why);
                            None
                        }
                    }
                }
                (Some(base), None) if base != sha => {
                    ancestry_error = Some("보드에 path 가 없다".to_string());
                    None
                }
                _ => None,
            };
            input.ancestry_error = ancestry_error.as_deref();
            input.busy = env.busy();
            deploy_plan(&input)
        }
        skip => skip,
    };
    match plan {
        DeployPlan::Start(sha) => start_deploy(state, notifier, &dir, target, env, sha).await,
        DeployPlan::Skip(why) => {
            let message = why.message();
            let logged = state
                .verify()
                .into_iter()
                .find(|s| s.board == target.board && s.branch == target.branch)
                .and_then(|s| s.logged_deploy);
            if logged.as_deref() != Some(message.as_str()) {
                println!(
                    "rocky: verify {} {} 배포 안 함 — {message}",
                    target.board, target.branch
                );
            }
            update_status(state, target, |s| {
                s.deploy = last;
                s.deploy_note = why.note();
                s.logged_deploy = Some(message);
            });
            false
        }
    }
}

/// 배포를 띄우고 pid 와 함께 기록한다. 띄우지 못하면 그 자리에서 실패로 남긴다(배너). 띄웠으면 true.
async fn start_deploy(
    state: &Arc<ServerState>,
    notifier: &VerifyNotifier,
    dir: &Path,
    target: &VerifyTarget,
    env: &DeployEnv,
    sha: String,
) -> bool {
    let _ = std::fs::create_dir_all(dir);
    let log = dir.join(format!("deploy-{}.log", &sha[..12]));
    let started = iso_now();
    match spawn_update(env, &sha, &log, state) {
        Ok(pid) => {
            let pid_started = match i32::try_from(pid) {
                Ok(pid) => started_at(pid).await,
                Err(_) => None,
            };
            println!(
                "rocky: verify {} {} 배포 시작 — {} update --main {} (pid {pid}, 로그 {})",
                target.board,
                target.branch,
                env.rocky.display(),
                short(&sha),
                log.display()
            );
            let record = DeployRecord {
                sha,
                state: DeployState::Running,
                pid: Some(pid),
                pid_started,
                started_at: started,
                finished_at: None,
                reason: None,
                log: log.to_string_lossy().to_string(),
            };
            write_record(dir, "deploy.json", &record);
            update_status(state, target, |s| {
                s.deploy = Some(record);
                s.deploy_note = None;
                s.logged_deploy = None;
            });
            true
        }
        Err(reason) => {
            let record = DeployRecord {
                sha,
                state: DeployState::Failed,
                pid: None,
                pid_started: None,
                started_at: started,
                finished_at: Some(iso_now()),
                reason: Some(reason),
                log: log.to_string_lossy().to_string(),
            };
            finish_deploy(state, notifier, dir, target, record);
            false
        }
    }
}

/// 대상들을 차례로 — 하나가 끝나야 다음. 배포를 켠 대상은 검증 뒤에 배포를 판정한다. 배포가 돌면(update 가 데몬을
/// 교체한다) 새 검증을 시작하지 않는다.
pub async fn tick(
    state: &Arc<ServerState>,
    runner: &Runner,
    notifier: &VerifyNotifier,
    root: &Path,
    cfg: &VerifyConfig,
    deploy: &DeployEnv,
) {
    let mut deploying = settle_deploys(state, notifier, root, cfg, deploy).await;
    for target in &cfg.targets {
        if !deploying {
            verify_target_with(state, runner, notifier, root, target, Some(deploy)).await;
        }
        if target.deploy
            && deploy_target(state, runner, notifier, root, target, deploy, deploying).await
        {
            deploying = true;
        }
    }
}

/// 기동 뒤 `first_after` 지나 처음, 이후 바퀴가 끝날 때마다 `interval_seconds` 쉬고 다시. 다시 돌리기 요청이 오면
/// 쉬는 중이라도(기동 직후의 첫 대기 포함) 바로 다음 바퀴로 간다(`verify_wake`). 데몬이 띄운 배포가 끝나도 깨운다.
pub fn spawn_verifier(
    state: Arc<ServerState>,
    runner: Runner,
    notifier: VerifyNotifier,
    root: PathBuf,
    cfg: VerifyConfig,
    deploy: DeployEnv,
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
            tick(&state, &runner, &notifier, &root, &cfg, &deploy).await;
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(cfg.interval_seconds())) => {}
                _ = state.verify_wake.notified() => {}
            }
        }
    });
}
