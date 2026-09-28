//! `mcp worklog` 표면 가드 — TS `src/index.test.ts` 의 도구 목록 검사 대응.

use rocky_cli::worklog_mcp::WorklogMcp;

#[test]
fn exposes_exactly_the_four_worklog_tools() {
    let mut names = WorklogMcp::tool_names();
    names.sort();
    assert_eq!(
        names,
        vec![
            "worklog_append",
            "worklog_read",
            "worklog_search",
            "worklog_status"
        ]
    );
}

/// 채널 선언 — `capabilities.experimental["claude/channel"]` 이 있어야 Claude Code 가 알림
/// 리스너를 건다. 도구 표면(4개)은 그대로다.
#[test]
fn declares_the_claude_channel_capability_and_instructions() {
    use rmcp::ServerHandler;
    use rocky_cli::channel::{channel_notification, CHANNEL_CAPABILITY, CHANNEL_METHOD};
    let dir = tempfile::tempdir().unwrap();
    let server = WorklogMcp::new(rocky_core::worklog::Worklog::from_env(
        Some(dir.path().to_string_lossy().as_ref()),
        None,
        Some(dir.path().to_path_buf()),
    ));
    let info = server.get_info();
    let experimental = info.capabilities.experimental.expect("experimental");
    assert!(experimental.contains_key(CHANNEL_CAPABILITY));
    assert!(info.capabilities.tools.is_some(), "도구는 그대로");
    assert!(info.instructions.unwrap().contains("<channel"));
    let n = channel_notification(
        "#3 확인·머지해도 된다",
        &std::collections::BTreeMap::from([("kind".to_string(), "ready".to_string())]),
    );
    let wire = serde_json::to_value(&n).unwrap();
    assert_eq!(wire["method"], CHANNEL_METHOD);
    assert_eq!(wire["params"]["content"], "#3 확인·머지해도 된다");
    assert_eq!(wire["params"]["meta"]["kind"], "ready");
}
