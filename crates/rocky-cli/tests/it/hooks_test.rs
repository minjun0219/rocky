//! TS `hooks/ensure-daemon.test.ts` 포팅 — 버전 인식 재기동의 분기들.

use std::cell::{Cell, RefCell};

use rocky_cli::client::{build_context, DaemonHealth};
use rocky_cli::hooks::{ensure_daemon_with, ensure_daemon_with_policy, EnsureDeps, RestartPolicy};

fn health(version: Option<&str>, pid: u32) -> DaemonHealth {
    DaemonHealth {
        ok: true,
        name: Some("rocky".into()),
        version: version.map(str::to_string),
        pid: Some(pid),
        issue_create_allowed: None,
    }
}

/// 기록 장치 — 각 테스트가 관심 있는 축(spawn/stop/replace 횟수)만 드러낸다.
#[derive(Default)]
struct Log {
    spawned: RefCell<u32>,
    stopped: RefCell<Vec<Option<u32>>>,
    replaced: RefCell<u32>,
    /// `Some` 이면 launchd 교체가 이 사유로 실패한다.
    replace_error: Option<&'static str>,
    /// `Some` 이면 spawn 이 이 사유로 실패한다.
    spawn_error: Option<&'static str>,
}

fn run(
    log: &Log,
    check: &dyn Fn(&str) -> Option<DaemonHealth>,
    managed: bool,
    stop_ok: bool,
) -> Option<String> {
    let ctx = build_context(8636, std::env::temp_dir(), "test");
    ensure_daemon_with(
        &ctx,
        &EnsureDeps {
            version: "1.0.0",
            check_health: check,
            spawn: &|_| {
                *log.spawned.borrow_mut() += 1;
                match log.spawn_error {
                    Some(error) => Err(error.to_string()),
                    None => Ok(()),
                }
            },
            stop: &|_, pid| {
                log.stopped.borrow_mut().push(pid);
                stop_ok
            },
            is_managed: &move || managed,
            replace_managed: &|| {
                *log.replaced.borrow_mut() += 1;
                match log.replace_error {
                    Some(error) => Err(error.to_string()),
                    None => Ok(()),
                }
            },
            // 기본: 포트의 데몬이 곧 launchd 의 데몬이다(고아 없음).
            managed_pid: &|| check("").and_then(|h| h.pid),
            pause: &|| {},
        },
    )
}

#[test]
fn same_version_running_is_a_no_op() {
    let log = Log::default();
    let warning = run(&log, &|_| Some(health(Some("1.0.0"), 111)), false, true);
    assert_eq!(*log.spawned.borrow(), 0);
    assert!(log.stopped.borrow().is_empty());
    assert_eq!(warning, None);
}

#[test]
fn absent_daemon_is_spawned() {
    let log = Log::default();
    run(&log, &|_| None, false, true);
    assert_eq!(*log.spawned.borrow(), 1);
    assert!(log.stopped.borrow().is_empty());
}

#[test]
fn stale_daemon_is_stopped_then_respawned() {
    let log = Log::default();
    run(&log, &|_| Some(health(Some("0.9.0"), 222)), false, true);
    assert_eq!(log.stopped.borrow().as_slice(), &[Some(222)]);
    assert_eq!(*log.spawned.borrow(), 1);
}

/// health 에 version 이 없던 시절(≤0.1.0)의 데몬 — 그대로 두면 영원히 안 올라온다.
#[test]
fn a_daemon_without_a_version_is_treated_as_stale() {
    let log = Log::default();
    run(&log, &|_| Some(health(None, 0)), false, true);
    assert_eq!(log.stopped.borrow().len(), 1);
    assert_eq!(*log.spawned.borrow(), 1);
}

/// 못 내리면 재기동하지 않는다 — 보드가 없는 것보다 구버전이라도 있는 게 낫다. 다만 그
/// 사실은 돌려준다(조용히 구버전으로 남지 않게).
#[test]
fn respawn_is_skipped_when_the_stale_daemon_survives() {
    let log = Log::default();
    let warning = run(&log, &|_| Some(health(Some("0.9.0"), 333)), false, false);
    assert_eq!(log.stopped.borrow().len(), 1);
    assert_eq!(*log.spawned.borrow(), 0);
    let warning = warning.expect("내리지 못한 사실을 알린다");
    assert!(warning.contains("v0.9.0") && warning.contains("pid 333"));
    assert!(warning.contains("rocky daemon stop"));
}

