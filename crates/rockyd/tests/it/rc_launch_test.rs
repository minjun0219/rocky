//! rc 서버 띄우기 · 재시작 — 가짜 프로세스 세계로 순서와 재시도를 고정한다(실제 `claude` 는 띄우지 않는다).

use std::collections::{HashSet, VecDeque};
use std::path::Path;
use std::sync::{Arc, Mutex};

use rocky_core::config::RcConfig;
use rocky_core::rc::RcAction;
use rockyd::rc::{RcCommand, RcController, RcOps, RcRefusal, RcSettings};
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
        now: Arc::new(|| chrono::Local::now().naive_local()),
        binary_mtime: Arc::new(|_| None),
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
        worktree: vec![],
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
    fixture_with(config(), session, running, script)
}

fn fixture_with(config: RcConfig, session: bool, running: bool, script: Vec<Behavior>) -> Fixture {
    fixture_from(config, probe_runner(session, running), running, script)
}

fn fixture_from(config: RcConfig, runner: Runner, running: bool, script: Vec<Behavior>) -> Fixture {
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
        Some(config),
        "/home/u".into(),
        dir.path().join("rc"),
        runner,
        ops(world.clone()),
    ));
    Fixture {
        _dir: dir,
        world,
        control,
    }
}

async fn run(f: &Fixture, label: &str, command: RcCommand) -> rocky_core::rc::RcResult {
    let target = f
        .control
        .begin(label, command.clone())
        .expect("받아야 한다");
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

/// 재시작을 받은 뒤 사람이 rocky.json 에서 그 대상을 뺐다 — 데몬은 바뀐 파일을 알아서 다시 읽으니 그 뒤의 프로브에는 대상 밖
/// 서버로 보인다. 그래도 떠 있는 서버로 보고 내린 뒤 띄운다(꺼진 것으로 보고 같은 폴더에 하나 더 띄우지 않는다).
#[tokio::test]
async fn a_target_dropped_mid_restart_is_still_stopped_first() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rocky.json");
    let rc = |pinned: Vec<&str>| json!({ "rc": { "root": "/w", "pinned": pinned, "targets": ["repo-b"] } });
    std::fs::write(&path, rc(vec!["repo-a"]).to_string()).unwrap();
    let world = Arc::new(Mutex::new(World {
        alive: [100].into(),
        next_pid: 200,
        script: vec![READY].into(),
        ..Default::default()
    }));
    let control = Arc::new(RcController::new(
        RcSettings::load(path.clone(), "/home/u".into()),
        "/home/u".into(),
        dir.path().join("rc"),
        probe_runner(false, true),
        ops(world.clone()),
    ));
    let command = RcCommand::Restart { fresh: true };
    let target = control.begin("repo-a", command.clone()).unwrap();
    std::fs::write(&path, rc(vec![]).to_string()).unwrap();
    let result = control.run(target, command).await;
    assert!(result.ok, "{}", result.message);
    let w = world.lock().unwrap();
    assert_eq!(
        w.signals.first(),
        Some(&(100, libc::SIGTERM)),
        "옛 서버를 먼저 내린다"
    );
    assert_eq!(w.spawns.len(), 1, "{:?}", w.spawns);
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
        vec![vec![
            "claude",
            "rc",
            "--name",
            "repo-a",
            "--session-id",
            "cse_1"
        ]],
        "열린 세션의 id 로 못 박는다 — -c 는 서버가 처음 만든 세션을 되살린다"
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
        vec![vec![
            "claude",
            "rc",
            "--name",
            "repo-a",
            "--no-create-session-in-dir"
        ]],
        "고정이어도 서버만 새로"
    );
}

// ── 워크트리 모드(`rc.worktree`) ──

fn worktree_config() -> RcConfig {
    RcConfig {
        worktree: vec!["repo-a".into()],
        ..config()
    }
}

#[tokio::test]
async fn worktree_target_starts_with_a_worktree_per_session() {
    let f = fixture_with(worktree_config(), false, false, vec![READY]);
    let result = run(&f, "repo-a", RcCommand::Start).await;
    assert!(result.ok, "{}", result.message);
    assert_eq!(
        f.world.lock().unwrap().spawns,
        vec![vec![
            "claude",
            "rc",
            "--name",
            "repo-a",
            "--no-create-session-in-dir",
            "--spawn",
            "worktree"
        ]]
    );
    let start = std::fs::read_to_string(f._dir.path().join("rc/events.jsonl"))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
        .find(|v| v["event"] == "start")
        .expect("띄운 기록");
    assert_eq!(start["fields"]["worktree"], true);
}

