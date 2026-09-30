//! 세션 받은편지함으로 밀어 넣기 — 경로 검증·한 줄 형식·본문·보낼 세션 고르기(순수).

use rocky_core::peer_inbox::{
    inbox_line, is_inbox_socket_path, pick_session, pr_session_message, review_session_message,
    session_candidates, InboxRegistration, REGISTRATION_TTL_SECS,
};
use rocky_core::prwatch::{PrEvent, PrEventKind};
use rocky_core::statusline::BoardLocation;

#[test]
fn only_claude_code_inbox_socket_paths_are_accepted() {
    // 데몬이 이 경로에 쓰므로 Claude Code 받은편지함 모양만 — 다른 서비스의 소켓을 가리키게 두지 않는다.
    assert!(is_inbox_socket_path("/tmp/cc-socks/43686.sock"));
    assert!(is_inbox_socket_path("/tmp/cc-socks-501/43686.sock"));
    assert!(is_inbox_socket_path("/private/tmp/cc-socks/1.sock"));
    for bad in [
        "",
        "relative/cc-socks/1.sock",
        "/tmp/other/1.sock",
        "/tmp/cc-socks/name.sock",
        "/tmp/cc-socks/1.socket",
        "/tmp/cc-socks/.sock",
        "/tmp/cc-socks/../docker.sock",
        "/var/run/docker.sock",
    ] {
        assert!(!is_inbox_socket_path(bad), "{bad}");
    }
}

#[test]
fn the_line_is_one_json_user_message_ending_in_newline() {
    // 2026-09-29 실측한 형식 — Claude Code 의 [uds-messaging] 안내문과 같은 모양.
    let line = inbox_line("안녕 \"따옴표\"\n둘째 줄");
    assert!(line.ends_with('\n'));
    assert_eq!(
        line.matches('\n').count(),
        1,
        "본문의 개행은 이스케이프된다"
    );
    let v: serde_json::Value = serde_json::from_str(line.trim_end()).unwrap();
    assert_eq!(v["type"], "user");
    assert_eq!(v["message"]["role"], "user");
    assert_eq!(v["message"]["content"], "안녕 \"따옴표\"\n둘째 줄");
}

fn event(kind: PrEventKind) -> PrEvent {
    PrEvent {
        kind,
        repo: "o/rocky".into(),
        number: 7,
        title: "PR 7".into(),
        url: "https://github.com/o/rocky/pull/7".into(),
        quiet: false,
        author: None,
    }
}

#[test]
fn only_ready_and_conflict_become_session_messages() {
    let ready = pr_session_message(&event(PrEventKind::Ready)).unwrap();
    assert!(ready.starts_with("rocky: o/rocky #7 머지 후보 — PR 7"));
    assert!(ready.contains("https://github.com/o/rocky/pull/7"));
    assert!(ready.contains("사용자가 직접 쓴 것이 아니다"));
    let conflict = pr_session_message(&event(PrEventKind::Conflict)).unwrap();
    assert!(conflict.contains("충돌") && conflict.contains("main 을 합쳐"));
    // 머지는 세션이 정리하도록 넘긴다 — 사람에게 다시 알리라는 말은 없다.
    let merged = pr_session_message(&event(PrEventKind::Merged)).unwrap();
    assert!(merged.starts_with("rocky: o/rocky #7 머지됨 — PR 7"));
    assert!(merged.contains("PushNotification 도 보내지 않는다") && merged.contains("after-merge"));
    assert!(PrEventKind::Merged.reaches_session() && !PrEventKind::Merged.notifies());
    // CI 실패는 세션이 원인을 보고 재실행 한 번 또는 수정 — 무엇을 할지까지 적는다.
    let ci = pr_session_message(&event(PrEventKind::CiFailed)).unwrap();
    assert!(ci.starts_with("rocky: o/rocky #7 CI 실패 — PR 7"));
    assert!(ci.contains("--log-failed") && ci.contains("한 번 재실행") && ci.contains("진짜 실패"));
    for kind in [
        PrEventKind::Opened,
        PrEventKind::Closed,
        PrEventKind::Unready,
    ] {
        assert!(pr_session_message(&event(kind)).is_none());
    }
}

fn reg(id: &str, cwd: &str, seen_at: i64) -> InboxRegistration {
    InboxRegistration {
        session_id: id.into(),
        socket: format!("/tmp/cc-socks/{seen_at}.sock"),
        cwd: cwd.into(),
        seen_at,
    }
}

#[test]
fn picks_the_most_recent_live_session_working_on_that_board() {
    let boards = vec![
        BoardLocation {
            key: "rocky".into(),
            path: Some("/w/rocky".into()),
        },
        BoardLocation {
            key: "tally".into(),
            path: None,
        },
    ];
    let now = 10_000;
    let regs = vec![
        reg("old", "/w/rocky", now - 100),
        reg("worktree", "/w/rocky/.claude/worktrees/todo-3", now - 10),
        reg("other", "/w/tally", now),
        reg("stale", "/w/rocky", now - REGISTRATION_TTL_SECS - 1),
    ];
    // 보드 경로 하위(워크트리 포함) 중 가장 최근 — 한 곳에만 보낸다.
    assert_eq!(
        pick_session(&regs, &boards, "rocky", now).map(|r| r.session_id.as_str()),
        Some("worktree")
    );
    // 경로가 없는 보드는 key 세그먼트로.
    assert_eq!(
        pick_session(&regs, &boards, "tally", now).map(|r| r.session_id.as_str()),
        Some("other")
    );
    assert!(pick_session(&regs, &boards, "nope", now).is_none());
    // 오래된 등록만 있으면 아무 데도 안 보낸다.
    assert!(pick_session(&regs[3..], &boards, "rocky", now).is_none());
}

/// 후보는 최근 순 전부 — 가장 최근 세션이 끝났으면 데몬이 그다음으로 넘어간다.
#[test]
fn candidates_come_newest_first_for_fallback() {
    let boards = vec![BoardLocation {
        key: "rocky".into(),
        path: Some("/w/rocky".into()),
    }];
    let now = 10_000;
    let regs = vec![
        reg("a", "/w/rocky", now - 300),
        reg("b", "/w/rocky", now - 10),
        reg("c", "/w/rocky", now - 100),
    ];
    let ids: Vec<&str> = session_candidates(&regs, &boards, "rocky", now)
        .iter()
        .map(|r| r.session_id.as_str())
        .collect();
    assert_eq!(ids, vec!["b", "c", "a"]);
}

#[test]
fn review_message_asks_the_pr_session_to_run_resolve_reviews() {
    let msg = review_session_message(&event(PrEventKind::Review)).unwrap();
    assert!(msg.starts_with("rocky: o/rocky #7 에 리뷰가 붙었다"));
    assert!(msg.contains("/rocky:review-fix 7"));
    assert!(msg.contains("코멘트·resolve·머지는 하지 않는다"));
    // 켜고 끄는 곳은 보드다 — 걷어낸 설정 키(`pr.autoResolve`)를 가리키면 세션이 없는 설정을 찾는다.
    assert!(msg.contains("rocky board auto-resolve off"), "{msg}");
    assert!(!msg.contains("pr.autoResolve"), "{msg}");
    assert!(review_session_message(&event(PrEventKind::Ready)).is_none());
    // ready·conflict 본문은 리뷰 도착을 다루지 않는다.
    assert!(pr_session_message(&event(PrEventKind::Review)).is_none());
}
