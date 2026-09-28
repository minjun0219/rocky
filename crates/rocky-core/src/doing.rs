//! "이 doing 이 살아 있나" / "이 핸드오프가 어디까지 갔나" 판정 — 순수 함수.
//! TS 원본 `src/doing.ts`.

use serde::{Deserialize, Serialize};

use crate::actors::is_agent_actor;
use crate::sessions::{match_board, AgentSession, SessionsResult};
use crate::types::{Handoff, HandoffStatus, Todo, TodoStatus};

/// doing 하나의 생존 상태.
///
/// - `Live` — 그 세션이 살아 있고 지금 일하고 있다.
/// - `Idle` — 세션은 살아 있는데 턴이 끝났고 done 이 안 왔다. **방치**다.
/// - `Gone` — 그 세션이 사라졌다.
/// - `Unknown` — 판별할 수 없다. 모르는 것과 없는 것은 다르므로 경고하지 않는다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DoingState {
    Live,
    Idle,
    Gone,
    Unknown,
}

/// `sessionId` 와 짧은 8자 `id` 를 **둘 다** 본다 — spawn 세션은 짧은 id 로 저장된다.
fn find_session<'a>(sessions: &'a [AgentSession], identifier: &str) -> Option<&'a AgentSession> {
    sessions
        .iter()
        .find(|s| s.session_id == identifier || s.id.as_deref() == Some(identifier))
}

fn state_of_session(session: &AgentSession) -> DoingState {
    // background 세션은 끝나도 잠시 목록에 남는다 — 있지만 죽은 것이다.
    if session.state.as_deref() == Some("done") {
        return DoingState::Gone;
    }
    if session.status == "busy" {
        DoingState::Live
    } else {
        DoingState::Idle
    }
}

/// doing 인 todo 의 생존 상태를 판정한다. 세션 귀속이 있으면 그 세션 하나만, 없으면
/// (에이전트 actor 일 때만) 보드 근사 — 그 보드에 활성 세션이 0개일 때만 `Gone`.
pub fn resolve_doing_state(todo: &Todo, board_key: &str, sessions: &SessionsResult) -> DoingState {
    if todo.status != TodoStatus::Doing {
        return DoingState::Unknown;
    }
    // 세션 목록을 못 얻는 환경에서는 아무것도 단정할 수 없다.
    if !sessions.available {
        return DoingState::Unknown;
    }
    if let Some(session_id) = &todo.doing_session_id {
        return match find_session(&sessions.sessions, session_id) {
            Some(session) => state_of_session(session),
            None => DoingState::Gone,
        };
    }
    match &todo.doing_by {
        Some(actor) if is_agent_actor(actor) => {
            if match_board(&sessions.sessions, board_key).is_empty() {
                DoingState::Gone
            } else {
                DoingState::Unknown
            }
        }
        _ => DoingState::Unknown,
    }
}

/// 죽은 세션이 쥔 doing 을 놓아주기까지의 유예 — 24시간. 세션 목록에서 잠깐 빠진
/// 경우(재시작 중)를 "죽었다" 로 오판하지 않으려는 여유다.
pub const AUTO_RELEASE_GRACE_SECS: i64 = 24 * 60 * 60;

fn iso_epoch(value: Option<&str>) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(value?)
        .ok()
        .map(|d| d.timestamp())
}

/// 이 doing 을 자동으로 놓아줄 것인가 — **에이전트 actor** 가 쥐고, 세션이 **`Gone`** 이고,
/// 착수 후 유예가 지났을 때만. 사람이 든 것·`Idle`(세션 살아 있음)·`Unknown` 은 절대 건드리지
/// 않는다. 시각을 못 읽으면 false(모르는 건 안 건드린다).
pub fn should_auto_release(todo: &Todo, state: DoingState, now_iso: &str, grace_secs: i64) -> bool {
    if todo.status != TodoStatus::Doing || state != DoingState::Gone {
        return false;
    }
    let Some(actor) = todo.doing_by.as_deref() else {
        return false;
    };
    if !is_agent_actor(actor) {
        return false;
    }
    let (Some(since), Some(now)) = (
        iso_epoch(todo.doing_since.as_deref()),
        iso_epoch(Some(now_iso)),
    ) else {
        return false;
    };
    now - since >= grace_secs
}

/// 자동 해제 때 남기는 댓글 — 누가 언제 들었다가 왜 풀렸는지.
pub fn auto_release_note(todo: &Todo, now_iso: &str) -> String {
    let actor = todo.doing_by.as_deref().unwrap_or("?");
    let since = todo.doing_since.as_deref().unwrap_or("?");
    let days = match (iso_epoch(Some(since)), iso_epoch(Some(now_iso))) {
        (Some(a), Some(b)) => format!(", {}일", (b - a) / 86_400),
        _ => String::new(),
    };
    format!(
        "세션 없음 — 진행중 자동 해제 ({actor} 착수 {}{days})",
        since.get(..10).unwrap_or(since)
    )
}

/// 핸드오프가 어디까지 갔는지 — 타임스탬프에서 파생한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum HandoffPhase {
    Pending,
    Delivered,
    Accepted,
    Completed,
    Cancelled,
}

/// 저장된 상태·타임스탬프를 한 단계로 접는다 — status enum 을 늘리지 않은 대가.
pub fn handoff_phase(handoff: &Handoff) -> HandoffPhase {
    match handoff.status {
        HandoffStatus::Cancelled => HandoffPhase::Cancelled,
        HandoffStatus::Pending => HandoffPhase::Pending,
        HandoffStatus::Delivered => {
            if handoff.completed_at.is_some() {
                HandoffPhase::Completed
            } else if handoff.accepted_at.is_some() {
                HandoffPhase::Accepted
            } else {
                HandoffPhase::Delivered
            }
        }
    }
}

/// "집어갔는데 아무 일도 안 일어났다" 인가. **시간 임계값을 쓰지 않는다** — 세션이
/// 사라졌거나 idle 인데 착수 기록이 없을 때만 경고다. 판별 불가면 false.
pub fn is_unstarted(handoff: &Handoff, sessions: &SessionsResult) -> bool {
    if handoff.status != HandoffStatus::Delivered || handoff.accepted_at.is_some() {
        return false;
    }
    if !sessions.available {
        return false;
    }
    match find_session(&sessions.sessions, &handoff.session_id) {
        None => true,
        Some(session) => state_of_session(session) != DoingState::Live,
    }
}
