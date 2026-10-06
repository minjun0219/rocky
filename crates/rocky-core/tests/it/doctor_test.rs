//! `rocky doctor` 의 실행 상태 판정 — 데몬 JSON 을 읽어 항목마다 ✓/⚠ 와 고치는 명령.

use chrono::{DateTime, Utc};
use rocky_core::doctor::{render, runtime_checks, RuntimeInput};
use rocky_core::setup::{Check, SetupReport};
use serde_json::json;

fn now() -> DateTime<Utc> {
    "2026-10-06T12:00:00Z".parse().unwrap()
}

fn find<'a>(checks: &'a [Check], id: &str) -> &'a Check {
    checks.iter().find(|c| c.id == id).unwrap()
}

fn healthy() -> serde_json::Value {
    json!({
        "dbIntegrity": "ok",
        "prWatch": { "available": true, "lastTick": "2026-10-06T11:58:00Z", "repos": ["o/r"] },
    })
}

#[test]
fn a_healthy_daemon_is_all_green_and_skips_what_it_did_not_get() {
    let checks = runtime_checks(
        &RuntimeInput {
            health: healthy(),
            ..Default::default()
        },
        now(),
    );
    // 받지 못한 응답(옛 데몬·로컬 아님)의 항목은 없다
    assert_eq!(
        checks.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
        vec!["db", "pr-watch"]
    );
    assert!(checks.iter().all(|c| c.ok), "{checks:?}");
    assert!(find(&checks, "pr-watch").detail.contains("2분 전"));
}

#[test]
fn db_and_pr_watch_problems_carry_a_fix() {
    let health = json!({
        "dbIntegrity": "*** in database main ***",
        "prWatch": { "available": false, "reason": "gh 인증 없음" },
    });
    let checks = runtime_checks(
        &RuntimeInput {
            health,
            ..Default::default()
        },
        now(),
    );
    assert!(!find(&checks, "db").ok);
    let pr = find(&checks, "pr-watch");
    assert!(!pr.ok);
    assert!(pr.detail.contains("gh 인증 없음"));
    assert_eq!(pr.fix.as_deref(), Some("gh auth status"));

    // 3분마다 도는데 15분 넘게 tick 이 없으면 멈춘 것
    let stale = json!({ "dbIntegrity": "ok", "prWatch": { "available": true, "lastTick": "2026-10-06T11:40:00Z" } });
    let checks = runtime_checks(
        &RuntimeInput {
            health: stale,
            ..Default::default()
        },
        now(),
    );
    assert!(!find(&checks, "pr-watch").ok);

    // 꺼 둔 감시·옛 데몬의 빈 칸은 문제가 아니다
    let off = json!({ "prWatch": null });
    let checks = runtime_checks(
        &RuntimeInput {
            health: off,
            ..Default::default()
        },
        now(),
    );
    assert!(checks.iter().all(|c| c.ok), "{checks:?}");
}

#[test]
fn a_failed_default_branch_verify_names_the_commit() {
    let verify = json!({ "targets": [
        { "board": "rocky", "branch": "main", "record": { "state": "failed", "subject": "feat: x (#1)" } },
        { "board": "tally", "branch": "main", "record": { "state": "passed", "subject": "y" } },
    ]});
    let checks = runtime_checks(
        &RuntimeInput {
            health: healthy(),
            verify: Some(verify),
            ..Default::default()
        },
        now(),
    );
    let v = find(&checks, "verify");
    assert!(!v.ok);
    assert!(v.detail.contains("rocky/main feat: x (#1)"));
    assert!(!v.detail.contains("tally"));
}

#[test]
fn delivery_failures_count_only_the_last_day() {
    let deliveries = |at: &str| {
        json!({
            "sessions": [{ "sessionId": "a" }],
            "ended": 2,
            "recent": [
                { "at": at, "ok": false, "reason": "받을 세션 등록 없음" },
                { "at": "2026-10-06T11:00:00Z", "ok": true },
            ],
        })
    };
    let recent = runtime_checks(
        &RuntimeInput {
            health: healthy(),
            deliveries: Some(deliveries("2026-10-06T11:30:00Z")),
            ..Default::default()
        },
        now(),
    );
    let d = find(&recent, "deliveries");
    assert!(!d.ok);
    assert!(d.detail.contains("1건") && d.detail.contains("받을 세션 등록 없음"));

    let old = runtime_checks(
        &RuntimeInput {
            health: healthy(),
            deliveries: Some(deliveries("2026-10-04T11:30:00Z")),
            ..Default::default()
        },
        now(),
    );
    let d = find(&old, "deliveries");
    assert!(d.ok);
    assert!(d.detail.contains("받는 세션 1개, 끝난 세션 2개 뺌"));
}

#[test]
fn rc_flags_logout_and_pinned_servers_that_are_down() {
    let rc = |auth: &str| {
        json!({
            "configured": true,
            "auth": auth,
            "servers": [
                { "label": "rocky", "pinned": true, "running": false },
                { "label": "tally", "pinned": false, "running": false },
                { "label": "web", "pinned": true, "running": true },
            ],
        })
    };
    let run = |v| {
        runtime_checks(
            &RuntimeInput {
                health: healthy(),
                rc: Some(v),
                ..Default::default()
            },
            now(),
        )
    };
    let out = run(rc("out"));
    assert!(find(&out, "rc").detail.contains("로그아웃"));
    let down = run(rc("in"));
    let c = find(&down, "rc");
    assert!(!c.ok);
    // 고정이 아닌 꺼진 서버는 문제로 치지 않는다
    assert!(c.detail.contains("1개 꺼짐: rocky"), "{}", c.detail);
    assert!(run(json!({ "configured": false })).iter().all(|c| c.ok));
}

#[test]
fn render_puts_the_fix_under_a_failed_check_and_says_when_the_daemon_is_unreachable() {
    let setup = SetupReport {
        checks: vec![],
        next_steps: vec![],
    };
    let health = json!({ "dbIntegrity": "ok", "prWatch": { "available": false, "reason": "r" } });
    let checks = runtime_checks(
        &RuntimeInput {
            health,
            ..Default::default()
        },
        now(),
    );
    let text = render(&setup, Some(&checks));
    assert!(text.contains("⚠ pr-watch"));
    assert!(text.contains("→ gh auth status"));
    assert!(text.contains("✓ db"));
    assert!(
        !text.contains("→ ~/.config"),
        "통과한 항목에는 고치는 명령을 달지 않는다"
    );
    assert!(render(&setup, None).contains("데몬에 닿지 못해"));
}
