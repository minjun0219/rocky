//! `claude rc` 서버 현황 프로브 — `ps` 한 번, `lsof` 한 번, `claude auth status`, `agy`. 판정은 `rocky_core::rc`.
//!
//! `claude rc` 는 아직 보기만 한다. `agy remote-control` 은 켜고 끌 수 있다(`agy_control`) — agy 가 데몬을 launchd
//! 잡으로 올리고 명령 자체는 곧 끝나므로 기존 러너(timeout + kill_on_drop)로 충분하다.

use std::sync::Arc;
use std::time::{Duration, Instant};

use rocky_core::config::RcConfig;
use rocky_core::rc::{self, AgyAction, AgyStatus, LiveServer, RcStatus};

use crate::runner::{BoxFut, CmdOutput, Runner};

const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
/// 화면 폴링이 `ps`·`lsof` 를 매번 부르지 않게.
pub const RC_CACHE_TTL: Duration = Duration::from_secs(5);

/// `agy remote-control` 켜기·끄기 명령 한도 — launchd 등록이 끼어 상태 조회보다 오래 걸린다.
const AGY_CONTROL_TIMEOUT: Duration = Duration::from_secs(60);
/// 켜고 끈 뒤 상태가 자리 잡기를 기다리는 횟수·간격 — 끈 직후엔 launchd 의 중간값이 잠깐 보인다.
const AGY_SETTLE_TRIES: usize = 6;
const AGY_SETTLE_GAP: Duration = Duration::from_millis(500);

pub type RcProvider = Arc<dyn Fn() -> BoxFut<RcStatus> + Send + Sync>;
/// agy 를 켜고 끈 뒤 새로 잰 현황. 명령이 실패하면 그 사유.
pub type AgyControl = Arc<dyn Fn(AgyAction) -> BoxFut<Result<RcStatus, String>> + Send + Sync>;

/// 프로브 명령 실패를 진단할 수 있게 — 무엇을 물었고(`what`) 어떻게 끝났나(종료 코드·stderr, 비었으면 그렇다고).
fn probe_failure(what: &str, out: &CmdOutput) -> String {
    let stderr = out.stderr.trim();
    let stderr = if stderr.is_empty() {
        "stderr 없음"
    } else {
        stderr
    };
    format!("{what} 실패(종료 코드 {}): {stderr}", out.code)
}

