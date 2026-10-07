//! TS `src/hooks/transcript.test.ts` + `log-turn.test.ts`(shouldCapture) 포팅.

use rocky_core::transcript::*;
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

/// 앞쪽에 긴 지난 턴들 + 마지막 턴. 끝에서부터 읽어도 파일 전체를 읽은 것과 같아야 한다.
fn long_transcript() -> String {
    let mut lines = Vec::new();
    for i in 0..200 {
        lines.push(json!({"type":"user","message":{"role":"user","content":format!("지난 요청 {i} {}", "x".repeat(300))}}).to_string());
        lines.push(json!({"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":format!("지난 답 {i}")}]}}).to_string());
    }
    // 마지막 턴 — 프롬프트 뒤에 도구 결과·도구 호출이 길게 붙는다(창 하나를 넘긴다).
    lines
        .push(json!({"type":"user","message":{"role":"user","content":"마지막 요청"}}).to_string());
    for _ in 0..40 {
        lines.push(json!({"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","name":"Bash","input":{"command":"y".repeat(200)}}]}}).to_string());
        lines.push(json!({"type":"user","message":{"role":"user","content":[{"type":"tool_result","content":"z".repeat(200)}]}}).to_string());
    }
    lines.push(json!({"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"끝"}]}}).to_string());
    lines.join("\n")
}

#[test]
fn reading_from_the_tail_matches_reading_the_whole_file() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("t.jsonl");
    let text = long_transcript();
    std::fs::write(&path, &text).unwrap();
    let whole = extract_turn(&text).expect("마지막 턴");
    assert_eq!(whole.req, "마지막 요청");
    assert_eq!(whole.tools, vec!["Bash(×40)".to_string()]);
    // 창이 작아 프롬프트가 안 잡히면 넓히고, 잘린 첫 줄은 버린다 — 어떤 창이든 결과가 같다.
    for window in [1, 100, 1_000, 10_000, TAIL_WINDOW, 1 << 30] {
        assert_eq!(
            extract_turn_from_tail(&path, window),
            Some(whole.clone()),
            "window {window}"
        );
    }
}

#[test]
fn reading_from_the_tail_without_a_prompt_is_none() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("t.jsonl");
    let only_tools = json!({"type":"user","message":{"role":"user","content":[{"type":"tool_result","content":"ok"}]}}).to_string();
    std::fs::write(&path, format!("{only_tools}\n{only_tools}")).unwrap();
    assert_eq!(extract_turn_from_tail(&path, 10), None);
    assert_eq!(
        extract_turn_from_tail(&tmp.path().join("없음.jsonl"), 10),
        None
    );
}

