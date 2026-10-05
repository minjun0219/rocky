//! rc 서버 띄우기 · 재시작 — 가짜 프로세스 세계로 순서와 재시도를 고정한다(실제 `claude` 는 띄우지 않는다).

use std::collections::{HashSet, VecDeque};
use std::path::Path;
use std::sync::{Arc, Mutex};

use rocky_core::config::RcConfig;
use rocky_core::rc::RcAction;
use rockyd::rc::{RcCommand, RcController, RcOps, RcRefusal};
use rockyd::runner::{CmdOutput, Runner};
use serde_json::json;

use crate::common::*;

/// 띄운 프로세스 하나가 어떻게 굴지.
#[derive(Clone)]
struct Behavior {
    out: &'static str,
    err: &'static str,
    /// 등록 판정 동안 살아 있나.
    stays: bool,
}

const READY: Behavior = Behavior {
    out: "·✔︎· Ready · x · main",
    err: "",
    stays: true,
};
const CONNECTED: Behavior = Behavior {
    out: "·✔︎· Connected · x · main",
    err: "",
    stays: true,
};
const SERVED: Behavior = Behavior {
    out: "",
    err:
        "Error: This folder is already served by a terminal `claude remote-control` on this device.",
    stays: true,
};
const DIES: Behavior = Behavior {
    out: "",
    err: "No conversation found to continue",
    stays: false,
};

#[derive(Default)]
struct World {
    alive: HashSet<u32>,
    /// SIGTERM 을 무시하는 pid — SIGKILL 이 필요하다.
    stubborn: HashSet<u32>,
    next_pid: u32,
    script: VecDeque<Behavior>,
    spawns: Vec<Vec<String>>,
    signals: Vec<(u32, i32)>,
}

fn ops(world: Arc<Mutex<World>>) -> RcOps {
    let w1 = world.clone();
    let w2 = world;
    RcOps {
        spawn: Arc::new(
            move |argv: &[String], _dir: &Path, out: &Path, err: &Path| {
                let mut w = w1.lock().unwrap();
                let b = w.script.pop_front().expect("예상보다 많이 띄웠다");
                std::fs::write(out, b.out)?;
                std::fs::write(err, b.err)?;
                w.next_pid += 1;
                let pid = w.next_pid;
                if b.stays {
                    w.alive.insert(pid);
                }
                w.spawns.push(argv.to_vec());
                Ok(pid)
            },
        ),
        signal: Arc::new(move |pid, sig| {
            let mut w = w2.lock().unwrap();
            if sig == 0 {
                return w.alive.contains(&pid);
            }
            w.signals.push((pid, sig));
            if !w.alive.contains(&pid) {
                return false;
            }
            if sig == libc::SIGKILL || !w.stubborn.contains(&pid) {
                w.alive.remove(&pid);
            }
            true
        }),
        // 시간은 흐르지 않는다 — 대기 횟수만 센다.
        sleep: Arc::new(|_| Box::pin(async {})),
    }
}

/// `repo-a`(고정)에 서버 100 이 떠 있고, `session` 이면 열린 세션 자식이 있다.
fn probe_runner(session: bool, running: bool) -> Runner {
    probe_runner_auth(session, running, true)
}

fn probe_runner_auth(session: bool, running: bool, logged_in: bool) -> Runner {
    Arc::new(move |argv: Vec<String>, _stdin, _timeout| {
        let mut ps = String::from("    1     0 30-00:00:00 /sbin/launchd\n");
        if running {
            ps.push_str("  100     1    10:00 claude rc --name repo-a\n");
            if session {
                ps.push_str(
                    "  101   100    05:00 /x/claude --sdk-url https://a/v1/code/sessions/cse_1\n",
                );
            }
        }
        let out = match argv[0].as_str() {
            "ps" => ps,
            "lsof" => "p100\nfcwd\nn/w/repo-a\n".to_string(),
            "claude" => format!(r#"{{"loggedIn": {logged_in}}}"#),
            _ => return Box::pin(async { CmdOutput::failure("없음") }),
        };
        Box::pin(async move {
            CmdOutput {
                code: 0,
                stdout: out,
                stderr: String::new(),
            }
        })
    })
}

fn config() -> RcConfig {
    RcConfig {
        root: Some("/w".into()),
        pinned: vec!["repo-a".into()],
        targets: vec!["repo-b".into()],
        supervise: false,
        nightly: None,
    }
}

struct Fixture {
    _dir: tempfile::TempDir,
    world: Arc<Mutex<World>>,
    control: Arc<RcController>,
}

fn fixture(session: bool, running: bool, script: Vec<Behavior>) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let world = Arc::new(Mutex::new(World {
        alive: if running {
            [100].into()
        } else {
            HashSet::new()
        },
        next_pid: 200,
        script: script.into(),
        ..Default::default()
    }));
    let control = Arc::new(RcController::new(
        Some(config()),
        "/home/u".into(),
        dir.path().join("rc"),
        probe_runner(session, running),
        ops(world.clone()),
    ));
    Fixture {
        _dir: dir,
        world,
        control,
    }
}