fn argv(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

/// `agy remote-control status` 결과 → 현황. 실행 파일이 없으면 spawn 이 실패해 출력이 비고 실패 코드다 — 설치 안 됨.
/// (설치돼 있어도 timeout 이면 같은 모양이라 구분하지 못한다.)
fn agy_status(out: &CmdOutput) -> Option<AgyStatus> {
    (out.ok() || !out.stdout.is_empty()).then(|| rc::parse_agy_status(&out.stdout))
}

fn agy_status_argv() -> Vec<String> {
    argv(&["agy", "remote-control", "status"])
}

/// 한 번 잰다. `rc` 블록이 없으면 `claude rc` 쪽은 돌리지 않고 agy 만 본다 — agy 줄은 설치 여부를 따른다.
pub async fn probe(runner: &Runner, config: Option<&RcConfig>, home: &str) -> RcStatus {
    let Some(config) = config else {
        let agy = runner(agy_status_argv(), String::new(), PROBE_TIMEOUT).await;
        return RcStatus {
            antigravity: agy_status(&agy),
            ..RcStatus::unconfigured()
        };
    };
    let targets = rc::resolve_targets(config, home);

    // 서로 기다릴 이유가 없는 셋은 같이 돌린다 — 하나가 timeout 까지 걸려도 요청이 그만큼만 늦다.
    let (ps, auth, agy) = tokio::join!(
        runner(
            argv(&["ps", "-axww", "-o", "pid=,ppid=,etime=,args="]),
            String::new(),
            PROBE_TIMEOUT,
        ),
        runner(
            argv(&["claude", "auth", "status", "--json"]),
            String::new(),
            PROBE_TIMEOUT,
        ),
        runner(agy_status_argv(), String::new(), PROBE_TIMEOUT),
    );
    let mut probe_error = (!ps.ok()).then(|| probe_failure("ps", &ps));
    let rows = rc::parse_ps(&ps.stdout);
    let servers = rc::servers(&rows);
    let live = if servers.is_empty() {
        Vec::new()
    } else {
        let pids = servers
            .iter()
            .map(|s| s.pid.to_string())
            .collect::<Vec<_>>()
            .join(",");
        // pid 마다 부르지 않는다 — 서버가 열 개면 lsof 열 번이다.
        let lsof = runner(
            argv(&["lsof", "-a", "-d", "cwd", "-p", &pids, "-F", "pn"]),
            String::new(),
            PROBE_TIMEOUT,
        )
        .await;
        // 그 사이 끝난 pid 가 하나라도 있으면 lsof 는 1 로 끝나되 나머지는 낸다 — 출력이 비었을 때만 실패다.
        if !lsof.ok() && lsof.stdout.trim().is_empty() {
            probe_error.get_or_insert_with(|| probe_failure(&format!("lsof -p {pids}"), &lsof));
        }
        let cwd = rc::parse_lsof_cwd(&lsof.stdout);
        servers
            .iter()
            .filter_map(|s| {
                Some(LiveServer {
                    pid: s.pid,
                    dir: cwd.get(&s.pid)?.clone(),
                    uptime_secs: s.uptime_secs,
                    sessions: rc::session_count(&rows, s.pid),
                })
            })
            .collect()
    };
    let (servers, strays) = rc::build_rows(&targets, &live);
    let antigravity = agy_status(&agy);

    RcStatus {
        configured: true,
        servers,
        strays,
        auth: rc::parse_auth_status(&auth.stdout),
        antigravity,
        probe_error,
        supervise: None,
        nightly: None,
    }
}

/// git 명령 하나의 한도 — 최근 활동은 사람이 부를 때만 재니 넉넉히.
const GIT_TIMEOUT: Duration = Duration::from_secs(5);

/// 대상 폴더의 git 사실(최근 활동). 실패한 명령은 빈 출력으로 읽는다 — 모르는 것은 활성으로 친다(`rc::is_active`).
/// `--no-optional-locks` — `status` 가 index 잠금을 잡아 사람 · 에이전트의 동시 git 명령과 부딪히지 않게.
pub async fn git_activity(runner: Runner, dir: String, now: i64) -> rc::Activity {
    let git = |args: &[&str]| {
        let mut v = vec!["git", "--no-optional-locks", "-C", dir.as_str()];
        v.extend_from_slice(args);
        runner(argv(&v), String::new(), GIT_TIMEOUT)
    };
    // 저장소인지는 출력이 아니라 종료 코드로 가른다.
    if !git(&["rev-parse", "--git-dir"]).await.ok() {
        return rc::parse_git_facts(false, "", "", "", "", now);
    }
    let (status, head, origin, log) = tokio::join!(
        git(&["status", "--porcelain", "-uno"]),
        git(&["symbolic-ref", "--quiet", "--short", "HEAD"]),
        git(&[
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD"
        ]),
        git(&["log", "-1", "--format=%ct%n%s"]),
    );
    let out = |o: CmdOutput| if o.ok() { o.stdout } else { String::new() };
    // `origin/HEAD` 는 clone 할 때만 생긴다(`git init` 뒤 push 한 레포에는 없다) — 없으면 main, master 순으로 짐작한다.
    let mut default_branch = out(origin);
    if default_branch.trim().is_empty() {
        for guess in ["main", "master"] {
            let r = format!("refs/heads/{guess}");
            if git(&["rev-parse", "--verify", "--quiet", &r]).await.ok() {
                default_branch = guess.to_string();
                break;
            }
        }
    }
    rc::parse_git_facts(
        true,
        &out(status),
        &out(head),
        &default_branch,
        &out(log),
        now,
    )
}

/// `agy remote-control start|stop` 을 한 번 돌린다. 실패면 무엇을 돌렸고 어떻게 끝났는지를 낸다.
pub async fn agy_control(runner: &Runner, action: AgyAction) -> Result<(), String> {
    let argv = action.argv();
    let out = runner(argv.clone(), String::new(), AGY_CONTROL_TIMEOUT).await;
    if out.ok() {
        Ok(())
    } else {
        Err(probe_failure(&argv.join(" "), &out))
    }
}

/// TTL 캐시를 씌운 조회기. 잠금을 프로브가 끝날 때까지 쥔다 — 만료 직후 겹친 요청은 진행 중인 프로브를
/// 기다렸다 그 결과를 받는다(요청마다 `ps`·`lsof` 를 새로 띄우지 않고, 늦게 끝난 옛 프로브가 새 결과를 덮지 않는다).
pub fn cached_rc(
    runner: Runner,
    config: Option<RcConfig>,
    home: String,
    ttl: Duration,
) -> RcProvider {
    rc_handles(runner, config, home, ttl).0
}

/// 조회기와 agy 손잡이를 **같은 캐시**로 묶는다. 손잡이는 캐시 잠금을 쥔 채 명령을 돌리고 다시 재서 넣는다 —
/// 그 사이 조회는 기다렸다 새 값을 받고, 켜기 직전의 옛 값이 5초 동안 남지 않는다.
pub fn rc_handles(
    runner: Runner,
    config: Option<RcConfig>,
    home: String,
    ttl: Duration,
) -> (RcProvider, AgyControl) {
    let cache: Arc<tokio::sync::Mutex<Option<(Instant, RcStatus)>>> =
        Arc::new(tokio::sync::Mutex::new(None));
    let config = Arc::new(config);
    let home = Arc::new(home);
    let provider: RcProvider = {
        let (runner, config, home, cache) =
            (runner.clone(), config.clone(), home.clone(), cache.clone());
        Arc::new(move || {
            let (runner, config, home, cache) =
                (runner.clone(), config.clone(), home.clone(), cache.clone());
            Box::pin(async move {
                let mut slot = cache.lock().await;
                if let Some((at, status)) = slot.as_ref() {
                    if at.elapsed() < ttl {
                        return status.clone();
                    }
                }
                let status = probe(&runner, config.as_ref().as_ref(), &home).await;
                *slot = Some((Instant::now(), status.clone()));
                status
            })
        })
    };
    let control: AgyControl = Arc::new(move |action| {
        let (runner, config, home, cache) =
            (runner.clone(), config.clone(), home.clone(), cache.clone());
        Box::pin(async move {
            let mut slot = cache.lock().await;
            let done = agy_control(&runner, action).await;
            // 실패해도 다시 잰다 — 반쯤 바뀐 상태(등록만 됨 등)가 화면에 남지 않게. 성공했으면 자리 잡을 때까지
            // 몇 번 더 잰다(못 잡아도 마지막 값을 낸다).
            let mut status = probe(&runner, config.as_ref().as_ref(), &home).await;
            if done.is_ok() {
                for _ in 1..AGY_SETTLE_TRIES {
                    if action.settled(status.antigravity.as_ref()) {
                        break;
                    }
                    tokio::time::sleep(AGY_SETTLE_GAP).await;
                    status = probe(&runner, config.as_ref().as_ref(), &home).await;
                }
            }
            *slot = Some((Instant::now(), status.clone()));
            done.map(|()| status)
        })
    });
    (provider, control)
}

// ── 띄우기 · 재시작 ───────────────────────────────────────────────────────────
// 서버는 **새 프로세스 그룹**으로 띄우고 핸들을 놓는다(`kill_on_drop` 없음). launchd 가 데몬 잡을 내려도
// (bootout) 새 그룹의 자식은 정리하지 않는다 — 2026-10-05 실측. 데몬이 먼저 내려가면 서버는 PPID 1 로 넘어간다.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

mod nightly;
pub use nightly::{spawn_rc_nightly, NIGHTLY_CHECK};

use rocky_core::rc::{LaunchMode, RcAction, RcResult, Registration, Target};

/// (argv, 작업 폴더, stdout 파일, stderr 파일) → pid.
pub type SpawnDetached =
    Arc<dyn Fn(&[String], &Path, &Path, &Path) -> std::io::Result<u32> + Send + Sync>;

/// 프로세스를 다루는 손 — 테스트가 가짜를 넣는다.
#[derive(Clone)]
pub struct RcOps {
    pub spawn: SpawnDetached,
    /// `kill(pid, sig)` 가 성공했나 — `sig == 0` 이면 살아 있나.
    pub signal: Arc<dyn Fn(u32, i32) -> bool + Send + Sync>,
    pub sleep: Arc<dyn Fn(Duration) -> BoxFut<()> + Send + Sync>,
    /// 이 기기의 현지 시각 — 야간의 마감(07:00)을 잰다. 테스트는 `sleep` 과 함께 흐르는 가짜 시계를 넣는다.
    pub now: Arc<dyn Fn() -> chrono::NaiveDateTime + Send + Sync>,
}

pub fn default_ops() -> RcOps {
    RcOps {
        spawn: Arc::new(|argv, dir, out, err| {
            use std::os::unix::process::CommandExt;
            let (program, args) = argv
                .split_first()
                .ok_or_else(|| std::io::Error::other("빈 argv"))?;
            let mut child = std::process::Command::new(program)
                .args(args)
                .current_dir(dir)
                .stdin(std::process::Stdio::null())
                .stdout(std::fs::File::create(out)?)
                .stderr(std::fs::File::create(err)?)
                // 데몬의 launchd 표식을 물려주지 않는다 — 자식이 자기를 launchd 잡으로 오판한다.
                .env_remove("XPC_SERVICE_NAME")
                .process_group(0)
                .spawn()?;
            let pid = child.id();
            // 좀비만 거둔다 — 기다리는 것 말고는 아무것도 하지 않는다(놓는다).
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            Ok(pid)
        }),
        signal: Arc::new(|pid, sig| {
            let Ok(pid) = libc::pid_t::try_from(pid) else {
                return false;
            };
            // SAFETY: 우리가 띄웠거나 `ps` 로 서버라고 확인한 pid 에 신호를 보낸다 — 패턴으로 고르지 않는다.
            unsafe { libc::kill(pid, sig) == 0 }
        }),
        sleep: Arc::new(|d| Box::pin(tokio::time::sleep(d))),
        now: Arc::new(|| chrono::Local::now().naive_local()),
    }
}

/// 기동 버전을 재는 `claude --version` 한도 — 새 바이너리의 첫 실행은 Gatekeeper 검사로 수십 초 멎는다(실측 8~35초).
const VERSION_TIMEOUT: Duration = Duration::from_secs(40);

/// 기록 파일을 지운다 — 처음부터 없었으면 지운 것이다.
fn forget(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// 한 번의 기동 시도를 어떻게 할지 — 사람이 부른 것과 야간이 다르다.
#[derive(Clone, Copy)]
struct Policy<'a> {
    /// `already served` 를 만났을 때 쉬는 간격들.
    backoff: &'a [Duration],
    /// 이미 잰 설치 버전(기동 기록에 쓴다) — None 이면 잰다.
    known_version: Option<&'a str>,
    /// 막 대화하는 턴을 기다리는 상한 — 0 이면 기다리지 않고 손대지 않는다(야간은 이미 오래 조용한 것만 골랐고, 마감이 있다).
    turn_wait: Duration,
}

/// 사람이 부른 띄우기 · 재시작.
const DAY: Policy<'static> = Policy {
    backoff: &rc::REGISTRATION_BACKOFF,
    known_version: None,
    turn_wait: rc::TURN_WAIT,
};