/// KeepAlive 가 PID kill 을 즉시 되살리므로 stop/spawn 이 아니라 job 을 교체해야 한다.
#[test]
fn a_managed_stale_daemon_is_replaced_not_killed() {
    let log = Log::default();
    run(&log, &|_| Some(health(Some("0.9.0"), 444)), true, true);
    assert_eq!(*log.replaced.borrow(), 1);
    assert!(log.stopped.borrow().is_empty());
    assert_eq!(*log.spawned.borrow(), 0);
}

/// 실제 사고(0.27→0.28): `install_launchd` 가 옛 job 을 내린 뒤 bootstrap 에 실패했는데 훅이
/// 결과를 버려, plist 만 새 경로이고 서비스도 데몬도 없는 상태가 남았다. 교체가 실패해
/// 데몬이 사라졌으면 launchd 밖에서라도 띄우고, 그 사실과 고치는 명령을 돌려준다.
#[test]
fn a_failed_replacement_that_lost_the_daemon_spawns_it_outside_launchd_and_warns() {
    let log = Log {
        replace_error: Some("Bootstrap failed: 5: Input/output error"),
        ..Log::default()
    };
    // 첫 health 는 구버전, 교체 뒤의 두 번째 health 는 없음(bootout 은 됐다).
    let calls = Cell::new(0);
    let check = |_: &str| {
        calls.set(calls.get() + 1);
        (calls.get() == 1).then(|| health(Some("0.9.0"), 7))
    };
    let warning = run(&log, &check, true, true);
    assert_eq!(*log.replaced.borrow(), 1);
    assert_eq!(
        *log.spawned.borrow(),
        1,
        "데몬이 없어졌으니 밖에서라도 띄운다"
    );
    assert!(log.stopped.borrow().is_empty());
    let warning = warning.expect("재등록 실패를 알린다");
    assert!(warning.contains("Input/output error"), "{warning}");
    assert!(warning.contains("launchd 밖에서 띄웠다"), "{warning}");
    assert!(warning.contains("rocky daemon install"), "{warning}");
}

/// fallback spawn 마저 실패하면 "밖에서 띄웠다" 고 말하면 안 된다 — 데몬이 없다는 사실과
/// spawn 의 사유(바이너리 없음, 포트, health 타임아웃)가 경고에 그대로 실린다.
#[test]
fn a_failed_fallback_spawn_is_reported_not_claimed() {
    let log = Log {
        replace_error: Some("Bootstrap failed: 5: Input/output error"),
        spawn_error: Some("rockyd 를 띄우지 못했다: No such file or directory"),
        ..Log::default()
    };
    let calls = Cell::new(0);
    let check = |_: &str| {
        calls.set(calls.get() + 1);
        (calls.get() == 1).then(|| health(Some("0.9.0"), 7))
    };
    let warning = run(&log, &check, true, true).expect("실패를 알린다");
    assert_eq!(*log.spawned.borrow(), 1);
    assert!(!warning.contains("밖에서 띄웠다"), "{warning}");
    assert!(warning.contains("No such file or directory"), "{warning}");
    assert!(warning.contains("지금 데몬이 없다"), "{warning}");
}

/// 데몬이 없어 SessionStart 가 띄우는 평범한 경로도 실패하면 알린다.
#[test]
fn a_failed_first_spawn_is_reported() {
    let log = Log {
        spawn_error: Some("rocky daemon did not start on port 8636"),
        ..Log::default()
    };
    let warning = run(&log, &|_| None, false, true).expect("실패를 알린다");
    assert!(warning.contains("port 8636"), "{warning}");
}

/// 교체가 실패했는데 구버전이 그대로 돌고 있으면(bootout 까지 실패) 띄우지 않는다 — 포트가
/// 이미 잡혀 있다. 경고만.
#[test]
fn a_failed_replacement_with_the_old_daemon_still_up_only_warns() {
    let log = Log {
        replace_error: Some("Boot-out failed: 1: Operation not permitted"),
        ..Log::default()
    };
    let warning = run(&log, &|_| Some(health(Some("0.9.0"), 8)), true, true);
    assert_eq!(*log.replaced.borrow(), 1);
    assert_eq!(*log.spawned.borrow(), 0);
    let warning = warning.expect("재등록 실패를 알린다");
    assert!(
        warning.contains("구버전 데몬(v0.9.0)이 그대로 돈다"),
        "{warning}"
    );
    assert!(warning.contains("rocky daemon install"), "{warning}");
}

#[test]
fn a_managed_daemon_on_the_same_version_is_untouched() {
    let log = Log::default();
    run(&log, &|_| Some(health(Some("1.0.0"), 555)), true, true);
    assert_eq!(*log.replaced.borrow(), 0);
    assert_eq!(*log.spawned.borrow(), 0);
}

