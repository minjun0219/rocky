//! `GET /api/summary`(요약 JSON)와 `POST /api/sessions/doing`(세션이 스스로 든 doing 의 귀속). 걷은 statusline 라우트의
//! 테스트 파일에 같이 있던 것을 옮겨 왔다 — statusline 단언만 뺐다.

use std::sync::Arc;

use crate::common::*;
use rocky_core::sessions::SessionsResult;
use rocky_core::types::*;
use rockyd::server::ServerState;
use rockyd::sessions_exec::fixed_sessions;

fn fixture_sessions() -> SessionsResult {
    let mut bg = sess(2, "/w/rocky-todo", "shortid8-full-uuid", "rocky-bg", "idle");
    bg.kind = "background".into();
    bg.id = Some("shortid8".into());
    available(vec![
        sess(1, "/w/rocky-todo", "sess-live", "rocky-todo-1e", "busy"),
        bg,
    ])
}

fn state_with_sessions(f: &Fx) -> Arc<ServerState> {
    rebuild(f, |o| o.sessions = Some(fixed_sessions(fixture_sessions())))
}

/// 배달된 핸드오프로 시작된 doing — 핸드오프 귀속(세션이 스스로 든 것은 훅이 따로 귀속시킨다).
fn started_by_session(f: &Fx, session_id: &str, title: &str) -> Todo {
    let todo = f
        .store
        .create_todo(
            &CreateTodoInput {
                board: "rocky-todo".into(),
                title: title.into(),
                ..Default::default()
            },
            "logan",
        )
        .unwrap();
    f.store
        .create_handoff(&CreateHandoffInput {
            todo_ref: todo.id.clone(),
            session_id: session_id.into(),
            session_name: None,
            session_cwd: None,
            note: None,
            actor: "logan".into(),
            current_board_id: None,
        })
        .unwrap();
    f.store.claim_handoff(session_id, HandoffVia::Stop).unwrap();
    f.store
        .set_todo_status(&todo.id, StatusAction::Start, "claude-code", None)
        .unwrap();
    todo
}

#[tokio::test]
async fn summary_ignores_handoffs_of_archived_todos() {
    let f = fx();
    let todo = f
        .store
        .create_todo(
            &CreateTodoInput {
                board: "rocky-todo".into(),
                title: "보관될 것".into(),
                ..Default::default()
            },
            "logan",
        )
        .unwrap();
    f.store
        .create_handoff(&CreateHandoffInput {
            todo_ref: todo.id.clone(),
            session_id: "sess-9".into(),
            session_name: None,
            session_cwd: None,
            note: None,
            actor: "logan".into(),
            current_board_id: None,
        })
        .unwrap();
    // 배달만 되고 착수 없음 → 요약의 "대기" 1.
    f.store.claim_handoff("sess-9", HandoffVia::Stop).unwrap();
    let state = state_with_sessions(&f);
    let (_, body) = get(&state, "/api/summary?cwd=/w/rocky-todo").await;
    assert_eq!(body["handoffsOpen"], 1);
    // todo 를 보관하면 그 핸드오프는 더 이상 대기가 아니다.
    f.store
        .set_todo_status(&todo.id, StatusAction::Archive, "logan", None)
        .unwrap();
    let (_, body) = get(&state, "/api/summary?cwd=/w/rocky-todo").await;
    assert_eq!(body["handoffsOpen"], 0);
}

#[tokio::test]
async fn summary_route_returns_board_counts_and_items() {
    let f = fx();
    f.store
        .create_todo(
            &CreateTodoInput {
                board: "rocky-todo".into(),
                title: "지난 마감".into(),
                due: Some("2020-01-01".into()),
                ..Default::default()
            },
            "logan",
        )
        .unwrap();
    started_by_session(&f, "sess-1", "진행중 것");
    let state = state_with_sessions(&f);
    let (status, body) = get(&state, "/api/summary?cwd=/w/rocky-todo").await;
    assert_eq!(status, 200);
    assert_eq!(body["board"], "rocky-todo");
    assert_eq!(body["overdue"], 1);
    assert_eq!(body["doing"], 1);
    // 배달 뒤 start 로 수락됐으니 "대기" 는 0 — 스토어의 open(미완료 배달 포함)과는 다른 판정.
    assert_eq!(body["handoffsOpen"], 0);
    assert_eq!(body["items"][0]["kind"], "overdue");
    assert_eq!(body["items"][1]["kind"], "doing");
    // 수집함 캐시가 없으면 collect 는 생략(모름).
    let (_, body) = get(&state, "/api/summary?cwd=/w/rocky-todo&cached=true").await;
    assert!(body.get("collect").is_none());
}

#[tokio::test]
async fn a_self_started_doing_is_claimed_by_the_session_hook() {
    let f = fx();
    let todo = f
        .store
        .create_todo(
            &CreateTodoInput {
                board: "rocky-todo".into(),
                title: "스스로 든 것".into(),
                ..Default::default()
            },
            "logan",
        )
        .unwrap();
    f.store
        .set_todo_status(&todo.id, StatusAction::Start, "claude-code", None)
        .unwrap();
    let state = state_with_sessions(&f);
    let claimed_by = |f: &Fx| {
        f.store
            .get_todo(&todo.id, None)
            .unwrap()
            .unwrap()
            .doing_session_id
    };
    // 사람이 아니라 세션이 든 것이지만, 훅이 부르기 전에는 귀속이 없다.
    assert_eq!(claimed_by(&f), None);

    let body = serde_json::json!({"sessionId": "sess-live", "todoId": todo.id});
    let (status, _) = call(
        &state,
        "POST",
        "/api/sessions/doing",
        Some(body.clone()),
        ReqOptions {
            peer: Some("100.64.0.9"),
            ..ReqOptions::default()
        },
    )
    .await;
    assert_eq!(status, 404, "원격에는 없다");

    let (status, claimed) = post(&state, "/api/sessions/doing", body.clone()).await;
    assert_eq!(status, 200, "{claimed}");
    assert_eq!(claimed["doingSessionClaimed"], true);
    assert_eq!(claimed_by(&f).as_deref(), Some("sess-live"));
    // 다시 불러도 덮지 않는다(이미 귀속).
    let (status, _) = post(
        &state,
        "/api/sessions/doing",
        serde_json::json!({"sessionId": "sess-other", "todoId": todo.id}),
    )
    .await;
    assert_eq!(status, 204);
}
