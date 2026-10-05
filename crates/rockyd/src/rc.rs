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
    }
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
            // 실패해도 다시 잰다 — 반쯤 바뀐 상태(등록만 됨 등)가 화면에 남지 않게.
            let status = probe(&runner, config.as_ref().as_ref(), &home).await;
            *slot = Some((Instant::now(), status.clone()));
            done.map(|()| status)
        })
    });
    (provider, control)
}

// ── 띄우기 · 재시작 ───────────────────────────────────────────────────────────
// 서버는 **새 프로세스 그룹**으로 띄우고 핸들을 놓는다(`kill_on_drop` 없음). launchd 가 데몬 잡을 내려도
// (bootout) 새 그룹의 자식은 정리하지 않는다 — 2026-10-05 실측. 데몬이 먼저 내려가면 서버는 PPID 1 로 넘어간다.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

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
    }
}

/// 띄우기인가 재시작인가.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RcCommand {
    Start,
    Restart { fresh: bool },
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
}

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
            RcCommand::Start => RcAction::Starting,
            RcCommand::Restart { .. } => RcAction::Restarting,
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
    }

    fn set_action(&self, label: &str, action: RcAction) {
        let mut busy = self.busy.lock().unwrap_or_else(|e| e.into_inner());
        busy.insert(label.to_string(), action);
    }

    /// `begin` 이 받은 일을 끝까지 한다 — 결과를 남기고 진행 중 표시를 푼다.
    pub async fn run(&self, target: Target, command: RcCommand) -> RcResult {
        let started = std::time::Instant::now();
        let outcome = self.execute(&target, command).await;
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
        self.busy
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&target.label);
        result
    }

    async fn execute(&self, target: &Target, command: RcCommand) -> Result<String, String> {
        // 지금 떠 있는지는 캐시 없이 다시 잰다 — 몇 초 전 현황으로 내리면 엉뚱한 pid 를 내린다.
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
        let mode = match (command, live) {
            (RcCommand::Start, Some((pid, _))) => {
                return Err(format!("이미 떠 있다(pid {pid}) — 다시 띄우려면 재시작"))
            }
            (RcCommand::Start, None) => rc::START_MODE,
            (RcCommand::Restart { fresh }, Some((pid, sessions))) => {
                let mode = rc::restart_mode(target.pinned, sessions > 0, fresh);
                if !self.stop(&target.label, pid).await {
                    return Err(format!("내리지 못했다(pid {pid}) — 손대지 않고 둔다"));
                }
                (self.ops.sleep)(rc::RESTART_DELAY).await;
                mode
            }
            // 꺼져 있던 것을 재시작하면 그냥 띄운다.
            (RcCommand::Restart { .. }, None) => rc::START_MODE,
        };
        let fresh = matches!(command, RcCommand::Restart { fresh: true });
        self.launch_until_up(target, mode, fresh).await
    }

    /// 띄우고 등록까지 본다. `already served` 면 쉬었다 다시, 이어받기가 안 뜨면 새로 한 번 더.
    async fn launch_until_up(
        &self,
        target: &Target,
        first: LaunchMode,
        fresh: bool,
    ) -> Result<String, String> {
        let mut mode = first;
        let mut backoff = rc::REGISTRATION_BACKOFF.iter();
        let mut retried_mode = false;
        loop {
            let pid = self.launch(target, mode)?;
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
                            "already served — claude.ai 쪽 등록이 3분 넘게 남아 있다. 잠시 뒤 다시"
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

    fn launch(&self, target: &Target, mode: LaunchMode) -> Result<u32, String> {
        std::fs::create_dir_all(&self.log_dir)
            .map_err(|e| format!("로그 폴더를 못 만든다({}): {e}", self.log_dir.display()))?;
        let argv = rc::server_argv(&target.label, mode);
        let pid = (self.ops.spawn)(
            &argv,
            Path::new(&target.dir),
            &self.log_path(&target.label, "out"),
            &self.log_path(&target.label, "err"),
        )
        .map_err(|e| format!("못 띄웠다({} 에서 {}): {e}", target.dir, argv.join(" ")))?;
        self.event(
            "start",
            &target.label,
            serde_json::json!({ "mode": mode, "pid": pid, "dir": target.dir }),
        );
        Ok(pid)
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
