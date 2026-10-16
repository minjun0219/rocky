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
//! **보드 싱크**(같은 연결, 규칙은 `docs/features/sync.md`):
//! - `{t:"sync", id?, since?, epoch?}` → `{t:"rows", id?, epoch, seq, full, rows:{boards, sections, todos, comments, notes,
//!   history, handoffs}}` — `full` 이면 클라이언트가 사본을 통째로 바꾼다(히스토리는 최근 2,000줄뿐이고 그보다 오래된
//!   줄은 REST 로 받는다). 다음에는 `since=seq`, `epoch` 로 묻는다.
//! - 그 뒤로는 보드 행이 바뀔 때마다 데몬이 같은 `rows`(`full:false`, 그 연결에 보낸 순번 이후)를 먼저 보낸다.
//!   `sync` 를 다시 보내면 그 커서로 다시 시작한다. 밀다가 읽기가 실패하면 `{t:"err", error}` 를 한 번 보내고 밀기를
//!   멈춘다 — 클라이언트가 `sync` 를 다시 보낸다.
//! - `{t:"tx", id, ops:[{op:"todo.create", id, …}, …]}` → `{t:"ack", id, seq}` 또는 `{t:"reject", id, at, error}`.
//!   어휘는 `rocky_core::sync::SyncOp`(원격에서도 이미 허용된 보드 쓰기뿐). `sync` 한 연결이면 결과 `rows` 를 `ack` 보다 먼저
//!   보낸다. 만들기는 클라이언트가 정한 id 를 쓰고, 그 id 가 이미 있으면 새로 만들지 않는다(다시 보내도 안전하다).
//!   트랜잭션은 사람의 쓰기다 — 에이전트 actor(`actors::is_agent_actor`)면 `reject` 한다.
//!
//! **cross-site 가드**: 웹소켓은 브라우저 CORS 밖이라, 다른 사이트의 페이지도 이 주소로 소켓을 열 수 있다
//! (cross-site WebSocket hijacking). 핸드셰이크에서 REST 변경과 같은 규칙(`is_cross_site_request`)으로 끊는다.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket};
use rocky_core::sync::{SyncOp, SyncRows};
use rocky_core::usage::{UsageEvent, UsageSource};
use serde_json::{json, Value};
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;

use crate::server::{decode_b64, encode_b64, ServerState};

/// 행이 바뀐 신호를 받은 뒤 모으는 시간 — 쓰기 하나가 여러 행을 건드려도 `rows` 프레임 하나로 보낸다.
const SYNC_BATCH: Duration = Duration::from_millis(20);

/// 싱크를 시작한 연결의 커서 — 이 연결에 마지막으로 보낸 머리.
struct SyncCursor {
    epoch: String,
    seq: i64,
}