/// 띄우기인가 재시작인가.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RcCommand {
    Start,
    Restart {
        fresh: bool,
    },
    /// 사람이 고른 세션(claude.ai 쪽 id — `rc::valid_session_id`)으로 못 박아 다시 띄운다. 꺼져 있으면 그 세션으로 띄운다.
    Pin(String),
    /// 감시가 꺼진 대상을 정한 방식으로 띄운다 — 그새 누가 띄웠으면 그대로 둔다(실패가 아니다).
    Revive(LaunchMode),
}

/// 요청을 받지 못한 이유 — 라우트가 상태 코드로 바꾼다.
#[derive(Debug, PartialEq, Eq)]
pub enum RcRefusal {
    /// 이 기기에서 rc 가 꺼져 있거나 그런 라벨이 없다.
    NotFound(String),
    /// 같은 대상에 이미 무엇이 진행 중이다.
    Busy(String),
    /// 같은 라벨의 대상이 둘이라 고를 수 없다.
    Ambiguous(String),
}

pub struct RcController {
    config: Option<RcConfig>,
    home: String,
    log_dir: PathBuf,
    runner: Runner,
    ops: RcOps,
    busy: std::sync::Mutex<HashMap<String, RcAction>>,
    last: std::sync::Mutex<HashMap<String, RcResult>>,
    supervise: std::sync::Mutex<SuperviseState>,
    nightly: std::sync::Mutex<nightly::NightlyRun>,
}

