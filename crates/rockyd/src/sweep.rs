//! 죽은 세션이 쥔 doing 의 자동 해제 — 주기 스윕.
//!
//! 에이전트가 `start` 만 하고 세션이 끝나면 그 항목은 누가 `stop`/`done` 을 눌러 주기
//! 전까지 영원히 "진행중"이다(실제로 56일짜리가 있었다). 판정은 `rocky_core::doing::
//! should_auto_release`(에이전트 actor · `Gone` · 착수 후 유예 경과)이고, 여기서는 그
//! 항목을 `stop` 으로 돌리고 댓글 한 줄을 남긴다. 사람이 든 것·세션이 살아 있는 것·모르는
//! 것은 건드리지 않는다. 핸드오프는 이 스윕의 대상이 아니다(자동 만료 없음).

use std::sync::Arc;
use std::time::Duration;

use rocky_core::doing::{
    auto_release_note, board_has_session, doing_unchanged, resolve_doing_state,
    should_auto_release, AUTO_RELEASE_GRACE_SECS,
};
use rocky_core::types::ListTodosFilter;
use rocky_core::types::{StatusAction, TodoStatus};

use crate::server::ServerState;

/// 자동 해제의 actor — 댓글과 히스토리에 이 이름으로 남는다.
pub const SWEEP_ACTOR: &str = "rocky";

/// 한 번 훑어 놓아준 항목의 ref 목록. 세션 목록을 못 얻으면 아무것도 하지 않는다.
/// `now_iso` 를 받는 이유는 테스트가 시간을 앞당겨 유예를 넘기기 위해서다.
pub async fn release_gone_doing(
    state: &Arc<ServerState>,
    now_iso: &str,
    grace_secs: i64,
) -> Vec<String> {
    let store = &state.store;
    let doing = store
        .list_todos(&ListTodosFilter {
            status: Some(TodoStatus::Doing),
            ..Default::default()
        })
        .unwrap_or_default();
    if doing.is_empty() {
        return Vec::new();
    }
    // 자동 해제는 상태를 바꾸므로 캐시 없는 목록으로 본다(`fresh_sessions`).
    let sessions = state.fresh_sessions().await;
    if !sessions.available {
        return Vec::new();
    }
    let mut released = Vec::new();
    for snapshot in doing {
        // 세션 조회를 기다리는 사이 바뀐 행은 건드리지 않는다 — 스냅샷과 현재가 같은 doing 일 때만.
        let Ok(Some(todo)) = store.get_todo(&snapshot.id, None) else {
            continue;
        };
        if !doing_unchanged(&snapshot, &todo) {
            continue;
        }
        let Ok(Some(board)) = store.board_by_id(&todo.board_id) else {
            continue;
        };
        // 귀속 세션이 없는 doing 은 보드 근사인데, key 가 디렉터리와 어긋난 보드(이름 변경)는
        // `resolve_doing_state` 가 세션이 돌아도 Gone 을 낸다 — 옛 key·path 까지 봐서 누가 있으면 둔다.
        if todo.doing_session_id.is_none() && board_has_session(&sessions.sessions, &board) {
            continue;
        }
        let doing_state = resolve_doing_state(&todo, &board.key, &sessions);
        if !should_auto_release(&todo, doing_state, now_iso, grace_secs) {
            continue;
        }
        let note = auto_release_note(&todo, now_iso);
        if store
            .set_todo_status(&todo.id, StatusAction::Stop, SWEEP_ACTOR, None)
            .is_err()
        {
            continue;
        }
        let _ = store.add_comment(&todo.id, &note, SWEEP_ACTOR, None);
        released.push(
            rocky_core::refs::ref_of(store, Some(&todo.board_id), todo.number, &todo.id)
                .unwrap_or_else(|_| todo.id.clone()),
        );
    }
    released
}

/// 데몬 수명 동안 `every` 마다 한 번 훑는다. 첫 스윕은 `first_after` 뒤 — 기동 직후에는
/// `claude agents --json` 이 아직 세션을 못 볼 수 있어 죽은 것으로 오판할 여지를 줄인다
/// (유예 24시간이 있어 어차피 한 번의 오판으로는 안 풀리지만, 굳이 기동을 무겁게 하지 않는다).
pub fn spawn_sweeper(state: Arc<ServerState>, first_after: Duration, every: Duration) {
    tokio::spawn(async move {
        tokio::time::sleep(first_after).await;
        loop {
            let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
            let released = release_gone_doing(&state, &now, AUTO_RELEASE_GRACE_SECS).await;
            if !released.is_empty() {
                println!(
                    "rocky: 세션 없는 진행중 자동 해제 — {}",
                    released.join(", ")
                );
            }
            tokio::time::sleep(every).await;
        }
    });
}
