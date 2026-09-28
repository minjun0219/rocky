//! TS 원본 `src/doing.test.ts` 포팅.

use rocky_core::doing::{
    auto_release_note, board_has_session, doing_unchanged, handoff_phase, is_unstarted,
    resolve_doing_state, should_auto_release, DoingState, HandoffPhase, AUTO_RELEASE_GRACE_SECS,
};
use rocky_core::sessions::{AgentSession, SessionsResult};
use rocky_core::types::*;

fn session() -> AgentSession {
    AgentSession {
        pid: 1234,
        cwd: "/Users/x/dev/rocky-todo".into(),
        kind: "interactive".into(),
        id: None,
        session_id: "sess-full-uuid".into(),
        name: "eelpout-a3".into(),
        status: "busy".into(),
        state: None,
        started_at: 0,
    }
}

fn available(sessions: Vec<AgentSession>) -> SessionsResult {
    SessionsResult {
        available: true,
        sessions,
        reason: None,
    }
}

fn unavailable() -> SessionsResult {
    SessionsResult::unavailable("claude CLI 를 실행할 수 없다")
}

fn doing_todo() -> Todo {
    Todo {
        id: "t1".into(),
        number: 1,
        board_id: "b1".into(),
        section_id: None,
        parent_id: None,
        title: "작업".into(),
        description: String::new(),
        status: TodoStatus::Doing,
        priority: TodoPriority::P4,
        due: None,
        labels: vec![],
        links: vec![],
        doing_by: Some("claude-code".into()),
        doing_since: None,
        doing_session_id: None,
        position: 1,
        created_at: "2026-07-01T00:00:00.000Z".into(),
        updated_at: "2026-07-01T00:00:00.000Z".into(),
        completed_at: None,
        archived_at: None,
    }
}

fn handoff() -> Handoff {
    Handoff {
        id: "h1".into(),
        todo_id: "t1".into(),
        session_id: "sess-full-uuid".into(),
        session_name: None,
        session_cwd: None,
        note: String::new(),
        actor: "logan".into(),
        status: HandoffStatus::Delivered,
        created_at: "2026-07-30T00:00:00.000Z".into(),
        delivered_at: Some("2026-07-30T00:00:01.000Z".into()),
        delivered_via: Some(HandoffVia::Stop),
        accepted_at: None,
        completed_at: None,
    }
}

// ── 세션 귀속이 있는 경우 ──

#[test]
fn attributed_busy_session_is_live() {
    let todo = Todo {
        doing_session_id: Some("sess-full-uuid".into()),
        ..doing_todo()
    };
    assert_eq!(
        resolve_doing_state(&todo, "rocky-todo", &available(vec![session()])),
        DoingState::Live
    );
}

#[test]
fn attributed_idle_session_is_idle() {
    let todo = Todo {
        doing_session_id: Some("sess-full-uuid".into()),
        ..doing_todo()
    };
    let s = AgentSession {
        status: "idle".into(),
        ..session()
    };
    assert_eq!(
        resolve_doing_state(&todo, "rocky-todo", &available(vec![s])),
        DoingState::Idle
    );
}

#[test]
fn background_state_done_is_gone_even_if_listed() {
    let todo = Todo {
        doing_session_id: Some("sess-full-uuid".into()),
        ..doing_todo()
    };
    let s = AgentSession {
        kind: "background".into(),
        status: "idle".into(),
        state: Some("done".into()),
        ..session()
    };
    assert_eq!(
        resolve_doing_state(&todo, "rocky-todo", &available(vec![s])),
        DoingState::Gone
    );
}

#[test]
fn missing_session_is_gone() {
    let todo = Todo {
        doing_session_id: Some("sess-full-uuid".into()),
        ..doing_todo()
    };
    let s = AgentSession {
        session_id: "other".into(),
        ..session()
    };
    assert_eq!(
        resolve_doing_state(&todo, "rocky-todo", &available(vec![s])),
        DoingState::Gone
    );
}

#[test]
fn short_spawn_id_finds_session_too() {
    // createSpawnedHandoff 는 full UUID 가 아니라 짧은 8자 id 를 저장한다.
    let todo = Todo {
        doing_session_id: Some("a1b2c3d4".into()),
        ..doing_todo()
    };
    let s = AgentSession {
        id: Some("a1b2c3d4".into()),
        session_id: "a1b2c3d4-full-uuid".into(),
        status: "busy".into(),
        ..session()
    };
    assert_eq!(
        resolve_doing_state(&todo, "rocky-todo", &available(vec![s])),
        DoingState::Live
    );
}

// ── 귀속이 없는 경우(보드 근사) ──

#[test]
fn no_sessions_on_board_is_gone() {
    let s = AgentSession {
        cwd: "/Users/x/dev/other-repo".into(),
        ..session()
    };
    assert_eq!(
        resolve_doing_state(&doing_todo(), "rocky-todo", &available(vec![s])),
        DoingState::Gone
    );
}