// ── 매 턴 정책: 오래된 데몬만 올린다 ────────────────────────────────────────────

fn run_only_if_older(log: &Log, check: &dyn Fn(&str) -> Option<DaemonHealth>, managed: bool) {
    run_with_policy(log, check, managed, RestartPolicy::OnlyIfOlder);
}

fn run_with_policy(
    log: &Log,
    check: &dyn Fn(&str) -> Option<DaemonHealth>,
    managed: bool,
    policy: RestartPolicy,
) {
    let ctx = build_context(8636, std::env::temp_dir(), "test");
    ensure_daemon_with_policy(
        &ctx,
        &EnsureDeps {
            version: "1.0.0",
            check_health: check,
            spawn: &|_| {
                *log.spawned.borrow_mut() += 1;
                Ok(())
            },
            stop: &|_, pid| {
                log.stopped.borrow_mut().push(pid);
                true
            },
            is_managed: &move || managed,
            replace_managed: &|| {
                *log.replaced.borrow_mut() += 1;
                Ok(())
            },
            managed_pid: &|| check("").and_then(|h| h.pid),
            pause: &|| {},
        },
        policy,
    );
}

#[test]
fn only_if_older_upgrades_an_older_daemon() {
    let log = Log::default();
    run_only_if_older(&log, &|_| Some(health(Some("0.9.9"), 1)), false);
    assert_eq!(*log.stopped.borrow(), vec![Some(1)]);
    assert_eq!(*log.spawned.borrow(), 1);
    // version 미보고도 옛것.
    let log = Log::default();
    run_only_if_older(&log, &|_| Some(health(None, 2)), false);
    assert_eq!(*log.spawned.borrow(), 1);
}

#[test]
fn only_if_older_leaves_same_newer_absent_and_unparseable_alone() {
    for h in [
        Some(health(Some("1.0.0"), 3)),
        Some(health(Some("1.1.0"), 4)),
        None,
        Some(health(Some("dev"), 5)),
    ] {
        let log = Log::default();
        run_only_if_older(&log, &|_| h.clone(), false);
        assert!(log.stopped.borrow().is_empty(), "{h:?}");
        assert_eq!(*log.spawned.borrow(), 0, "{h:?}");
        assert_eq!(*log.replaced.borrow(), 0, "{h:?}");
    }
}

#[test]
fn only_if_older_replaces_a_managed_older_daemon() {
    let log = Log::default();
    run_only_if_older(&log, &|_| Some(health(Some("0.9.0"), 6)), true);
    assert_eq!(*log.replaced.borrow(), 1);
    assert!(log.stopped.borrow().is_empty());
}

// ── `rocky daemon restart`: 버전과 상관없이 늘 교체한다 ─────────────────────────────

#[test]
fn always_restarts_a_same_version_daemon() {
    let log = Log::default();
    run_with_policy(
        &log,
        &|_| Some(health(Some("1.0.0"), 7)),
        false,
        RestartPolicy::Always,
    );
    assert_eq!(*log.stopped.borrow(), vec![Some(7)]);
    assert_eq!(*log.spawned.borrow(), 1);
}

#[test]
fn always_reregisters_a_managed_daemon_instead_of_killing_it() {
    let log = Log::default();
    run_with_policy(
        &log,
        &|_| Some(health(Some("1.0.0"), 8)),
        true,
        RestartPolicy::Always,
    );
    assert_eq!(*log.replaced.borrow(), 1);
    assert!(log.stopped.borrow().is_empty());
}

#[test]
fn always_spawns_when_nothing_is_running() {
    let log = Log::default();
    run_with_policy(&log, &|_| None, false, RestartPolicy::Always);
    assert_eq!(*log.spawned.borrow(), 1);
}

/// 받은편지함 등록 본문 — 세션 id·소켓·cwd 가 다 있어야 보낸다(없으면 등록하지 않는다).
#[test]
fn inbox_registration_needs_session_socket_and_cwd() {
    use rocky_cli::hooks::inbox_registration_body;
    let input = serde_json::json!({ "session_id": "s1", "cwd": "/w/rocky" });
    let body = inbox_registration_body(&input, Some("/tmp/cc-socks/1.sock"), None).unwrap();
    assert_eq!(body["sessionId"], "s1");
    assert_eq!(body["socket"], "/tmp/cc-socks/1.sock");
    assert_eq!(body["cwd"], "/w/rocky");
    // 훅 입력에 cwd 가 없으면 프로세스 cwd 로.
    let no_cwd = serde_json::json!({ "session_id": "s1" });
    assert_eq!(
        inbox_registration_body(&no_cwd, Some("/tmp/cc-socks/1.sock"), Some("/w/x")).unwrap()
            ["cwd"],
        "/w/x"
    );
    // 받은편지함이 없는 세션(환경 변수 없음)·세션 id 없음은 보내지 않는다.
    assert!(inbox_registration_body(&input, None, None).is_none());
    assert!(inbox_registration_body(&input, Some(""), None).is_none());
    assert!(inbox_registration_body(
        &serde_json::json!({}),
        Some("/tmp/cc-socks/1.sock"),
        Some("/w")
    )
    .is_none());
}

