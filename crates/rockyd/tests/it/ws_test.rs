//! 노트 소켓(`/api/ws`) — 실제 서버에 실제 웹소켓 클라이언트로. 문서·편집·방송·프레즌스가 연결 하나로 오가고,
//! 다른 사이트에서 연 소켓은 핸드셰이크에서 끊긴다.

use crate::common::*;
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use rocky_core::note_doc::NoteDoc;
use rocky_core::types::StatusAction;
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn unb64(text: &str) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode(text)
        .unwrap()
}

async fn serve(f: &Fx) -> std::net::SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = rockyd::daemon::build_router(f.state.clone(), None);
    tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap();
    });
    addr
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn send(ws: &mut Socket, frame: Value) {
    ws.send(Message::Text(frame.to_string().into()))
        .await
        .unwrap();
}

/// 다음 텍스트 프레임 — 2초 안에 안 오면 실패.
async fn next(ws: &mut Socket) -> Value {
    loop {
        let msg = tokio::time::timeout(std::time::Duration::from_secs(2), ws.next())
            .await
            .expect("프레임이 와야 한다")
            .unwrap()
            .unwrap();
        if let Message::Text(text) = msg {
            return serde_json::from_str(&text).unwrap();
        }
    }
}

async fn note(f: &Fx, content: &str) -> String {
    let (status, body) = post(
        &f.state,
        "/api/notes",
        json!({ "title": "소켓", "content": content }),
    )
    .await;
    assert_eq!(status, 201, "{body}");
    body["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn one_socket_carries_doc_edits_broadcasts_and_presence() {
    let f = fx();
    let id = note(&f, "hello").await;
    let addr = serve(&f).await;
    let (mut a, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/api/ws?actor=logan"))
        .await
        .unwrap();
    let (mut b, _) =
        tokio_tungstenite::connect_async(format!("ws://{addr}/api/ws?actor=claude-code"))
            .await
            .unwrap();

    // b 가 구독한다
    send(&mut b, json!({ "t": "sub", "note": id })).await;
    assert_eq!(next(&mut b).await, json!({ "t": "subbed", "note": id }));

    // a 가 문서를 받아 고친다
    send(&mut a, json!({ "t": "doc", "id": 1, "note": id })).await;
    let doc = next(&mut a).await;
    assert_eq!(doc["t"], "ok");
    assert_eq!(doc["id"], 1);
    let client = NoteDoc::from_state(&unb64(doc["body"]["update"].as_str().unwrap())).unwrap();
    assert_eq!(client.text(), "hello");
    let before = client.state_vector();
    client.set_text("hello world");
    let update = client.diff_since(&before).unwrap();
    send(
        &mut a,
        json!({ "t": "update", "id": 2, "note": id, "update": b64(&update), "client": "ca" }),
    )
    .await;
    let ack = next(&mut a).await;
    assert_eq!(
        (
            ack["t"].as_str(),
            ack["id"].as_i64(),
            ack["body"]["changed"].as_bool()
        ),
        (Some("ok"), Some(2), Some(true))
    );

    // b 는 같은 연결로 그 편집을 받는다
    let ev = next(&mut b).await;
    assert_eq!(ev["t"], "ev");
    assert_eq!(ev["ev"]["kind"], "update");
    assert_eq!(ev["ev"]["client"], "ca");
    let mirror = NoteDoc::open(None, "");
    mirror
        .apply(&unb64(doc["body"]["update"].as_str().unwrap()))
        .unwrap();
    mirror
        .apply(&unb64(ev["ev"]["update"].as_str().unwrap()))
        .unwrap();
    assert_eq!(mirror.text(), "hello world");

    // 프레즌스 — 저장 없이 방송만, actor 는 a 의 쿼리
    send(
        &mut a,
        json!({ "t": "presence", "id": 3, "note": id, "client": "ca", "state": { "cursor": 5 } }),
    )
    .await;
    assert_eq!(next(&mut a).await["t"], "ok");
    let presence = next(&mut b).await;
    assert_eq!(presence["ev"]["kind"], "presence");
    assert_eq!(presence["ev"]["actor"], "logan");
    assert_eq!(presence["ev"]["state"]["cursor"], 5);

    // 없는 노트·모르는 프레임은 err
    send(&mut a, json!({ "t": "doc", "id": 4, "note": "없음" })).await;
    assert_eq!(next(&mut a).await["t"], "err");
    send(&mut a, json!({ "t": "nope", "id": 5, "note": id })).await;
    assert_eq!(next(&mut a).await["t"], "err");
}

/// 웹소켓은 CORS 밖이다 — 다른 사이트의 페이지가 연 소켓은 핸드셰이크에서 403.
#[tokio::test]
async fn a_cross_site_socket_is_refused_at_the_handshake() {
    let f = fx();
    let addr = serve(&f).await;
    for (header, value) in [
        ("origin", "https://evil.example"),
        ("sec-fetch-site", "cross-site"),
    ] {
        let mut req = format!("ws://{addr}/api/ws").into_client_request().unwrap();
        req.headers_mut().insert(header, value.parse().unwrap());
        let err = tokio_tungstenite::connect_async(req)
            .await
            .expect_err("거부돼야 한다");
        assert!(err.to_string().contains("403"), "{header}: {err}");
    }
    // 같은 사이트(Origin = 이 데몬)는 열린다
    let mut req = format!("ws://{addr}/api/ws").into_client_request().unwrap();
    req.headers_mut()
        .insert("origin", format!("http://{addr}").parse().unwrap());
    assert!(tokio_tungstenite::connect_async(req).await.is_ok());
}

// ── 보드 싱크(`sync` → `rows`) ─────────────────────────────────────────────

async fn create_todo(f: &Fx, title: &str) -> String {
    let (status, body) = post(
        &f.state,
        "/api/todos",
        json!({ "board": "sync", "title": title }),
    )
    .await;
    assert_eq!(status, 201, "{body}");
    body["id"].as_str().unwrap().to_string()
}

fn todo_ids(frame: &Value) -> Vec<String> {
    frame["rows"]["todos"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["id"].as_str().unwrap().to_string())
        .collect()
}

async fn connect(addr: std::net::SocketAddr) -> Socket {
    tokio_tungstenite::connect_async(format!("ws://{addr}/api/ws?actor=logan"))
        .await
        .unwrap()
        .0
}

#[tokio::test]
async fn sync_sends_a_full_copy_then_pushes_changed_rows() {
    let f = fx();
    let first = create_todo(&f, "first").await;
    let addr = serve(&f).await;
    let mut ws = connect(addr).await;
    send(&mut ws, json!({ "t": "sync" })).await;
    let copy = next(&mut ws).await;
    assert_eq!(copy["t"], "rows");
    assert_eq!(copy["full"], true);
    assert_eq!(todo_ids(&copy), vec![first]);
    let seq = copy["seq"].as_i64().unwrap();

    let second = create_todo(&f, "second").await;
    let pushed = next(&mut ws).await;
    assert_eq!(pushed["t"], "rows");
    assert_eq!(pushed["full"], false);
    assert_eq!(pushed["epoch"], copy["epoch"]);
    assert_eq!(todo_ids(&pushed), vec![second]);
    assert!(pushed["seq"].as_i64().unwrap() > seq);
}

/// 클라이언트는 마지막으로 받은 `rows` 의 `seq`·`epoch` 를 들고 다시 붙는다 — 끊긴 사이의 쓰기만 받는다.
#[tokio::test]
async fn sync_resumes_from_the_last_frame_after_reconnecting() {
    let f = fx();
    create_todo(&f, "old").await;
    let addr = serve(&f).await;
    let mut ws = connect(addr).await;
    send(&mut ws, json!({ "t": "sync", "id": 1 })).await;
    let copy = next(&mut ws).await;
    assert_eq!(copy["id"], 1);
    let (seq, epoch) = (copy["seq"].clone(), copy["epoch"].clone());
    ws.close(None).await.unwrap();

    let newer = create_todo(&f, "newer").await;
    let mut again = connect(addr).await;
    send(
        &mut again,
        json!({ "t": "sync", "since": seq, "epoch": epoch }),
    )
    .await;
    let resumed = next(&mut again).await;
    assert_eq!(resumed["full"], false);
    assert_eq!(todo_ids(&resumed), vec![newer]);

    // 다른 DB 의 사본이면 통째로 다시 준다.
    send(
        &mut again,
        json!({ "t": "sync", "since": seq, "epoch": "not-this-db" }),
    )
    .await;
    let everything = next(&mut again).await;
    assert_eq!(everything["full"], true);
    assert_eq!(todo_ids(&everything).len(), 2);
}

/// 히스토리를 남기지 않는 쓰기(세션 귀속)도 바뀐 행으로 밀려온다.
#[tokio::test]
async fn writes_without_history_are_pushed_too() {
    let f = fx();
    let id = create_todo(&f, "claimed").await;
    f.store
        .set_todo_status(&id, StatusAction::Start, "claude-code", None)
        .unwrap();
    let addr = serve(&f).await;
    let mut ws = connect(addr).await;
    send(&mut ws, json!({ "t": "sync" })).await;
    assert_eq!(next(&mut ws).await["full"], true);

    f.store
        .claim_doing_session(&id, "session-1", None)
        .unwrap()
        .expect("귀속이 붙어야 이 테스트가 뜻이 있다");
    let pushed = next(&mut ws).await;
    assert_eq!(todo_ids(&pushed), vec![id]);
    assert_eq!(pushed["rows"]["todos"][0]["doingSessionId"], "session-1");
    assert_eq!(pushed["rows"]["history"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn a_socket_that_never_synced_gets_no_rows() {
    let f = fx();
    let addr = serve(&f).await;
    let mut ws = connect(addr).await;
    create_todo(&f, "quiet").await;
    let got = tokio::time::timeout(std::time::Duration::from_millis(300), ws.next()).await;
    assert!(got.is_err(), "sync 전에는 프레임이 오면 안 된다: {got:?}");
}

/// 같은 노트를 2초 안에 다시 고치면 전역 변경 이벤트가 조절돼 나오지 않는다 — 그래도 바뀐 노트 행은 밀려온다. 깨우기가
/// 변경 이벤트가 아니라 SQLite 훅인 이유이고, 이 테스트가 그것을 가른다.
#[tokio::test]
async fn doc_edits_inside_the_event_throttle_are_pushed_too() {
    let f = fx();
    let id = note(&f, "hello").await;
    let addr = serve(&f).await;
    let mut ws = connect(addr).await;
    send(&mut ws, json!({ "t": "sync" })).await;
    assert_eq!(next(&mut ws).await["full"], true);

    for text in ["hello 1", "hello 2"] {
        let doc = f.store.note_doc_state(&id, None, None).unwrap();
        let client = NoteDoc::from_state(&doc.update).unwrap();
        let before = client.state_vector();
        client.set_text(text);
        let update = client.diff_since(&before).unwrap();
        f.store
            .apply_note_update(&id, &update, "logan", Some("ca"), None)
            .unwrap();
        let pushed = next(&mut ws).await;
        assert_eq!(pushed["rows"]["notes"][0]["content"], text, "{pushed}");
    }
}
