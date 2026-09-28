//! 설치·설정 점검 — 재료 → 체크 목록/다음 할 일.

use rocky_core::config::{ExposeChannel, ExposeValue, InboxSource, TodoConfig};
use rocky_core::setup::{
    build_report, default_config_json, render_report, statusline_snippet, statusline_wired,
    CheckKind, ConfigFileState, DaemonState, SetupInput,
};
use rocky_core::statusline::BoardLocation;

fn input() -> SetupInput {
    SetupInput {
        config: ConfigFileState {
            path: "/home/u/.config/rocky/rocky.json".into(),
            exists: true,
            valid_json: true,
        },
        todo: TodoConfig::default(),
        port: 8636,
        cli_version: "0.26.1".into(),
        install_current: Some("v0.26.1".into()),
        daemon: Some(DaemonState {
            version: Some("0.26.1".into()),
            pid: Some(42),
        }),
        launchd_registered: true,
        statusline_command: Some("/home/u/.claude/statusline.sh".into()),
        statusline_script: Some("curl -sf http://127.0.0.1:8636/api/statusline?cwd=$cwd".into()),
        cwd: Some("/w/rocky".into()),
        repo_key: Some("rocky".into()),
        boards: vec![BoardLocation {
            key: "rocky".into(),
            path: Some("/w/rocky".into()),
        }],
        cli_on_path: Some("/home/u/.local/bin/rocky".into()),
        cli_link: Some("/home/u/.local/share/rocky/current/rocky".into()),
        local_bin_on_path: true,
    }
}

#[test]
fn cli_check_tells_apart_missing_link_and_missing_path() {
    let mut i = input();
    i.cli_on_path = None;
    i.cli_link = None;
    let c = build_report(&i).check("cli").unwrap().clone();
    assert!(!c.ok && c.fix.as_deref() == Some("rocky config link"));

    let mut i = input();
    i.cli_on_path = None;
    i.local_bin_on_path = false;
    let c = build_report(&i).check("cli").unwrap().clone();
    assert!(!c.ok && c.fix.as_deref().unwrap().contains("export PATH"));

    let r = build_report(&input());
    assert!(r.check("cli").unwrap().ok);
}

#[test]
fn fully_set_up_machine_has_no_next_steps() {
    let r = build_report(&input());
    assert!(r.checks.iter().all(|c| c.ok), "{:?}", r.checks);
    assert!(r.next_steps.is_empty());
    let text = render_report(&r);
    assert!(text.starts_with("✓ config"));
    assert!(!text.contains("다음 할 일"));
}

#[test]
fn missing_pieces_become_ordered_next_steps() {
    let mut i = input();
    i.config.exists = false;
    i.daemon = None;
    i.launchd_registered = false;
    i.statusline_command = None;
    i.statusline_script = None;
    i.boards[0].path = None;
    let r = build_report(&i);
    // Required(데몬) 가 Optional 들보다 앞.
    assert_eq!(r.next_steps[0], "rocky daemon start");
    assert!(r.next_steps.contains(&"rocky config init".to_string()));
    assert!(r.next_steps.contains(&"rocky daemon install".to_string()));
    assert!(r.next_steps.contains(&"rocky board path".to_string()));
    assert!(r
        .next_steps
        .iter()
        .any(|s| s.contains("/api/statusline?cwd=$cwd&session=$sid")));
    let text = render_report(&r);
    assert!(text.contains("✗ daemon"));
    assert!(text.contains("· config"));
    assert!(text.contains("다음 할 일:\n  1. rocky daemon start"));
}

#[test]
fn broken_config_json_is_required_not_optional() {
    let mut i = input();
    i.config.valid_json = false;
    let r = build_report(&i);
    let c = r.check("config").unwrap();
    assert_eq!(c.kind, CheckKind::Required);
    assert!(!c.ok);
    assert!(c.detail.contains("기본값으로 무시"));
}

#[test]
fn daemon_version_mismatch_is_flagged_with_restart() {
    let mut i = input();
    i.daemon = Some(DaemonState {
        version: Some("0.25.0".into()),
        pid: Some(7),
    });
    let r = build_report(&i);
    let c = r.check("daemon").unwrap();
    assert!(!c.ok);
    assert!(c.detail.contains("v0.25.0") && c.detail.contains("CLI 는 v0.26.1"));
    assert_eq!(
        c.fix.as_deref(),
        Some("rocky daemon stop && rocky daemon start")
    );
}

#[test]
fn info_checks_show_effective_values() {
    let mut i = input();
    i.todo.session_summary = Some(false);
    i.todo.expose = Some(ExposeValue::Channels(vec![
        ExposeChannel::Lan,
        ExposeChannel::TailscaleServe,
    ]));
    i.todo.inbox = vec![InboxSource {
        name: "todoist".into(),
        command: vec!["x".into()],
        timeout_ms: None,
    }];
    let r = build_report(&i);
    assert!(!r.check("session-summary").unwrap().ok);
    assert_eq!(r.check("expose").unwrap().detail, "lan, tailscale-serve");
    assert_eq!(r.check("inbox").unwrap().detail, "todoist");
    // 기본 expose 는 off 표시.
    let r = build_report(&input());
    assert!(r.check("expose").unwrap().detail.starts_with("off"));
}

#[test]
fn board_check_covers_missing_board_and_path_mismatch() {
    let mut i = input();
    i.boards = vec![];
    let r = build_report(&i);
    let c = r.check("board").unwrap();
    assert!(!c.ok && c.detail.contains("보드 `rocky` 없음"));

    let mut i = input();
    i.boards[0].path = Some("/elsewhere".into());
    let r = build_report(&i);
    let c = r.check("board").unwrap();
    assert!(!c.ok && c.detail.contains("cwd 를 덮지 않는다"));

    let mut i = input();
    i.repo_key = None;
    let r = build_report(&i);
    assert!(r.check("board").unwrap().ok);
}

#[test]
fn statusline_wiring_is_detected_in_command_or_script() {
    assert!(statusline_wired(
        Some("curl http://127.0.0.1:8636/api/statusline"),
        None
    ));
    assert!(statusline_wired(
        Some("/x/statusline.sh"),
        Some("rt=$(curl -sf \"http://127.0.0.1:8636/api/statusline?cwd=$cwd\")")
    ));
    assert!(!statusline_wired(
        Some("/x/cc-usage statusline"),
        Some("echo hi")
    ));
    assert!(!statusline_wired(None, None));
    assert!(statusline_snippet(9000).contains("127.0.0.1:9000/api/statusline"));
}

#[test]
fn default_config_is_valid_json_with_schema_and_safe_defaults() {
    let text = default_config_json();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert!(v["$schema"]
        .as_str()
        .unwrap()
        .ends_with("rocky.schema.json"));
    assert_eq!(v["todo"]["expose"], "off");
    assert_eq!(v["todo"]["sessionSummary"], true);
    assert!(text.ends_with('\n'));
}

#[test]
fn report_json_shape() {
    let r = build_report(&input());
    let v = serde_json::to_value(&r).unwrap();
    assert_eq!(v["checks"][0]["id"], "config");
    assert_eq!(v["checks"][0]["kind"], "optional");
    assert!(v["checks"][0].get("fix").is_none());
    assert_eq!(v["nextSteps"], serde_json::json!([]));
}
