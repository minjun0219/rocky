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
            "claude" => r#"{"loggedIn": true}"#.to_string(),
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