/// 감시의 메모리 상태 — 켜졌나, 마지막 바퀴, 자격 기록(파일과 같은 값), 대상별 연속 실패와 다음 시도 시각(unix 초).
#[derive(Default)]
struct SuperviseState {
    enabled: bool,
    last_tick: Option<String>,
    mark: Option<rc::AuthMark>,
    failures: HashMap<String, (u32, i64)>,
}

/// 사람에게 알리는 함수 — (제목, 본문). 기본은 macOS 배너(`osascript`), 테스트는 붙잡는다.
pub type RcNotifier = Arc<dyn Fn(String, String) + Send + Sync>;

impl RcController {
    pub fn new(
        config: Option<RcConfig>,
        home: String,
        log_dir: PathBuf,
        runner: Runner,
        ops: RcOps,
    ) -> Self {
        RcController {
            config,
            home,
            log_dir,
            runner,
            ops,
            busy: std::sync::Mutex::new(HashMap::new()),
            last: std::sync::Mutex::new(HashMap::new()),
            supervise: std::sync::Mutex::new(SuperviseState::default()),
            nightly: std::sync::Mutex::new(nightly::NightlyRun::default()),
        }
    }

    /// 요청을 받는다 — 대상을 찾고 진행 중 표시를 건다. 실제 일은 `run` 이 백그라운드에서 한다.
    pub fn begin(&self, label: &str, command: RcCommand) -> Result<Target, RcRefusal> {
        let Some(config) = &self.config else {
            return Err(RcRefusal::NotFound("이 기기에서는 rc 가 꺼져 있다".into()));
        };
        let matches: Vec<Target> = rc::resolve_targets(config, &self.home)
            .into_iter()
            .filter(|t| t.label == label)
            .collect();
        let target = match matches.as_slice() {
            [] => return Err(RcRefusal::NotFound(format!("rc 대상이 아니다: {label}"))),
            [one] => one.clone(),
            _ => {
                return Err(RcRefusal::Ambiguous(format!(
                    "같은 라벨의 대상이 둘 이상이다: {label} — rocky.json 의 rc 목록을 고친다"
                )))
            }
        };
        let mut busy = self.busy.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(action) = busy.get(label) {
            return Err(RcRefusal::Busy(format!(
                "{label} 은(는) 이미 진행 중이다({action:?})"
            )));
        }
        let action = match command {
            RcCommand::Start | RcCommand::Revive(_) => RcAction::Starting,
            RcCommand::Restart { .. } | RcCommand::Pin(_) => RcAction::Restarting,
        };
        busy.insert(label.to_string(), action);
        Ok(target)
    }

    /// 현황에 진행 중 표시와 마지막 결과를 얹는다.
    pub fn decorate(&self, status: &mut RcStatus) {
        let busy = self.busy.lock().unwrap_or_else(|e| e.into_inner());
        let last = self.last.lock().unwrap_or_else(|e| e.into_inner());
        for row in &mut status.servers {
            row.action = busy.get(&row.label).copied();
            row.last_result = last.get(&row.label).cloned();
        }
        drop((busy, last));
        // 구버전 — 기동 버전 기록이 설치 버전과 다르다. 설치 버전은 설치 경로로 읽는다(화면 폴링마다 claude 를 띄우지 않게).
        if let Some(installed) = nightly::installed_version_fs(&self.home) {
            for row in &mut status.servers {
                row.stale = row.running
                    && std::fs::read_to_string(self.log_path(&row.label, "version"))
                        .is_ok_and(|r| !r.trim().is_empty() && r.trim() != installed);
            }
        }
        status.nightly = self.nightly_info();
        let sup = self.supervise.lock().unwrap_or_else(|e| e.into_inner());
        if !sup.enabled {
            return;
        }
        let mark = sup.mark.clone().unwrap_or_default();
        let now = chrono::Utc::now().timestamp();
        for row in &mut status.servers {
            row.auth_suspect = row.running && rc::auth_suspect(row.uptime_secs, &mark, now);
        }
        status.supervise = Some(rc::SuperviseInfo {
            last_tick: sup.last_tick.clone(),
            logged_out: mark.still_out(),
        });
    }

    /// 현황 행마다 최근 활동(git)을 싣는다 — 폴더끼리도 같이 돈다(명령마다 5초 한도라 하나가 멎어도 전체가 그만큼만 늦다).
    pub async fn add_activity(&self, status: &mut RcStatus) {
        let now = chrono::Utc::now().timestamp();
        let handles: Vec<_> = status
            .servers
            .iter()
            .map(|r| tokio::spawn(git_activity(self.runner.clone(), r.dir.clone(), now)))
            .collect();
        for (row, handle) in status.servers.iter_mut().zip(handles) {
            row.activity = handle.await.ok();
        }
    }

    /// 감시를 켠다(`rc.supervise`) — 현황에 감시 상태와 자격 의심이 실린다. 루프는 `spawn_rc_supervisor` 가 돈다.
    pub fn enable_supervise(&self) {
        let mut sup = self.supervise.lock().unwrap_or_else(|e| e.into_inner());
        sup.enabled = true;
        if sup.mark.is_none() {
            sup.mark = Some(self.read_mark());
        }
    }

    fn mark_path(&self) -> PathBuf {
        self.log_dir.join("auth.json")
    }

    /// `rc/auth.json` — 데몬을 다시 띄워도 "끊겼다 돌아왔다" 를 알아채게 남긴다. 없거나 깨졌으면 빈 기록.
    fn read_mark(&self) -> rc::AuthMark {
        std::fs::read_to_string(self.mark_path())
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default()
    }