/// 이어받기(`-c` · `--session-id`)는 `--spawn` 과 같이 못 쓰는 단일 세션 모드다 — 워크트리 대상은 열린 세션이 있어도
/// 이어받지 않고 워크트리 모드 서버로 다시 띄운다.
#[tokio::test]
async fn worktree_restart_does_not_resume_a_live_session() {
    let f = fixture_with(worktree_config(), true, true, vec![READY]);
    let result = run(&f, "repo-a", RcCommand::Restart { fresh: false }).await;
    assert!(result.ok, "{}", result.message);
    assert!(!result.message.contains("이어받기"), "{}", result.message);
    let w = f.world.lock().unwrap();
    assert_eq!(w.signals.first(), Some(&(100, libc::SIGTERM)));
    assert_eq!(
        w.spawns,
        vec![vec![
            "claude",
            "rc",
            "--name",
            "repo-a",
            "--no-create-session-in-dir",
            "--spawn",
            "worktree"
        ]]
    );
}

/// 설정에서 워크트리를 껐지만 떠 있는 서버는 아직 워크트리 모드다 — 세션이 제 워크트리에서 돌아, 폴더에서 이어받지 않고
/// 설정대로(같은 폴더 모드) 서버만 띄운다.
#[tokio::test]
async fn restart_after_turning_worktree_off_does_not_resume_into_the_folder() {
    let runner: Runner = Arc::new(move |argv: Vec<String>, _stdin, _timeout| {
        let out = match argv[0].as_str() {
            "ps" => concat!(
                "    1     0 30-00:00:00 /sbin/launchd\n",
                "  100     1    10:00 claude rc --name repo-a --no-create-session-in-dir --spawn worktree\n",
                "  101   100    05:00 /x/claude --sdk-url https://a/v1/code/sessions/cse_1\n",
            )
            .to_string(),
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
    });
    let f = fixture_from(config(), runner, true, vec![READY]);
    let result = run(&f, "repo-a", RcCommand::Restart { fresh: false }).await;
    assert!(result.ok, "{}", result.message);
    assert_eq!(
        f.world.lock().unwrap().spawns,
        vec![vec![
            "claude",
            "rc",
            "--name",
            "repo-a",
            "--no-create-session-in-dir"
        ]]
    );
}

/// rocky.json 의 rc 블록 — repo-a(고정)와 워크트리 목록.
fn rc_file(worktree: &[&str]) -> String {
    json!({ "rc": { "root": "/w", "pinned": ["repo-a"], "worktree": worktree } }).to_string()
}

/// `dir/rocky.json` 을 다시 읽는 기동기 — repo-a 에 서버 100 이 떠 있다(첫 프로브). 대기마다 `on_sleep` 을 부른다.
fn file_backed(
    dir: &Path,
    home: &str,
    runner: Runner,
    on_sleep: impl Fn(std::time::Duration) + Send + Sync + 'static,
) -> (Arc<Mutex<World>>, Arc<RcController>) {
    let world = Arc::new(Mutex::new(World {
        alive: [100].into(),
        next_pid: 200,
        script: vec![READY].into(),
        ..Default::default()
    }));
    let control = Arc::new(RcController::new(
        RcSettings::load(dir.join("rocky.json"), home.into()),
        home.into(),
        dir.join("rc"),
        runner,
        RcOps {
            sleep: Arc::new(move |d| {
                on_sleep(d);
                Box::pin(async {})
            }),
            ..ops(world.clone())
        },
    ));
    (world, control)
}

/// 재시작을 받은 뒤 사람이 그 대상을 워크트리 목록에 넣었다 — 받을 때가 아니라 지금 설정으로 판정하고 띄운다(열린 세션을
/// 폴더에서 이어받지 않고 워크트리 모드 서버로).
#[tokio::test]
async fn worktree_turned_on_after_the_request_is_used_for_the_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rocky.json");
    std::fs::write(&path, rc_file(&[])).unwrap();
    let (world, control) = file_backed(dir.path(), "/home/u", probe_runner(true, true), |_| {});
    let command = RcCommand::Restart { fresh: false };
    let target = control.begin("repo-a", command.clone()).unwrap();
    assert!(!target.worktree);
    std::fs::write(&path, rc_file(&["repo-a"])).unwrap();
    let result = control.run(target, command).await;
    assert!(result.ok, "{}", result.message);
    assert_eq!(
        world.lock().unwrap().spawns,
        vec![vec![
            "claude",
            "rc",
            "--name",
            "repo-a",
            "--no-create-session-in-dir",
            "--spawn",
            "worktree"
        ]]
    );
}