/// 하네스가 넣은 메시지는 턴을 나누되 요청 칸엔 짧은 이름만 — 원문은 회고에서 잡음이다.
#[test]
fn injected_messages_keep_the_turn_boundary_but_get_a_short_label() {
    let notification = [
        json!({"type":"user","message":{"role":"user","content":"진짜 요청"}}),
        json!({"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"앞 턴"}]}}),
        json!({"type":"user","message":{"role":"user","content":"<task-notification> <task-id>b1</task-id> ..."}}),
        json!({"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"알림 처리"}]}}),
    ]
    .iter()
    .map(|v| v.to_string())
    .collect::<Vec<_>>()
    .join("\n");
    let turn = extract_turn(&notification).unwrap();
    assert_eq!(turn.req, "(백그라운드 작업 알림)");
    assert_eq!(turn.did, "알림 처리", "앞 턴과 겹치지 않는다");
    assert_eq!(
        label_injected("<bash-stdout>ok</bash-stdout>".into()),
        "(셸 출력)"
    );
    assert_eq!(label_injected("평범한 요청".into()), "평범한 요청");
}

// ── Antigravity(agy) 트랜스크립트 ───────────────────────────────────────────

/// 실측(agy 1.2.17) 모양 — 사람 입력은 `<USER_REQUEST>` 로 감싸이고 하네스 블록이 붙는다. 첫 턴, 둘째 턴(훅이 넣은
/// `userMessage`·ephemeral 이 끼어 있다) 순서.
fn agy_transcript() -> String {
    [
        json!({"step_index":0,"source":"USER_EXPLICIT","type":"USER_INPUT","content":"<USER_REQUEST>\n앞 턴\n</USER_REQUEST>"}),
        json!({"step_index":1,"source":"MODEL","type":"PLANNER_RESPONSE","content":"앞 턴 끝"}),
        json!({"step_index":2,"source":"USER_EXPLICIT","type":"USER_INPUT",
               "content":"<USER_REQUEST>\n버튼 색을 바꿔 줘\n</USER_REQUEST>\n<ADDITIONAL_METADATA>\nThe current local time is: 2026-10-07T08:35:48+09:00.\n</ADDITIONAL_METADATA>"}),
        json!({"step_index":3,"source":"SYSTEM_SDK","type":"EPHEMERAL_MESSAGE","content":"# rocky: 마지막 확인 이후 호출자의 보드 변경"}),
        json!({"step_index":4,"source":"SYSTEM_SDK","type":"USER_INPUT","content":"<USER_REQUEST>\n훅이 넣은 메시지\n</USER_REQUEST>"}),
        json!({"step_index":5,"source":"MODEL","type":"PLANNER_RESPONSE","thinking":"생각은 보지 않는다",
               "tool_calls":[{"name":"view_file","args":{"AbsolutePath":"/w/a.css"}},
                             {"name":"call_mcp_tool","args":{"ServerName":"rocky_rocky","ToolName":"todo_status","Arguments":{}}}]}),
        json!({"step_index":6,"source":"MODEL","type":"GENERIC","content":"도구 결과는 한 일이 아니다"}),
        json!({"step_index":7,"source":"MODEL","type":"PLANNER_RESPONSE","content":"중간 보고",
               "tool_calls":[{"name":"view_file","args":{}},{"name":"replace_file_content","args":{}}]}),
        json!({"step_index":8,"source":"MODEL","type":"PLANNER_RESPONSE","content":"초록으로 바꿨습니다"}),
    ]
    .iter()
    .map(|v| v.to_string())
    .collect::<Vec<_>>()
    .join("\n")
}

/// 마지막 사람 입력부터 — `<USER_REQUEST>` 안쪽만 요청으로, 훅이 넣은 `USER_INPUT`(`SYSTEM_SDK`)은 경계가 아니다. MCP 도구는
/// `call_mcp_tool` 대신 Claude Code 와 같은 `mcp__서버__도구` 로 센다.
#[test]
fn agy_takes_the_last_human_turn() {
    assert_eq!(
        agy::extract_turn(&agy_transcript()),
        Some(TurnParts {
            req: "버튼 색을 바꿔 줘".into(),
            tools: vec![
                "view_file(×2)".into(),
                "mcp__rocky_rocky__todo_status".into(),
                "replace_file_content".into()
            ],
            did: "초록으로 바꿨습니다".into(),
        })
    );
}

#[test]
fn agy_without_a_human_input_is_none() {
    let only_model =
        json!({"source":"MODEL","type":"PLANNER_RESPONSE","content":"혼잣말"}).to_string();
    assert_eq!(agy::extract_turn(&only_model), None);
    assert_eq!(agy::extract_turn("깨진 줄\n{\"source\":"), None);
}

/// 끝에서 읽어도 파일 전체를 읽은 것과 같다 — 작은 창에서 시작해 사람 입력이 나올 때까지 넓힌다.
#[test]
fn agy_tail_matches_the_whole_file() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("transcript_full.jsonl");
    std::fs::write(&path, agy_transcript()).unwrap();
    let whole = agy::extract_turn(&agy_transcript());
    for window in [1, 64, 1024, 1 << 20] {
        assert_eq!(
            agy::extract_turn_from_tail(&path, window),
            whole,
            "window {window}"
        );
    }
}
