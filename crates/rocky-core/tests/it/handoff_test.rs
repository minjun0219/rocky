//! TS 원본 `src/handoff.test.ts` 포팅.

use rocky_core::handoff::{
    build_handoff_poke, build_handoff_prompt, build_handoff_prompt_from, held_by_session,
    held_todo_reminder, HandoffPokeInput, HandoffPromptInput, HeldTodo,
};
use rocky_core::types::*;

fn base() -> ClaimedHandoff {
    ClaimedHandoff {
        handoff: Handoff {
            id: "h1".into(),
            todo_id: "t1".into(),
            session_id: "sess-1".into(),
            session_name: Some("eelpout-a3".into()),
            session_cwd: None,
            note: String::new(),
            actor: "logan".into(),
            status: HandoffStatus::Delivered,
            created_at: "2026-07-26T12:00:00.000Z".into(),
            delivered_at: None,
            delivered_via: None,
            accepted_at: None,
            completed_at: None,
        },
        todo_ref: "rocky#11".into(),
        todo_title: "todo - 에이전트 작업 요청".into(),
        remaining: 0,
    }
}

#[test]
fn prompt_carries_actor_ref_title() {
    let prompt = build_handoff_prompt(&base());
    assert!(prompt.contains("logan → rocky#11"));
    assert!(prompt.contains("todo - 에이전트 작업 요청"));
    assert!(prompt.contains("todo_status"));
}

#[test]
fn prompt_carries_note_when_present() {
    let mut claimed = base();
    claimed.handoff.note = "테스트부터 짜줘".into();
    assert!(build_handoff_prompt(&claimed).contains("메모: 테스트부터 짜줘"));
}

#[test]
fn prompt_has_no_note_line_when_empty() {
    assert!(!build_handoff_prompt(&base()).contains("메모:"));
}

#[test]
fn prompt_mentions_remaining_count() {
    let mut claimed = base();
    claimed.remaining = 2;
    assert!(build_handoff_prompt(&claimed).contains("2건"));
}

#[test]
fn prompt_has_no_remaining_line_when_zero() {
    assert!(!build_handoff_prompt(&base()).contains("대기 중인 요청이"));
}

#[test]
fn prompt_from_builds_same_without_claim() {
    let prompt = build_handoff_prompt_from(&HandoffPromptInput {
        actor: "logan",
        note: "테스트부터",
        todo_ref: "rocky#16",
        todo_title: "세션 띄우기",
        remaining: 0,
    });
    assert!(prompt.contains("logan → rocky#16 \"세션 띄우기\""));
    assert!(prompt.contains("메모: 테스트부터"));
    assert!(!prompt.contains("대기 중인 요청이"));
}

#[test]
fn poke_shape() {
    let poke = build_handoff_poke(&HandoffPokeInput {
        session_name: "eelpout-a3",
        todo_ref: "rocky-todo-11",
        todo_title: "세션 띄우기",
    });
    // to 는 세션 이름
    assert_eq!(poke.to, "eelpout-a3");
    // 참조와 제목으로 어느 건인지 알아볼 수 있다
    assert!(poke.message.contains("rocky-todo-11"));
    assert!(poke.message.contains("세션 띄우기"));
    // 훅 주입이 없어도 착수할 수 있는 폴백
    assert!(poke.message.contains("todo_list { id: \"rocky-todo-11\" }"));
    // 본문(메모·착수 지시)은 싣지 않는다 — 같은 턴의 훅 주입과 겹친다
    assert!(!poke.message.contains("todo_status"));
    assert!(!poke.message.contains("메모:"));
}

/// 착수만 말하고 끝을 말하지 않으면 세션은 일을 마치고도 doing 으로 남겨 둔다.
#[test]
fn the_prompt_says_how_to_close_the_todo() {
    let prompt = build_handoff_prompt(&base());
    assert!(prompt.contains("action: \"done\""), "{prompt}");
    assert!(prompt.contains("action: \"stop\""), "{prompt}");
}

#[test]
fn held_todos_are_the_doing_ones_attributed_to_this_session() {
    let todos = serde_json::json!([
        { "ref": "rocky-1", "title": "내 것", "status": "doing", "doingSessionId": "s1" },
        { "ref": "rocky-2", "title": "남의 것", "status": "doing", "doingSessionId": "s2" },
        { "ref": "rocky-3", "title": "사람이 든 것", "status": "doing" },
        { "ref": "rocky-4", "title": "끝난 것", "status": "done", "doingSessionId": "s1" },
        { "ref": "rocky-5", "title": "보관", "status": "doing", "doingSessionId": "s1", "archivedAt": "2026-10-01T00:00:00Z" },
        { "ref": "rocky-6", "title": "PR 올림", "status": "doing", "doingSessionId": "s1",
          "links": [{ "url": "https://example.com" }, { "url": "https://github.com/o/r/pull/3" }] }
    ]);
    assert_eq!(
        held_by_session(&todos, "s1"),
        vec![
            HeldTodo {
                todo_ref: "rocky-1".into(),
                title: "내 것".into(),
                awaits_pr: false,
                claimed: false,
            },
            HeldTodo {
                todo_ref: "rocky-6".into(),
                title: "PR 올림".into(),
                awaits_pr: true,
                claimed: false,
            }
        ]
    );
    assert!(held_by_session(&serde_json::json!({ "error": "x" }), "s1").is_empty());
}

