//! PR 감시의 순수 판정 — GraphQL 응답 파싱, ready 규칙, 전이, 알림 문구.

use rocky_core::prwatch::{
    diff, is_ready, notification_text, osascript_args, parse_pull_requests, CiState, PrEventKind,
    PrSnapshot,
};
use serde_json::json;

fn thread(resolved: bool, mine: &[&str]) -> serde_json::Value {
    json!({
        "isResolved": resolved,
        "comments": { "nodes": [ { "reactions": { "nodes":
            mine.iter().map(|c| json!({ "content": c, "user": { "login": "me" } })).collect::<Vec<_>>()
        } } ] }
    })
}

fn pr(
    number: i64,
    state: &str,
    rollup: Option<&str>,
    threads: Vec<serde_json::Value>,
) -> serde_json::Value {
    json!({
        "number": number, "title": format!("PR {number}"), "url": format!("https://x/pull/{number}"),
        "state": state, "isDraft": false, "headRefOid": "abcdef0123456789", "mergeStateStatus": "CLEAN",
        "baseRefName": "main", "updatedAt": "2026-09-28T10:00:00Z",
        "commits": { "nodes": [ { "commit": { "statusCheckRollup": rollup.map(|s| json!({ "state": s })) } } ] },
        "reviewThreads": { "nodes": threads }
    })
}

fn data(prs: Vec<serde_json::Value>) -> serde_json::Value {
    json!({ "viewer": { "login": "me" }, "repository": { "pullRequests": { "nodes": prs } } })
}

#[test]
fn parses_ci_threads_and_ready_from_the_query_shape() {
    let d = data(vec![
        pr(
            1,
            "OPEN",
            Some("SUCCESS"),
            vec![thread(false, &["EYES"]), thread(true, &[])],
        ),
        pr(2, "OPEN", Some("FAILURE"), vec![thread(false, &[])]),
        pr(3, "OPEN", None, vec![thread(false, &["ROCKET"])]),
        pr(4, "MERGED", Some("SUCCESS"), vec![]),
    ]);
    let snaps = parse_pull_requests(&d, "o/r", "main").unwrap();
    assert_eq!(snaps.len(), 4);
    let s1 = &snaps[0];
    assert_eq!(
        (s1.ci, s1.unhandled, s1.rocket, s1.ready),
        (CiState::Pass, 0, 0, true)
    );
    assert_eq!(s1.head, "abcdef0");
    let s2 = &snaps[1];
    assert_eq!((s2.ci, s2.unhandled, s2.ready), (CiState::Fail, 1, false));
    let s3 = &snaps[2];
    assert_eq!(
        (s3.ci, s3.rocket, s3.ready),
        (CiState::Pass, 1, false),
        "check 없음 = pass, 🚀 는 막는다"
    );
    assert!(!snaps[3].ready, "머지된 것은 ready 가 아니다");
}

#[test]
fn a_wrong_shape_is_an_error_not_an_empty_list() {
    assert!(parse_pull_requests(&json!({ "viewer": { "login": "me" } }), "o/r", "main").is_err());
    assert!(parse_pull_requests(&json!({}), "o/r", "main").is_err());
}

#[test]
fn ready_rules() {
    let ok = CiState::Pass;
    assert!(is_ready("OPEN", false, "main", "main", "CLEAN", ok, 0, 0));
    assert!(
        is_ready("OPEN", false, "main", "main", "BLOCKED", ok, 0, 0),
        "BLOCKED 은 룰셋 사유 — 막지 않는다"
    );
    assert!(
        !is_ready("OPEN", true, "main", "main", "CLEAN", ok, 0, 0),
        "draft"
    );
    assert!(
        !is_ready("OPEN", false, "feat/a", "main", "CLEAN", ok, 0, 0),
        "스택 위층"
    );
    assert!(
        !is_ready("OPEN", false, "main", "main", "DIRTY", ok, 0, 0),
        "충돌"
    );
    assert!(!is_ready(
        "OPEN",
        false,
        "main",
        "main",
        "CLEAN",
        CiState::Pending,
        0,
        0
    ));
    assert!(!is_ready("OPEN", false, "main", "main", "CLEAN", ok, 1, 0));
    assert!(!is_ready("OPEN", false, "main", "main", "CLEAN", ok, 0, 1));
    assert!(!is_ready(
        "CLOSED", false, "main", "main", "CLEAN", ok, 0, 0
    ));
}

fn snap(number: i64, state: &str, ready: bool, merge_state: &str) -> PrSnapshot {
    PrSnapshot {
        repo: "o/r".into(),
        number,
        title: format!("PR {number}"),
        url: format!("https://x/pull/{number}"),
        state: state.into(),
        is_draft: false,
        base: "main".into(),
        head: "abcdef0".into(),
        merge_state: merge_state.into(),
        ci: CiState::Pass,
        unhandled: 0,
        rocket: 0,
        ready,
        updated_at: "2026-09-28T10:00:00Z".into(),
    }
}

#[test]
fn transitions_are_only_what_a_person_acts_on() {
    let prev = vec![
        snap(1, "OPEN", false, "BLOCKED"),
        snap(2, "OPEN", true, "CLEAN"),
        snap(3, "OPEN", false, "CLEAN"),
        snap(4, "OPEN", true, "CLEAN"),
        snap(9, "OPEN", false, "CLEAN"),
    ];
    let cur = vec![
        snap(1, "OPEN", true, "CLEAN"),      // ready 됨
        snap(2, "MERGED", false, "UNKNOWN"), // 머지
        snap(3, "OPEN", false, "DIRTY"),     // 충돌
        snap(4, "OPEN", false, "CLEAN"),     // 다시 대기
        snap(5, "OPEN", true, "CLEAN"),      // 새 PR, 이미 ready
        snap(6, "CLOSED", false, "UNKNOWN"), // 처음 보는 닫힌 PR — 무시
                                             // 9 는 창 밖으로 밀림 — 무시
    ];
    let kinds: Vec<(i64, PrEventKind)> = diff(&prev, &cur)
        .iter()
        .map(|e| (e.number, e.kind))
        .collect();
    assert_eq!(
        kinds,
        vec![
            (1, PrEventKind::Ready),
            (2, PrEventKind::Merged),
            (3, PrEventKind::Conflict),
            (4, PrEventKind::Unready),
            (5, PrEventKind::Opened),
            (5, PrEventKind::Ready),
        ]
    );
    assert!(diff(&cur, &cur).is_empty());
    assert!(PrEventKind::Ready.notifies() && PrEventKind::Conflict.notifies());
    assert!(!PrEventKind::Merged.notifies() && !PrEventKind::Opened.notifies());
}

#[test]
fn notification_text_and_osascript_escaping() {
    let events = diff(&[], &[snap(7, "OPEN", true, "CLEAN")]);
    let ready = events
        .iter()
        .find(|e| e.kind == PrEventKind::Ready)
        .unwrap();
    let (title, body) = notification_text(ready);
    assert_eq!(title, "rocky · o/r");
    assert_eq!(body, "#7 확인·머지해도 된다 — PR 7");
    let args = osascript_args("t \"q\"", "b \\ x");
    assert_eq!(args[0], "osascript");
    assert_eq!(
        args[2],
        "display notification \"b \\\\ x\" with title \"t \\\"q\\\"\""
    );
}