async fn run(f: &Fixture, label: &str, command: RcCommand) -> rocky_core::rc::RcResult {
    let target = f.control.begin(label, command).expect("받아야 한다");
    f.control.run(target, command).await
}

fn events(f: &Fixture) -> Vec<String> {
    std::fs::read_to_string(f._dir.path().join("rc/events.jsonl"))
        .unwrap_or_default()
        .lines()
        .map(|l| {
            let v: serde_json::Value = serde_json::from_str(l).unwrap();
            v["event"].as_str().unwrap().to_string()
        })
        .collect()
}

#[tokio::test]
async fn restart_with_live_session_stops_then_resumes() {
    let f = fixture(true, true, vec![CONNECTED]);
    let result = run(&f, "repo-a", RcCommand::Restart { fresh: false }).await;
    assert!(result.ok, "{}", result.message);
    assert!(result.message.contains("이어받기"));
    let w = f.world.lock().unwrap();
    assert_eq!(
        w.signals.first(),
        Some(&(100, libc::SIGTERM)),
        "옛 서버를 먼저 SIGTERM"
    );
    assert_eq!(
        w.spawns,
        vec![vec!["claude", "rc", "--name", "repo-a", "-c"]]
    );
    drop(w);
    assert_eq!(events(&f), vec!["stop", "start", "result"]);
}

#[tokio::test]
async fn fresh_restart_does_not_resume() {
    let f = fixture(true, true, vec![CONNECTED]);
    let result = run(&f, "repo-a", RcCommand::Restart { fresh: true }).await;
    assert!(result.ok);
    assert_eq!(
        f.world.lock().unwrap().spawns,
        vec![vec!["claude", "rc", "--name", "repo-a"]],
        "고정은 세션까지 새로"
    );
}

#[tokio::test]
async fn already_served_backs_off_and_retries() {
    let f = fixture(false, false, vec![SERVED, READY]);
    let result = run(&f, "repo-a", RcCommand::Start).await;
    assert!(result.ok, "{}", result.message);
    let w = f.world.lock().unwrap();
    assert_eq!(w.spawns.len(), 2);
    // 첫 서버(201)는 스스로 내려가길 기다리지 않고 내린다.
    assert!(w.signals.contains(&(201, libc::SIGTERM)));
    drop(w);
    assert_eq!(
        events(&f),
        vec!["start", "stop", "retry", "start", "result"]
    );
}

#[tokio::test]
async fn served_three_times_gives_up_with_reason() {
    let f = fixture(false, false, vec![SERVED, SERVED, SERVED]);
    let result = run(&f, "repo-a", RcCommand::Start).await;
    assert!(!result.ok);
    assert!(result.message.contains("already served"));
    assert_eq!(
        f.world.lock().unwrap().spawns.len(),
        3,
        "처음 + 45초 + 90초"
    );
}

#[tokio::test]
async fn resume_that_dies_falls_back_to_fresh_once() {
    let f = fixture(true, true, vec![DIES, CONNECTED]);
    let result = run(&f, "repo-a", RcCommand::Restart { fresh: false }).await;
    assert!(result.ok, "{}", result.message);
    assert_eq!(
        f.world.lock().unwrap().spawns,
        vec![
            vec!["claude", "rc", "--name", "repo-a", "-c"],
            vec!["claude", "rc", "--name", "repo-a"],
        ]
    );
}

#[tokio::test]
async fn fresh_start_that_dies_reports_stderr() {
    let f = fixture(false, false, vec![DIES]);
    let result = run(&f, "repo-a", RcCommand::Start).await;
    assert!(!result.ok);
    assert_eq!(
        result.message,
        "뜨자마자 내려갔다 — No conversation found to continue"
    );
}

#[tokio::test]
async fn start_refuses_when_already_running() {
    let f = fixture(false, true, vec![]);
    let result = run(&f, "repo-a", RcCommand::Start).await;
    assert!(!result.ok);
    assert!(result.message.contains("이미 떠 있다(pid 100)"));
    assert!(f.world.lock().unwrap().spawns.is_empty());
}

