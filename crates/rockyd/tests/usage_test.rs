//! 사용 로그 — REST 입구·웹 이벤트 라우트가 싱크에 무엇을 넘기는지.

mod common;

use axum::body::Body;
use axum::http::Request;
use common::*;
use rocky_core::usage::UsageSource;
use rockyd::server::handle_api;
use rockyd::usage_sink::capture_sink;

#[tokio::test]
async fn rest_calls_are_recorded_by_shape_with_actor_client_and_status() {
    let f = fx();
    let (sink, captured) = capture_sink();
    let state = rebuild(&f, |o| o.usage = Some(sink));
    // 성공 — 웹 클라이언트 헤더 없이 User-Agent 로 추정.
    let req = Request::builder()
        .method("GET")
        .uri("/api/boards")
        .header("x-rocky-actor", "logan")
        .header("user-agent", "Mozilla/5.0")
        .body(Body::empty())
        .unwrap();
    handle_api(&state, req, Some("127.0.0.1".into())).await;
    // 실패(없는 todo) — TUI 헤더.
    let req = Request::builder()
        .method("GET")
        .uri("/api/todos/nope-99?board=rocky-todo")
        .header("x-rocky-client", "tui")
        .body(Body::empty())
        .unwrap();
    handle_api(&state, req, Some("127.0.0.1".into())).await;
    // 1초마다 도는 것은 안 남긴다.
    for path in ["/api/health", "/api/statusline?cwd=/w"] {
        let req = Request::builder()
            .method("GET")
            .uri(path)
            .body(Body::empty())
            .unwrap();
        handle_api(&state, req, Some("127.0.0.1".into())).await;
    }
    let events = captured.lock().unwrap().clone();
    assert_eq!(events.len(), 2, "{events:?}");
    assert_eq!(events[0].source, UsageSource::Rest);
    assert_eq!(events[0].name, "GET /api/boards");
    assert_eq!(events[0].actor.as_deref(), Some("logan"));
    assert_eq!(events[0].client.as_deref(), Some("web"));
    assert!(events[0].ok && events[0].ms.is_some());
    assert_eq!(events[1].name, "GET /api/todos/:ref");
    assert_eq!(events[1].client.as_deref(), Some("tui"));
    assert!(!events[1].ok);
}

#[tokio::test]
async fn web_events_are_named_and_bounded() {
    let f = fx();
    let (sink, captured) = capture_sink();
    let state = rebuild(&f, |o| o.usage = Some(sink));
    let (status, _) = post(
        &state,
        "/api/usage",
        serde_json::json!({ "name": "web:now-row", "meta": { "kind": "dead" } }),
    )
    .await;
    assert_eq!(status, 200);
    // 이름 규칙 위반은 400 — 아무 문자열이나 로그에 실어 보내는 통로가 아니다.
    let (status, _) = post(
        &state,
        "/api/usage",
        serde_json::json!({ "name": "anything" }),
    )
    .await;
    assert_eq!(status, 400);
    let events = captured.lock().unwrap().clone();
    // 웹 이벤트 하나 + 그 POST 두 건은 라우트 자체가 제외라 REST 로는 안 남는다.
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0].source, UsageSource::Web);
    assert_eq!(events[0].name, "web:now-row");
    assert_eq!(events[0].client.as_deref(), Some("web"));
    assert_eq!(events[0].meta, Some(serde_json::json!({ "kind": "dead" })));
}
