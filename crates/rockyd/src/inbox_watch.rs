//! 수집함 구독 감시 — 세션이 구독한 소스만 주기적으로 읽어, 처음 보는 항목을 그 세션 받은편지함에 한 줄씩
//! 보낸다(PR 감시의 세션 알림과 같은 소켓). 알리기만 한다 — 착수는 오너가 정한다(`peer_inbox::inbox_item_message`).
//!
//! 구독이 없는 소스는 읽지 않는다: 수집함은 보드를 볼 때만 부르는 외부 호출이고, 여기서 도는 건 누가
//! 구독해 둔 것뿐이다. 새 항목 판정은 스토어의 `inbox_seen`(구독 시점의 항목은 기준선으로 이미 적힘).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use crate::server::ServerState;

/// 받은편지함 쓰기 실패가 "세션이 끝났다" 는 뜻인가 — 소켓 파일이 없거나 받는 쪽이 없을 때만. 시간 초과
/// 같은 일시적 실패는 다음 주기에 다시 보낸다(본 것으로 적지 않았으니 놓치지 않는다).
fn session_gone(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
    )
}

/// 한 번 돈다 — 구독된 소스마다 한 번 읽고, 세션마다 아직 안 본 항목(이미 보드에 올라간 것 제외)을 메시지
/// **하나**로 보낸다. 보내기에 성공해야 그 세션에게 "본 것" 이 된다. 소스의 실행 argv 가 구독 때와 달라졌으면
/// (지우고 같은 이름으로 다시 등록 등) 보내지 않고 기준선을 다시 잡는다 — 새 명령의 항목이 쏟아지지 않게.
/// 끝난 세션(소켓 없음·연결 거부)은 구독을 걷는다. 보낸 메시지 수를 돌려준다(테스트용).
pub async fn tick(state: &Arc<ServerState>) -> usize {
    let subs = match state.store.inbox_subscriptions() {
        Ok(subs) => subs,
        Err(error) => {
            eprintln!("rocky: 수집함 구독 목록을 못 읽었다 — {error}");
            return 0;
        }
    };
    if subs.is_empty() {
        return 0;
    }
    let mut by_source: HashMap<String, Vec<rocky_core::inbox::InboxSubscription>> = HashMap::new();
    for sub in subs {
        by_source.entry(sub.source.clone()).or_default().push(sub);
    }
    let sources = (state.inbox_sources)();
    let linked = state.store.linked_urls().unwrap_or_default();
    // 보낼 곳은 **살아 있는 등록**(훅이 턴마다 갱신, TTL 안)의 소켓뿐이다. 구독 때 적은 소켓으로 보내지
    // 않는다 — 소켓 이름이 숫자라, 끝난 세션의 경로를 다른 세션이 다시 쓰고 있으면 엉뚱한 세션에 간다.
    let now = chrono::Utc::now().timestamp();
    let registrations: Vec<_> = state
        .inboxes()
        .into_iter()
        .filter(|r| now - r.seen_at <= rocky_core::peer_inbox::REGISTRATION_TTL_SECS)
        .collect();
    // DB 에서 되살린 등록은 그 세션이 살아 있고 소켓이 그 세션의 것일 때만 — 목록을 못 읽으면 쓰지 않는다.
    let sessions = if registrations.iter().any(|r| r.restored) {
        let result = state.fresh_sessions().await;
        result.available.then_some(result.sessions)
    } else {
        None
    };
    let live: HashMap<String, String> = registrations
        .into_iter()
        .filter(|r| {
            !r.restored
                || sessions
                    .as_deref()
                    .is_some_and(|list| rocky_core::peer_inbox::restored_registration_live(r, list))
        })
        .map(|r| (r.session_id, r.socket))
        .collect();
    let mut sent = 0;
    for (name, subs) in by_source {
        // 소스가 설정·보드에서 사라졌으면 읽을 게 없다 — 구독은 남겨 둔다(되살아나면 지문으로 기준선을 다시 잡는다).
        let Some(source) = sources.iter().find(|s| s.name == name) else {
            continue;
        };
        let result = crate::inbox_exec::fetch_source(&state.runner(), source).await;
        if !result.available {
            continue; // 실패는 다음 주기에 — 본 것으로 적지 않았으니 놓치지 않는다.
        }
        let fingerprint = crate::inbox_exec::cache_key(source);
        let ids: Vec<String> = result.items.iter().map(|i| i.id.clone()).collect();
        for sub in subs {
            // "보내지 않기" 를 켠 세션은 미룬다(본 것으로 적지 않으니 다시 켜면 받는다).
            if state.is_muted(&sub.session_id) {
                continue;
            }
            if sub.fingerprint != fingerprint {
                if let Err(error) = state.store.subscribe_inbox(
                    &name,
                    &sub.session_id,
                    &sub.socket,
                    &fingerprint,
                    &ids,
                ) {
                    eprintln!("rocky: 수집함 {name} 기준선을 다시 못 잡았다 — {error}");
                }
                continue;
            }
            let unseen = match state.store.unseen_inbox_ids(&name, &sub.session_id, &ids) {
                Ok(unseen) => unseen,
                Err(error) => {
                    eprintln!("rocky: 수집함 {name} 의 본 항목을 못 읽었다 — {error}");
                    continue;
                }
            };
            if unseen.is_empty() {
                continue;
            }
            // 이미 어느 보드에 올라간 항목은 알릴 것이 없다 — 본 것으로만 적는다.
            let new: Vec<&rocky_core::inbox::InboxItem> = result
                .items
                .iter()
                .filter(|i| unseen.contains(&i.id))
                .filter(|i| !i.url.as_deref().is_some_and(|u| linked.contains(u)))
                .collect();
            if !new.is_empty() {
                let Some(socket) = live.get(&sub.session_id).cloned() else {
                    // 살아 있는 등록이 없다 — 보내지 않고 미룬다(본 것으로 적지 않는다). 등록 없이 TTL 이
                    // 지난 구독은 끝난 세션의 것이라 걷는다.
                    let stale = chrono::DateTime::parse_from_rfc3339(&sub.created_at)
                        .map(|t| {
                            now - t.timestamp() > rocky_core::peer_inbox::REGISTRATION_TTL_SECS
                        })
                        .unwrap_or(false);
                    if stale {
                        let _ = state.store.unsubscribe_inbox(None, &sub.session_id);
                    }
                    continue;
                };
                let line = rocky_core::peer_inbox::inbox_line(
                    &rocky_core::peer_inbox::inbox_item_message(&name, &new),
                );
                let written = tokio::task::spawn_blocking(move || {
                    crate::prwatch::write_inbox(&socket, &line)
                })
                .await;
                state.record_delivery(rocky_core::peer_inbox::Delivery {
                    at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                    kind: "inbox".into(),
                    subject: format!("{name} 새 항목 {}건", new.len()),
                    url: None,
                    session_id: sub.session_id.clone(),
                    ok: matches!(written, Ok(Ok(()))),
                    reason: match &written {
                        Ok(Ok(())) => None,
                        Ok(Err(e)) => Some(e.to_string()),
                        Err(e) => Some(e.to_string()),
                    },
                });
                match written {
                    Ok(Ok(())) => sent += 1,
                    Ok(Err(error)) if session_gone(&error) => {
                        // 세션 id 는 로그에 남기지 않는다(CodeQL: 민감 정보 평문 로깅).
                        eprintln!("rocky: 수집함 {name} — 구독 세션 하나가 끝났다({error}), 구독을 걷는다");
                        let _ = state.store.unsubscribe_inbox(None, &sub.session_id);
                        continue;
                    }
                    Ok(Err(error)) => {
                        eprintln!("rocky: 수집함 {name} — 구독 세션 하나에 못 썼다({error}), 다음 주기에 다시");
                        continue;
                    }
                    Err(_) => continue,
                }
            }
            let _ = state.store.mark_inbox_seen(&name, &sub.session_id, &unseen);
        }
    }
    sent
}

/// 감시 루프 — `first` 뒤 처음, 그 뒤 `every` 마다. 구독이 없으면 한 tick 이 스토어 조회 하나로 끝난다.
pub fn spawn_inbox_watcher(state: Arc<ServerState>, first: Duration, every: Duration) {
    tokio::spawn(async move {
        tokio::time::sleep(first).await;
        loop {
            tick(&state).await;
            tokio::time::sleep(every).await;
        }
    });
}
