//! TS `src/server.test.ts` 의 statusline route describe 포팅.

mod common;

use std::sync::Arc;

use common::*;
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

fn statusline_state(f: &Fx, template: Option<&str>) -> Arc<ServerState> {
    let template = template.map(str::to_string);
    rebuild(f, move |o| {
        o.sessions = Some(fixed_sessions(fixture_sessions()));
        o.statusline_template = template;
    })
}

async fn line_from(state: &Arc<ServerState>, query: &str) -> String {
    let (status, body) = get(state, &format!("/api/statusline{query}")).await;
    assert_eq!(status, 200);
    body.as_str().unwrap_or_default().to_string()
}

/// 배달된 핸드오프로 시작된 doing — 세션 귀속이 붙는 유일한 경로.
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
async fn empty_board_gives_empty_body_without_session_lookup() {
    let f = fx();
    use std::sync::atomic::{AtomicUsize, Ordering};
    let calls = Arc::new(AtomicUsize::new(0));
    let counting = {
        let calls = calls.clone();
        let result = fixture_sessions();
        Arc::new(move || {
            calls.fetch_add(1, Ordering::SeqCst);
            let result = result.clone();
            Box::pin(async move { result }) as rockyd::runner::BoxFut<SessionsResult>
        })
    };
    let state = rebuild(&f, |o| o.sessions = Some(counting));
    let line = line_from(&state, "?session=sess-live&cwd=/w/rocky-todo").await;
    assert_eq!(line, "");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn carries_my_anchor_with_ref_and_title() {
    let f = fx();
    started_by_session(&f, "sess-live", "statusline API 추가");
    let state = statusline_state(&f, None);
    let line = line_from(&state, "?session=sess-live&cwd=/w/rocky-todo").await;
    assert_eq!(line, "⏺ rocky-todo-1 statusline API 추가");
}

#[tokio::test]
async fn someone_elses_item_is_not_my_anchor() {
    let f = fx();
    started_by_session(&f, "sess-live", "남의 작업");
    let state = statusline_state(&f, None);
    let line = line_from(&state, "?session=other-session&cwd=/w/rocky-todo").await;
    assert!(!line.contains("남의 작업"));
}

#[tokio::test]
async fn comment_count_appears() {
    let f = fx();
    let todo = started_by_session(&f, "sess-live", "작업");
    f.store
        .add_comment(&todo.id, "이거 먼저 봐줘", "logan", None)
        .unwrap();
    f.store
        .add_comment(&todo.id, "그리고 이것도", "logan", None)
        .unwrap();
    let state = statusline_state(&f, None);
    let line = line_from(&state, "?session=sess-live&cwd=/w/rocky-todo").await;
    assert!(line.contains("💬2"), "{line}");
}

#[tokio::test]
async fn short_id_attribution_matches_full_uuid_request() {
    let f = fx();
    started_by_session(&f, "shortid8", "백그라운드 작업");
    let state = statusline_state(&f, None);
    let line = line_from(&state, "?session=shortid8-full-uuid&cwd=/w/rocky-todo").await;
    assert!(line.contains("백그라운드 작업"), "{line}");
}

#[tokio::test]
async fn pending_handoff_counts_as_inbox() {
    let f = fx();
    let todo = f
        .store
        .create_todo(
            &CreateTodoInput {
                board: "rocky-todo".into(),
                title: "넘길 일".into(),
                ..Default::default()
            },
            "logan",
        )
        .unwrap();
    f.store
        .create_handoff(&CreateHandoffInput {
            todo_ref: todo.id,
            session_id: "sess-live".into(),
            session_name: None,
            session_cwd: None,
            note: None,
            actor: "logan".into(),
            current_board_id: None,
        })
        .unwrap();
    let state = statusline_state(&f, None);
    assert_eq!(
        line_from(&state, "?session=sess-live&cwd=/w/rocky-todo").await,
        "✉1"
    );
}

#[tokio::test]
async fn other_sessions_pending_is_not_my_inbox() {
    let f = fx();
    let todo = f
        .store
        .create_todo(
            &CreateTodoInput {
                board: "rocky-todo".into(),
                title: "넘길 일".into(),
                ..Default::default()
            },
            "logan",
        )
        .unwrap();
    f.store
        .create_handoff(&CreateHandoffInput {
            todo_ref: todo.id,
            session_id: "someone-else".into(),
            session_name: None,
            session_cwd: None,
            note: None,
            actor: "logan".into(),
            current_board_id: None,
        })
        .unwrap();
    let state = statusline_state(&f, None);
    assert_eq!(
        line_from(&state, "?session=sess-live&cwd=/w/rocky-todo").await,
        ""
    );
}

#[tokio::test]
async fn gone_session_doing_becomes_stale_warning() {
    let f = fx();
    started_by_session(&f, "sess-gone", "버려진 작업");
    let state = statusline_state(&f, None);
    assert_eq!(
        line_from(&state, "?session=sess-live&cwd=/w/rocky-todo").await,
        "⚠1"
    );
}

#[tokio::test]
async fn unavailable_sessions_do_not_warn() {
    let f = fx();
    started_by_session(&f, "sess-gone", "버려진 작업");
    let state = rebuild(&f, |o| {
        o.sessions = Some(fixed_sessions(SessionsResult::unavailable(
            "claude CLI 없음",
        )))
    });
    assert_eq!(
        line_from(&state, "?session=sess-live&cwd=/w/rocky-todo").await,
        ""
    );
}

#[tokio::test]
async fn cwd_narrows_board_scope() {
    let f = fx();
    started_by_session(&f, "sess-gone", "버려진 작업");
    f.store.ensure_board("ogpeek", None, "logan").unwrap();
    let state = statusline_state(&f, None);
    assert_eq!(
        line_from(&state, "?session=sess-live&cwd=/w/ogpeek").await,
        ""
    );
}

#[tokio::test]
async fn many_stale_doings_counted() {
    // TS 는 boardKeyOf 스파이로 "보드당 한 번" 을 검증했다 — Rust 는 메서드를 못 감싸
    // (요청-로컬 HashMap 캐시가 코드 구조로 보장), 판정 결과만 고정한다.
    let f = fx();
    started_by_session(&f, "sess-gone", "버려진 작업 1");
    started_by_session(&f, "sess-gone", "버려진 작업 2");
    started_by_session(&f, "sess-gone", "버려진 작업 3");
    let state = statusline_state(&f, None);
    assert_eq!(
        line_from(&state, "?session=nobody&cwd=/w/rocky-todo").await,
        "⚠3"
    );
}

#[tokio::test]
async fn template_is_configurable() {
    let f = fx();
    started_by_session(&f, "sess-gone", "버려진 작업");
    let state = statusline_state(&f, Some("[doing={doing}][ stale={stale}]"));
    assert_eq!(
        line_from(&state, "?session=sess-live&cwd=/w/rocky-todo").await,
        "doing=1 stale=1"
    );
}

// ── due / collect (보드 마감 · 수집함 미올림) ──

fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

#[tokio::test]
async fn due_counts_today_and_overdue_but_not_done_or_future() {
    let f = fx();
    for (title, due, done) in [
        ("지난", "2020-01-01", false),
        ("오늘", today().as_str(), false),
        ("미래", "2099-01-01", false),
        ("끝난 지난", "2020-01-01", true),
    ] {
        let todo = f
            .store
            .create_todo(
                &CreateTodoInput {
                    board: "rocky-todo".into(),
                    title: title.into(),
                    due: Some(due.into()),
                    ..Default::default()
                },
                "logan",
            )
            .unwrap();
        if done {
            f.store
                .set_todo_status(&todo.id, StatusAction::Done, "logan", None)
                .unwrap();
        }
    }
    let state = statusline_state(&f, Some("[⏰{due}]"));
    assert_eq!(line_from(&state, "?cwd=/w/rocky-todo").await, "⏰2");
    // 보드가 안 풀리면 전체 기준 — 같은 2.
    assert_eq!(line_from(&state, "?cwd=/nowhere").await, "⏰2");
}

#[tokio::test]
async fn collect_uses_cached_inbox_only_and_excludes_promoted() {
    let f = fx();
    let mut todo_input = CreateTodoInput {
        board: "rocky-todo".into(),
        title: "올라간 것".into(),
        ..Default::default()
    };
    todo_input.links = Some(vec![TodoLink {
        url: "https://x/a".into(),
        title: None,
    }]);
    f.store.create_todo(&todo_input, "logan").unwrap();
    let inbox = rocky_core::inbox::InboxResponse {
        sources: vec![rocky_core::inbox::InboxSourceResult {
            name: "s".into(),
            available: true,
            reason: None,
            fetched_at: "t".into(),
            items: vec![
                rocky_core::inbox::InboxItem {
                    id: "a".into(),
                    title: "a".into(),
                    url: Some("https://x/a".into()),
                    note: None,
                    due: None,
                    created_at: None,
                },
                rocky_core::inbox::InboxItem {
                    id: "b".into(),
                    title: "b".into(),
                    url: Some("https://x/b".into()),
                    note: None,
                    due: None,
                    created_at: None,
                },
            ],
        }],
    };
    let state = rebuild(&f, move |o| {
        o.sessions = Some(fixed_sessions(fixture_sessions()));
        o.statusline_template = Some("[📥{collect}]".into());
        o.inbox = Some(rockyd::inbox_exec::fixed_inbox(inbox));
    });
    assert_eq!(line_from(&state, "?cwd=/w/rocky-todo").await, "📥1");
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
    let state = statusline_state(&f, None);
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
    let state = statusline_state(&f, None);
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