    fn write_mark(&self, mark: &rc::AuthMark) {
        let _ = std::fs::create_dir_all(&self.log_dir);
        if let Ok(raw) = serde_json::to_string(mark) {
            let _ = std::fs::write(self.mark_path(), raw);
        }
    }

    /// 감시 한 바퀴 — 캐시 없이 재고, 자격 관찰을 갱신해 끊김 · 회복을 한 번씩 알리고, 꺼진 고정 서버를 띄운다(연속 실패한
    /// 대상은 쉬는 동안 건너뛴다). 띄운 라벨을 돌려준다. 띄우기는 사람이 부르는 것과 같은 길(`begin` → `run`)이라 사람이
    /// 재시작 중인 대상은 `Busy` 로 자연히 건너뛴다.
    pub async fn supervise_tick(self: &Arc<Self>, notify: &RcNotifier) -> Vec<String> {
        let now = chrono::Utc::now().timestamp();
        let mut status = probe(&self.runner, self.config.as_ref(), &self.home).await;
        self.decorate(&mut status);

        let mark = {
            let mut sup = self.supervise.lock().unwrap_or_else(|e| e.into_inner());
            if sup.mark.is_none() {
                sup.mark = Some(self.read_mark());
            }
            sup.mark.clone().unwrap_or_default()
        };
        let (next, transition) = rc::next_auth_mark(status.auth, &mark, now);
        if next != mark {
            self.write_mark(&next);
            self.supervise
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .mark = Some(next.clone());
        }
        match transition {
            rc::AuthTransition::LoggedOut => {
                self.event("auth", "", serde_json::json!({ "state": "out" }));
                notify(
                    "rocky · rc 감시".into(),
                    "데몬 맥락의 claude 자격이 끊겼다 — 고정 서버를 되살리지 못한다. 셸이 로그인돼 있어도 launchd 쪽(키체인)은 따로다"
                        .into(),
                );
            }
            rc::AuthTransition::Recovered => {
                let suspects = status
                    .servers
                    .iter()
                    .filter(|s| s.running && rc::auth_suspect(s.uptime_secs, &next, now))
                    .count();
                self.event(
                    "auth",
                    "",
                    serde_json::json!({ "state": "in", "suspects": suspects }),
                );
                let tail = if suspects > 0 {
                    format!(" — 끊기기 전에 뜬 서버 {suspects}개는 다시 띄우기를 권한다")
                } else {
                    String::new()
                };
                notify(
                    "rocky · rc 감시".into(),
                    format!("데몬 맥락의 claude 자격이 돌아왔다{tail}"),
                );
            }
            rc::AuthTransition::None => {}
        }

        let marked = self.revive_marks();
        for label in rc::stale_revive_marks(&status, &marked) {
            let by = if status.servers.iter().any(|s| s.label == label) {
                "already-running"
            } else {
                "not-a-target"
            };
            self.clear_revive(&label, by);
        }
        let due: Vec<rc::Revive> = {
            let sup = self.supervise.lock().unwrap_or_else(|e| e.into_inner());
            rc::revive_candidates(&status, &marked)
                .into_iter()
                .filter(|revive| {
                    sup.failures
                        .get(&revive.label)
                        .is_none_or(|(_, next_at)| *next_at <= now)
                })
                .collect()
        };
        let mut started = Vec::new();
        let mut handles = Vec::new();
        for rc::Revive { label, mode } in due {
            let command = RcCommand::Revive(mode);
            let Ok(target) = self.begin(&label, command.clone()) else {
                continue;
            };
            let has_mark = marked.contains(&label);
            let reason = if has_mark {
                "revive-mark"
            } else {
                "pinned-down"
            };
            self.event(
                "supervise",
                &label,
                serde_json::json!({ "reason": reason, "mode": mode }),
            );
            started.push(label.clone());
            let this = self.clone();
            handles.push(tokio::spawn(async move {
                let result = this.run(target, command).await;
                this.record_attempt(&label, result.ok);
                if result.ok && has_mark {
                    this.clear_revive(&label, "started");
                }
            }));
        }
        for handle in handles {
            let _ = handle.await;
        }
        self.supervise
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .last_tick = Some(chrono::Utc::now().to_rfc3339());
        started
    }

    /// 감시가 띄운 결과 — 뜨면 쉬기를 지우고, 못 뜨면 연속 실패를 세어 다음 시도를 미룬다.
    fn record_attempt(&self, label: &str, ok: bool) {
        let mut sup = self.supervise.lock().unwrap_or_else(|e| e.into_inner());
        if ok {
            sup.failures.remove(label);
            return;
        }
        let failures = sup.failures.get(label).map_or(0, |(n, _)| *n) + 1;
        let next_at =
            chrono::Utc::now().timestamp() + rc::failure_backoff(failures).as_secs() as i64;
        sup.failures.insert(label.to_string(), (failures, next_at));
    }

    fn set_action(&self, label: &str, action: RcAction) {
        let mut busy = self.busy.lock().unwrap_or_else(|e| e.into_inner());
        busy.insert(label.to_string(), action);
    }

    /// `begin` 이 받은 일을 끝까지 한다 — 결과를 남기고 진행 중 표시를 푼다.
    pub async fn run(&self, target: Target, command: RcCommand) -> RcResult {
        let result = self.attempt(&target, command, DAY).await;
        self.release(&target.label);
        result
    }

    /// 진행 중 표시를 푼다.
    fn release(&self, label: &str) {
        self.busy
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(label);
    }

