//! 사용 로그 — REST 입구·웹 이벤트 라우트가 싱크에 무엇을 넘기는지.

use crate::common::*;
use axum::body::Body;
use axum::http::Request;
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
    // 실패(없는 todo) — 클라이언트 헤더.
    let req = Request::builder()
        .method("GET")
        .uri("/api/todos/nope-99?board=rocky-todo")
        .header("x-rocky-client", "mcp")
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
    assert_eq!(events[1].client.as_deref(), Some("mcp"));
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

/// `KNOWN_SURFACES` 의 REST 항목이 실제 라우트인지 — 각 모양을 요청해 라우트 없음(`not found:
/// METHOD PATH`)이 아닌지 본다. 없는 id 로 부르므로 404 는 나와도 되지만 그건 라우트가 잡힌
/// 뒤의 "todo not found" 라 본문이 다르다.
#[tokio::test]
async fn every_known_rest_surface_is_a_real_route() {
    let f = fx();
    let (sink, _captured) = capture_sink();
    let state = rebuild(&f, |o| o.usage = Some(sink));
    for (source, name) in rocky_core::usage::KNOWN_SURFACES {
        // 웹소켓(`WS /api/ws`)은 REST 입구가 아니라 axum 라우터의 업그레이드다 — `ws_test` 가 실제 클라이언트로 본다.
        if *source != UsageSource::Rest || name.starts_with("WS ") {
            continue;
        }
        let (method, path) = name.split_once(' ').unwrap();
        let path = path.replace(":ref", "zzzz-nope");
        let req = Request::builder()
            .method(method)
            .uri(path.as_str())
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap();
        let response = handle_api(&state, req, Some("127.0.0.1".into())).await;
        let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .unwrap();
        let text = String::from_utf8_lossy(&bytes).to_string();
        assert!(
            !text.contains(&format!("not found: {method} ")),
            "{name} 은 라우트가 아니다: {text}"
        );
    }
}

/// 반대 방향 — `dispatch` 에 글자 그대로 적힌 라우트(`path == "/api/…"`)는 `KNOWN_SURFACES` 에 있거나
/// 기록 제외(`normalize_route` 가 None)여야 한다. 새 라우트를 만들고 목록에 안 넣으면 `rocky usage` 가 그
/// 표면을 "안 쓴 것" 으로도 못 보여 준다. 접두어로 고르는 라우트(`/api/todos/:ref/…`)는 소스에서 뽑을 수
/// 없어 여기서는 못 본다.
#[test]
fn every_literal_route_is_known_or_skipped() {
    let source = include_str!("../../src/server.rs");
    let known: Vec<&str> = rocky_core::usage::KNOWN_SURFACES
        .iter()
        .filter(|(s, _)| *s == UsageSource::Rest)
        .filter_map(|(_, name)| name.split_once(' ').map(|(_, path)| path))
        .collect();
    let needle = "path == \"";
    let mut missing = Vec::new();
    let mut seen = 0;
    for (at, _) in source.match_indices(needle) {
        let rest = &source[at + needle.len()..];
        let Some(end) = rest.find('"') else { continue };
        let path = &rest[..end];
        if !path.starts_with("/api/") {
            continue;
        }
        seen += 1;
        let skipped = rocky_core::usage::normalize_route("GET", path).is_none();
        if !skipped && !known.contains(&path) {
            missing.push(path.to_string());
        }
    }
    assert!(
        seen > 20,
        "server.rs 에서 라우트를 거의 못 찾았다({seen}) — 매칭 모양이 바뀌었나"
    );
    missing.sort();
    missing.dedup();
    assert!(
        missing.is_empty(),
        "KNOWN_SURFACES(crates/rocky-core/src/usage.rs)에 없는 라우트: {missing:?}"
    );
}