#[tokio::test]
async fn stubborn_server_gets_sigkill_after_grace() {
    let f = fixture(false, true, vec![READY]);
    f.world.lock().unwrap().stubborn.insert(100);
    let result = run(&f, "repo-a", RcCommand::Restart { fresh: false }).await;
    assert!(result.ok, "{}", result.message);
    let signals = f.world.lock().unwrap().signals.clone();
    assert_eq!(signals[0], (100, libc::SIGTERM));
    assert!(signals.contains(&(100, libc::SIGKILL)));
}

#[tokio::test]
async fn non_pinned_restart_without_session_is_server_only() {
    // repo-b 는 꺼져 있다 → 재시작은 그냥 띄우기(이름으로 띄우면 세션까지).
    let f = fixture(false, true, vec![CONNECTED]);
    let result = run(&f, "repo-b", RcCommand::Restart { fresh: false }).await;
    assert!(result.ok);
    assert_eq!(
        f.world.lock().unwrap().spawns,
        vec![vec!["claude", "rc", "--name", "repo-b"]]
    );
}

#[tokio::test]
async fn begin_refuses_unknown_busy_and_off() {
    let f = fixture(false, false, vec![]);
    assert!(matches!(
        f.control.begin("nope", RcCommand::Start),
        Err(RcRefusal::NotFound(_))
    ));
    f.control.begin("repo-a", RcCommand::Start).unwrap();
    assert!(matches!(
        f.control
            .begin("repo-a", RcCommand::Restart { fresh: false }),
        Err(RcRefusal::Busy(_))
    ));
    // 다른 대상은 겹쳐도 된다.
    assert!(f.control.begin("repo-b", RcCommand::Start).is_ok());

    let off = RcController::new(
        None,
        "/home/u".into(),
        f._dir.path().join("off"),
        probe_runner(false, false),
        ops(f.world.clone()),
    );
    assert!(matches!(
        off.begin("repo-a", RcCommand::Start),
        Err(RcRefusal::NotFound(_))
    ));
}

#[tokio::test]
async fn status_shows_action_then_result() {
    let f = fixture(false, false, vec![READY]);
    let target = f.control.begin("repo-a", RcCommand::Start).unwrap();
    let mut status =
        rockyd::rc::probe(&probe_runner(false, false), Some(&config()), "/home/u").await;
    f.control.decorate(&mut status);
    assert_eq!(status.servers[0].action, Some(RcAction::Starting));
    f.control.run(target, RcCommand::Start).await;
    f.control.decorate(&mut status);
    assert_eq!(status.servers[0].action, None);
    assert!(status.servers[0].last_result.as_ref().unwrap().ok);
}

#[tokio::test]
async fn routes_are_local_only_and_accept_with_202() {
    let f = fixture(false, false, vec![READY]);
    let control = f.control.clone();
    let fx_ = fx();
    let state = rebuild(&fx_, move |o| o.rc_control = Some(control));
    let remote = ReqOptions {
        peer: Some("100.64.0.1"),
        ..Default::default()
    };
    let (code, _) = call(&state, "POST", "/api/rc/servers/repo-a/start", None, remote).await;
    assert_eq!(code, 403);
    let (code, body) = post(&state, "/api/rc/servers/nope/start", json!({})).await;
    assert_eq!(code, 404, "{body}");
    let (code, body) = post(&state, "/api/rc/servers/repo-a/start", json!({})).await;
    assert_eq!(code, 202, "{body}");

    // rc 가 꺼진 기기(제어기 없음)면 404.
    let (code, _) = post(
        &fx_.state,
        "/api/rc/servers/repo-a/restart",
        json!({"fresh": true}),
    )
    .await;
    assert_eq!(code, 404);
}