    /// 한 번 해 보고 결과를 남긴다 — 진행 중 표시는 그대로 둔다(야간은 못 뜬 대상을 회복이 끝날 때까지 쥔다).
    async fn attempt(&self, target: &Target, command: RcCommand, policy: Policy<'_>) -> RcResult {
        let started = std::time::Instant::now();
        let outcome = self.execute(target, command, policy).await;
        let (ok, message) = match outcome {
            Ok(m) => (true, m),
            Err(m) => (false, m),
        };
        let result = RcResult {
            ok,
            message: message.clone(),
            at: chrono::Utc::now().to_rfc3339(),
        };
        self.event(
            "result",
            &target.label,
            serde_json::json!({ "ok": ok, "message": message, "secs": started.elapsed().as_secs() }),
        );
        self.last
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(target.label.clone(), result.clone());
        result
    }

    async fn execute(
        &self,
        target: &Target,
        command: RcCommand,
        policy: Policy<'_>,
    ) -> Result<String, String> {
        // 기동 버전 기록에 쓸 설치 버전을 먼저 잰다 — 내리기 전에(새 바이너리의 첫 실행이 멎어도 서버가 꺼져 있는 시간이
        // 늘지 않게), 프로브보다 먼저(멎은 동안 현황이 묵지 않게). 지금 떠 있는지는 그 뒤 캐시 없이 다시 잰다 — 묵은 현황으로
        // 내리면 엉뚱한 pid 를 내린다. 야간은 update 뒤 이미 잰 값을 넘긴다 — 다시 재면 또 멎을 수 있고, 못 재면 기록을
        // 지워 다음 야간이 그 서버를 모르게 된다.
        let mut version = match policy.known_version {
            Some(v) => Some(v.to_string()),
            None => self.claude_version().await,
        };
        let status = probe(&self.runner, self.config.as_ref(), &self.home).await;
        if let Some(err) = status.probe_error {
            return Err(format!("현황을 못 읽어 손대지 않았다 — {err}"));
        }
        // 확실히 로그아웃일 때만 막는다(모르면 띄워 본다). 데몬이 launchd 로 돌면 셸과 자격을 읽는 곳이 다르다 —
        // launchd 맥락은 키체인을 읽어서, 셸은 로그인돼 있어도 여기는 로그아웃일 수 있다(2026-10-05 실측).
        // 그대로 띄우면 서버가 "You must be logged in" 으로 곧 내려가고, 재시작이면 떠 있던 서버까지 잃는다.
        if status.auth == rc::AuthState::Out {
            return Err(
                "데몬 맥락에서 claude 가 로그인돼 있지 않다 — 손대지 않았다. 데몬을 띄운 맥락(launchd 면 키체인)의 자격을 고친 뒤 다시"
                    .into(),
            );
        }
        let live = status
            .servers
            .iter()
            .find(|s| s.dir == target.dir)
            .filter(|s| s.running)
            .and_then(|s| s.pid.map(|pid| (pid, s.sessions)));
        let session = match &command {
            RcCommand::Pin(id) => Some(id.as_str()),
            _ => None,
        };
        // 꺼져 있던 것을 재시작하면 그냥 띄운다(세션을 골랐으면 그 세션으로).
        let down_mode = if session.is_some() {
            LaunchMode::Pin
        } else {
            rc::START_MODE
        };
        let mode = match (&command, live) {
            (RcCommand::Start, Some((pid, _))) => {
                return Err(format!("이미 떠 있다(pid {pid}) — 다시 띄우려면 재시작"))
            }
            (RcCommand::Start, None) => rc::START_MODE,
            (RcCommand::Revive(_), Some((pid, _))) => {
                return Ok(format!("이미 떠 있다(pid {pid}) — 그대로 둔다"))
            }
            (RcCommand::Revive(mode), None) => *mode,
            (RcCommand::Restart { .. } | RcCommand::Pin(_), Some((pid, sessions))) => {
                match self
                    .wait_turn(target, pid, sessions, policy.turn_wait)
                    .await?
                {
                    Some((pid, sessions, waited)) => {
                        // 기다리는 사이(최대 10분) 자동 업데이트로 설치본이 바뀌었을 수 있다 — 띄우기 직전 값으로 기록한다.
                        if waited && policy.known_version.is_none() {
                            version = self.claude_version().await;
                        }
                        let mode = match &command {
                            RcCommand::Restart { fresh } => {
                                rc::restart_mode(target.pinned, sessions > 0, *fresh)
                            }
                            _ => LaunchMode::Pin,
                        };
                        if !self.stop(&target.label, pid).await {
                            return Err(format!("내리지 못했다(pid {pid}) — 손대지 않고 둔다"));
                        }
                        (self.ops.sleep)(rc::RESTART_DELAY).await;
                        mode
                    }
                    // 기다리는 사이 서버가 내려갔다.
                    None => down_mode,
                }
            }
            (RcCommand::Restart { .. } | RcCommand::Pin(_), None) => down_mode,
        };
        let fresh = matches!(command, RcCommand::Restart { fresh: true });
        self.launch_until_up(
            target,
            mode,
            fresh,
            version.as_deref(),
            session,
            policy.backoff,
        )
        .await
    }