/// 턴을 기다리는 사이(최대 10분) 사람이 그 대상을 워크트리 목록에 넣었다 — 기다린 뒤 다시 읽어 판정하고 띄운다.
#[tokio::test]
async fn worktree_turned_on_during_the_turn_wait_is_used_for_the_restart() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let projects = home.join(".claude/projects/-w-repo-a");
    std::fs::create_dir_all(&projects).unwrap();
    let log = projects.join("s.jsonl");
    std::fs::write(&log, "{}\n").unwrap();
    let path = dir.path().join("rocky.json");
    std::fs::write(&path, rc_file(&[])).unwrap();
    // 첫 대기 동안 사람이 설정을 고치고 턴이 끝난다(대화 기록이 사라져 조용하다).
    let (world, control) = file_backed(
        dir.path(),
        &home.to_string_lossy(),
        probe_runner(true, true),
        move |d| {
            if d == rocky_core::rc::TURN_POLL {
                std::fs::write(&path, rc_file(&["repo-a"])).unwrap();
                let _ = std::fs::remove_file(&log);
            }
        },
    );
    let command = RcCommand::Restart { fresh: false };
    let target = control.begin("repo-a", command.clone()).unwrap();
    let result = control.run(target, command).await;
    assert!(result.ok, "{}", result.message);
    assert!(event_names(&dir).contains(&"turn-wait".to_string()));
    assert_eq!(
        world.lock().unwrap().spawns,
        vec![vec![
            "claude",
            "rc",
            "--name",
            "repo-a",
            "--no-create-session-in-dir",
            "--spawn",
            "worktree"
        ]]
    );
}

/// 턴을 기다리는 사이 서버가 내려갔고 사람이 그 대상을 워크트리 목록에 넣었다 — 그냥 띄우되 기다린 뒤의 설정으로.
#[tokio::test]
async fn a_server_gone_during_the_turn_wait_starts_with_the_latest_worktree_setting() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let projects = home.join(".claude/projects/-w-repo-a");
    std::fs::create_dir_all(&projects).unwrap();
    std::fs::write(projects.join("s.jsonl"), "{}\n").unwrap();
    let path = dir.path().join("rocky.json");
    std::fs::write(&path, rc_file(&[])).unwrap();
    // 첫 ps 만 서버와 막 대화하는 세션을 보이고, 그 뒤로는 내려가 있다.
    let (up, gone) = (probe_runner(true, true), probe_runner(false, false));
    let ps_calls = Arc::new(Mutex::new(0usize));
    let runner: Runner = Arc::new(move |argv: Vec<String>, stdin, timeout| {
        let n = {
            let mut n = ps_calls.lock().unwrap();
            if argv[0] == "ps" {
                *n += 1;
            }
            *n
        };
        if n <= 1 {
            up(argv, stdin, timeout)
        } else {
            gone(argv, stdin, timeout)
        }
    });
    let (world, control) = file_backed(dir.path(), &home.to_string_lossy(), runner, move |d| {
        if d == rocky_core::rc::TURN_POLL {
            std::fs::write(&path, rc_file(&["repo-a"])).unwrap();
        }
    });
    let command = RcCommand::Restart { fresh: false };
    let target = control.begin("repo-a", command.clone()).unwrap();
    let result = control.run(target, command).await;
    assert!(result.ok, "{}", result.message);
    let w = world.lock().unwrap();
    assert!(w.signals.is_empty(), "내릴 서버가 없었다");
    assert_eq!(
        w.spawns,
        vec![vec![
            "claude",
            "rc",
            "--name",
            "repo-a",
            "--no-create-session-in-dir",
            "--spawn",
            "worktree"
        ]]
    );
}