/// 진짜 프로세스로 — 띄운 서버는 데몬과 다른 **새 프로세스 그룹**의 우두머리여야 launchd 가 데몬 잡을 내려도
/// 산다(2026-10-05 bootout 실측). `sleep` 으로 대신하고 끝에 pid 로 내린다.
#[tokio::test]
async fn real_spawn_leaves_a_new_process_group() {
    let dir = tempfile::tempdir().unwrap();
    let ops = rockyd::rc::default_ops();
    let argv = vec!["sleep".to_string(), "30".to_string()];
    let pid = (ops.spawn)(
        &argv,
        dir.path(),
        &dir.path().join("x.out"),
        &dir.path().join("x.err"),
    )
    .expect("sleep 을 띄운다");
    let pgid = std::process::Command::new("ps")
        .args(["-o", "pgid=", "-p", &pid.to_string()])
        .output()
        .unwrap();
    let pgid: u32 = String::from_utf8_lossy(&pgid.stdout)
        .trim()
        .parse()
        .unwrap();
    let own = std::process::Command::new("ps")
        .args(["-o", "pgid=", "-p", &std::process::id().to_string()])
        .output()
        .unwrap();
    let own: u32 = String::from_utf8_lossy(&own.stdout).trim().parse().unwrap();
    assert_eq!(pgid, pid, "새 그룹의 우두머리");
    assert_ne!(pgid, own, "데몬(테스트)과 다른 그룹");
    assert!((ops.signal)(pid, 0));
    assert!((ops.signal)(pid, libc::SIGTERM));
    for _ in 0..50 {
        if !(ops.signal)(pid, 0) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(
        !(ops.signal)(pid, 0),
        "SIGTERM 으로 내려가고 대기 스레드가 거둔다"
    );
}

#[tokio::test]
async fn logged_out_daemon_context_touches_nothing() {
    // 재시작인데 데몬 맥락이 로그아웃이면 떠 있는 서버를 내리지도 않는다 — 내리면 다시 못 띄운다.
    let dir = tempfile::tempdir().unwrap();
    let world = Arc::new(Mutex::new(World {
        alive: [100].into(),
        next_pid: 200,
        ..Default::default()
    }));
    let control = RcController::new(
        Some(config()),
        "/home/u".into(),
        dir.path().join("rc"),
        probe_runner_auth(true, true, false),
        ops(world.clone()),
    );
    let command = RcCommand::Restart { fresh: false };
    let target = control.begin("repo-a", command).unwrap();
    let result = control.run(target, command).await;
    assert!(!result.ok);
    assert!(result.message.contains("로그인돼 있지 않다"));
    let w = world.lock().unwrap();
    assert!(w.signals.is_empty(), "내리지 않았다");
    assert!(w.spawns.is_empty(), "띄우지 않았다");
}

// ── 감시(되살리기) ──

/// 바퀴마다 바뀌는 프로브 — (repo-a 가 떠 있나, 데몬 맥락이 로그인인가, repo-a 가 떠 있은 시간).
fn probe_runner_live(state: Arc<Mutex<(bool, bool, u64)>>) -> Runner {
    Arc::new(move |argv: Vec<String>, _stdin, _timeout| {
        let (running, logged_in, up) = *state.lock().unwrap();
        let mut ps = String::from("    1     0 30-00:00:00 /sbin/launchd\n");
        if running {
            ps.push_str(&format!(
                "  100     1 {:02}:{:02}:{:02} claude rc --name repo-a\n",
                up / 3600,
                (up / 60) % 60,
                up % 60
            ));
        }
        let out = match argv[0].as_str() {
            "ps" => ps,
            "lsof" => "p100\nfcwd\nn/w/repo-a\n".to_string(),
            "claude" => format!(r#"{{"loggedIn": {logged_in}}}"#),
            _ => return Box::pin(async { CmdOutput::failure("없음") }),
        };
        Box::pin(async move {
            CmdOutput {
                code: 0,
                stdout: out,
                stderr: String::new(),
            }
        })
    })
}

fn supervisor(
    state: Arc<Mutex<(bool, bool, u64)>>,
    script: Vec<Behavior>,
) -> (tempfile::TempDir, Arc<Mutex<World>>, Arc<RcController>) {
    let dir = tempfile::tempdir().unwrap();
    let world = Arc::new(Mutex::new(World {
        next_pid: 200,
        script: script.into(),
        ..Default::default()
    }));
    let control = Arc::new(RcController::new(
        Some(config()),
        "/home/u".into(),
        dir.path().join("rc"),
        probe_runner_live(state),
        ops(world.clone()),
    ));
    control.enable_supervise();
    (dir, world, control)
}

fn notifier() -> (rockyd::rc::RcNotifier, Arc<Mutex<Vec<String>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = seen.clone();
    (
        Arc::new(move |_title, body| sink.lock().unwrap().push(body)),
        seen,
    )
}

#[tokio::test]
async fn supervise_revives_only_the_stopped_pinned_server() {
    let state = Arc::new(Mutex::new((false, true, 0)));
    let (_dir, world, control) = supervisor(state, vec![CONNECTED]);
    let (notify, seen) = notifier();
    let started = control.supervise_tick(&notify).await;
    assert_eq!(started, vec!["repo-a"], "repo-b 는 고정이 아니다");
    assert_eq!(
        world.lock().unwrap().spawns,
        vec![vec!["claude", "rc", "--name", "repo-a"]]
    );
    assert!(
        seen.lock().unwrap().is_empty(),
        "로그인이 이어지면 알리지 않는다"
    );
}

#[tokio::test]
async fn supervise_backs_off_after_a_failed_revive() {
    let state = Arc::new(Mutex::new((false, true, 0)));
    let (_dir, world, control) = supervisor(state, vec![DIES]);
    let (notify, _) = notifier();
    assert_eq!(control.supervise_tick(&notify).await, vec!["repo-a"]);
    // 2분을 쉬는 동안 다음 바퀴는 두드리지 않는다.
    assert!(control.supervise_tick(&notify).await.is_empty());
    assert_eq!(world.lock().unwrap().spawns.len(), 1);
}

#[tokio::test]
async fn supervise_alerts_once_on_logout_and_once_on_recovery() {
    let state = Arc::new(Mutex::new((false, false, 0)));
    let (dir, world, control) = supervisor(state.clone(), vec![CONNECTED]);
    let (notify, seen) = notifier();
    assert!(
        control.supervise_tick(&notify).await.is_empty(),
        "로그아웃이면 띄우지 않는다"
    );
    assert!(control.supervise_tick(&notify).await.is_empty());
    assert_eq!(
        seen.lock().unwrap().len(),
        1,
        "같은 상태가 이어지면 다시 울리지 않는다"
    );
    assert!(seen.lock().unwrap()[0].contains("끊겼다"));
    assert!(world.lock().unwrap().spawns.is_empty());
    let mark: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.path().join("rc/auth.json")).unwrap())
            .unwrap();
    assert!(
        mark["lastOut"].is_i64(),
        "데몬을 다시 띄워도 알아채게 남긴다"
    );

    state.lock().unwrap().1 = true;
    assert_eq!(control.supervise_tick(&notify).await, vec!["repo-a"]);
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 2);
    assert!(seen[1].contains("돌아왔다"));
}