    /// 열린 세션이 막 대화하는 중이면 그 턴이 끝나길 기다린다 — `TURN_POLL` 마다 캐시 없이 다시 재며 `limit` 까지.
    /// 턴이 끝나면 그때의 (pid, 세션 수, 기다렸나), 기다리는 사이 서버가 내려갔으면 None, 끝내 안 끝나면(또는 `limit` 이
    /// 0 이면) 손대지 않고 Err. "끝남" 은 대화 기록이 2분 조용한 것이다 — 2분 넘게 아무것도 쓰지 않는 도구 호출 ·
    /// 서브에이전트는 못 본다(자기 서버 재시작은 그래서 CLI 가 막는다).
    async fn wait_turn(
        &self,
        target: &Target,
        pid: u32,
        sessions: usize,
        limit: Duration,
    ) -> Result<Option<(u32, usize, bool)>, String> {
        let busy = |sessions: usize| {
            rc::turn_in_progress(
                sessions > 0,
                nightly::last_write(&self.home, &target.dir),
                chrono::Utc::now().timestamp(),
                rc::TURN_QUIET,
            )
        };
        if !busy(sessions) {
            return Ok(Some((pid, sessions, false)));
        }
        if limit.is_zero() {
            return Err("열린 세션이 막 대화하는 중이다 — 손대지 않았다".into());
        }
        self.set_action(&target.label, RcAction::Waiting);
        self.event(
            "turn-wait",
            &target.label,
            serde_json::json!({ "pid": pid }),
        );
        let mut waited = Duration::ZERO;
        let mut probe_error = None;
        while waited < limit {
            (self.ops.sleep)(rc::TURN_POLL).await;
            waited += rc::TURN_POLL;
            let status = probe(&self.runner, self.config.as_ref(), &self.home).await;
            if let Some(err) = status.probe_error {
                probe_error = Some(err);
                continue;
            }
            probe_error = None;
            let live = status
                .servers
                .iter()
                .find(|s| s.dir == target.dir && s.running)
                .and_then(|s| s.pid.map(|pid| (pid, s.sessions)));
            let Some((pid, sessions)) = live else {
                self.set_action(&target.label, RcAction::Restarting);
                return Ok(None);
            };
            if !busy(sessions) {
                self.set_action(&target.label, RcAction::Restarting);
                return Ok(Some((pid, sessions, true)));
            }
        }
        Err(match probe_error {
            Some(err) => format!("기다리는 동안 현황을 못 읽어 재시작하지 않았다 — {err}"),
            None => format!(
                "대화가 {}분 넘게 이어져 재시작하지 않았다 — 턴이 끝난 뒤 다시",
                limit.as_secs() / 60
            ),
        })
    }

    /// 설치된 claude 버전. 못 재면 None — 기록하지 않고 "모름" 으로 둔다.
    async fn claude_version(&self) -> Option<String> {
        let out = (self.runner)(
            vec!["claude".into(), "--version".into()],
            String::new(),
            VERSION_TIMEOUT,
        )
        .await;
        out.ok()
            .then(|| rc::parse_claude_version(&out.stdout))
            .flatten()
    }

    /// 띄우고 등록까지 본다. `already served` 면 쉬었다 다시, 이어받기가 안 뜨면 새로 한 번 더.
    async fn launch_until_up(
        &self,
        target: &Target,
        first: LaunchMode,
        fresh: bool,
        version: Option<&str>,
        session: Option<&str>,
        backoff: &[Duration],
    ) -> Result<String, String> {
        let mut mode = first;
        let mut backoff = backoff.iter();
        let mut retried_mode = false;
        loop {
            let pid = self.launch(target, mode, version, session)?;
            match self.judge(&target.label, pid).await {
                Registration::Connected => {
                    return Ok(format!("떴다 — {}(pid {pid})", rc::mode_note(mode, fresh)))
                }
                Registration::Pending if (self.ops.signal)(pid, 0) => {
                    return Ok(format!(
                        "떴다(등록은 확인 못 함) — {}(pid {pid})",
                        rc::mode_note(mode, fresh)
                    ))
                }
                Registration::Served => {
                    // 곧 스스로 내려가지만 기다리지 않고 내린다 — 다음 기동과 겹치지 않게.
                    self.stop(&target.label, pid).await;
                    let Some(wait) = backoff.next() else {
                        return Err(
                            "already served — claude.ai 쪽 등록이 쉬고 다시 띄워도 풀리지 않았다. 잠시 뒤 다시"
                                .into(),
                        );
                    };
                    self.set_action(&target.label, RcAction::Retrying);
                    self.event(
                        "retry",
                        &target.label,
                        serde_json::json!({ "reason": "served", "waitSecs": wait.as_secs() }),
                    );
                    (self.ops.sleep)(*wait).await;
                }
                Registration::Pending => {
                    // 프로세스가 사라졌다.
                    if rc::may_fail_to_start(mode) && !retried_mode {
                        retried_mode = true;
                        mode = rc::retry_mode(target.pinned);
                        self.set_action(&target.label, RcAction::Retrying);
                        self.event(
                            "retry",
                            &target.label,
                            serde_json::json!({ "reason": "resume-down", "mode": mode }),
                        );
                        continue;
                    }
                    let err = self.read_log(&target.label, "err");
                    let tail = err
                        .lines()
                        .rev()
                        .find(|l| !l.trim().is_empty())
                        .unwrap_or("");
                    return Err(format!("뜨자마자 내려갔다 — {}", tail.trim()));
                }
            }
        }
    }

    fn log_path(&self, label: &str, ext: &str) -> PathBuf {
        self.log_dir.join(format!("{label}.{ext}"))
    }

    fn read_log(&self, label: &str, ext: &str) -> String {
        std::fs::read_to_string(self.log_path(label, ext)).unwrap_or_default()
    }

