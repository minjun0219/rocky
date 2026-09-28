//! 노트 CRDT 문서 라우트 — 상태 받기 · update 적용·방송 · 프레즌스 · 노트별 SSE.

mod common;

use base64::Engine;
use common::*;
use rocky_core::note_doc::NoteDoc;
use serde_json::{json, Value};

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn unb64(text: &str) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode(text)
        .unwrap()
}

async fn note(f: &Fx, content: &str) -> String {
    let (status, body) = post(
        &f.state,
        "/api/notes",
        json!({ "board": "rocky", "title": "패드", "content": content }),
    )
    .await;
    assert_eq!(status, 201, "{body}");
    body["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn get_doc_gives_a_state_a_client_can_open_and_diff_against() {
    let f = fx();
    let id = note(&f, "hello").await;
    let (status, body) = get(&f.state, &format!("/api/notes/{id}/doc")).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["noteId"], id);
    let client = NoteDoc::from_state(&unb64(body["update"].as_str().unwrap())).unwrap();
    assert_eq!(client.text(), "hello");
    // 아는 만큼(sv)을 주면 빈 차분.
    let sv = body["sv"].as_str().unwrap();
    let (status, diff) = get(
        &f.state,
        &format!("/api/notes/{id}/doc?sv={}", urlencoding(sv)),
    )
    .await;
    assert_eq!(status, 200, "{diff}");
    assert!(
        !client
            .apply(&unb64(diff["update"].as_str().unwrap()))
            .unwrap()
            .state_changed
    );
}

#[tokio::test]
async fn post_doc_applies_updates_content_and_broadcasts_to_the_note_stream() {
    let f = fx();
    let id = note(&f, "hello").await;
    let (_, body) = get(&f.state, &format!("/api/notes/{id}/doc")).await;
    let client = NoteDoc::from_state(&unb64(body["update"].as_str().unwrap())).unwrap();
    let server_sv = unb64(body["sv"].as_str().unwrap());
    let mut rx = f.state.note_stream(&id).subscribe();

    client.append("from web");
    let diff = client.diff_since(&server_sv).unwrap();
    let (status, applied) = post(
        &f.state,
        &format!("/api/notes/{id}/doc"),
        json!({ "update": b64(&diff), "client": "c1" }),
    )
    .await;
    assert_eq!(status, 200, "{applied}");
    assert_eq!(applied["changed"], true);
    // 읽는 쪽의 진실(content)이 따라온다.
    let (_, shown) = get(&f.state, &format!("/api/notes/{id}")).await;
    assert_eq!(shown["note"]["content"], "hello\nfrom web");
    // 같은 노트의 구독자가 그 update 를 받는다.
    let event: Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
    assert_eq!(event["kind"], "update");
    assert_eq!(event["client"], "c1");
    assert_eq!(event["actor"], "tester");
    // 방송된 조각을 서버의 옛 상태 위에 얹으면 새 본문이 된다.
    let replay = NoteDoc::from_state(&unb64(body["update"].as_str().unwrap())).unwrap();
    replay
        .apply(&unb64(event["update"].as_str().unwrap()))
        .unwrap();
    assert_eq!(replay.text(), "hello\nfrom web");
    // 이미 아는 update 를 다시 보내면 바뀐 게 없고 방송도 없다.
    let (status, again) = post(
        &f.state,
        &format!("/api/notes/{id}/doc"),
        json!({ "update": b64(&diff) }),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(again["changed"], false);
    assert!(rx.try_recv().is_err());
}

#[tokio::test]
async fn bad_updates_are_400_and_unknown_notes_are_404() {
    let f = fx();
    let id = note(&f, "x").await;
    let (status, _) = post(
        &f.state,
        &format!("/api/notes/{id}/doc"),
        json!({ "update": "not base64!" }),
    )
    .await;
    assert_eq!(status, 400);
    let (status, _) = post(
        &f.state,
        &format!("/api/notes/{id}/doc"),
        json!({ "update": b64(b"junk") }),
    )
    .await;
    assert_eq!(status, 400);
    let (status, _) = post(&f.state, &format!("/api/notes/{id}/doc"), json!({})).await;
    assert_eq!(status, 400);
    for path in ["/api/notes/nope-9/doc", "/api/notes/nope-9/doc/events"] {
        let (status, body) = get(&f.state, path).await;
        assert_eq!(status, 404, "{path}: {body}");
    }
    let (status, _) = post(&f.state, "/api/notes/nope-9/presence", json!({})).await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn presence_is_broadcast_but_never_stored() {
    let f = fx();
    let id = note(&f, "x").await;
    let mut rx = f.state.note_stream(&id).subscribe();
    let (status, _) = call(
        &f.state,
        "POST",
        &format!("/api/notes/{id}/presence"),
        Some(json!({ "client": "c1", "state": { "cursor": 3 } })),
        ReqOptions {
            actor: "logan",
            ..ReqOptions::default()
        },
    )
    .await;
    assert_eq!(status, 200);
    let event: Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
    assert_eq!(event["kind"], "presence");
    assert_eq!(event["actor"], "logan");
    assert_eq!(event["state"]["cursor"], 3);
    let (_, shown) = get(&f.state, &format!("/api/notes/{id}")).await;
    assert_eq!(shown["history"].as_array().unwrap().len(), 1, "create 뿐");
}

/// 노트별 SSE 는 `text/event-stream` 이고 전역 `/api/events` 와 채널이 다르다.
#[tokio::test]
async fn note_events_is_an_sse_response_on_its_own_channel() {
    let f = fx();
    let id = note(&f, "x").await;
    let request = axum::http::Request::builder()
        .method("GET")
        .uri(format!("/api/notes/{id}/doc/events"))
        .body(axum::body::Body::empty())
        .unwrap();
    let response = rockyd::server::handle_api(&f.state, request, Some("127.0.0.1".into())).await;
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["content-type"], "text/event-stream");
    assert_eq!(f.state.note_stream(&id).receiver_count(), 1);
    assert_eq!(f.state.events.receiver_count(), 0);
}

/// MCP `note_write`/CLI 가 지나는 PATCH 경로의 편집도 노트 스트림에 방송된다 — 열린 웹
/// 편집기가 에이전트의 append 를 즉시 본다(실제로 브라우저에서 못 받아 잡힌 구멍).
#[tokio::test]
async fn agent_edits_through_patch_are_broadcast_to_the_note_stream() {
    let f = fx();
    let id = note(&f, "hello").await;
    let (_, body) = get(&f.state, &format!("/api/notes/{id}/doc")).await;
    let client = NoteDoc::from_state(&unb64(body["update"].as_str().unwrap())).unwrap();
    let mut rx = f.state.note_stream(&id).subscribe();
    let (status, _) = call(
        &f.state,
        "PATCH",
        &format!("/api/notes/{id}"),
        Some(json!({ "content": "에이전트가 적음", "mode": "append" })),
        ReqOptions {
            actor: "codex",
            ..ReqOptions::default()
        },
    )
    .await;
    assert_eq!(status, 200);
    let event: Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
    assert_eq!(event["kind"], "update");
    assert_eq!(event["actor"], "codex");
    assert!(event["client"].is_null());
    client
        .apply(&unb64(event["update"].as_str().unwrap()))
        .unwrap();
    assert_eq!(client.text(), "hello\n에이전트가 적음");
}

fn urlencoding(s: &str) -> String {
    s.replace('+', "%2B")
        .replace('/', "%2F")
        .replace('=', "%3D")
}