#[tokio::test]
async fn recovery_marks_servers_started_before_the_logout() {
    let now = chrono::Utc::now().timestamp();
    // repo-a 는 1시간 전에 떴고, 30분 전에 끊겼다가 10분 전에 돌아왔다(기록은 데몬이 다시 떠도 파일에 남아 있다).
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("rc")).unwrap();
    std::fs::write(
        dir.path().join("rc/auth.json"),
        format!(r#"{{"lastOut": {}, "lastIn": {}}}"#, now - 1800, now - 600),
    )
    .unwrap();
    let live = probe_runner_live(Arc::new(Mutex::new((true, true, 3600))));
    let control = RcController::new(
        Some(config()),
        "/home/u".into(),
        dir.path().join("rc"),
        live.clone(),
        ops(Arc::new(Mutex::new(World::default()))),
    );
    control.enable_supervise();
    let mut status = rockyd::rc::probe(&live, Some(&config()), "/home/u").await;
    control.decorate(&mut status);
    let a = status.servers.iter().find(|s| s.label == "repo-a").unwrap();
    assert!(a.auth_suspect, "끊기기 전에 뜬 서버");
    assert!(
        !status
            .supervise
            .as_ref()
            .expect("감시가 켜져 있다")
            .logged_out
    );
}

#[tokio::test]
async fn status_has_no_supervise_block_when_off() {
    let f = fixture(false, true, vec![]);
    let mut status =
        rockyd::rc::probe(&probe_runner(false, true), Some(&config()), "/home/u").await;
    f.control.decorate(&mut status);
    assert!(status.supervise.is_none());
    assert!(status.servers.iter().all(|s| !s.auth_suspect));
}

// ── 기동 버전 기록 · 되살림 표식 ──

/// `claude --version` 에만 답하는 러너를 씌운다 — None 이면 실패(시간 초과처럼).
fn with_version(inner: Runner, version: Option<&'static str>) -> Runner {
    Arc::new(move |argv: Vec<String>, stdin, timeout| {
        if argv.get(1).map(String::as_str) == Some("--version") {
            return Box::pin(async move {
                match version {
                    Some(v) => CmdOutput {
                        code: 0,
                        stdout: v.to_string(),
                        stderr: String::new(),
                    },
                    None => CmdOutput::failure("시간 초과"),
                }
            });
        }
        inner(argv, stdin, timeout)
    })
}

