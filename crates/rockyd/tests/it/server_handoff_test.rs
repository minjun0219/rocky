//! TS `src/server.test.ts` 포팅 3/4 — handoff routes + doingState + claim 게이트.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use crate::common::*;
use rocky_core::sessions::SessionsResult;
use rocky_core::types::*;
use rockyd::sessions_exec::{fixed_sessions, SessionsProvider};
use serde_json::{json, Value};

fn fixture_sessions() -> SessionsResult {
    available(vec![
        sess(1, "/w/rocky-todo", "sess-1", "rocky-todo-1e", "idle"),
        sess(2, "/w/forses", "sess-2", "forses-90", "busy"),
    ])
}

fn counting(counter: Arc<AtomicUsize>, result: SessionsResult) -> SessionsProvider {
    Arc::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
        let result = result.clone();
        Box::pin(async move { result })
    })
}

fn create(f: &Fx, board: &str, title: &str) -> Todo {
    f.store
        .create_todo(
            &CreateTodoInput {
                board: board.into(),
                title: title.into(),
                ..Default::default()
            },
            "logan",
        )
        .unwrap()
}

fn handoff_input(todo_ref: &str, session_id: &str) -> CreateHandoffInput {
    CreateHandoffInput {
        todo_ref: todo_ref.into(),
        session_id: session_id.into(),
        session_name: None,
        session_cwd: None,
        note: None,
        actor: "logan".into(),
        current_board_id: None,
    }
}

#[tokio::test]
async fn get_sessions_lists_with_board_matching() {
    let f = fx();
    let state = rebuild(&f, |o| {
        o.sessions = Some(fixed_sessions(fixture_sessions()))
    });
    let (status, body) = get(&state, "/api/sessions?board=rocky-todo").await;
    assert_eq!(status, 200);
    assert_eq!(body["available"], true);
    let sessions = body["sessions"].as_array().unwrap();
    let by_name = |name: &str| sessions.iter().find(|s| s["name"] == name).unwrap();
    assert_eq!(by_name("rocky-todo-1e")["matched"], true);
    assert_eq!(by_name("forses-90")["matched"], false);
}

