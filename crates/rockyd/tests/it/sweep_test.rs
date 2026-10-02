//! 죽은 세션이 쥔 doing 의 자동 해제 — 데몬 스윕.


use crate::common::*;
use rocky_core::doing::AUTO_RELEASE_GRACE_SECS;
use rocky_core::sessions::SessionsResult;
use rocky_core::types::*;
use rockyd::sessions_exec::fixed_sessions;
use rockyd::sweep::{release_gone_doing, SWEEP_ACTOR};

fn started(f: &Fx, title: &str, actor: &str) -> Todo {
    let todo = f
        .store
        .create_todo(
            &CreateTodoInput {
                board: "rocky-todo".into(),
                title: title.into(),
                ..Default::default()
            },
            "logan",
        )
        .unwrap();
    f.store
        .set_todo_status(&todo.id, StatusAction::Start, actor, None)
        .unwrap()
}

/// 착수 시각에서 `hours` 뒤.
fn hours_after(todo: &Todo, hours: i64) -> String {
    let since = chrono::DateTime::parse_from_rfc3339(todo.doing_since.as_deref().unwrap()).unwrap();
    (since + chrono::Duration::hours(hours)).to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

#[tokio::test]
async fn releases_agent_doing_whose_session_is_gone_after_grace() {
    let f = fx();
    let agent = started(&f, "에이전트가 들고 죽음", "claude-code");
    let human = started(&f, "사람이 듦", "logan");
    // 그 보드에 세션이 하나도 없다 → 에이전트 doing 은 Gone.
    let state = rebuild(&f, |o| o.sessions = Some(fixed_sessions(available(vec![]))));

    // 유예 전에는 손대지 않는다.
    assert!(
        release_gone_doing(&state, &hours_after(&agent, 23), AUTO_RELEASE_GRACE_SECS)
            .await
            .is_empty()
    );
    assert_eq!(
        f.store.get_todo(&agent.id, None).unwrap().unwrap().status,
        TodoStatus::Doing
    );

    // 유예가 지나면 에이전트 것만 풀린다.
    let released =
        release_gone_doing(&state, &hours_after(&agent, 25), AUTO_RELEASE_GRACE_SECS).await;
    assert_eq!(released, vec!["rocky-todo-1".to_string()]);
    let after = f.store.get_todo(&agent.id, None).unwrap().unwrap();
    assert_eq!(after.status, TodoStatus::Todo);
    assert!(after.doing_by.is_none());
    let comments = f.store.list_comments(&agent.id, false).unwrap();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].actor, SWEEP_ACTOR);
    assert!(comments[0].body.contains("자동 해제") && comments[0].body.contains("claude-code"));
    // 사람이 든 것은 그대로.
    assert_eq!(
        f.store.get_todo(&human.id, None).unwrap().unwrap().status,
        TodoStatus::Doing
    );

    // 두 번째 스윕은 할 일이 없다.
    assert!(
        release_gone_doing(&state, &hours_after(&agent, 26), AUTO_RELEASE_GRACE_SECS)
            .await
            .is_empty()
    );
}

#[tokio::test]
async fn leaves_doing_alone_when_a_session_is_alive_or_sessions_are_unknown() {
    let f = fx();
    let agent = started(&f, "보드에 세션 있음", "claude-code");
    let later = hours_after(&agent, 48);
    // 같은 보드 경로에 세션이 하나라도 살아 있으면 Unknown — 건드리지 않는다.
    let state = rebuild(&f, |o| {
        o.sessions = Some(fixed_sessions(available(vec![sess(
            1,
            "/w/rocky-todo",
            "sess-1",
            "rocky-todo-1e",
            "idle",
        )])))
    });
    assert!(release_gone_doing(&state, &later, AUTO_RELEASE_GRACE_SECS)
        .await
        .is_empty());
    // 세션 목록 자체를 못 얻으면 아무것도 단정하지 않는다.
    let state = rebuild(&f, |o| {
        o.sessions = Some(fixed_sessions(SessionsResult::unavailable("claude 없음")))
    });
    assert!(release_gone_doing(&state, &later, AUTO_RELEASE_GRACE_SECS)
        .await
        .is_empty());
    assert_eq!(
        f.store.get_todo(&agent.id, None).unwrap().unwrap().status,
        TodoStatus::Doing
    );
}

#[tokio::test]
async fn does_not_release_unattributed_doing_when_the_board_path_or_old_key_has_a_session() {
    let f = fx();
    let agent = started(&f, "이름 바뀐 보드", "claude-code");
    let later = hours_after(&agent, 48);
    // key 를 디렉터리와 어긋나게 바꾼다 — match_board 로는 세션을 못 찾는다.
    f.store
        .update_board(
            "rocky-todo",
            &BoardPatch {
                key: Some("renamed-board".into()),
                ..Default::default()
            },
            "logan",
        )
        .unwrap();
    // 옛 key 세그먼트를 가진 세션이 돈다 → 둔다.
    let state = rebuild(&f, |o| {
        o.sessions = Some(fixed_sessions(available(vec![sess(
            1,
            "/w/rocky-todo",
            "sess-1",
            "rocky-todo-1e",
            "busy",
        )])))
    });
    assert!(release_gone_doing(&state, &later, AUTO_RELEASE_GRACE_SECS)
        .await
        .is_empty());
    // 설정된 path 하위 세션이 돈다 → 둔다.
    f.store
        .set_board_path("renamed-board", "/w/elsewhere", "logan")
        .unwrap();
    let state = rebuild(&f, |o| {
        o.sessions = Some(fixed_sessions(available(vec![sess(
            2,
            "/w/elsewhere/sub",
            "sess-2",
            "x",
            "idle",
        )])))
    });
    assert!(release_gone_doing(&state, &later, AUTO_RELEASE_GRACE_SECS)
        .await
        .is_empty());
    assert_eq!(
        f.store.get_todo(&agent.id, None).unwrap().unwrap().status,
        TodoStatus::Doing
    );
    // 정말 아무도 없으면 풀린다.
    let state = rebuild(&f, |o| o.sessions = Some(fixed_sessions(available(vec![]))));
    assert_eq!(
        release_gone_doing(&state, &later, AUTO_RELEASE_GRACE_SECS)
            .await
            .len(),
        1
    );
}