fn controller(dir: &Path, runner: Runner, world: Arc<Mutex<World>>) -> Arc<RcController> {
    Arc::new(RcController::new(
        Some(config()),
        "/home/u".into(),
        dir.join("rc"),
        runner,
        ops(world),
    ))
}

#[tokio::test]
async fn launch_records_the_installed_version_or_forgets_it() {
    let dir = tempfile::tempdir().unwrap();
    let world = Arc::new(Mutex::new(World {
        next_pid: 200,
        script: vec![READY, READY].into(),
        ..Default::default()
    }));
    let record = dir.path().join("rc/repo-a.version");
    let known = controller(
        dir.path(),
        with_version(probe_runner(false, false), Some("2.1.300 (Claude Code)\n")),
        world.clone(),
    );
    let target = known.begin("repo-a", RcCommand::Start).unwrap();
    assert!(known.run(target, RcCommand::Start).await.ok);
    assert_eq!(std::fs::read_to_string(&record).unwrap(), "2.1.300\n");

    // 못 재면 옛 값을 남기지 않는다 — 새 바이너리로 뜬 서버를 구버전으로 보게 된다.
    let unknown = controller(
        dir.path(),
        with_version(probe_runner(false, false), None),
        world,
    );
    let target = unknown.begin("repo-a", RcCommand::Start).unwrap();
    assert!(unknown.run(target, RcCommand::Start).await.ok);
    assert!(!record.exists(), "모름 — 야간이 건드리지 않는다");
}

#[tokio::test]
async fn revive_leaves_a_server_that_came_up_meanwhile() {
    let f = fixture(false, true, vec![]);
    let command = RcCommand::Revive(rocky_core::rc::LaunchMode::Session);
    let result = run(&f, "repo-a", command).await;
    assert!(result.ok, "그새 누가 띄웠으면 실패가 아니다");
    assert!(result.message.contains("그대로 둔다"));
    assert!(f.world.lock().unwrap().spawns.is_empty());
}

#[tokio::test]
async fn supervise_revives_marked_unpinned_as_server_only_and_clears_marks() {
    // repo-a(고정)는 떠 있고, repo-b(비고정)는 야간이 내리고 못 띄워 표식이 남았다.
    let state = Arc::new(Mutex::new((true, true, 600)));
    let (dir, world, control) = supervisor(state, vec![READY]);
    let rc_dir = dir.path().join("rc");
    std::fs::create_dir_all(&rc_dir).unwrap();
    for label in ["repo-a", "repo-b", "gone"] {
        std::fs::write(rc_dir.join(format!("{label}.revive")), "/w/x\n").unwrap();
    }
    let (notify, _) = notifier();
    assert_eq!(control.supervise_tick(&notify).await, vec!["repo-b"]);
    assert_eq!(
        world.lock().unwrap().spawns,
        vec![vec![
            "claude",
            "rc",
            "--name",
            "repo-b",
            "--no-create-session-in-dir"
        ]],
        "사람이 부른 기동이 아니라 서버만"
    );
    for label in ["repo-a", "repo-b", "gone"] {
        assert!(
            !rc_dir.join(format!("{label}.revive")).exists(),
            "{label} 표식이 남았다"
        );
    }
    let cleared: Vec<(String, String)> = std::fs::read_to_string(rc_dir.join("events.jsonl"))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
        .filter(|v| v["event"] == "revive-cleared")
        .map(|v| {
            (
                v["label"].as_str().unwrap().to_string(),
                v["fields"]["by"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(
        cleared,
        vec![
            ("gone".to_string(), "not-a-target".to_string()),
            ("repo-a".to_string(), "already-running".to_string()),
            ("repo-b".to_string(), "started".to_string()),
        ]
    );
}

#[tokio::test]
async fn supervise_keeps_the_mark_when_the_revive_fails() {
    let state = Arc::new(Mutex::new((true, true, 600)));
    let (dir, _world, control) = supervisor(state, vec![DIES]);
    let mark = dir.path().join("rc/repo-b.revive");
    std::fs::create_dir_all(mark.parent().unwrap()).unwrap();
    std::fs::write(&mark, "/w/repo-b\n").unwrap();
    let (notify, _) = notifier();
    assert_eq!(control.supervise_tick(&notify).await, vec!["repo-b"]);
    assert!(mark.exists(), "다음 바퀴(쉬기 뒤)에 다시 띄운다");
}
