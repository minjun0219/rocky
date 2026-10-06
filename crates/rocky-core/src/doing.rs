//! "이 doing 이 살아 있나" / "이 핸드오프가 어디까지 갔나" 판정 — 순수 함수.
//! TS 원본 `src/doing.ts`.

use serde::{Deserialize, Serialize};

use crate::actors::is_agent_actor;
use crate::sessions::{match_board, AgentSession, SessionsResult};
use crate::statusline::is_under;
use crate::types::{Board, Handoff, HandoffStatus, Todo, TodoStatus};

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

/// doing 인 todo 의 생존 상태를 판정한다. 핸드오프 귀속이 있으면 그 세션 하나만, 없으면
/// (에이전트 actor 일 때만) 보드 근사 — 그 보드에 활성 세션이 0개일 때만 `Gone`.
///
/// 세션이 스스로 든 것의 귀속(`doing_session_claimed`)은 **보지 않는다** — 그 세션은 Stop 에서 "닫았나?" 를 묻지 않으므로
/// 턴이 끝날 때마다 `Idle`(방치·멈춤·이어받기 추천)이 되고, `/clear` 로 id 가 바뀌면 `Gone`(자동 해제)이 된다. 그 귀속은
/// statusline ⏺·턴 태그에만 쓴다(오너 결정 2026-10-06).
pub fn resolve_doing_state(todo: &Todo, board_key: &str, sessions: &SessionsResult) -> DoingState {
    if todo.status != TodoStatus::Doing {
        return DoingState::Unknown;
    }
    // 세션 목록을 못 얻는 환경에서는 아무것도 단정할 수 없다.
    if !sessions.available {
        return DoingState::Unknown;
    }
    if let Some(session_id) = todo
        .doing_session_id
        .as_ref()
        .filter(|_| !todo.doing_session_claimed)
    {
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

/// 이 보드에 붙은 세션이 **하나라도** 있는가 — 현재 key 뿐 아니라 옛 key(별칭)와 설정된
/// `path` 하위까지 본다. `match_board` 는 현재 key 세그먼트만 보므로 key 를 디렉터리와 어긋나게
/// 바꾼 보드는 세션이 버젓이 돌아도 `Gone` 이 나온다 — 표시라면 사람이 고르면 그만이지만 자동
/// 해제는 "정말 아무도 없다" 를 확신해야 해서 이 넓은 판정을 쓴다.
pub fn board_has_session(sessions: &[AgentSession], board: &Board) -> bool {
    if !match_board(sessions, &board.key).is_empty() {
        return true;
    }
    if board
        .previous_keys
        .iter()
        .flatten()
        .any(|k| !match_board(sessions, k).is_empty())
    {
        return true;
    }
    match board.path.as_deref() {
        Some(path) => sessions.iter().any(|s| is_under(&s.cwd, path)),
        None => false,
    }
}

/// 스냅샷 이후 그 doing 이 그대로인가 — 세션 조회를 기다리는 사이(최대 수 초) 사람이나
/// 에이전트가 done/stop/재착수했으면 스냅샷 기준의 해제는 엉뚱한 행을 되돌린다. 상태·actor·
/// 착수 시각·귀속 세션이 전부 같을 때만 같은 doing 으로 본다.
pub fn doing_unchanged(snapshot: &Todo, current: &Todo) -> bool {
    current.status == TodoStatus::Doing
        && current.doing_by == snapshot.doing_by
        && current.doing_since == snapshot.doing_since
        && current.doing_session_id == snapshot.doing_session_id
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

/// 아직 아무도 착수하지 않은 배달 건 — 대기(pending) 다음으로 취소할 수 있는 것.
fn awaits_start(handoff: &Handoff) -> bool {
    handoff.status == HandoffStatus::Delivered
        && handoff.accepted_at.is_none()
        && handoff.completed_at.is_none()
}

/// `rocky handoff REF --cancel` 이 취소할 한 건 — 대기 중인 요청이 먼저, 없으면 배달됐지만 착수 안 한 것 중 가장 오래된
/// 것(다음 `start` 가 수락할 바로 그것). `handoffs` 는 아무 순서여도 된다.
pub fn cancel_target<'a>(handoffs: &'a [Handoff], todo_id: &str) -> Option<&'a Handoff> {
    let of_todo = || handoffs.iter().filter(move |h| h.todo_id == todo_id);
    of_todo()
        .filter(|h| h.status == HandoffStatus::Pending)
        .min_by(|a, b| a.created_at.cmp(&b.created_at))
        .or_else(|| {
            of_todo()
                .filter(|h| awaits_start(h))
                .min_by(|a, b| a.created_at.cmp(&b.created_at))
        })
}

/// 배달 뒤 이만큼은 받은 세션이 목록에 없어도 무효로 하지 않는다 — 막 뜬 세션이 `claude agents` 에 늦게 잡히는 틈.
pub const GONE_HANDOFF_GRACE_SECS: i64 = 10 * 60;

/// 배달됐지만 착수 안 한 건 중 다른 세션의 `start` 가 수락하기 전에 받은 세션을 볼 것.
#[derive(Debug, Clone, Copy)]
pub struct Overdue<'a> {
    pub handoff: &'a Handoff,
    /// 더 새 요청에 밀렸다 — 사람이 다시 보냈다. 받은 세션이 일하는 중이 아니면 버린 것으로 본다.
    pub superseded: bool,
}

/// 볼 것 — 배달 뒤 유예가 지났거나, **더 새 요청에 밀린 것**(막 받은 세션의 것이 아니니 유예가 필요 없다). `handoffs` 는
/// 그 할 일의 핸드오프 전부(상태 무관). 비면 세션 목록을 부르지 않는다.
pub fn overdue_unaccepted<'a>(
    handoffs: &'a [Handoff],
    now_iso: &str,
    grace_secs: i64,
) -> Vec<Overdue<'a>> {
    let Some(now) = iso_epoch(Some(now_iso)) else {
        return Vec::new();
    };
    handoffs
        .iter()
        .filter(|h| awaits_start(h))
        .filter_map(|h| {
            let superseded = handoffs.iter().any(|later| {
                later.status != HandoffStatus::Cancelled && later.created_at > h.created_at
            });
            let past_grace =
                iso_epoch(h.delivered_at.as_deref()).is_some_and(|at| now - at >= grace_secs);
            (superseded || past_grace).then_some(Overdue {
                handoff: h,
                superseded,
            })
        })
        .collect()
}

/// 그중 버려진 것 — 받은 세션이 목록에 없거나 끝난(`done`) background 행이면 버려진 것이고, 다시 보내 밀린 것은 받은
/// 세션이 일하는 중(`busy`)이 아니기만 해도 버려진 것이다(사람이 다른 세션을 골랐다 — `start` 를 부르는 세션은 그 턴에
/// 있어 `busy` 다). 목록을 못 얻었으면 비운다(모름 ≠ 없음).
pub fn gone_handoffs<'a>(overdue: &[Overdue<'a>], sessions: &SessionsResult) -> Vec<&'a Handoff> {
    if !sessions.available {
        return Vec::new();
    }
    overdue
        .iter()
        .filter(
            |o| match find_session(&sessions.sessions, &o.handoff.session_id) {
                None => true,
                Some(s) if o.superseded => state_of_session(s) != DoingState::Live,
                Some(s) => state_of_session(s) == DoingState::Gone,
            },
        )
        .map(|o| o.handoff)
        .collect()
}