/// 실제 폴더에 워크트리 모드 서버(repo-a)와 열린 세션이 있고, `.claude/worktrees/<worktree>` 의 대화 기록이 방금 바뀌었다.
fn talking_in_worktree(
    worktree: &str,
    script: Vec<Behavior>,
) -> (tempfile::TempDir, Arc<Mutex<World>>, Arc<RcController>) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("w");
    let repo = root.join("repo-a");
    let session_dir = repo.join(".claude/worktrees").join(worktree);
    std::fs::create_dir_all(&session_dir).unwrap();
    let home = dir.path().join("home");
    let projects = home
        .join(".claude/projects")
        .join(rocky_core::rc::project_dir_name(
            &session_dir.to_string_lossy(),
        ));
    std::fs::create_dir_all(&projects).unwrap();
    std::fs::write(projects.join("s.jsonl"), "{}\n").unwrap();
    let repo_dir = repo.to_string_lossy().into_owned();
    let runner: Runner = Arc::new(move |argv: Vec<String>, _stdin, _timeout| {
        let out = match argv[0].as_str() {
            "ps" => concat!(
                "    1     0 30-00:00:00 /sbin/launchd\n",
                "  100     1    10:00 claude rc --name repo-a --no-create-session-in-dir --spawn worktree\n",
                "  101   100    05:00 /x/claude --sdk-url https://a/v1/code/sessions/cse_1\n",
            )
            .to_string(),
            "lsof" => format!("p100\nfcwd\nn{repo_dir}\n"),
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
    });
    let world = Arc::new(Mutex::new(World {
        alive: [100].into(),
        next_pid: 200,
        script: script.into(),
        ..Default::default()
    }));
    let control = Arc::new(RcController::new(
        Some(RcConfig {
            root: Some(root.to_string_lossy().into_owned()),
            ..worktree_config()
        }),
        home.to_string_lossy().into_owned(),
        dir.path().join("rc"),
        runner,
        ops(world.clone()),
    ));
    (dir, world, control)
}

#[tokio::test]
async fn restart_waits_for_a_turn_in_a_session_worktree() {
    // 세션은 제 워크트리(`bridge-<세션 id>`)에서 대화한다 — 폴더 자신의 기록은 없어도 바쁘다.
    let (_dir, world, control) = talking_in_worktree("bridge-cse_1", vec![]);
    let command = RcCommand::Restart { fresh: false };
    let target = control.begin("repo-a", command.clone()).unwrap();
    let result = control.run(target, command).await;
    assert!(!result.ok);
    assert!(
        result.message.contains("10분 넘게 이어져"),
        "{}",
        result.message
    );
    let w = world.lock().unwrap();
    assert!(w.signals.is_empty() && w.spawns.is_empty(), "손대지 않았다");
}

#[tokio::test]
async fn a_handoff_worktree_turn_does_not_hold_the_restart() {
    // `todo-<n>` 은 rocky 핸드오프의 워크트리 — 제 서버가 따로 있어 이 서버의 턴이 아니다.
    let (_dir, world, control) = talking_in_worktree("todo-41", vec![READY]);
    let command = RcCommand::Restart { fresh: false };
    let target = control.begin("repo-a", command.clone()).unwrap();
    let result = control.run(target, command).await;
    assert!(result.ok, "{}", result.message);
    assert_eq!(world.lock().unwrap().spawns.len(), 1);
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
        "처음 + 60초 + 120초"
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
            vec!["claude", "rc", "--name", "repo-a", "--session-id", "cse_1"],
            vec![
                "claude",
                "rc",
                "--name",
                "repo-a",
                "--no-create-session-in-dir"
            ],
        ],
        "못 박은 세션이 안 뜨면 서버만 한 번 더"
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
    // repo-b 는 꺼져 있다 → 재시작은 그냥 띄우기(서버만).
    let f = fixture(false, true, vec![CONNECTED]);
    let result = run(&f, "repo-b", RcCommand::Restart { fresh: false }).await;
    assert!(result.ok);
    assert_eq!(
        f.world.lock().unwrap().spawns,
        vec![vec![
            "claude",
            "rc",
            "--name",
            "repo-b",
            "--no-create-session-in-dir"
        ]]
    );
}

