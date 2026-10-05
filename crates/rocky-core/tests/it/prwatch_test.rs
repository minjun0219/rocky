//! PR 감시의 순수 판정 — GraphQL 응답 파싱, ready 규칙, 전이, 알림 문구.

use std::time::Duration;

use rocky_core::prwatch::{
    bridge_payload, detail_query, diff, is_rate_limit_error, is_ready, list_query,
    notification_text, osascript_args, parse_pr_details, parse_pr_list, pause_for, CiState,
    PrEventKind, PrSnapshot, RateLimit, RATE_LIMIT_BLIND_PAUSE_SECS, RATE_LIMIT_FLOOR,
};
use serde_json::json;

/// 스레드 — 첫 코멘트에 내가 단 리액션은 `viewerHasReacted` 로만 온다(노드를 받지 않는다).
fn thread(resolved: bool, mine: &[&str]) -> serde_json::Value {
    json!({
        "isResolved": resolved,
        "comments": { "nodes": [ {
            "eyes": { "viewerHasReacted": mine.contains(&"EYES") },
            "rocket": { "viewerHasReacted": mine.contains(&"ROCKET") }
        } ] }
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

/// 닫힌 PR 은 상태 조각(`prState`)만 온다 — commits·reviewThreads 가 없다.
fn closed_pr(number: i64, state: &str) -> serde_json::Value {
    json!({
        "number": number, "title": format!("PR {number}"), "url": format!("https://x/pull/{number}"),
        "state": state, "isDraft": false, "headRefOid": "abcdef0123456789", "mergeStateStatus": "UNKNOWN",
        "baseRefName": "main", "updatedAt": "2026-09-28T10:00:00Z"
    })
}

/// 목록 응답 — 열린 것은 번호만 뜻이 있고(상태 조각), 닫힌 것은 그대로.
fn list_data(prs: Vec<serde_json::Value>) -> serde_json::Value {
    let (open, recent): (Vec<_>, Vec<_>) = prs.into_iter().partition(|p| p["state"] == "OPEN");
    json!({
        "rateLimit": { "cost": 12, "remaining": 4800, "resetAt": "2026-09-28T11:00:00Z" },
        "repository": { "open": { "pageInfo": { "hasNextPage": false }, "nodes": open }, "recent": { "nodes": recent } }
    })
}

/// 상세 응답 — `p<번호>` 별칭마다 PR 하나.
fn detail_data(prs: Vec<serde_json::Value>) -> serde_json::Value {
    let mut repository = serde_json::Map::new();
    for p in prs {
        repository.insert(format!("p{}", p["number"]), p);
    }
    json!({ "rateLimit": { "cost": 3, "remaining": 4700, "resetAt": "2026-09-28T11:00:00Z" }, "repository": repository })
}

#[test]
fn parses_ci_threads_and_ready_from_the_query_shape() {
    let d = detail_data(vec![
        pr(
            1,
            "OPEN",
            Some("SUCCESS"),
            vec![thread(false, &["ROCKET"]), thread(true, &[])],
        ),
        pr(2, "OPEN", Some("FAILURE"), {
            let mut t = thread(false, &[]);
            t["id"] = json!("T_new");
            vec![t]
        }),
        pr(3, "OPEN", None, vec![thread(false, &["EYES"])]),
    ]);
    let mut snaps = parse_pr_details(&d, "o/r", "main").unwrap();
    snaps.sort_by_key(|s| s.number);
    assert_eq!(snaps.len(), 3);
    let s1 = &snaps[0];
    assert_eq!(
        (s1.ci, s1.unhandled, s1.decision, s1.ready),
        (CiState::Pass, 0, 0, true)
    );
    assert_eq!(s1.head, "abcdef0");
    let s2 = &snaps[1];
    assert_eq!((s2.ci, s2.unhandled, s2.ready), (CiState::Fail, 1, false));
    assert_eq!(
        s2.unhandled_ids,
        vec!["T_new".to_string()],
        "처리 안 된 스레드의 id 를 기억한다"
    );
    assert!(s1.unhandled_ids.is_empty(), "🚀 단 스레드는 목록에 없다");
    let s3 = &snaps[2];
    assert_eq!(
        (s3.ci, s3.decision, s3.ready),
        (CiState::Pass, 1, false),
        "check 없음 = pass, 👀(결정 필요) 는 막는다"
    );

    let list = parse_pr_list(
        &list_data(vec![pr(7, "OPEN", None, vec![]), closed_pr(4, "MERGED")]),
        "o/r",
        "main",
    )
    .unwrap();
    assert_eq!(list.open, vec![7], "열린 것은 번호만 — 상세 쿼리의 대상");
    assert!(!list.open_truncated);
    assert_eq!(list.closed.len(), 1);
    let c = &list.closed[0];
    assert!(!c.ready, "머지된 것은 ready 가 아니다");
    assert_eq!(
        (c.ci, c.unhandled, c.state.as_str()),
        (CiState::Pass, 0, "MERGED"),
        "닫힌 PR 은 상태 조각만 — CI·스레드 없이도 스냅숏이 완성된다"
    );
}

#[test]
fn a_wrong_shape_is_an_error_not_an_empty_list() {
    assert!(parse_pr_list(&json!({ "repository": {} }), "o/r", "main").is_err());
    assert!(parse_pr_list(&json!({}), "o/r", "main").is_err());
    assert!(parse_pr_details(&json!({}), "o/r", "main").is_err());
}

/// 예산 — 응답의 `rateLimit`, 바닥 판정, 리셋까지 쉬는 길이.
#[test]
fn rate_limit_is_read_and_decides_the_pause() {
    let d = list_data(vec![]);
    let rl = RateLimit::of(&d).unwrap();
    assert_eq!((rl.cost, rl.remaining), (12, 4800));
    assert!(!rl.exhausted());
    assert!(RateLimit::of(&json!({ "repository": {} })).is_none());
    let low = RateLimit {
        cost: 1,
        remaining: RATE_LIMIT_FLOOR - 1,
        reset_at: "2026-09-28T11:00:00Z".into(),
    };
    assert!(low.exhausted());
    let now = chrono::DateTime::parse_from_rfc3339("2026-09-28T10:40:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    assert_eq!(
        pause_for(Some(&low), now),
        Duration::from_secs(20 * 60 + 60),
        "리셋까지 + 1분 여유"
    );
    let past = RateLimit {
        reset_at: "2026-09-28T10:00:00Z".into(),
        ..low.clone()
    };
    assert_eq!(
        pause_for(Some(&past), now),
        Duration::from_secs(RATE_LIMIT_BLIND_PAUSE_SECS),
        "리셋이 지났으면(낡은 정보) 고정 길이"
    );
    assert_eq!(
        pause_for(None, now),
        Duration::from_secs(RATE_LIMIT_BLIND_PAUSE_SECS)
    );
    let far = RateLimit {
        reset_at: "2026-09-29T10:00:00Z".into(),
        ..low
    };
    assert_eq!(
        pause_for(Some(&far), now),
        Duration::from_secs(61 * 60),
        "상한은 한도 창 + 여유"
    );
}

/// gh 는 한도에 걸려도 응답 JSON 을 stdout 에 낸다 — `errors[].type` 이 정본, stderr 는 폴백.
#[test]
fn rate_limit_errors_are_recognised_from_body_or_stderr() {
    let body = r#"{"errors":[{"type":"RATE_LIMIT","code":"graphql_rate_limit","message":"API rate limit already exceeded for user ID 1."}]}"#;
    assert!(is_rate_limit_error(body, ""));
    let body2 = r#"{"errors":[{"type":"RATE_LIMITED","message":"API rate limit exceeded for user ID 1."}]}"#;
    assert!(
        is_rate_limit_error(body2, ""),
        "GitHub 은 두 철자를 다 낸다"
    );
    assert!(is_rate_limit_error(
        "",
        "gh: API rate limit already exceeded for user ID 1."
    ));
    assert!(!is_rate_limit_error(
        r#"{"errors":[{"type":"NOT_FOUND","message":"x"}]}"#,
        "gh: HTTP 404: Not Found"
    ));
    assert!(!is_rate_limit_error("", "gh: HTTP 404: Not Found"));
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
        unhandled_ids: vec![],
        decision: 0,
        ready,
        updated_at: "2026-09-28T10:00:00Z".into(),
        author: None,
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
        snap(6, "CLOSED", false, "UNKNOWN"), // 처음 보는 닫힌 PR — 닫힘이 온다(머지된 PR 을 구독한 경우와 같다)
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
            (6, PrEventKind::Closed),
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
    assert_eq!(body, "#7 머지 후보 — PR 7");
    let args = osascript_args("t \"q\"", "b \\ x");
    assert_eq!(args[0], "osascript");
    assert_eq!(
        args[2],
        "display notification \"b \\\\ x\" with title \"t \\\"q\\\"\""
    );
}

/// 상세 쿼리는 번호마다 별칭 하나로 한 요청 — 사라진 PR(`null`)은 건너뛴다. 번호가 없으면 부르지 않는다.
#[test]
fn detail_query_batches_numbers_and_parses_each_alias() {
    assert!(detail_query(&[]).is_none());
    let q = detail_query(&[9, 12]).unwrap();
    assert!(q.contains("p9: pullRequest(number:9) { ...prFields }"));
    assert!(q.contains("p12: pullRequest(number:12) { ...prFields }"));
    let mut d = detail_data(vec![pr(9, "MERGED", Some("SUCCESS"), vec![])]);
    d["repository"]["p12"] = json!(null);
    let snaps = parse_pr_details(&d, "o/r", "main").unwrap();
    assert_eq!(snaps.len(), 1);
    assert_eq!((snaps[0].number, snaps[0].state.as_str()), (9, "MERGED"));
    assert!(
        parse_pr_details(&json!({ "repository": {} }), "o/r", "main")
            .unwrap()
            .is_empty()
    );
    for q in [list_query(), q] {
        assert!(q.contains("fragment prState"));
        assert!(q.contains("rateLimit { cost remaining resetAt }"));
        assert!(
            !q.contains("user { login }"),
            "리액션 노드를 받지 않는다 — 비용"
        );
    }
    assert!(
        !list_query().contains("reviewThreads") && list_query().contains("{ ...prState }"),
        "목록은 상태만 — 스레드는 열린 PR 에만 상세로"
    );
    assert!(
        list_query().contains("states:[OPEN]") && list_query().contains("states:[MERGED, CLOSED]")
    );
    assert!(detail_query(&[1]).unwrap().contains("viewerHasReacted"));
}

/// 닫혔던 PR 이 다시 열리면 처음 보는 열린 PR 처럼 — opened 와, 이미 ready 면 ready 도.
#[test]
fn a_reopened_pr_emits_opened_and_ready() {
    let prev = vec![snap(1, "CLOSED", false, "UNKNOWN")];
    let cur = vec![snap(1, "OPEN", true, "CLEAN")];
    let kinds: Vec<PrEventKind> = diff(&prev, &cur).iter().map(|e| e.kind).collect();
    assert_eq!(kinds, vec![PrEventKind::Opened, PrEventKind::Ready]);
    let cur = vec![snap(1, "OPEN", false, "DIRTY")];
    let kinds: Vec<PrEventKind> = diff(&prev, &cur).iter().map(|e| e.kind).collect();
    assert_eq!(kinds, vec![PrEventKind::Opened, PrEventKind::Conflict]);
}

/// 스레드가 첫 페이지를 넘치면 못 본 것이 있다 — ready 로 판정하지 않는다.
#[test]
fn truncated_review_threads_block_ready() {
    let mut node = pr(1, "OPEN", Some("SUCCESS"), vec![thread(false, &["ROCKET"])]);
    node["reviewThreads"]["pageInfo"] = json!({ "hasNextPage": true });
    let snaps = parse_pr_details(&detail_data(vec![node]), "o/r", "main").unwrap();
    assert_eq!((snaps[0].unhandled, snaps[0].ready), (1, false));
}

/// 열린 것이 창을 넘치면 그 사실을 알린다 — 데몬이 직전 스냅숏의 열린 것으로 보충한다.
#[test]
fn a_truncated_open_list_is_flagged() {
    let mut d = list_data(vec![pr(1, "OPEN", None, vec![])]);
    d["repository"]["open"]["pageInfo"]["hasNextPage"] = json!(true);
    let list = parse_pr_list(&d, "o/r", "main").unwrap();
    assert!(list.open_truncated && list.open == vec![1]);
}

/// 브릿지 stdin JSON — 전이의 재료 전부 + 배너와 같은 문구.
#[test]
fn bridge_payload_carries_the_transition_and_the_banner_text() {
    let events = diff(&[], &[snap(7, "OPEN", true, "CLEAN")]);
    let ready = events
        .iter()
        .find(|e| e.kind == PrEventKind::Ready)
        .unwrap();
    let v = bridge_payload(ready);
    assert_eq!(v["kind"], "ready");
    assert_eq!(v["repo"], "o/r");
    assert_eq!(v["number"], 7);
    assert_eq!(v["url"], "https://x/pull/7");
    assert_eq!(v["heading"], "rocky · o/r");
    assert_eq!(v["text"], "#7 머지 후보 — PR 7");
}

/// 리뷰 도착 — 처리 안 된 스레드가 늘면(봇·사람 리뷰가 새로 붙으면) Review. 🚀 로 줄어드는 건 아니다.
#[test]
fn unresolved_threads_growing_is_a_review_transition() {
    let mut before = snap(1, "OPEN", false, "CLEAN");
    before.unhandled = 0;
    let mut after = before.clone();
    after.unhandled = 2;
    let kinds: Vec<PrEventKind> = diff(&[before.clone()], &[after.clone()])
        .iter()
        .map(|e| e.kind)
        .collect();
    assert_eq!(kinds, vec![PrEventKind::Review]);
    // 처리해서 줄면 전이 없음.
    assert!(diff(&[after.clone()], &[before.clone()]).is_empty());
    // 처음 볼 때 이미 스레드가 있으면 열림과 함께 리뷰 도착.
    let kinds: Vec<PrEventKind> = diff(&[], &[after]).iter().map(|e| e.kind).collect();
    assert_eq!(kinds, vec![PrEventKind::Opened, PrEventKind::Review]);
    // 사람에게 배너로 알리지는 않는다.
    assert!(!PrEventKind::Review.notifies());
    assert_eq!(PrEventKind::Review.action(), "pr-review");
}

/// Codex 지적(#210) — 스레드 하나를 🚀 로 처리하는 사이 새 스레드가 붙으면 수는 그대로다. 수가 아니라
/// 처음 보는 스레드 id 로 새 리뷰를 가린다.
#[test]
fn a_new_thread_replacing_a_handled_one_is_still_a_review() {
    let mut before = snap(1, "OPEN", false, "CLEAN");
    before.unhandled = 1;
    before.unhandled_ids = vec!["T_a".into()];
    let mut after = before.clone();
    after.unhandled_ids = vec!["T_b".into()];
    let kinds: Vec<PrEventKind> = diff(&[before.clone()], &[after.clone()])
        .iter()
        .map(|e| e.kind)
        .collect();
    assert_eq!(kinds, vec![PrEventKind::Review]);
    // 같은 스레드가 그대로면 전이 없음.
    assert!(diff(&[after.clone()], &[after.clone()]).is_empty());
    // id 가 없던 옛 스냅숏(필드 도입 전 저장분)은 수로 비교한다 — 업그레이드 첫 tick 에 이미
    // 알던 스레드를 새 리뷰로 보지 않는다.
    let mut legacy = before.clone();
    legacy.unhandled_ids.clear();
    assert!(diff(&[legacy], &[after]).is_empty());
}

/// Codex 지적(#210) — 닫혔다 다시 열린 PR 에 처리 안 된 스레드가 있으면 그것도 리뷰 도착이다.
#[test]
fn a_reopened_pr_with_unhandled_threads_is_a_review() {
    let closed = snap(1, "CLOSED", false, "CLEAN");
    let mut reopened = snap(1, "OPEN", false, "CLEAN");
    reopened.unhandled = 1;
    reopened.unhandled_ids = vec!["T_a".into()];
    let kinds: Vec<PrEventKind> = diff(&[closed], &[reopened])
        .iter()
        .map(|e| e.kind)
        .collect();
    assert_eq!(kinds, vec![PrEventKind::Opened, PrEventKind::Review]);
}

/// 리뷰를 세션에 처리시킬지는 그 레포를 둔 보드의 reviewFix 로 정한다(대소문자 무시). 레포가 같아도
/// 켠 보드가 없으면 끔, 켠 보드라도 레포가 다르면 끔.
#[test]
fn review_fix_follows_the_board_that_holds_the_repo() {
    use rocky_core::prwatch::review_fix_enabled;
    use rocky_core::types::Board;
    let board = |repo: Option<&str>, on: bool| Board {
        id: "b".into(),
        key: "k".into(),
        title: "k".into(),
        description: None,
        repo: repo.map(str::to_string),
        path: None,
        previous_keys: None,
        review_fix: on,
        created_at: "2026-09-29T00:00:00Z".into(),
        archived_at: None,
        pr_authors: Vec::new(),
    };
    assert!(review_fix_enabled(
        &[board(Some("Minjun0219/Mdwire"), true)],
        "minjun0219/mdwire"
    ));
    assert!(!review_fix_enabled(&[board(Some("o/r"), false)], "o/r"));
    assert!(!review_fix_enabled(&[board(Some("o/other"), true)], "o/r"));
    assert!(!review_fix_enabled(&[board(None, true)], "o/r"));
    assert!(review_fix_enabled(
        &[board(Some("o/r"), false), board(Some("o/r"), true)],
        "o/r"
    ));
}

/// CI 실패 — 통과·진행 중에서 실패로 바뀔 때, 또는 새 head 가 실패일 때만. 같은 head 에서 실패가
/// 이어지면 다시 내지 않고, 재실행(진행 중을 거침)이 또 실패하면 또 낸다. 세션에만 간다.
#[test]
fn ci_failure_is_sent_to_the_session_once_per_failing_run() {
    let kinds = |a: &PrSnapshot, b: &PrSnapshot| -> Vec<PrEventKind> {
        diff(std::slice::from_ref(a), std::slice::from_ref(b))
            .iter()
            .map(|e| e.kind)
            .collect()
    };
    let pass = snap(1, "OPEN", false, "CLEAN");
    let mut pending = pass.clone();
    pending.ci = CiState::Pending;
    let mut fail = pass.clone();
    fail.ci = CiState::Fail;
    assert_eq!(kinds(&pending, &fail), vec![PrEventKind::CiFailed]);
    assert_eq!(kinds(&pass, &fail), vec![PrEventKind::CiFailed]);
    // 같은 head 에서 실패가 이어지면 조용하다.
    assert!(kinds(&fail, &fail).is_empty());
    // 새 커밋도 실패 — 진행 중을 못 봤어도 다시 낸다.
    let mut fail_new_head = fail.clone();
    fail_new_head.head = "1234567".into();
    assert_eq!(kinds(&fail, &fail_new_head), vec![PrEventKind::CiFailed]);
    // 처음 보는 PR 이 이미 실패 중이면 열림과 함께.
    let first: Vec<PrEventKind> = diff(&[], &[fail.clone()]).iter().map(|e| e.kind).collect();
    assert_eq!(first, vec![PrEventKind::Opened, PrEventKind::CiFailed]);
    // 사람 배너는 없고 세션에만.
    assert!(!PrEventKind::CiFailed.notifies() && PrEventKind::CiFailed.reaches_session());
    assert_eq!(PrEventKind::CiFailed.action(), "pr-ci-failed");
}

#[test]
fn filter_search_query_and_parsing() {
    use rocky_core::prwatch::{filter_search_query, is_filter_query, parse_filter_search};
    assert_eq!(
        filter_search_query("  project:org/5 author:@me "),
        "is:pr is:open sort:updated-desc project:org/5 author:@me"
    );
    assert!(is_filter_query("repo:o/r label:\"needs review\""));
    assert!(!is_filter_query("   "));
    assert!(!is_filter_query("a\nb"));
    assert!(!is_filter_query(&"x".repeat(257)));
    let data = serde_json::json!({ "search": { "nodes": [
        { "number": 3, "repository": { "nameWithOwner": "o/r" } },
        {},
        { "number": 9, "repository": { "nameWithOwner": "o/other" } }
    ] } });
    assert_eq!(
        parse_filter_search(&data),
        vec![("o/r".to_string(), 3), ("o/other".to_string(), 9)]
    );
}

fn links(urls: &[&str]) -> Vec<rocky_core::types::TodoLink> {
    urls.iter()
        .map(|u| rocky_core::types::TodoLink {
            url: u.to_string(),
            title: None,
        })
        .collect()
}

#[test]
fn pr_urls_parse_with_tails_and_reject_non_prs() {
    use rocky_core::prwatch::parse_pr_url;
    assert_eq!(
        parse_pr_url("https://github.com/o/r/pull/12"),
        Some(("o/r".into(), 12))
    );
    assert_eq!(
        parse_pr_url("https://github.com/o/r/pull/12/files#diff-abc"),
        Some(("o/r".into(), 12))
    );
    assert_eq!(
        parse_pr_url("https://github.com/o/r/pull/12?w=1"),
        Some(("o/r".into(), 12))
    );
    assert_eq!(parse_pr_url("https://github.com/o/r/issues/12"), None);
    assert_eq!(parse_pr_url("https://gitlab.com/o/r/pull/12"), None);
    assert_eq!(parse_pr_url("https://github.com/o/r/pull/abc"), None);
}

#[test]
fn a_merged_pr_completes_the_todo_that_links_it() {
    use rocky_core::prwatch::{linked_todo_action, LinkedTodoAction};
    let l = links(&["https://github.com/O/R/pull/7/files", "https://example.com"]);
    assert_eq!(
        linked_todo_action(&l, "o/r", 7, true, &[]),
        LinkedTodoAction::Complete
    );
    assert_eq!(
        linked_todo_action(&l, "o/r", 8, true, &[]),
        LinkedTodoAction::NotLinked
    );
    assert_eq!(
        linked_todo_action(&l, "o/other", 7, true, &[]),
        LinkedTodoAction::NotLinked
    );
}

#[test]
fn a_todo_with_another_open_pr_waits_for_it() {
    use rocky_core::prwatch::{linked_todo_action, LinkedTodoAction};
    let l = links(&[
        "https://github.com/o/r/pull/7",
        "https://github.com/o/r/pull/8",
        "https://github.com/o/r/pull/9",
    ]);
    // 8 은 아직 감시 중(열림), 9 는 감시 밖(이미 끝났거나 모른다) — 8 만 기다린다.
    let open = vec![("O/R".to_string(), 8), ("o/r".to_string(), 7)];
    assert_eq!(
        linked_todo_action(&l, "o/r", 7, true, &open),
        LinkedTodoAction::WaitFor(vec![("o/r".into(), 8)])
    );
}

#[test]
fn a_pr_closed_without_merge_leaves_the_status_alone() {
    use rocky_core::prwatch::{linked_todo_action, LinkedTodoAction};
    let l = links(&["https://github.com/o/r/pull/7"]);
    assert_eq!(
        linked_todo_action(&l, "o/r", 7, false, &[]),
        LinkedTodoAction::ClosedUnmerged
    );
}
