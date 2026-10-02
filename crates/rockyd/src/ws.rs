//! 웹 노트 동시 편집 소켓(`GET /api/ws`) — 노트 문서·편집·프레즌스를 **연결 하나**로 오간다.
//!
//! 예전엔 노트마다 SSE(`/doc/events`)를 열고 편집·프레즌스를 건마다 POST 했다 — 브라우저의 호스트당 연결
//! 한도(HTTP/1.1 6개) 때문에 편집기 연결을 포커스 때 열고 20초 뒤 닫는 절차가 필요했고, 편집마다 요청 왕복이
//! 붙었다. 같은 일을 하는 HTTP 라우트는 그대로 둔다(소켓을 못 여는 환경의 폴백).
//!
//! 프레임은 JSON 텍스트다. 요청(`id` 가 있으면 같은 `id` 로 답한다):
//! - `{t:"doc", id, note, sv?}` → `{t:"ok", id, body:{noteId, update, sv}}` — 문서(상태 벡터가 있으면 차분)
//! - `{t:"update", id, note, update, client}` → `{t:"ok", id, body:{changed, updatedAt}}`
//! - `{t:"presence", id, note, client, state}` → `{t:"ok", id}` — 저장하지 않고 방송만
//! - `{t:"sub", note}` → `{t:"subbed", note}` 뒤로 `{t:"ev", note, ev}` — 노트별 방송 그대로
//! - `{t:"unsub", note}`
//!
//! 방송이 밀리면 `{t:"lag", note}` — 클라이언트가 상태 벡터로 차분을 다시 받는다(SSE 판은 끊고 재접속했다).
//! 실패는 `{t:"err", id, error}`.
//!
//! **cross-site 가드**: 웹소켓은 브라우저 CORS 밖이라, 다른 사이트의 페이지도 이 주소로 소켓을 열 수 있다
//! (cross-site WebSocket hijacking). 핸드셰이크에서 REST 변경과 같은 규칙(`is_cross_site_request`)으로 끊는다.

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use rocky_core::usage::{UsageEvent, UsageSource};
use serde_json::{json, Value};
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;

use crate::server::{decode_b64, encode_b64, ServerState};

/// 연결 하나를 끝까지 돈다.
pub async fn serve_socket(mut socket: WebSocket, state: Arc<ServerState>, actor: String) {
    let mut event = UsageEvent::new(UsageSource::Rest, "WS /api/ws".to_string(), true);
    event.actor = Some(actor.clone());
    event.client = Some("web".into());
    state.record_usage(event);

    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<String>();
    let mut subs: HashMap<String, JoinHandle<()>> = HashMap::new();
    loop {
        tokio::select! {
            incoming = socket.recv() => {
                let Some(Ok(message)) = incoming else { break };
                let text = match message {
                    Message::Text(text) => text.to_string(),
                    Message::Close(_) => break,
                    _ => continue,
                };
                for reply in handle_frame(&state, &actor, &text, &mut subs, &out_tx) {
                    if socket.send(Message::Text(reply.into())).await.is_err() {
                        break;
                    }
                }
            }
            Some(outgoing) = out_rx.recv() => {
                if socket.send(Message::Text(outgoing.into())).await.is_err() {
                    break;
                }
            }
        }
    }
    for (_, task) in subs {
        task.abort();
    }
}

