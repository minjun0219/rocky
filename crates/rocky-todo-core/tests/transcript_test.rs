//! TS `src/hooks/transcript.test.ts` + `log-turn.test.ts`(shouldCapture) 포팅.

use rocky_todo_core::transcript::*;
use serde_json::json;

fn transcript() -> String {
    [
        json!({"type":"user","message":{"role":"user","content":[{"type":"text","text":"엔드포인트 검색해줘"}]}}),
        json!({"type":"assistant","message":{"role":"assistant","content":[
            {"type":"text","text":"검색합니다"},
            {"type":"tool_use","name":"worklog_search","input":{}},
            {"type":"tool_use","name":"worklog_search","input":{}}
        ]}}),
        json!({"type":"user","message":{"role":"user","content":[{"type":"tool_result","content":"ok"}]}}),
        json!({"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"3개 찾았습니다"}]}}),
    ]
    .iter()
    .map(|v| v.to_string())
    .collect::<Vec<_>>()
    .join("\n")
}

#[test]
fn extracts_last_prompt_tools_with_count_and_final_text() {
    let parts = extract_turn(&transcript()).unwrap();
    assert_eq!(parts.req, "엔드포인트 검색해줘");
    assert_eq!(parts.tools, vec!["worklog_search(×2)"]);
    assert_eq!(parts.did, "3개 찾았습니다");
}

#[test]
fn tool_result_only_user_message_is_not_a_prompt_boundary() {
    assert_eq!(
        extract_turn(&transcript()).unwrap().req,
        "엔드포인트 검색해줘"
    );
}

#[test]
fn returns_none_without_a_user_prompt() {
    let only_assistant = json!({"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"hi"}]}}).to_string();
    assert!(extract_turn(&only_assistant).is_none());
    assert!(extract_turn("").is_none());
}

#[test]
fn skips_malformed_lines() {
    let text = format!("not json\n{}\n{{\"partial\":", transcript());
    assert_eq!(extract_turn(&text).unwrap().req, "엔드포인트 검색해줘");
}

#[test]
fn string_content_is_accepted() {
    let text = json!({"message":{"role":"user","content":"  plain prompt  "}}).to_string();
    let parts = extract_turn(&text).unwrap();
    assert_eq!(parts.req, "plain prompt");
    assert!(parts.tools.is_empty());
    assert_eq!(parts.did, "");
}

#[test]
fn build_content_collapses_whitespace_and_joins() {
    let parts = TurnParts {
        req: "a  b\n\nc".into(),
        tools: vec!["x".into(), "y".into()],
        did: "done".into(),
    };
    assert_eq!(
        build_turn_content(&parts, 800),
        "req: a b c | tools: x, y | did: done"
    );
}

#[test]
fn build_content_truncates_with_ellipsis() {
    let parts = TurnParts {
        req: "abcdefgh".into(),
        tools: vec![],
        did: String::new(),
    };
    assert_eq!(
        build_turn_content(&parts, 4),
        "req: abcd… | tools: (none) | did: (none)"
    );
}

#[test]
fn build_content_caps_tools_at_20() {
    let parts = TurnParts {
        req: String::new(),
        tools: (0..25).map(|i| format!("t{i}")).collect(),
        did: String::new(),
    };
    let s = build_turn_content(&parts, 800);
    assert!(s.starts_with("req: (none) | tools: t0, t1"));
    assert!(s.contains("did: (none)"));
    let tools = s
        .split("tools: ")
        .nth(1)
        .unwrap()
        .split(" | ")
        .next()
        .unwrap();
    assert_eq!(tools.split(", ").count(), 20);
}

#[test]
fn should_capture_defaults_and_overrides() {
    assert!(should_capture(None, None));
    assert!(should_capture(None, Some(true)));
    assert!(!should_capture(None, Some(false)));
    for v in ["0", "false", "off", "no", " OFF "] {
        assert!(!should_capture(Some(v), Some(true)), "{v}");
    }
    assert!(should_capture(Some("1"), Some(false)));
    assert!(should_capture(Some("   "), None));
}