#[tokio::test]
async fn start_session_opens_a_new_session_only_when_asked() {
    // 이름으로 띄우면 서버만, `StartSession` 일 때만 세션과 함께.
    let f = fixture(false, false, vec![CONNECTED]);
    assert!(run(&f, "repo-a", RcCommand::Start).await.ok);
    let g = fixture(false, false, vec![CONNECTED]);
    assert!(run(&g, "repo-a", RcCommand::StartSession).await.ok);
    assert_eq!(
        f.world.lock().unwrap().spawns,
        vec![vec![
            "claude",
            "rc",
            "--name",
            "repo-a",
            "--no-create-session-in-dir"
        ]]
    );
    assert_eq!(
        g.world.lock().unwrap().spawns,
        vec![vec!["claude", "rc", "--name", "repo-a"]]
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
    let target = control.begin("repo-a", command.clone()).unwrap();
    let result = control.run(target, command).await;
    assert!(!result.ok);
    assert!(result.message.contains("로그아웃이다"));
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
        vec![vec![
            "claude",
            "rc",
            "--name",
            "repo-a",
            "--no-create-session-in-dir"
        ]],
        "고정을 되살려도 서버만"
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

#[tokio::test]
async fn a_record_that_cannot_be_written_is_not_left_stale() {
    // 기록 자리에 폴더가 있으면 쓰기도 지우기도 실패한다 — 기동은 그대로 되고, 왜 못 남겼는지 이벤트에 남긴다.
    let dir = tempfile::tempdir().unwrap();
    let record = dir.path().join("rc/repo-a.version");
    std::fs::create_dir_all(&record).unwrap();
    let world = Arc::new(Mutex::new(World {
        next_pid: 200,
        script: vec![READY].into(),
        ..Default::default()
    }));
    let control = controller(
        dir.path(),
        with_version(probe_runner(false, false), Some("2.1.300")),
        world,
    );
    let target = control.begin("repo-a", RcCommand::Start).unwrap();
    assert!(control.run(target, RcCommand::Start).await.ok);
    let line = std::fs::read_to_string(dir.path().join("rc/events.jsonl"))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
        .find(|v| v["event"] == "version-record")
        .expect("기록 실패를 남긴다");
    assert!(line["fields"]["path"]
        .as_str()
        .unwrap()
        .ends_with("repo-a.version"));
    assert_eq!(line["fields"]["forgotten"], false);
}

// ── 낮 재시작의 턴 대기 · 세션 못 박기 ──

/// repo-a 에 열린 세션이 방금 대화했다. `quiet_from` 번째 `ps` 부터 대화 기록이 10분 전 것이 된다(턴이 끝났다).
fn talking(
    quiet_from: Option<usize>,
    script: Vec<Behavior>,
) -> (tempfile::TempDir, Arc<Mutex<World>>, Arc<RcController>) {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let projects = home.join(".claude/projects/-w-repo-a");
    std::fs::create_dir_all(&projects).unwrap();
    let log = projects.join("s.jsonl");
    std::fs::write(&log, "{}\n").unwrap();
    let calls = Arc::new(Mutex::new(0usize));
    let inner = probe_runner(true, true);
    let runner: Runner = Arc::new(move |argv: Vec<String>, stdin, timeout| {
        if argv[0] == "ps" {
            let mut n = calls.lock().unwrap();
            *n += 1;
            if quiet_from.is_some_and(|q| *n >= q) {
                let old = std::time::SystemTime::now() - std::time::Duration::from_secs(600);
                std::fs::File::options()
                    .write(true)
                    .open(&log)
                    .unwrap()
                    .set_modified(old)
                    .unwrap();
            }
        }
        inner(argv, stdin, timeout)
    });
    let world = Arc::new(Mutex::new(World {
        alive: [100].into(),
        next_pid: 200,
        script: script.into(),
        ..Default::default()
    }));
    let control = Arc::new(RcController::new(
        Some(config()),
        home.to_string_lossy().into_owned(),
        dir.path().join("rc"),
        runner,
        ops(world.clone()),
    ));
    (dir, world, control)
}

fn event_names(dir: &tempfile::TempDir) -> Vec<String> {
    std::fs::read_to_string(dir.path().join("rc/events.jsonl"))
        .unwrap_or_default()
        .lines()
        .map(|l| {
            serde_json::from_str::<serde_json::Value>(l).unwrap()["event"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect()
}

#[tokio::test]
async fn restart_waits_for_the_turn_to_end_then_resumes() {
    // 첫 ps(내리기 직전)는 막 대화하는 중, 두 번 다시 본 뒤 턴이 끝났다.
    let (dir, world, control) = talking(Some(3), vec![CONNECTED]);
    let command = RcCommand::Restart { fresh: false };
    let target = control.begin("repo-a", command.clone()).unwrap();
    let result = control.run(target, command).await;
    assert!(result.ok, "{}", result.message);
    assert_eq!(
        world.lock().unwrap().spawns,
        vec![vec![
            "claude",
            "rc",
            "--name",
            "repo-a",
            "--session-id",
            "cse_1"
        ]],
        "턴이 끝난 뒤 그 세션을 이어받는다"
    );
    let events = event_names(&dir);
    let wait = events
        .iter()
        .position(|e| e == "turn-wait")
        .expect("기다렸다");
    let stop = events.iter().position(|e| e == "stop").expect("내렸다");
    assert!(wait < stop, "{events:?}");
}

#[tokio::test]
async fn restart_never_cuts_a_turn_that_keeps_going() {
    let (_dir, world, control) = talking(None, vec![]);
    let command = RcCommand::Restart { fresh: false };
    let target = control.begin("repo-a", command.clone()).unwrap();
    let result = control.run(target, command).await;
    assert!(!result.ok);
    assert!(
        result.message.contains("10분 넘게 이어져"),
        "{}",
        result.message
    );
    let w = world.lock().unwrap();
    assert!(w.signals.is_empty() && w.spawns.is_empty(), "손대지 않았다");
}

#[tokio::test]
async fn pinned_session_restart_uses_that_session_or_starts_fresh() {
    let f = fixture(true, true, vec![CONNECTED]);
    let result = run(&f, "repo-a", RcCommand::Pin("cse_01abc".into())).await;
    assert!(result.ok, "{}", result.message);
    assert!(result.message.contains("세션 이어받기(--session-id)"));
    assert_eq!(
        f.world.lock().unwrap().spawns,
        vec![vec![
            "claude",
            "rc",
            "--name",
            "repo-a",
            "--session-id",
            "cse_01abc"
        ]]
    );

    // 만료됐거나 틀린 세션이면 뜨자마자 내려간다 — 서버만 한 번 더.
    let f = fixture(true, true, vec![DIES, CONNECTED]);
    let result = run(&f, "repo-a", RcCommand::Pin("cse_gone".into())).await;
    assert!(result.ok, "{}", result.message);
    assert_eq!(
        f.world.lock().unwrap().spawns,
        vec![
            vec![
                "claude",
                "rc",
                "--name",
                "repo-a",
                "--session-id",
                "cse_gone"
            ],
            vec![
                "claude",
                "rc",
                "--name",
                "repo-a",
                "--no-create-session-in-dir"
            ],
        ]
    );
}

#[tokio::test]
async fn restart_route_checks_the_session_id() {
    let f = fixture(false, false, vec![READY]);
    let control = f.control.clone();
    let fx_ = fx();
    let state = rebuild(&fx_, move |o| o.rc_control = Some(control));
    let (code, body) = post(
        &state,
        "/api/rc/servers/repo-a/restart",
        json!({"session": "3f2c9a10-1b2c-4d5e-8f90-0a1b2c3d4e5f"}),
    )
    .await;
    assert_eq!(code, 400, "로컬 전사본 UUID 는 거른다: {body}");
    let (code, _) = post(
        &state,
        "/api/rc/servers/repo-a/restart",
        json!({"session": "cse_1", "fresh": true}),
    )
    .await;
    assert_eq!(code, 400, "둘은 같이 줄 수 없다");
    let remote = ReqOptions {
        peer: Some("100.64.0.1"),
        ..Default::default()
    };
    let (code, _) = call(
        &state,
        "POST",
        "/api/rc/servers/repo-a/restart",
        Some(json!({"session": "bad"})),
        remote,
    )
    .await;
    assert_eq!(code, 403, "모양을 보기 전에 로컬부터");
    let (code, _) = post(
        &state,
        "/api/rc/servers/repo-a/restart",
        json!({"session": 42}),
    )
    .await;
    assert_eq!(code, 400, "문자열이 아닌 session 을 조용히 버리지 않는다");
    let (code, body) = post(
        &state,
        "/api/rc/servers/repo-a/restart",
        json!({"session": "cse_1"}),
    )
    .await;
    assert_eq!(code, 202, "{body}");
    // 받은 세션이 실제로 기동 인자에 들어간다(꺼져 있던 대상이라 그 세션으로 띄운다).
    for _ in 0..200 {
        if !f.world.lock().unwrap().spawns.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    assert_eq!(
        f.world.lock().unwrap().spawns,
        vec![vec![
            "claude",
            "rc",
            "--name",
            "repo-a",
            "--session-id",
            "cse_1"
        ]]
    );
}

#[tokio::test]
async fn new_session_start_route() {
    // `rocky rc start --new-session` 이 보내는 것 — 새 세션과 함께. serverOnly 와 같이 주면 400.
    let f = fixture(false, false, vec![READY]);
    let control = f.control.clone();
    let fx_ = fx();
    let state = rebuild(&fx_, move |o| o.rc_control = Some(control));
    let (code, _) = post(
        &state,
        "/api/rc/servers/repo-b/start",
        json!({"newSession": true, "serverOnly": true}),
    )
    .await;
    assert_eq!(code, 400, "둘은 같이 줄 수 없다");
    let (code, body) = post(
        &state,
        "/api/rc/servers/repo-b/start",
        json!({"newSession": true}),
    )
    .await;
    assert_eq!(code, 202, "{body}");
    for _ in 0..200 {
        if !f.world.lock().unwrap().spawns.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    assert_eq!(
        f.world.lock().unwrap().spawns,
        vec![vec!["claude", "rc", "--name", "repo-b"]]
    );
}

#[tokio::test]
async fn server_only_start_makes_no_session() {
    // `rocky rc start --all` 이 보내는 것 — 세션 없이 서버만, 떠 있으면 그대로.
    let f = fixture(false, false, vec![READY]);
    let control = f.control.clone();
    let fx_ = fx();
    let state = rebuild(&fx_, move |o| o.rc_control = Some(control));
    let (code, body) = post(
        &state,
        "/api/rc/servers/repo-b/start",
        json!({"serverOnly": true}),
    )
    .await;
    assert_eq!(code, 202, "{body}");
    for _ in 0..200 {
        if !f.world.lock().unwrap().spawns.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    assert_eq!(
        f.world.lock().unwrap().spawns,
        vec![vec![
            "claude",
            "rc",
            "--name",
            "repo-b",
            "--no-create-session-in-dir"
        ]]
    );
}

#[tokio::test]
async fn restart_resumes_the_open_session_that_talked_last() {
    // 서버 100 에 세션 둘 — 먼저 열린 101(cse_first)이 가장 최근에 대화했다(받은편지함 등록 시각).
    let dir = tempfile::tempdir().unwrap();
    let world = Arc::new(Mutex::new(World {
        alive: [100].into(),
        next_pid: 200,
        script: vec![CONNECTED].into(),
        ..Default::default()
    }));
    let runner: Runner = Arc::new(|argv: Vec<String>, _stdin, _timeout| {
        let out = match argv[0].as_str() {
            "ps" => "    1     0 30-00:00:00 /sbin/launchd\n  100     1    10:00 claude rc --name repo-a\n  101   100    09:00 /x/claude --sdk-url https://a/v1/code/sessions/cse_first\n  102   100    01:00 /x/claude --sdk-url https://a/v1/code/sessions/cse_later\n".to_string(),
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
    });
    let control = Arc::new(RcController::new(
        Some(config()),
        "/home/u".into(),
        dir.path().join("rc"),
        runner,
        ops(world.clone()),
    ));
    let reg = |pid: u32, seen_at: i64| rocky_core::peer_inbox::InboxRegistration {
        session_id: format!("s{pid}"),
        socket: format!("/tmp/cc-socks/{pid}.sock"),
        cwd: "/w/repo-a".into(),
        seen_at,
        restored: false,
    };
    control.set_inbox_source(Arc::new(move || vec![reg(101, 50), reg(102, 10)]));
    let command = RcCommand::Restart { fresh: false };
    let target = control.begin("repo-a", command.clone()).unwrap();
    let result = control.run(target, command).await;
    assert!(result.ok, "{}", result.message);
    assert_eq!(
        world.lock().unwrap().spawns,
        vec![vec![
            "claude",
            "rc",
            "--name",
            "repo-a",
            "--session-id",
            "cse_first"
        ]],
        "나중에 열렸어도 덜 쓴 세션이 아니라, 가장 최근에 대화한 세션"
    );
}