#[test]
fn any_session_on_board_is_unknown() {
    assert_eq!(
        resolve_doing_state(&doing_todo(), "rocky-todo", &available(vec![session()])),
        DoingState::Unknown
    );
}

#[test]
fn worktree_counts_as_board_session() {
    let s = AgentSession {
        cwd: "/Users/x/dev/rocky-todo/.claude/worktrees/todo-12".into(),
        ..session()
    };
    assert_eq!(
        resolve_doing_state(&doing_todo(), "rocky-todo", &available(vec![s])),
        DoingState::Unknown
    );
}

#[test]
fn human_doing_is_not_judged() {
    let todo = Todo {
        doing_by: Some("logan".into()),
        ..doing_todo()
    };
    let s = AgentSession {
        cwd: "/Users/x/dev/other-repo".into(),
        ..session()
    };
    assert_eq!(
        resolve_doing_state(&todo, "rocky-todo", &available(vec![s])),
        DoingState::Unknown
    );
}

// ── 판정하지 않는 경우 ──

#[test]
fn unavailable_sessions_is_always_unknown() {
    let attributed = Todo {
        doing_session_id: Some("sess-x".into()),
        ..doing_todo()
    };
    assert_eq!(
        resolve_doing_state(&attributed, "rocky-todo", &unavailable()),
        DoingState::Unknown
    );
    assert_eq!(
        resolve_doing_state(&doing_todo(), "rocky-todo", &unavailable()),
        DoingState::Unknown
    );
}

#[test]
fn non_doing_todo_is_unknown() {
    let todo = Todo {
        status: TodoStatus::Todo,
        doing_by: None,
        ..doing_todo()
    };
    assert_eq!(
        resolve_doing_state(&todo, "rocky-todo", &available(vec![])),
        DoingState::Unknown
    );
}

// ── handoffPhase ──

#[test]
fn pending_and_cancelled_use_status() {
    let pending = Handoff {
        status: HandoffStatus::Pending,
        delivered_at: None,
        ..handoff()
    };
    assert_eq!(handoff_phase(&pending), HandoffPhase::Pending);
    let cancelled = Handoff {
        status: HandoffStatus::Cancelled,
        ..handoff()
    };
    assert_eq!(handoff_phase(&cancelled), HandoffPhase::Cancelled);
}

#[test]
fn delivered_only_is_delivered() {
    assert_eq!(handoff_phase(&handoff()), HandoffPhase::Delivered);
}

#[test]
fn accepted_at_makes_accepted() {
    let h = Handoff {
        accepted_at: Some("2026-07-30T00:01:00.000Z".into()),
        ..handoff()
    };
    assert_eq!(handoff_phase(&h), HandoffPhase::Accepted);
}

#[test]
fn completed_at_makes_completed() {
    let h = Handoff {
        accepted_at: Some("2026-07-30T00:01:00.000Z".into()),
        completed_at: Some("2026-07-30T00:09:00.000Z".into()),
        ..handoff()
    };
    assert_eq!(handoff_phase(&h), HandoffPhase::Completed);
}

#[test]
fn cancelled_beats_completed() {
    let h = Handoff {
        status: HandoffStatus::Cancelled,
        completed_at: Some("x".into()),
        ..handoff()
    };
    assert_eq!(handoff_phase(&h), HandoffPhase::Cancelled);
}

// ── isUnstarted ──

#[test]
fn missing_session_is_unstarted() {
    let s = AgentSession {
        session_id: "other".into(),
        ..session()
    };
    assert!(is_unstarted(&handoff(), &available(vec![s])));
}

#[test]
fn idle_session_is_unstarted() {
    let s = AgentSession {
        status: "idle".into(),
        ..session()
    };
    assert!(is_unstarted(&handoff(), &available(vec![s])));
}

#[test]
fn busy_session_is_quiet_no_time_threshold() {
    assert!(!is_unstarted(&handoff(), &available(vec![session()])));
}

#[test]
fn accepted_is_not_unstarted() {
    let h = Handoff {
        accepted_at: Some("2026-07-30T00:01:00.000Z".into()),
        ..handoff()
    };
    let s = AgentSession {
        status: "idle".into(),
        ..session()
    };
    assert!(!is_unstarted(&h, &available(vec![s])));
}

#[test]
fn pending_is_not_unstarted() {
    let h = Handoff {
        status: HandoffStatus::Pending,
        delivered_at: None,
        ..handoff()
    };
    assert!(!is_unstarted(&h, &available(vec![])));
}

#[test]
fn unavailable_sessions_is_not_judged() {
    assert!(!is_unstarted(&handoff(), &unavailable()));
}

// ── 자동 해제 ────────────────────────────────────────────────────────────────

