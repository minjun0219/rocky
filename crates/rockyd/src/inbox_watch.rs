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
    // 훅이 턴마다 갱신하는 세션 등록 — 세션이 이어 열려(resume) 소켓이 바뀌었으면 그쪽이 맞다.
    let live: HashMap<String, String> = state
        .inboxes()
        .into_iter()
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
            if sub.fingerprint != fingerprint {
                let socket = live.get(&sub.session_id).unwrap_or(&sub.socket);
                if let Err(error) =
                    state
                        .store
                        .subscribe_inbox(&name, &sub.session_id, socket, &fingerprint, &ids)
                {
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
                let socket = live
                    .get(&sub.session_id)
                    .cloned()
                    .unwrap_or(sub.socket.clone());
                if socket != sub.socket {
                    let _ = state
                        .store
                        .update_inbox_subscription_socket(&sub.session_id, &socket);
                }
                let line = rocky_core::peer_inbox::inbox_line(
                    &rocky_core::peer_inbox::inbox_item_message(&name, &new),
                );
                let written = tokio::task::spawn_blocking(move || {
                    crate::prwatch::write_inbox(&socket, &line)
                })
                .await;
                match written {
                    Ok(Ok(())) => sent += 1,
                    Ok(Err(error)) if session_gone(&error) => {
                        eprintln!(
                            "rocky: 수집함 {name} — 세션 {} 이 끝났다({error}), 구독을 걷는다",
                            sub.session_id
                        );
                        let _ = state.store.unsubscribe_inbox(None, &sub.session_id);
                        continue;
                    }
                    Ok(Err(error)) => {
                        eprintln!(
                            "rocky: 수집함 {name} — 세션 {} 에 못 썼다({error}), 다음 주기에 다시",
                            sub.session_id
                        );
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
