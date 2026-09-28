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
        },
        RestartPolicy::OnlyIfOlder,
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