/// 프레임 하나 → 바로 돌려줄 답(들). 구독은 `subs` 에 작업으로 걸고, 방송은 `out` 으로 흘려보낸다.
fn handle_frame(
    state: &Arc<ServerState>,
    actor: &str,
    text: &str,
    subs: &mut HashMap<String, JoinHandle<()>>,
    out: &mpsc::UnboundedSender<String>,
) -> Vec<String> {
    let Ok(frame) = serde_json::from_str::<Value>(text) else {
        return vec![json!({ "t": "err", "error": "bad frame" }).to_string()];
    };
    let id = frame.get("id").cloned().unwrap_or(Value::Null);
    let str_of = |k: &str| frame.get(k).and_then(Value::as_str).map(str::to_string);
    let Some(note) = str_of("note") else {
        return vec![json!({ "t": "err", "id": id, "error": "note is required" }).to_string()];
    };
    let store = &state.store;
    let reply = |result: Result<Value, String>| match result {
        Ok(body) => json!({ "t": "ok", "id": id, "body": body }).to_string(),
        Err(error) => json!({ "t": "err", "id": id, "error": error }).to_string(),
    };
    match frame.get("t").and_then(Value::as_str).unwrap_or("") {
        "doc" => {
            let result = (|| {
                let since = match str_of("sv") {
                    Some(sv) => Some(decode_b64(&sv)?),
                    None => None,
                };
                let doc = store
                    .note_doc_state(&note, since.as_deref(), None)
                    .map_err(|e| e.to_string())?;
                Ok(json!({
                    "noteId": doc.note_id,
                    "update": encode_b64(&doc.update),
                    "sv": encode_b64(&doc.state_vector),
                }))
            })();
            vec![reply(result)]
        }
        "update" => {
            let result = (|| {
                let update = str_of("update").ok_or("update is required")?;
                let bytes = decode_b64(&update)?;
                // 방송은 스토어의 문서 이벤트가 한다 — HTTP 경로·에이전트 경로와 같은 길.
                let applied = store
                    .apply_note_update(&note, &bytes, actor, str_of("client").as_deref(), None)
                    .map_err(|e| e.to_string())?;
                Ok(json!({ "changed": applied.changed, "updatedAt": applied.note.updated_at }))
            })();
            vec![reply(result)]
        }
        "presence" => {
            let result = (|| {
                let found = store
                    .get_note(&note, None)
                    .map_err(|e| e.to_string())?
                    .ok_or_else(|| format!("note not found: {note}"))?;
                state.broadcast_note(
                    &found.id,
                    &json!({
                        "kind": "presence",
                        "client": str_of("client"),
                        "actor": actor,
                        "state": frame.get("state").cloned().unwrap_or(Value::Null),
                        "at": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                    }),
                );
                Ok(Value::Null)
            })();
            vec![reply(result)]
        }
        "sub" => {
            let found = match store.get_note(&note, None) {
                Ok(Some(found)) => found,
                Ok(None) => {
                    return vec![
                        json!({ "t": "err", "id": id, "error": format!("note not found: {note}") })
                            .to_string(),
                    ]
                }
                Err(e) => {
                    return vec![json!({ "t": "err", "id": id, "error": e.to_string() }).to_string()]
                }
            };
            if let Some(old) = subs.remove(&note) {
                old.abort();
            }
            let receiver = state.subscribe_note(&found.id);
            subs.insert(
                note.clone(),
                tokio::spawn(forward(receiver, note.clone(), out.clone())),
            );
            vec![json!({ "t": "subbed", "note": note }).to_string()]
        }
        "unsub" => {
            if let Some(task) = subs.remove(&note) {
                task.abort();
            }
            Vec::new()
        }
        other => vec![
            json!({ "t": "err", "id": id, "error": format!("unknown frame: {other}") }).to_string(),
        ],
    }
}

/// 노트 방송 → 소켓. 밀리면 `lag` 를 알리고 이어 듣는다(클라이언트가 차분을 다시 받는다).
async fn forward(
    mut receiver: broadcast::Receiver<String>,
    note: String,
    out: mpsc::UnboundedSender<String>,
) {
    loop {
        let frame = match receiver.recv().await {
            Ok(payload) => {
                let ev = serde_json::from_str::<Value>(&payload).unwrap_or(Value::Null);
                json!({ "t": "ev", "note": note, "ev": ev })
            }
            Err(broadcast::error::RecvError::Lagged(_)) => json!({ "t": "lag", "note": note }),
            Err(broadcast::error::RecvError::Closed) => return,
        };
        if out.send(frame.to_string()).is_err() {
            return;
        }
    }
}