// ── launchd 밖의 고아 데몬 ───────────────────────────────────────────────────────
//
// 실제 사고(0.38.0): plist 는 v0.38.0 을 가리키는데 포트는 launchd 밖에서 뜬 v0.37.0(PPID 1)이 쥐고 있었다.
// 교체(bootout→bootstrap)는 launchd 자기 프로세스만 내려 고아가 남고, 새 데몬은 "already running" 으로
// 끝나는데 `rocky update`·`daemon restart` 는 성공이라 찍었다.

/// 고아가 포트를 쥔 채 교체가 "성공" 한 상황 — 고아 pid 를 내린 뒤 launchd 의 새 데몬이 받는다.
fn run_orphan(
    log: &Log,
    orphan_dies: bool,
    launchd_pid: Option<u32>,
    policy: RestartPolicy,
) -> Option<String> {
    let ctx = build_context(8636, std::env::temp_dir(), "test");
    let orphan_alive = Cell::new(true);
    let check = |_: &str| {
        Some(if orphan_alive.get() {
            health(Some("0.9.0"), 900)
        } else {
            health(Some("1.0.0"), 901)
        })
    };
    ensure_daemon_with_policy(
        &ctx,
        &EnsureDeps {
            version: "1.0.0",
            check_health: &check,
            spawn: &|_| {
                *log.spawned.borrow_mut() += 1;
                Ok(())
            },
            stop: &|_, pid| {
                log.stopped.borrow_mut().push(pid);
                if orphan_dies {
                    orphan_alive.set(false);
                }
                orphan_dies
            },
            is_managed: &|| true,
            replace_managed: &|| {
                *log.replaced.borrow_mut() += 1;
                Ok(())
            },
            managed_pid: &|| {
                if orphan_alive.get() {
                    launchd_pid
                } else {
                    Some(901)
                }
            },
            pause: &|| {},
        },
        policy,
    )
}

#[test]
fn an_orphan_holding_the_port_is_stopped_after_the_job_is_replaced() {
    for policy in [RestartPolicy::ExactVersion, RestartPolicy::Always] {
        let log = Log::default();
        // launchd 의 새 데몬은 떠서 포트를 기다리는 중(pid 901)
        let warning = run_orphan(&log, true, Some(901), policy);
        assert_eq!(*log.replaced.borrow(), 1);
        assert_eq!(log.stopped.borrow().as_slice(), &[Some(900)], "{policy:?}");
        assert_eq!(*log.spawned.borrow(), 0, "launchd 밖에서 또 띄우지 않는다");
        assert_eq!(warning, None, "{policy:?}");
    }
}

/// launchd 의 데몬이 "already running" 으로 끝나 `spawn scheduled` 인 상태(job pid 없음)도 같은 고아다.
#[test]
fn an_orphan_is_found_while_the_job_waits_to_respawn() {
    let log = Log::default();
    let warning = run_orphan(&log, true, None, RestartPolicy::ExactVersion);
    assert_eq!(log.stopped.borrow().as_slice(), &[Some(900)]);
    assert_eq!(warning, None);
}

/// 고아를 못 내리면 성공이라 하지 않는다 — 무엇이 포트에 남았는지와 확인 명령을 돌려준다.
#[test]
fn a_surviving_orphan_is_reported_not_called_a_success() {
    let log = Log::default();
    let warning =
        run_orphan(&log, false, Some(901), RestartPolicy::Always).expect("고아가 남으면 알린다");
    assert_eq!(
        log.stopped.borrow().as_slice(),
        &[Some(900)],
        "한 번만 내린다"
    );
    assert!(
        warning.contains("v0.9.0") && warning.contains("pid 900"),
        "{warning}"
    );
    assert!(warning.contains("pid 901"), "{warning}");
    assert!(warning.contains("rocky daemon status"), "{warning}");
}