    fn launch(
        &self,
        target: &Target,
        mode: LaunchMode,
        version: Option<&str>,
        session: Option<&str>,
    ) -> Result<u32, String> {
        std::fs::create_dir_all(&self.log_dir)
            .map_err(|e| format!("로그 폴더를 못 만든다({}): {e}", self.log_dir.display()))?;
        let argv = rc::server_argv(&target.label, mode, session);
        let pid = (self.ops.spawn)(
            &argv,
            Path::new(&target.dir),
            &self.log_path(&target.label, "out"),
            &self.log_path(&target.label, "err"),
        )
        .map_err(|e| format!("못 띄웠다({} 에서 {}): {e}", target.dir, argv.join(" ")))?;
        // 기동 버전 기록 — 야간 재시작이 이것과 설치 버전을 맞대 구버전을 가린다. 못 쟀으면 지운다: 옛 값을 남기면 새
        // 바이너리로 뜬 서버를 구버전으로 보고, 빈 기록은 "모름" 이라 야간이 건드리지 않는다.
        let record = self.log_path(&target.label, "version");
        let written = match version {
            Some(v) => std::fs::write(&record, format!("{v}\n")),
            None => forget(&record),
        };
        if let Err(e) = written {
            // 기동은 이미 됐다 — 서버는 그대로 두고, 옛 값만은 남기지 않으려 한 번 더 지워 본다.
            let forgotten = forget(&record).is_ok();
            self.event(
                "version-record",
                &target.label,
                serde_json::json!({ "path": record, "error": e.to_string(), "forgotten": forgotten }),
            );
        }
        self.event(
            "start",
            &target.label,
            serde_json::json!({ "mode": mode, "pid": pid, "dir": target.dir, "version": version, "session": session }),
        );
        Ok(pid)
    }

    /// 되살림 표식(`rc/<라벨>.revive`)이 있는 라벨 — 야간 재시작이 내리고 못 띄운 대상이다. 감시가 비고정이어도 서버
    /// 모드로 띄우고, 뜨거나 누가 이미 띄웠으면 지운다.
    fn revive_marks(&self) -> HashSet<String> {
        let Ok(entries) = std::fs::read_dir(&self.log_dir) else {
            return HashSet::new();
        };
        entries
            .filter_map(|e| {
                let name = e.ok()?.file_name().into_string().ok()?;
                name.strip_suffix(".revive").map(str::to_string)
            })
            .collect()
    }

    /// 내리기 **전에** 찍는다 — 도중에 데몬이 죽어도 감시가 살리게. 내용은 폴더(사람이 볼 때).
    fn mark_revive(&self, target: &Target) {
        let _ = std::fs::create_dir_all(&self.log_dir);
        let _ = std::fs::write(
            self.log_path(&target.label, "revive"),
            format!("{}\n", target.dir),
        );
    }

    fn clear_revive(&self, label: &str, by: &str) {
        if std::fs::remove_file(self.log_path(label, "revive")).is_ok() {
            self.event("revive-cleared", label, serde_json::json!({ "by": by }));
        }
    }

    /// 등록을 기다린다 — `Connected` · `Served` 를 보면 바로, 프로세스가 사라졌으면 `Pending`(호출자가 생존을 다시 본다).
    async fn judge(&self, label: &str, pid: u32) -> Registration {
        (self.ops.sleep)(rc::REGISTRATION_FIRST).await;
        let mut waited = rc::REGISTRATION_FIRST;
        loop {
            let reg =
                rc::read_registration(&self.read_log(label, "out"), &self.read_log(label, "err"));
            if reg != Registration::Pending
                || !(self.ops.signal)(pid, 0)
                || waited >= rc::REGISTRATION_WAIT
            {
                return reg;
            }
            (self.ops.sleep)(rc::REGISTRATION_POLL).await;
            waited += rc::REGISTRATION_POLL;
        }
    }

    /// SIGTERM → 유예 동안 200ms 간격으로 확인 → 살아 있으면 SIGKILL. 내려갔으면 true.
    async fn stop(&self, label: &str, pid: u32) -> bool {
        if !(self.ops.signal)(pid, libc::SIGTERM) {
            return !(self.ops.signal)(pid, 0);
        }
        let step = Duration::from_millis(200);
        let mut waited = Duration::ZERO;
        while (self.ops.signal)(pid, 0) {
            if waited >= rc::STOP_GRACE {
                (self.ops.signal)(pid, libc::SIGKILL);
                (self.ops.sleep)(step).await;
                let down = !(self.ops.signal)(pid, 0);
                self.event(
                    "stop",
                    label,
                    serde_json::json!({ "pid": pid, "kill": true, "down": down }),
                );
                return down;
            }
            (self.ops.sleep)(step).await;
            waited += step;
        }
        self.event(
            "stop",
            label,
            serde_json::json!({ "pid": pid, "kill": false, "down": true }),
        );
        true
    }

    /// `<todo dir>/rc/events.jsonl` 에 한 줄 — 옛 CLI 결과와 맞대는 기록이다. 쓰기 실패는 삼킨다(기록이 동작을 막지 않게).
    fn event(&self, event: &str, label: &str, fields: serde_json::Value) {
        use std::io::Write;
        let line = serde_json::json!({
            "ts": chrono::Utc::now().to_rfc3339(),
            "event": event,
            "label": label,
            "fields": fields,
        });
        let _ = std::fs::create_dir_all(&self.log_dir);
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.log_dir.join("events.jsonl"))
        {
            let _ = writeln!(f, "{line}");
        }
    }
}

/// 감시 루프 — `first` 뒤 처음, 그 뒤 바퀴가 끝날 때마다 `every` 쉬고 다시(바퀴가 겹치지 않는다).
pub fn spawn_rc_supervisor(
    control: Arc<RcController>,
    notify: RcNotifier,
    first: Duration,
    every: Duration,
) {
    control.enable_supervise();
    tokio::spawn(async move {
        tokio::time::sleep(first).await;
        loop {
            control.supervise_tick(&notify).await;
            tokio::time::sleep(every).await;
        }
    });
}