#[test]
fn the_stop_reminder_asks_once_and_never_loops() {
    let held = vec![HeldTodo {
        todo_ref: "rocky-1".into(),
        title: "보드 피드".into(),
        awaits_pr: false,
        claimed: false,
    }];
    let reminder = held_todo_reminder(&held, false).expect("들고 있으면 묻는다");
    assert!(
        reminder.contains("rocky-1") && reminder.contains("보드 피드"),
        "{reminder}"
    );
    assert!(
        reminder.contains("\"done\"") && reminder.contains("\"stop\""),
        "{reminder}"
    );
    // 그 확인으로 이어진 턴에서 또 막으면 "아직 하는 중" 인 세션을 영영 못 멈춘다.
    assert_eq!(held_todo_reminder(&held, true), None);
    assert_eq!(held_todo_reminder(&[], false), None);
}

/// PR 을 링크한 할 일은 머지를 기다리는 중이다 — 턴마다 막지 않는다(머지되면 데몬이 완료한다).
#[test]
fn the_stop_reminder_skips_todos_waiting_on_a_pr() {
    let waiting = HeldTodo {
        todo_ref: "rocky-2".into(),
        title: "PR 올림".into(),
        awaits_pr: true,
        claimed: false,
    };
    assert_eq!(
        held_todo_reminder(std::slice::from_ref(&waiting), false),
        None
    );
    let both = vec![
        waiting,
        HeldTodo {
            todo_ref: "rocky-1".into(),
            title: "아직 하는 중".into(),
            awaits_pr: false,
            claimed: false,
        },
    ];
    let reminder = held_todo_reminder(&both, false).expect("PR 없는 것은 묻는다");
    assert!(
        reminder.contains("rocky-1") && !reminder.contains("rocky-2"),
        "{reminder}"
    );
}

/// 세션이 스스로 든 것(훅이 귀속 — `doingSessionClaimed`)도 이 세션의 것이지만 Stop 에서는 묻지 않는다(오너 결정 2026-10-06).
#[test]
fn self_started_todos_are_held_but_not_asked_about() {
    let todos = serde_json::json!([
        { "ref": "rocky-7", "title": "스스로 든 것", "status": "doing", "doingSessionId": "s1", "doingSessionClaimed": true },
        { "ref": "rocky-8", "title": "넘겨받은 것", "status": "doing", "doingSessionId": "s1" }
    ]);
    let held = held_by_session(&todos, "s1");
    assert_eq!(held.len(), 2);
    assert!(held[0].claimed && !held[1].claimed);
    let reminder = held_todo_reminder(&held, false).expect("넘겨받은 것은 묻는다");
    assert!(
        reminder.contains("rocky-8") && !reminder.contains("rocky-7"),
        "{reminder}"
    );
    assert_eq!(held_todo_reminder(&held[..1], false), None);
}

/// PostToolUse 입력에서 방금 start 한 할 일 — MCP 응답은 문자열 JSON 을 품은 content 블록이다. start 가 아니거나 실패했으면 없다.
#[test]
fn started_todo_ids_read_the_mcp_response() {
    use rocky_core::handoff::started_todo_ids;
    let todo =
        serde_json::json!({"id": "abc123", "ref": "rocky-9", "status": "doing", "title": "t"})
            .to_string();
    let input = |action: &str, response: serde_json::Value| {
        serde_json::json!({
            "session_id": "s1",
            "tool_name": "mcp__plugin_rocky_rocky__todo_status",
            "tool_input": {"id": "rocky-9", "action": action},
            "tool_response": response,
        })
    };
    let blocks = serde_json::json!([{"type": "text", "text": todo}]);
    assert_eq!(
        started_todo_ids(&input("start", blocks.clone())),
        ["abc123"]
    );
    assert_eq!(
        started_todo_ids(&input(
            "start",
            serde_json::json!({"content": blocks.clone()})
        )),
        ["abc123"]
    );
    assert!(started_todo_ids(&input("done", blocks)).is_empty());
    // 실패한 start — 에러 문구뿐이다.
    let failed = serde_json::json!([{"type": "text", "text": "todo not found: rocky-9"}]);
    assert!(started_todo_ids(&input("start", failed)).is_empty());
    // 이미 끝난 할 일의 응답(status done)도 아니다.
    let done = serde_json::json!({"id": "abc123", "status": "done"}).to_string();
    assert!(started_todo_ids(&input(
        "start",
        serde_json::json!([{"type": "text", "text": done}])
    ))
    .is_empty());
}