/// 연결 하나를 끝까지 돈다.
pub async fn serve_socket(mut socket: WebSocket, state: Arc<ServerState>, actor: String) {
    record(&state, "WS /api/ws", &actor);

    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<String>();
    let mut subs: HashMap<String, JoinHandle<()>> = HashMap::new();
    let mut cursor: Option<SyncCursor> = None;
    let mut rows_changed = state.sync_rows.subscribe();
    loop {
        tokio::select! {
            incoming = socket.recv() => {
                let Some(Ok(message)) = incoming else { break };
                let text = match message {
                    Message::Text(text) => text.to_string(),
                    Message::Close(_) => break,
                    _ => continue,
                };
                let replies = match serde_json::from_str::<Value>(&text) {
                    Err(_) => vec![json!({ "t": "err", "error": "bad frame" }).to_string()],
                    Ok(frame) if frame.get("t").and_then(Value::as_str) == Some("sync") => {
                        record(&state, "WS /api/ws sync", &actor);
                        // 읽기 전에 신호를 본 것으로 표시한다 — 읽은 뒤의 변경은 다음 신호가 다시 읽게 한다.
                        rows_changed.borrow_and_update();
                        let id = frame.get("id").cloned().unwrap_or(Value::Null);
                        let since = frame.get("since").and_then(Value::as_i64);
                        let epoch = frame.get("epoch").and_then(Value::as_str).map(str::to_string);
                        match read_rows(&state, since, epoch).await {
                            Ok(rows) => {
                                let reply = rows_frame(&rows, &id);
                                cursor = Some(SyncCursor { epoch: rows.epoch, seq: rows.seq });
                                vec![reply]
                            }
                            Err(error) => vec![json!({ "t": "err", "id": id, "error": error }).to_string()],
                        }
                    }
                    Ok(frame) if frame.get("t").and_then(Value::as_str) == Some("tx") => {
                        record(&state, "WS /api/ws tx", &actor);
                        let id = frame.get("id").cloned().unwrap_or(Value::Null);
                        match apply_tx(&state, &frame, &actor).await {
                            Ok(seq) => {
                                // 결과 행을 `ack` 보다 먼저 보낸다 — `ack` 를 받은 클라이언트의 사본에는 이미 결과가 있다.
                                let mut replies = Vec::new();
                                if let Some(current) = cursor.take() {
                                    rows_changed.borrow_and_update();
                                    match read_rows(&state, Some(current.seq), Some(current.epoch)).await {
                                        Ok(rows) => {
                                            if rows.full || !rows.rows.is_empty() {
                                                replies.push(rows_frame(&rows, &Value::Null));
                                            }
                                            cursor = Some(SyncCursor { epoch: rows.epoch, seq: rows.seq });
                                        }
                                        Err(error) => replies.push(json!({ "t": "err", "error": error }).to_string()),
                                    }
                                }
                                replies.push(json!({ "t": "ack", "id": id, "seq": seq }).to_string());
                                replies
                            }
                            Err((at, error)) => {
                                vec![json!({ "t": "reject", "id": id, "at": at, "error": error }).to_string()]
                            }
                        }
                    }
                    Ok(frame) => handle_frame(&state, &actor, &frame, &mut subs, &out_tx),
                };
                for reply in replies {
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
            changed = rows_changed.changed(), if cursor.is_some() => {
                if changed.is_err() {
                    break;
                }
                tokio::time::sleep(SYNC_BATCH).await;
                rows_changed.borrow_and_update();
                let Some(current) = cursor.take() else { continue };
                let frame = match read_rows(&state, Some(current.seq), Some(current.epoch)).await {
                    Ok(rows) => {
                        // 커밋마다 신호가 오므로 이미 보낸 변경이면 읽을 게 없다.
                        let fresh = rows.full || !rows.rows.is_empty();
                        let frame = fresh.then(|| rows_frame(&rows, &Value::Null));
                        cursor = Some(SyncCursor { epoch: rows.epoch, seq: rows.seq });
                        frame
                    }
                    // 커서를 비운 채 둔다 — 한 번만 알리고 밀기를 멈춘다. 클라이언트가 `sync` 로 다시 시작한다.
                    Err(error) => Some(json!({ "t": "err", "error": error }).to_string()),
                };
                if let Some(frame) = frame {
                    if socket.send(Message::Text(frame.into())).await.is_err() {
                        break;
                    }
                }
            }
        }
    }
    for (_, task) in subs {
        task.abort();
    }
}

fn record(state: &ServerState, surface: &str, actor: &str) {
    let mut event = UsageEvent::new(UsageSource::Rest, surface.to_string(), true);
    event.actor = Some(actor.to_string());
    event.client = Some("web".into());
    state.record_usage(event);
}

/// `{t:"tx", ops:[…]}` 의 op 를 차례로 적용하고 그 뒤의 머리 순번을 돌려준다. op 가 하나라도 읽히지 않으면 아무것도
/// 적용하지 않는다. 적용하다 실패하면 그 op 에서 멈추고 자리(`at`)와 이유를 돌려준다 — 앞의 op 는 남는다(결과는 어차피
/// `rows` 로 돌아온다).
async fn apply_tx(
    state: &Arc<ServerState>,
    frame: &Value,
    actor: &str,
) -> Result<i64, (Option<usize>, String)> {
    // 트랜잭션은 사람(웹·폰)의 쓰기다. 에이전트는 MCP·REST 로 쓴다 — 에이전트의 `start` 는 핸드오프를 수락하고, 그 앞에서
    // 세션 목록을 조회(프로세스 실행)한다. 그 길을 소켓에는 열지 않는다.
    if rocky_core::actors::is_agent_actor(actor) {
        return Err((
            None,
            format!("tx is for people — agent actor {actor} writes through MCP or REST"),
        ));
    }
    let Some(ops) = frame.get("ops").and_then(Value::as_array) else {
        return Err((None, "ops is required".to_string()));
    };
    let ops = ops
        .iter()
        .enumerate()
        .map(|(at, op)| {
            serde_json::from_value::<SyncOp>(op.clone())
                .map_err(|e| (Some(at), format!("bad op: {e}")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let state = state.clone();
    let actor = actor.to_string();
    tokio::task::spawn_blocking(move || {
        for (at, op) in ops.iter().enumerate() {
            state
                .store
                .apply_sync_op(op, &actor)
                .map_err(|e| (Some(at), e.to_string()))?;
        }
        state
            .store
            .sync_head()
            .map(|head| head.seq)
            .map_err(|e| (None, e.to_string()))
    })
    .await
    .map_err(|e| (None, format!("tx task failed: {e}")))?
}

/// 스토어 읽기는 막는 호출(SQLite + std 락)이라 비동기 워커 밖에서 돈다.
async fn read_rows(
    state: &Arc<ServerState>,
    since: Option<i64>,
    epoch: Option<String>,
) -> Result<SyncRows, String> {
    let state = state.clone();
    tokio::task::spawn_blocking(move || {
        state
            .store
            .sync_rows_since(since, epoch.as_deref())
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("sync read task failed: {e}"))?
}

/// `rows` 프레임. 요청에 `id` 가 있었으면 같은 `id` 로 답한다(밀어주기는 `id` 가 없다).
fn rows_frame(rows: &SyncRows, id: &Value) -> String {
    let mut frame = json!({
        "t": "rows",
        "epoch": rows.epoch,
        "seq": rows.seq,
        "full": rows.full,
        "rows": rows.rows,
    });
    if !id.is_null() {
        frame["id"] = id.clone();
    }
    frame.to_string()
}

/// 노트 프레임 하나 → 바로 돌려줄 답(들). 구독은 `subs` 에 작업으로 걸고, 방송은 `out` 으로 흘려보낸다.
fn handle_frame(
    state: &Arc<ServerState>,
    actor: &str,
    frame: &Value,
    subs: &mut HashMap<String, JoinHandle<()>>,
    out: &mpsc::UnboundedSender<String>,
) -> Vec<String> {
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