/// background 행에는 `~/.claude/jobs/<id>/state.json` 의 요약이 `job` 으로 붙는다. 파일이 없거나 형식이 다르면
/// 그 행만 `job` 이 없다(목록은 그대로).
#[tokio::test]
async fn background_sessions_carry_job_summary() {
    let jobs = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(jobs.path().join("0da6a98a")).unwrap();
    std::fs::write(
        jobs.path().join("0da6a98a/state.json"),
        r#"{"state":"blocked","detail":"3 PR 머지, 결정 대기","needs":"룰셋을 끌지 정해 주세요","tokens":129916,"updatedAt":"2026-08-10T08:28:00Z"}"#,
    )
    .unwrap();
    std::fs::create_dir_all(jobs.path().join("bad00000")).unwrap();
    std::fs::write(jobs.path().join("bad00000/state.json"), "not json").unwrap();
    let mut blocked = sess(0, "/w/rocky-todo", "0da6a98a-full", "rocky-todo-25", "idle");
    blocked.pid = None;
    blocked.kind = "background".into();
    blocked.id = Some("0da6a98a".into());
    blocked.state = Some("blocked".into());
    let mut broken = blocked.clone();
    broken.session_id = "bad00000-full".into();
    broken.id = Some("bad00000".into());
    let mut escape = blocked.clone();
    escape.session_id = "escape-full".into();
    escape.id = Some("../0da6a98a".into());
    let dir = jobs.path().to_path_buf();
    let f = fx();
    let state = rebuild(&f, |o| {
        o.sessions = Some(fixed_sessions(available(vec![
            blocked,
            broken,
            escape,
            sess(1, "/w/rocky-todo", "sess-1", "rocky-todo-1e", "idle"),
        ])));
        o.claude_jobs_dir = Some(dir);
    });
    let (status, body) = get(&state, "/api/sessions").await;
    assert_eq!(status, 200);
    let sessions = body["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 4);
    let by_id = |id: &str| sessions.iter().find(|s| s["sessionId"] == id).unwrap();
    let job = &by_id("0da6a98a-full")["job"];
    assert_eq!(job["needs"], "룰셋을 끌지 정해 주세요");
    assert!(
        by_id("0da6a98a-full").get("pid").is_none(),
        "pid 없는 행은 pid 를 싣지 않는다"
    );
    assert!(
        by_id("bad00000-full").get("job").is_none(),
        "형식이 다르면 job 이 없다"
    );
    assert!(
        by_id("escape-full").get("job").is_none(),
        "경로를 벗어나는 id 는 읽지 않는다"
    );
    assert!(
        by_id("sess-1").get("job").is_none(),
        "interactive 행에는 job 이 없다"
    );
}

#[tokio::test]
async fn sessions_unavailable_is_reported() {
    let f = fx();
    let state = rebuild(&f, |o| {
        o.sessions = Some(fixed_sessions(SessionsResult::unavailable(
            "claude CLI 없음",
        )))
    });
    let (status, body) = get(&state, "/api/sessions").await;
    assert_eq!(status, 200);
    assert_eq!(body["available"], false);
    assert_eq!(body["reason"], "claude CLI 없음");
}

#[tokio::test]
async fn post_handoff_with_session_id_gives_snapshot() {
    let f = fx();
    let state = rebuild(&f, |o| {
        o.sessions = Some(fixed_sessions(fixture_sessions()))
    });
    let todo = create(&f, "rocky-todo", "x");
    let (status, body) = post(
        &state,
        &format!("/api/todos/{}/handoff", todo.id),
        json!({"sessionId":"sess-1","note":"테스트부터"}),
    )
    .await;
    assert_eq!(status, 201);
    assert_eq!(body["sessionName"], "rocky-todo-1e");
    assert_eq!(body["sessionCwd"], "/w/rocky-todo");
    assert_eq!(body["note"], "테스트부터");
}

#[tokio::test]
async fn post_handoff_returns_poke() {
    let f = fx();
    let state = rebuild(&f, |o| {
        o.sessions = Some(fixed_sessions(fixture_sessions()))
    });
    let todo = create(&f, "rocky-todo", "프렁크 래치 로깅");
    let (status, body) = post(
        &state,
        &format!("/api/todos/{}/handoff", todo.id),
        json!({"sessionId":"sess-1"}),
    )
    .await;
    assert_eq!(status, 201);
    // `to` 는 SendMessage 가 그대로 받는 세션 이름 — sessionId 가 아니다.
    assert_eq!(body["poke"]["to"], "rocky-todo-1e");
    let message = body["poke"]["message"].as_str().unwrap();
    assert!(message.contains("프렁크 래치 로깅"));
    assert!(message.contains("todo_list"));
}

#[tokio::test]
async fn post_handoff_auto_matches_board_when_session_omitted() {
    let f = fx();
    let state = rebuild(&f, |o| {
        o.sessions = Some(fixed_sessions(fixture_sessions()))
    });
    let todo = create(&f, "rocky-todo", "x");
    let (status, body) = post(
        &state,
        &format!("/api/todos/{}/handoff", todo.id),
        json!({}),
    )
    .await;
    assert_eq!(status, 201);
    assert_eq!(body["sessionId"], "sess-1");
}

#[tokio::test]
async fn no_or_many_candidates_is_409_with_list() {
    let f = fx();
    let state = rebuild(&f, |o| {
        o.sessions = Some(fixed_sessions(fixture_sessions()))
    });
    let todo = create(&f, "gotgan", "x");
    let (status, body) = post(
        &state,
        &format!("/api/todos/{}/handoff", todo.id),
        json!({}),
    )
    .await;
    assert_eq!(status, 409);
    assert!(body["error"].as_str().unwrap().contains("고르라"));
    assert!(body["candidates"].is_array());
}

#[tokio::test]
async fn existing_pending_is_409() {
    let f = fx();
    let state = rebuild(&f, |o| {
        o.sessions = Some(fixed_sessions(fixture_sessions()))
    });
    let todo = create(&f, "rocky-todo", "x");
    f.store
        .create_handoff(&handoff_input(&todo.id, "sess-1"))
        .unwrap();
    let (status, _) = post(
        &state,
        &format!("/api/todos/{}/handoff", todo.id),
        json!({"sessionId":"sess-1"}),
    )
    .await;
    assert_eq!(status, 409);
}

#[tokio::test]
async fn unknown_todo_is_404() {
    let f = fx();
    let state = rebuild(&f, |o| {
        o.sessions = Some(fixed_sessions(fixture_sessions()))
    });
    let (status, _) = post(
        &state,
        "/api/todos/zzzzzzzz/handoff",
        json!({"sessionId":"sess-1"}),
    )
    .await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn non_string_session_id_is_400_not_auto_match() {
    let f = fx();
    let state = rebuild(&f, |o| {
        o.sessions = Some(fixed_sessions(fixture_sessions()))
    });
    let todo = create(&f, "rocky-todo", "x");
    let (status, _) = post(
        &state,
        &format!("/api/todos/{}/handoff", todo.id),
        json!({"sessionId": 42}),
    )
    .await;
    assert_eq!(status, 400);
    assert!(f.store.pending_handoff_of(&todo.id).unwrap().is_none());
}

#[tokio::test]
async fn unknown_session_id_is_400() {
    let f = fx();
    let state = rebuild(&f, |o| {
        o.sessions = Some(fixed_sessions(fixture_sessions()))
    });
    let todo = create(&f, "rocky-todo", "x");
    let (status, _) = post(
        &state,
        &format!("/api/todos/{}/handoff", todo.id),
        json!({"sessionId":"ghost"}),
    )
    .await;
    assert_eq!(status, 400);
}

#[tokio::test]
async fn claim_gives_one_then_204() {
    let f = fx();
    let state = rebuild(&f, |o| {
        o.sessions = Some(fixed_sessions(fixture_sessions()))
    });
    let todo = create(&f, "rocky-todo", "핸드오프");
    f.store
        .create_handoff(&handoff_input(&todo.id, "sess-1"))
        .unwrap();

    let (status, body) = post(
        &state,
        "/api/handoffs/claim",
        json!({"sessionId":"sess-1","via":"stop"}),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["todoTitle"], "핸드오프");
    assert_eq!(body["remaining"], 0);

    let (status, _) = post(
        &state,
        "/api/handoffs/claim",
        json!({"sessionId":"sess-1","via":"stop"}),
    )
    .await;
    assert_eq!(status, 204);
}

#[tokio::test]
async fn get_handoffs_marks_gone_target_stale() {
    let f = fx();
    let state = rebuild(&f, |o| {
        o.sessions = Some(fixed_sessions(fixture_sessions()))
    });
    let todo = create(&f, "rocky-todo", "x");
    f.store
        .create_handoff(&handoff_input(&todo.id, "ghost-session"))
        .unwrap();
    let (status, body) = get(&state, "/api/handoffs?status=pending").await;
    assert_eq!(status, 200);
    assert_eq!(body[0]["stale"], true);
}

#[tokio::test]
async fn cancel_200_then_400() {
    let f = fx();
    let state = rebuild(&f, |o| {
        o.sessions = Some(fixed_sessions(fixture_sessions()))
    });
    let todo = create(&f, "rocky-todo", "x");
    let handoff = f
        .store
        .create_handoff(&handoff_input(&todo.id, "sess-1"))
        .unwrap();
    let (status, _) = post(
        &state,
        &format!("/api/handoffs/{}/cancel", handoff.id),
        json!({}),
    )
    .await;
    assert_eq!(status, 200);
    let (status, _) = post(
        &state,
        &format!("/api/handoffs/{}/cancel", handoff.id),
        json!({}),
    )
    .await;
    assert_eq!(status, 400);
}

#[tokio::test]
async fn get_handoffs_skips_session_lookup_without_pending() {
    let f = fx();
    let calls = Arc::new(AtomicUsize::new(0));
    let state = rebuild(&f, |o| {
        o.sessions = Some(counting(calls.clone(), fixture_sessions()))
    });
    let todo = create(&f, "rocky-todo", "x");
    let cancelled = f
        .store
        .create_handoff(&handoff_input(&todo.id, "sess-1"))
        .unwrap();
    f.store.cancel_handoff(&cancelled.id, "logan").unwrap();

    let (status, body) = get(&state, "/api/handoffs?status=pending").await;
    assert_eq!(status, 200);
    assert_eq!(body, json!([]));
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    f.store
        .create_handoff(&handoff_input(&todo.id, "sess-1"))
        .unwrap();
    let (status, _) = get(&state, "/api/handoffs").await;
    assert_eq!(status, 200);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn get_handoffs_queries_sessions_for_unstarted_candidates() {
    let f = fx();
    let calls = Arc::new(AtomicUsize::new(0));
    let state = rebuild(&f, |o| {
        o.sessions = Some(counting(calls.clone(), fixture_sessions()))
    });
    let todo = create(&f, "rocky-todo", "x");
    f.store
        .create_handoff(&handoff_input(&todo.id, "sess-1"))
        .unwrap();
    f.store.claim_handoff("sess-1", HandoffVia::Stop).unwrap();

    let (status, body) = get(&state, "/api/handoffs").await;
    assert_eq!(status, 200);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    // sess-1 은 idle — 집어가 놓고 아무것도 안 했다.
    assert_eq!(body[0]["phase"], "delivered");
    assert_eq!(body[0]["unstarted"], true);
}

#[tokio::test]
async fn accepted_handoff_is_not_unstarted() {
    let f = fx();
    let state = rebuild(&f, |o| {
        o.sessions = Some(fixed_sessions(fixture_sessions()))
    });
    let todo = create(&f, "rocky-todo", "x");
    f.store
        .create_handoff(&handoff_input(&todo.id, "sess-1"))
        .unwrap();
    f.store.claim_handoff("sess-1", HandoffVia::Stop).unwrap();
    f.store
        .set_todo_status(&todo.id, StatusAction::Start, "claude-code", None)
        .unwrap();

    let (_, body) = get(&state, "/api/handoffs").await;
    assert_eq!(body[0]["phase"], "accepted");
    assert_eq!(body[0]["unstarted"], false);
}

#[tokio::test]
async fn open_true_includes_pending_and_undone_delivered() {
    let f = fx();
    let state = rebuild(&f, |o| {
        o.sessions = Some(fixed_sessions(fixture_sessions()))
    });
    let waiting = create(&f, "rocky-todo", "대기");
    let finished = create(&f, "rocky-todo", "완료");
    f.store
        .create_handoff(&handoff_input(&waiting.id, "sess-1"))
        .unwrap();
    f.store
        .create_handoff(&handoff_input(&finished.id, "sess-2"))
        .unwrap();
    f.store.claim_handoff("sess-2", HandoffVia::Stop).unwrap();
    f.store
        .set_todo_status(&finished.id, StatusAction::Done, "claude-code", None)
        .unwrap();

    let (_, body) = get(&state, "/api/handoffs?open=true&board=rocky-todo").await;
    let todo_ids: Vec<&str> = body
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["todoId"].as_str().unwrap())
        .collect();
    assert_eq!(todo_ids, vec![waiting.id.as_str()]);
}

// ── doingState — GET /api/todos ──

fn started_by_session(f: &Fx, session_id: &str) -> Todo {
    let todo = create(f, "rocky-todo", "작업");
    f.store
        .create_handoff(&handoff_input(&todo.id, session_id))
        .unwrap();
    f.store.claim_handoff(session_id, HandoffVia::Stop).unwrap();
    f.store
        .set_todo_status(&todo.id, StatusAction::Start, "claude-code", None)
        .unwrap();
    todo
}

async fn doing_state_of(state: &Arc<rockyd::server::ServerState>, todo_id: &str) -> Option<String> {
    let (_, body) = get(state, "/api/todos?board=rocky-todo").await;
    body.as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"] == todo_id)
        .and_then(|t| t.get("doingState"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

#[tokio::test]
async fn doing_state_busy_session_is_live() {
    let f = fx();
    let state = rebuild(&f, |o| {
        o.sessions = Some(fixed_sessions(fixture_sessions()))
    });
    let todo = started_by_session(&f, "sess-2"); // busy
    assert_eq!(
        doing_state_of(&state, &todo.id).await.as_deref(),
        Some("live")
    );
}

#[tokio::test]
async fn doing_state_idle_session_is_idle() {
    let f = fx();
    let state = rebuild(&f, |o| {
        o.sessions = Some(fixed_sessions(fixture_sessions()))
    });
    let todo = started_by_session(&f, "sess-1"); // idle
    assert_eq!(
        doing_state_of(&state, &todo.id).await.as_deref(),
        Some("idle")
    );
}

#[tokio::test]
async fn doing_state_missing_session_is_gone() {
    let f = fx();
    let state = rebuild(&f, |o| {
        o.sessions = Some(fixed_sessions(fixture_sessions()))
    });
    let todo = started_by_session(&f, "ghost");
    assert_eq!(
        doing_state_of(&state, &todo.id).await.as_deref(),
        Some("gone")
    );
}

#[tokio::test]
async fn doing_state_unattributed_no_board_session_is_gone() {
    let f = fx();
    let state = rebuild(&f, |o| {
        o.sessions = Some(fixed_sessions(available(vec![sess(
            2,
            "/w/forses",
            "sess-2",
            "forses-90",
            "busy",
        )])))
    });
    let todo = create(&f, "rocky-todo", "x");
    f.store
        .set_todo_status(&todo.id, StatusAction::Start, "claude-code", None)
        .unwrap();
    assert_eq!(
        doing_state_of(&state, &todo.id).await.as_deref(),
        Some("gone")
    );
}

#[tokio::test]
async fn doing_state_unavailable_sessions_is_unknown() {
    let f = fx();
    let state = rebuild(&f, |o| {
        o.sessions = Some(fixed_sessions(SessionsResult::unavailable("claude 없음")))
    });
    let todo = started_by_session(&f, "sess-1");
    assert_eq!(
        doing_state_of(&state, &todo.id).await.as_deref(),
        Some("unknown")
    );
}

#[tokio::test]
async fn no_doing_skips_session_lookup() {
    let f = fx();
    let calls = Arc::new(AtomicUsize::new(0));
    let state = rebuild(&f, |o| {
        o.sessions = Some(counting(calls.clone(), fixture_sessions()))
    });
    create(&f, "rocky-todo", "그냥 todo");
    let (status, _) = get(&state, "/api/todos?board=rocky-todo").await;
    assert_eq!(status, 200);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn non_doing_items_have_no_doing_state() {
    let f = fx();
    let state = rebuild(&f, |o| {
        o.sessions = Some(fixed_sessions(fixture_sessions()))
    });
    let plain = create(&f, "rocky-todo", "대기 항목");
    started_by_session(&f, "sess-1"); // doing 이 있어야 세션 조회가 일어난다
    let (_, body) = get(&state, "/api/todos?board=rocky-todo").await;
    let plain_out = body
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"] == plain.id.as_str())
        .unwrap();
    assert!(plain_out.get("doingState").is_none());
}

#[tokio::test]
async fn unknown_board_gives_empty_list_not_whole_queue() {
    let f = fx();
    let state = rebuild(&f, |o| {
        o.sessions = Some(fixed_sessions(fixture_sessions()))
    });
    let todo = create(&f, "rocky-todo", "x");
    f.store
        .create_handoff(&handoff_input(&todo.id, "sess-1"))
        .unwrap();
    let (status, body) = get(&state, "/api/handoffs?board=no-such-board").await;
    assert_eq!(status, 200);
    assert_eq!(body, json!([]));
}

#[tokio::test]
async fn unavailable_sessions_do_not_mark_stale() {
    let f = fx();
    let state = rebuild(&f, |o| {
        o.sessions = Some(fixed_sessions(SessionsResult::unavailable("claude 없음")))
    });
    let todo = create(&f, "rocky-todo", "x");
    f.store
        .create_handoff(&handoff_input(&todo.id, "sess-1"))
        .unwrap();
    let (_, body) = get(&state, "/api/handoffs?status=pending").await;
    assert_eq!(body[0]["stale"], false);
}

// ── claim 로컬 게이트 ──

#[tokio::test]
async fn claim_blocks_lan_with_404() {
    let f = fx();
    let todo = create(&f, "rocky-todo", "x");
    f.store
        .create_handoff(&handoff_input(&todo.id, "sess-1"))
        .unwrap();
    let (status, body) = call(
        &f.state,
        "POST",
        "/api/handoffs/claim",
        Some(json!({"sessionId":"sess-1","via":"stop"})),
        ReqOptions {
            peer: Some("192.168.1.20"),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(status, 404); // 403 이 아니다 — 존재 자체를 숨긴다
    assert!(body["error"].as_str().unwrap().contains("not found"));
    assert!(f.store.pending_handoff_of(&todo.id).unwrap().is_some()); // 큐 소진 안 됨
}

#[tokio::test]
async fn claim_blocks_tailscale_proxy_with_404() {
    let f = fx();
    let todo = create(&f, "rocky-todo", "x");
    f.store
        .create_handoff(&handoff_input(&todo.id, "sess-1"))
        .unwrap();
    let (status, _) = call(
        &f.state,
        "POST",
        "/api/handoffs/claim",
        Some(json!({"sessionId":"sess-1","via":"stop"})),
        ReqOptions {
            headers: vec![("tailscale-user-login", "someone@example.com")],
            peer: Some("127.0.0.1"),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(status, 404);
    assert!(f.store.pending_handoff_of(&todo.id).unwrap().is_some());
}

#[tokio::test]
async fn claim_accepts_local_hook() {
    let f = fx();
    let todo = create(&f, "rocky-todo", "x");
    f.store
        .create_handoff(&handoff_input(&todo.id, "sess-1"))
        .unwrap();
    let (status, body) = post(
        &f.state,
        "/api/handoffs/claim",
        json!({"sessionId":"sess-1","via":"prompt"}),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["handoff"]["deliveredVia"], "prompt");
}

/// Claude Code 받은편지함 모양의 소켓(`…/cc-socks-*/<숫자>.sock`).
fn inbox_socket(dir: &std::path::Path) -> (std::os::unix::net::UnixListener, String) {
    let socks = dir.join("cc-socks-test");
    std::fs::create_dir_all(&socks).unwrap();
    let path = socks.join("42.sock");
    let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
    listener.set_nonblocking(true).unwrap();
    (listener, path.to_string_lossy().to_string())
}

fn drain(listener: &std::os::unix::net::UnixListener) -> Vec<String> {
    use std::io::Read;
    let mut out = Vec::new();
    while let Ok((mut stream, _)) = listener.accept() {
        stream.set_nonblocking(false).unwrap();
        let mut text = String::new();
        stream.read_to_string(&mut text).unwrap();
        out.push(text);
    }
    out
}

#[tokio::test]
async fn handoff_wakes_a_session_that_registered_its_inbox() {
    // 웹의 "에이전트에게 보내기" 는 poke 를 보낼 길이 없다 — 데몬이 그 세션의 받은편지함에 꽂아 턴을 연다.
    let f = fx();
    let state = rebuild(&f, |o| {
        o.sessions = Some(fixed_sessions(fixture_sessions()))
    });
    let dir = tempfile::tempdir().unwrap();
    let (listener, socket) = inbox_socket(dir.path());
    post(
        &state,
        "/api/sessions/inbox",
        json!({ "sessionId": "sess-1", "socket": socket, "cwd": "/w/rocky-todo" }),
    )
    .await;
    let todo = create(&f, "rocky-todo", "깨워서 넘긴다");
    let (status, body) = post(
        &state,
        &format!("/api/todos/{}/handoff", todo.id),
        json!({"sessionId":"sess-1"}),
    )
    .await;
    assert_eq!(status, 201);
    assert_eq!(body["woke"], true);
    let lines = drain(&listener);
    assert_eq!(lines.len(), 1);
    assert!(
        lines[0].contains("보드에서 작업 요청이 도착했다"),
        "{}",
        lines[0]
    );
    assert!(lines[0].contains("깨워서 넘긴다"));
    // 큐에도 그대로 있다 — 열린 턴의 훅이 여기서 집어 전체 지시를 주입한다.
    assert!(f.store.pending_handoff_of(&todo.id).unwrap().is_some());

    // 받은편지함을 등록하지 않은 세션은 지금처럼 큐에서 다음 턴을 기다린다.
    let other = create(&f, "forses", "등록 없는 세션");
    let (status, body) = post(
        &state,
        &format!("/api/todos/{}/handoff", other.id),
        json!({"sessionId":"sess-2"}),
    )
    .await;
    assert_eq!(status, 201);
    assert_eq!(body["woke"], false);
}

#[tokio::test]
async fn a_remote_handoff_is_queued_but_does_not_wake_the_session() {
    // 노출된 주소(프록시 헤더)로 온 핸드오프는 세션을 움직이지 않는다 — 큐에만 넣는다(세션 띄우기와 같은 경계).
    let f = fx();
    let state = rebuild(&f, |o| {
        o.sessions = Some(fixed_sessions(fixture_sessions()))
    });
    let dir = tempfile::tempdir().unwrap();
    let (listener, socket) = inbox_socket(dir.path());
    post(
        &state,
        "/api/sessions/inbox",
        json!({ "sessionId": "sess-1", "socket": socket, "cwd": "/w/rocky-todo" }),
    )
    .await;
    let todo = create(&f, "rocky-todo", "원격에서 넘김");
    let (status, body) = call(
        &state,
        "POST",
        &format!("/api/todos/{}/handoff", todo.id),
        Some(json!({"sessionId":"sess-1"})),
        ReqOptions {
            headers: vec![("x-forwarded-for", "100.64.0.9")],
            ..Default::default()
        },
    )
    .await;
    assert_eq!(status, 201);
    assert_eq!(body["woke"], false);
    assert!(drain(&listener).is_empty());
    assert!(f.store.pending_handoff_of(&todo.id).unwrap().is_some());
}

#[tokio::test]
async fn handoff_picks_its_target_from_the_uncached_session_list() {
    // 읽기 라우트의 목록(지난 값)엔 sess-1 이 살아 있지만 캐시 없는 목록에선 이미 끝났다 —
    // 핸드오프는 큐에 쓰는 일이라 캐시 없는 쪽을 봐야 한다(끝난 세션 앞에 아무도 집지 않을 핸드오프가 남는다).
    let f = fx();
    let state = rebuild(&f, |o| {
        o.sessions = Some(fixed_sessions(fixture_sessions()));
        o.spawn_sessions = Some(fixed_sessions(available(vec![sess(
            2,
            "/w/forses",
            "sess-2",
            "forses-90",
            "busy",
        )])));
    });
    let todo = create(&f, "rocky-todo", "x");
    let (status, _) = post(
        &state,
        &format!("/api/todos/{}/handoff", todo.id),
        json!({"sessionId":"sess-1"}),
    )
    .await;
    assert_eq!(status, 400);
    assert!(f.store.pending_handoff_of(&todo.id).unwrap().is_none());
}

/// 잠든 background 세션(Claude Code 2.1.289 의 실제 모양 — pid·status 없음, cwd 는 레포 루트)은 자동 대상이 아니다.
/// 목록에 남아 보드와 맞지만, 일을 넘기면 아무도 집지 않는 큐가 된다.
#[tokio::test]
async fn auto_match_skips_dormant_background_sessions() {
    let mut dormant = sess(0, "/w/rocky-todo", "0da6a98a-full", "rocky-todo-25", "idle");
    dormant.pid = None;
    dormant.kind = "background".into();
    dormant.id = Some("0da6a98a".into());
    dormant.state = Some("blocked".into());
    let f = fx();
    let both = rebuild(&f, |o| {
        o.sessions = Some(fixed_sessions(available(vec![
            dormant.clone(),
            sess(1, "/w/rocky-todo", "sess-1", "rocky-todo-1e", "idle"),
        ])))
    });
    let todo = create(&f, "rocky-todo", "x");
    let (status, body) = post(&both, &format!("/api/todos/{}/handoff", todo.id), json!({})).await;
    assert_eq!(status, 201, "잠든 세션을 빼면 후보는 하나다: {body}");
    assert_eq!(body["sessionId"], "sess-1");

    let alone = rebuild(&f, |o| {
        o.sessions = Some(fixed_sessions(available(vec![dormant])))
    });
    let other = create(&f, "rocky-todo", "y");
    let (status, body) = post(
        &alone,
        &format!("/api/todos/{}/handoff", other.id),
        json!({}),
    )
    .await;
    assert_eq!(status, 409, "잠든 세션뿐이면 사람이 고른다: {body}");
}

// ── start 직전: 받은 세션이 사라진 배달 무효 ──

/// `session_id` 앞으로 배달까지 마친 핸드오프 — 배달 시각을 `minutes` 분 전으로 돌린다.
fn delivered_ago(f: &Fx, todo: &Todo, session_id: &str, minutes: i64) -> Handoff {
    let handoff = f
        .store
        .create_handoff(&handoff_input(&todo.id, session_id))
        .unwrap();
    f.store.claim_handoff(session_id, HandoffVia::Stop).unwrap();
    let at = (chrono::Utc::now() - chrono::Duration::minutes(minutes))
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    rusqlite::Connection::open(&f.db_path)
        .unwrap()
        .execute(
            "UPDATE handoffs SET created_at = ?1, delivered_at = ?1 WHERE id = ?2",
            rusqlite::params![at, handoff.id],
        )
        .unwrap();
    handoff
}

async fn start_as(state: &Arc<rockyd::server::ServerState>, todo: &Todo, actor: &str) -> Value {
    let (status, body) = call(
        state,
        "POST",
        &format!("/api/todos/{}/status", todo.id),
        Some(json!({"action":"start"})),
        ReqOptions {
            actor,
            ..Default::default()
        },
    )
    .await;
    assert_eq!(status, 200, "{body}");
    body
}

fn handoff_status(f: &Fx, id: &str) -> HandoffStatus {
    f.store
        .list_handoffs(&ListHandoffsFilter::default())
        .unwrap()
        .into_iter()
        .find(|h| h.id == id)
        .unwrap()
        .status
}

/// 멈춘 세션이 받고 사라진 배달을 다른 세션의 start 가 수락하지 않는다 — doing 이 사라진 세션에 귀속되면
/// Stop 확인 · 턴 태그가 엉뚱한 곳으로 가고 24시간 뒤 자동 해제가 일하는 중인 할 일을 멈춘다(2026-10-05 실측).
#[tokio::test]
async fn agent_start_cancels_a_delivery_whose_session_is_gone() {
    let f = fx();
    let state = rebuild(&f, |o| {
        o.spawn_sessions = Some(fixed_sessions(available(vec![sess(
            1,
            "/w/rocky-todo",
            "sess-1",
            "rocky-todo-1e",
            "busy",
        )])))
    });
    let todo = create(&f, "rocky-todo", "x");
    let stale = delivered_ago(&f, &todo, "sess-gone", 30);

    let body = start_as(&state, &todo, "claude-code").await;
    assert!(body["doingSessionId"].is_null(), "{body}");
    assert_eq!(handoff_status(&f, &stale.id), HandoffStatus::Cancelled);
    let history = f
        .store
        .list_history(&ListHistoryFilter {
            entity_id: Some(todo.id.clone()),
            ..Default::default()
        })
        .unwrap();
    assert!(
        history
            .iter()
            .any(|h| h.action == "handoff-cancel" && h.actor == rockyd::sweep::SWEEP_ACTOR),
        "{history:?}"
    );
}

/// 받은 세션이 죽은 걸 보고 사람이 곧바로 다시 보냈으면 — 옛 배달은 유예 안이어도 밀린 것이라 취소되고, 새로 받은
/// 세션의 start 가 제 배달을 수락한다.
#[tokio::test]
async fn early_resend_does_not_leave_the_dead_delivery_to_be_accepted() {
    let f = fx();
    let state = rebuild(&f, |o| {
        o.spawn_sessions = Some(fixed_sessions(available(vec![sess(
            2,
            "/w/rocky-todo",
            "sess-b",
            "rocky-todo-b",
            "busy",
        )])))
    });
    let todo = create(&f, "rocky-todo", "x");
    let dead = delivered_ago(&f, &todo, "sess-a", 3);
    let resent = delivered_ago(&f, &todo, "sess-b", 1);

    let body = start_as(&state, &todo, "claude-code").await;
    assert_eq!(body["doingSessionId"], "sess-b", "{body}");
    assert_eq!(handoff_status(&f, &dead.id), HandoffStatus::Cancelled);
    assert_eq!(handoff_status(&f, &resent.id), HandoffStatus::Delivered);
}

/// 받은 세션이 살아서 쉬고 있어도, 사람이 다시 보냈으면 옛 배달은 버린 것이다 — 새로 받은 세션의 start 가 제 것을 수락한다.
#[tokio::test]
async fn resend_past_an_idle_session_drops_its_delivery() {
    let f = fx();
    let state = rebuild(&f, |o| {
        o.spawn_sessions = Some(fixed_sessions(available(vec![
            sess(1, "/w/rocky-todo", "sess-a", "rocky-todo-a", "idle"),
            sess(2, "/w/rocky-todo", "sess-b", "rocky-todo-b", "busy"),
        ])))
    });
    let todo = create(&f, "rocky-todo", "x");
    let idle = delivered_ago(&f, &todo, "sess-a", 30);
    let resent = delivered_ago(&f, &todo, "sess-b", 1);

    let body = start_as(&state, &todo, "claude-code").await;
    assert_eq!(body["doingSessionId"], "sess-b", "{body}");
    assert_eq!(handoff_status(&f, &idle.id), HandoffStatus::Cancelled);
    assert_eq!(handoff_status(&f, &resent.id), HandoffStatus::Delivered);
}

/// 받은 세션이 살아 있거나(쉬어도), 배달 직후거나, 목록을 못 얻으면 그대로 수락한다.
#[tokio::test]
async fn agent_start_accepts_a_delivery_that_is_alive_fresh_or_unknown() {
    let cases: [(&str, i64, SessionsResult); 3] = [
        (
            "살아 있음",
            30,
            available(vec![sess(
                1,
                "/w/rocky-todo",
                "sess-1",
                "rocky-todo-1e",
                "idle",
            )]),
        ),
        ("배달 직후", 1, available(vec![])),
        ("목록 모름", 30, SessionsResult::unavailable("테스트")),
    ];
    for (label, minutes, sessions) in cases {
        let f = fx();
        let state = rebuild(&f, |o| o.spawn_sessions = Some(fixed_sessions(sessions)));
        let todo = create(&f, "rocky-todo", "x");
        let handoff = delivered_ago(&f, &todo, "sess-1", minutes);
        let body = start_as(&state, &todo, "claude-code").await;
        assert_eq!(body["doingSessionId"], "sess-1", "{label}: {body}");
        assert_eq!(
            handoff_status(&f, &handoff.id),
            HandoffStatus::Delivered,
            "{label}"
        );
    }
}

/// 사람이 누른 start 는 핸드오프를 수락하지 않으므로 세션 목록을 부르지도 않는다.
#[tokio::test]
async fn human_start_does_not_look_up_sessions() {
    let f = fx();
    let counter = Arc::new(AtomicUsize::new(0));
    let state = rebuild(&f, |o| {
        o.spawn_sessions = Some(counting(counter.clone(), available(vec![])))
    });
    let todo = create(&f, "rocky-todo", "x");
    let handoff = delivered_ago(&f, &todo, "sess-gone", 30);
    start_as(&state, &todo, "logan").await;
    assert_eq!(counter.load(Ordering::SeqCst), 0);
    assert_eq!(handoff_status(&f, &handoff.id), HandoffStatus::Delivered);
}

/// 받고 착수 안 한 배달은 사람이 취소할 수 있다(할 일 상세 · `rocky handoff --cancel`), 착수한 것은 거절한다.
#[tokio::test]
async fn cancel_route_takes_unstarted_delivery_but_not_accepted() {
    let f = fx();
    let todo = create(&f, "rocky-todo", "x");
    let unstarted = delivered_ago(&f, &todo, "sess-1", 0);
    let (status, body) = post(
        &f.state,
        &format!("/api/handoffs/{}/cancel", unstarted.id),
        json!({}),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["status"], "cancelled");

    let other = create(&f, "rocky-todo", "y");
    let accepted = delivered_ago(&f, &other, "sess-2", 0);
    f.store
        .set_todo_status(&other.id, StatusAction::Start, "claude-code", None)
        .unwrap();
    let (status, _) = post(
        &f.state,
        &format!("/api/handoffs/{}/cancel", accepted.id),
        json!({}),
    )
    .await;
    assert_eq!(status, 400);
}

/// `?todo=` 는 그 할 일의 핸드오프만 준다(참조 · id 둘 다) — `rocky handoff --cancel` 이 완료된 이력까지 전부 받지 않게.
/// 못 풀면 빈 목록.
#[tokio::test]
async fn get_handoffs_filters_by_todo() {
    let f = fx();
    let mine = create(&f, "rocky-todo", "x");
    let other = create(&f, "rocky-todo", "y");
    let wanted = delivered_ago(&f, &mine, "sess-1", 0);
    delivered_ago(&f, &other, "sess-2", 0);

    for todo in [mine.id.as_str(), "rocky-todo-1"] {
        let (status, body) = get(&f.state, &format!("/api/handoffs?todo={todo}")).await;
        assert_eq!(status, 200, "{body}");
        let ids: Vec<&str> = body
            .as_array()
            .unwrap()
            .iter()
            .map(|h| h["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, vec![wanted.id.as_str()], "{todo}");
    }
    let (status, body) = get(&f.state, "/api/handoffs?todo=rocky-todo-99").await;
    assert_eq!(status, 200);
    assert_eq!(body, json!([]));
}

/// 세션 멈추기 — 로컬 요청만, 살아 있는 background 세션만, `claude stop` 에는 목록의 짧은 id 를 넘긴다. CLI 가 실패하면
/// 그 이유를 409 로 그대로 보인다.
#[tokio::test]
async fn stop_runs_claude_stop_for_live_background_sessions_only() {
    use rockyd::runner::{CmdOutput, Runner};

    let mut live = sess(
        10,
        "/w/rocky/.claude/worktrees/todo-3",
        "aaaa1111-full",
        "todo-3",
        "busy",
    );
    live.kind = "background".into();
    live.id = Some("aaaa1111".into());
    live.state = Some("working".into());
    let mut dormant = live.clone();
    dormant.pid = None;
    dormant.session_id = "bbbb2222-full".into();
    dormant.id = Some("bbbb2222".into());
    dormant.state = Some("blocked".into());
    let mut failing = live.clone();
    failing.session_id = "cccc3333-full".into();
    failing.id = Some("cccc3333".into());
    let calls: Arc<std::sync::Mutex<Vec<Vec<String>>>> = Arc::default();
    let seen = calls.clone();
    let runner: Runner = Arc::new(move |argv: Vec<String>, _stdin, _timeout| {
        seen.lock().unwrap().push(argv.clone());
        Box::pin(async move {
            if argv.get(2).map(String::as_str) == Some("cccc3333") {
                CmdOutput {
                    code: 1,
                    stdout: String::new(),
                    stderr: "No job matching 'cccc3333'.".into(),
                }
            } else {
                CmdOutput {
                    code: 0,
                    stdout: "stopped".into(),
                    stderr: String::new(),
                }
            }
        })
    });
    let f = fx();
    let state = rebuild(&f, |o| {
        o.sessions = Some(fixed_sessions(available(vec![
            live,
            dormant,
            failing,
            sess(1, "/w/rocky", "term-full", "term", "idle"),
        ])));
        o.gh_runner = Some(runner);
    });

    let remote = call(
        &state,
        "POST",
        "/api/sessions/stop",
        Some(json!({ "sessionId": "aaaa1111-full" })),
        ReqOptions {
            peer: Some("100.64.0.1"),
            ..ReqOptions::default()
        },
    )
    .await;
    assert_eq!(remote.0, 403, "노출된 표면은 거절");
    let (status, _) = post(&state, "/api/sessions/stop", json!({ "sessionId": " " })).await;
    assert_eq!(status, 400);
    let (status, _) = post(&state, "/api/sessions/stop", json!({ "sessionId": "nope" })).await;
    assert_eq!(status, 404);
    let (status, body) = post(
        &state,
        "/api/sessions/stop",
        json!({ "sessionId": "term-full" }),
    )
    .await;
    assert_eq!(status, 409);
    assert!(
        body["error"].as_str().unwrap().contains("term-full"),
        "{body}"
    );
    let (status, _) = post(
        &state,
        "/api/sessions/stop",
        json!({ "sessionId": "bbbb2222-full" }),
    )
    .await;
    assert_eq!(status, 409, "pid 없이 잠든 세션은 받지 않는다");
    assert!(
        calls.lock().unwrap().is_empty(),
        "거절한 요청은 claude 를 부르지 않는다"
    );

    let (status, body) = post(
        &state,
        "/api/sessions/stop",
        json!({ "sessionId": "aaaa1111-full" }),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["id"], "aaaa1111");
    let (status, body) = post(
        &state,
        "/api/sessions/stop",
        json!({ "sessionId": "cccc3333-full" }),
    )
    .await;
    assert_eq!(status, 409);
    assert!(
        body["error"].as_str().unwrap().contains("No job matching"),
        "{body}"
    );
    assert_eq!(
        *calls.lock().unwrap(),
        vec![
            vec!["claude".to_string(), "stop".into(), "aaaa1111".into()],
            vec!["claude".to_string(), "stop".into(), "cccc3333".into()],
        ]
    );
}