fn doing_by(actor: &str, since: &str) -> Todo {
    Todo {
        id: "t1".into(),
        number: 1,
        board_id: "b".into(),
        section_id: None,
        parent_id: None,
        title: "x".into(),
        description: String::new(),
        status: TodoStatus::Doing,
        priority: TodoPriority::P2,
        due: None,
        labels: vec![],
        links: vec![],
        doing_by: Some(actor.into()),
        doing_since: Some(since.into()),
        doing_session_id: None,
        position: 1,
        created_at: since.into(),
        updated_at: since.into(),
        completed_at: None,
        archived_at: None,
    }
}

#[test]
fn auto_release_only_for_agent_gone_and_past_grace() {
    let since = "2026-08-03T10:00:00.000Z";
    let later = "2026-08-04T10:00:01.000Z"; // 24h + 1s
    let soon = "2026-08-04T09:59:59.000Z";
    let t = doing_by("claude-code", since);
    assert!(should_auto_release(
        &t,
        DoingState::Gone,
        later,
        AUTO_RELEASE_GRACE_SECS
    ));
    assert!(!should_auto_release(
        &t,
        DoingState::Gone,
        soon,
        AUTO_RELEASE_GRACE_SECS
    ));
    // 세션이 살아 있거나 모르면 절대.
    for st in [DoingState::Idle, DoingState::Live, DoingState::Unknown] {
        assert!(
            !should_auto_release(&t, st, later, AUTO_RELEASE_GRACE_SECS),
            "{st:?}"
        );
    }
    // 사람이 든 것은 절대.
    assert!(!should_auto_release(
        &doing_by("logan", since),
        DoingState::Gone,
        later,
        AUTO_RELEASE_GRACE_SECS
    ));
    // doing 이 아니면 절대.
    let mut done = doing_by("claude-code", since);
    done.status = TodoStatus::Done;
    assert!(!should_auto_release(
        &done,
        DoingState::Gone,
        later,
        AUTO_RELEASE_GRACE_SECS
    ));
    // 시각을 못 읽으면 건드리지 않는다.
    let mut bad = doing_by("claude-code", since);
    bad.doing_since = Some("어제".into());
    assert!(!should_auto_release(
        &bad,
        DoingState::Gone,
        later,
        AUTO_RELEASE_GRACE_SECS
    ));
    assert!(!should_auto_release(
        &t,
        DoingState::Gone,
        "지금",
        AUTO_RELEASE_GRACE_SECS
    ));
}

#[test]
fn auto_release_note_names_actor_start_and_days() {
    let t = doing_by("claude-code", "2026-08-03T10:00:00.000Z");
    assert_eq!(
        auto_release_note(&t, "2026-09-28T09:00:00.000Z"),
        "세션 없음 — 진행중 자동 해제 (claude-code 착수 2026-08-03, 55일)"
    );
}

fn board(key: &str, previous: Option<Vec<&str>>, path: Option<&str>) -> Board {
    Board {
        id: "b".into(),
        key: key.into(),
        title: key.into(),
        description: None,
        repo: None,
        path: path.map(str::to_string),
        previous_keys: previous.map(|v| v.into_iter().map(str::to_string).collect()),
        created_at: String::new(),
        archived_at: None,
    }
}

#[test]
fn board_has_session_sees_current_key_old_keys_and_path() {
    let mut s = session();
    s.cwd = "/Users/x/dev/gotgan".into();
    let sessions = vec![s];
    // 현재 key 가 경로 세그먼트에 있으면.
    assert!(board_has_session(&sessions, &board("gotgan", None, None)));
    // 이름을 바꿔 key 가 어긋나도 옛 key 로 잡는다.
    assert!(board_has_session(
        &sessions,
        &board("gotgan-v2", Some(vec!["gotgan"]), None)
    ));
    // 설정된 path 하위면 key 와 무관.
    assert!(board_has_session(
        &sessions,
        &board("renamed", None, Some("/Users/x/dev/gotgan"))
    ));
    assert!(board_has_session(
        &sessions,
        &board("renamed", None, Some("/Users/x/dev"))
    ));
    // 아무것도 안 맞으면 없다.
    assert!(!board_has_session(
        &sessions,
        &board("renamed", Some(vec!["other"]), Some("/elsewhere"))
    ));
    assert!(!board_has_session(
        &[],
        &board("gotgan", None, Some("/Users/x/dev/gotgan"))
    ));
}

#[test]
fn doing_unchanged_requires_same_state_actor_start_and_session() {
    let snap = doing_by("claude-code", "2026-08-03T10:00:00.000Z");
    assert!(doing_unchanged(&snap, &snap.clone()));
    let mut done = snap.clone();
    done.status = TodoStatus::Done;
    assert!(!doing_unchanged(&snap, &done));
    let mut restarted = snap.clone();
    restarted.doing_since = Some("2026-09-01T00:00:00.000Z".into());
    assert!(!doing_unchanged(&snap, &restarted));
    let mut human = snap.clone();
    human.doing_by = Some("logan".into());
    assert!(!doing_unchanged(&snap, &human));
    let mut attributed = snap.clone();
    attributed.doing_session_id = Some("sess".into());
    assert!(!doing_unchanged(&snap, &attributed));
}