/// 매 턴 훅은 5초 timeout 이라 교체 뒤 기다리지 않는다 — 고아 정리는 SessionStart·update·restart 몫.
#[test]
fn the_per_turn_hook_does_not_wait_for_the_replacement() {
    let log = Log::default();
    let warning = run_orphan(&log, true, Some(901), RestartPolicy::OnlyIfOlder);
    assert_eq!(*log.replaced.borrow(), 1);
    assert!(
        log.stopped.borrow().is_empty(),
        "매 턴엔 고아를 내리러 기다리지 않는다"
    );
    assert_eq!(warning, None);
}

/// pid 를 보고하지 않는 옛 데몬은 고아인지 모른다 — 함부로 내리지 않고, 버전이 안 맞으면 알린다.
#[test]
fn a_port_holder_without_a_pid_is_not_killed_but_reported() {
    let ctx = build_context(8636, std::env::temp_dir(), "test");
    let log = Log::default();
    let old = DaemonHealth {
        pid: None,
        ..health(Some("0.9.0"), 0)
    };
    let warning = ensure_daemon_with_policy(
        &ctx,
        &EnsureDeps {
            version: "1.0.0",
            check_health: &|_| Some(old.clone()),
            spawn: &|_| Ok(()),
            stop: &|_, pid| {
                log.stopped.borrow_mut().push(pid);
                true
            },
            is_managed: &|| true,
            replace_managed: &|| Ok(()),
            managed_pid: &|| Some(901),
            pause: &|| {},
        },
        RestartPolicy::Always,
    )
    .expect("버전이 안 맞으면 알린다");
    assert!(log.stopped.borrow().is_empty());
    assert!(
        warning.contains("v0.9.0") && warning.contains("rocky daemon status"),
        "{warning}"
    );
}

/// `hook claim-doing`(PostToolUse) — 방금 start 한 할 일의 id 를 응답에서 읽어 데몬에 이 세션의 귀속을 요청한다.
/// start 가 아니거나 서브에이전트면 아무것도 보내지 않는다. 데몬이 없어도 조용히 성공한다.
#[test]
fn claim_doing_hook_posts_the_started_todo_for_this_session() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let log = seen.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            // 머리와 본문이 따로 올 수 있다 — content-length 만큼 다 받을 때까지 읽는다.
            let mut raw = Vec::new();
            let mut buf = [0u8; 16384];
            loop {
                let n = stream.read(&mut buf).unwrap_or(0);
                if n == 0 {
                    break;
                }
                raw.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&raw);
                let Some(head_end) = text.find("\r\n\r\n") else {
                    continue;
                };
                let length = text[..head_end]
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length: "))
                    .and_then(|v| v.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                if raw.len() >= head_end + 4 + length {
                    break;
                }
            }
            log.lock()
                .unwrap()
                .push(String::from_utf8_lossy(&raw).into_owned());
            let _ = stream.write_all(b"HTTP/1.1 204 No Content\r\nconnection: close\r\n\r\n");
        }
    });
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("rocky.json");
    std::fs::write(
        &config,
        format!(r#"{{"todo":{{"port":{port},"dir":"/nonexistent","expose":"off"}}}}"#),
    )
    .unwrap();
    let todo = serde_json::json!({"id": "abc123", "status": "doing", "ref": "rocky-9"}).to_string();
    let run = |input: serde_json::Value| {
        let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_rocky"))
            .args(["hook", "claim-doing"])
            .env("HOME", dir.path())
            .env("ROCKY_USAGE_DIR", dir.path().join("usage"))
            .env("ROCKY_CONFIG", &config)
            .stdin(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.to_string().as_bytes())
            .unwrap();
        assert!(child.wait().unwrap().success());
    };
    let input = |action: &str, agent: Option<&str>| {
        let mut v = serde_json::json!({
            "session_id": "sess-1",
            "hook_event_name": "PostToolUse",
            "tool_name": "mcp__plugin_rocky_rocky__todo_status",
            "tool_input": {"id": "rocky-9", "action": action},
            "tool_response": [{"type": "text", "text": todo}],
        });
        if let Some(agent) = agent {
            v["agent_id"] = agent.into();
        }
        v
    };
    run(input("done", None));
    run(input("start", Some("sub-1")));
    assert!(
        seen.lock().unwrap().is_empty(),
        "보내면 안 되는 경우에 보냈다"
    );
    run(input("start", None));
    let requests = seen.lock().unwrap().join("\n");
    assert!(
        requests.starts_with("POST /api/sessions/doing"),
        "{requests}"
    );
    let body: serde_json::Value =
        serde_json::from_str(requests.split("\r\n\r\n").nth(1).unwrap_or_default()).unwrap();
    assert_eq!(
        body,
        serde_json::json!({"sessionId": "sess-1", "todoId": "abc123"})
    );
}
