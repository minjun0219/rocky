//! PR 감시 잡 — 가짜 gh 러너로 한 tick: 스냅숏 저장·전이·알림·health·`/api/prs`·예산.

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::*;
use rocky_core::prwatch::{diff, notification_text, PrEventKind, PrSnapshot};
use rockyd::prwatch::{tick, Notifier};
use rockyd::runner::{CmdOutput, Runner};
use serde_json::{json, Value};

/// PR 을 구독한다 — 감시는 구독한 PR 만 한다. 구독은 그 레포를 "이미 보던 레포" 로 표시하므로 기준선이 필요 없다.
fn sub(f: &Fx, repo: &str, number: i64, session: Option<&str>) {
    f.store.subscribe_pr(repo, number, session).unwrap();
}

fn pr(number: i64, state: &str, rollup: &str, merge_state: &str, base: &str) -> Value {
    json!({
        "number": number, "title": format!("PR {number}"), "url": format!("https://github.com/o/r/pull/{number}"),
        "state": state, "isDraft": false, "headRefOid": "0123456789abcdef", "mergeStateStatus": merge_state,
        "baseRefName": base, "updatedAt": "2026-09-28T10:00:00Z",
        "commits": { "nodes": [ { "commit": { "statusCheckRollup": { "state": rollup } } } ] },
        "reviewThreads": { "nodes": [] }
    })
}

fn rate_limit(remaining: i64, cost: i64) -> Value {
    json!({ "cost": cost, "remaining": remaining, "resetAt": "2099-01-01T00:00:00Z" })
}

/// 레포별 PR 풀을 두는 가짜 `gh` — 목록 쿼리에는 상태만, 상세 쿼리에는 별칭마다 PR 을 낸다.
/// 풀 대신 `"RATE_LIMIT"` 이면 한도 에러, `<name>:remaining` 이 있으면 그 잔여를 싣는다.
/// 호출 argv 도 남긴다.
fn fake_gh(responses: Arc<Mutex<Value>>, calls: Arc<Mutex<Vec<Vec<String>>>>) -> Runner {
    Arc::new(move |cmd, _stdin, _timeout| {
        let responses = responses.clone();
        let calls = calls.clone();
        Box::pin(async move {
            calls.lock().unwrap().push(cmd.clone());
            let name = cmd
                .iter()
                .find_map(|a| a.strip_prefix("name="))
                .unwrap_or("")
                .to_string();
            let query = cmd
                .iter()
                .find_map(|a| a.strip_prefix("query="))
                .unwrap_or("")
                .to_string();
            let responses = responses.lock().unwrap();
            if query.contains("search(type:ISSUE") {
                let nodes = responses.get("search").cloned().unwrap_or(json!([]));
                return CmdOutput {
                    code: 0,
                    stdout: json!({ "data": {
                        "rateLimit": rate_limit(4800, 1), "search": { "nodes": nodes }
                    } })
                    .to_string(),
                    stderr: String::new(),
                };
            }
            let pool = match responses.get(&name) {
                // 한도 — gh 는 실패해도 응답 JSON 을 stdout 에 낸다.
                Some(Value::String(s)) if s == "RATE_LIMIT" => return CmdOutput {
                    code: 1,
                    stdout:
                        json!({ "errors": [ { "type": "RATE_LIMIT", "code": "graphql_rate_limit",
                            "message": "API rate limit already exceeded for user ID 1." } ] })
                        .to_string(),
                    stderr: "gh: API rate limit already exceeded for user ID 1.".into(),
                },
                Some(Value::Array(pool)) => pool.clone(),
                _ => return CmdOutput::failure("gh: HTTP 404: Not Found"),
            };
            let remaining = responses
                .get(format!("{name}:remaining"))
                .and_then(Value::as_i64)
                .unwrap_or(4800);
            let data = if query.contains("pullRequest(number:") {
                let mut repository = serde_json::Map::new();
                repository.insert("defaultBranchRef".into(), json!({ "name": "main" }));
                for piece in query.split("pullRequest(number:").skip(1) {
                    let n: i64 = piece
                        .split(')')
                        .next()
                        .and_then(|d| d.parse().ok())
                        .expect("number");
                    let found = pool.iter().find(|p| p["number"] == n).cloned();
                    repository.insert(format!("p{n}"), found.unwrap_or(Value::Null));
                }
                json!({ "rateLimit": rate_limit(remaining - 3, 3), "viewer": { "login": "me" }, "repository": repository })
            } else {
                let visible = pool.iter().filter(|p| p["__hidden"] != true);
                let (open, recent): (Vec<_>, Vec<_>) = visible.partition(|p| p["state"] == "OPEN");
                json!({
                    "rateLimit": rate_limit(remaining, 7),
                    "viewer": { "login": "me" },
                    "repository": {
                        "defaultBranchRef": { "name": "main" },
                        "open": { "pageInfo": { "hasNextPage": false }, "nodes": open },
                        "recent": { "nodes": recent }
                    }
                })
            };
            CmdOutput {
                code: 0,
                stdout: json!({ "data": data }).to_string(),
                stderr: String::new(),
            }
        })
    })
}

type Seen = Arc<Mutex<Vec<(String, String)>>>;

fn capture() -> (Notifier, Seen) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = seen.clone();
    (
        Arc::new(move |event| sink.lock().unwrap().push(notification_text(event))),
        seen,
    )
}

/// 목록 쿼리 호출 수 — 레포당 tick 마다 하나.
fn list_calls(calls: &Arc<Mutex<Vec<Vec<String>>>>) -> usize {
    calls
        .lock()
        .unwrap()
        .iter()
        .filter(|c| c.iter().any(|a| a.contains("states:[OPEN]")))
        .count()
}

fn detail_calls_for(calls: &Arc<Mutex<Vec<Vec<String>>>>, number: i64) -> usize {
    calls
        .lock()
        .unwrap()
        .iter()
        .filter(|c| {
            c.iter()
                .any(|a| a.contains(&format!("pullRequest(number:{number})")))
        })
        .count()
}

#[tokio::test]
async fn a_tick_queries_only_subscribed_prs_and_passes_ready_conflict_and_merged_on() {
    let f = fx();
    f.store.ensure_board("rocky", None, "tester").unwrap();
    f.store.set_board_repo("rocky", "o/r", "tester").unwrap();
    f.store.ensure_board("norepo", None, "tester").unwrap();
    sub(&f, "o/r", 1, None);
    sub(&f, "o/r", 2, None);
    // 3 은 레포에 있지만 구독하지 않았다 — 묻지도, 남기지도 않는다.
    let responses = Arc::new(Mutex::new(json!({
        "r": [pr(1, "OPEN", "SUCCESS", "CLEAN", "main"), pr(2, "OPEN", "PENDING", "CLEAN", "main"),
              pr(3, "OPEN", "SUCCESS", "CLEAN", "main")]
    })));
    let calls = Arc::new(Mutex::new(Vec::new()));
    let runner = fake_gh(responses.clone(), calls.clone());
    let (notifier, seen) = capture();

    let events = tick(&f.state, &runner, &notifier, true).await.events;
    assert_eq!(list_calls(&calls), 0, "레포 목록은 보지 않는다");
    assert_eq!(calls.lock().unwrap().len(), 1, "구독한 둘을 상세 한 번에");
    assert_eq!(
        detail_calls_for(&calls, 3),
        0,
        "구독하지 않은 PR 은 묻지 않는다"
    );
    assert!(calls.lock().unwrap()[0].iter().any(|a| a == "owner=o"));
    assert_eq!(
        events
            .iter()
            .map(|e| (e.number, e.kind))
            .collect::<Vec<_>>(),
        vec![
            (1, PrEventKind::Opened),
            (1, PrEventKind::Ready),
            (2, PrEventKind::Opened)
        ]
    );
    assert_eq!(seen.lock().unwrap().len(), 1, "ready 만 알린다");
    assert_eq!(seen.lock().unwrap()[0].1, "#1 머지 후보 — PR 1");
    let status = f.state.pr_watch();
    assert!(status.available && status.repos == vec!["o/r".to_string()]);

    // 다음 tick: 1 은 머지, 2 는 충돌 → 알림기에는 둘 다 간다. 머지된 1 은 구독이 걷힌다.
    *responses.lock().unwrap() = json!({
        "r": [pr(1, "MERGED", "SUCCESS", "UNKNOWN", "main"), pr(2, "OPEN", "SUCCESS", "DIRTY", "main")]
    });
    let events = tick(&f.state, &runner, &notifier, true).await.events;
    let mut kinds: Vec<(i64, PrEventKind)> = events.iter().map(|e| (e.number, e.kind)).collect();
    kinds.sort_by_key(|(n, _)| *n);
    assert_eq!(
        kinds,
        vec![(1, PrEventKind::Merged), (2, PrEventKind::Conflict)]
    );
    let mut texts: Vec<String> = seen.lock().unwrap()[1..]
        .iter()
        .map(|(_, b)| b.clone())
        .collect();
    texts.sort();
    assert_eq!(texts, vec!["#1 머지됨 — PR 1", "#2 충돌 — PR 2"]);
    assert!(
        f.store.pr_subscription("o/r", 1).unwrap().is_none(),
        "머지되면 구독을 걷는다"
    );
    assert!(f.store.pr_subscription("o/r", 2).unwrap().is_some());

    // 전이는 보드 히스토리에, 그리고 /api/prs 로 읽힌다.
    let (status, body) = get(&f.state, "/api/prs?board=rocky&open=true").await;
    assert_eq!(status, 200);
    assert_eq!(body.as_array().unwrap().len(), 1);
    assert_eq!(body[0]["number"], 2);
    assert_eq!(body[0]["mergeState"], "DIRTY");
    let (_, none) = get(&f.state, "/api/prs?board=norepo").await;
    assert_eq!(
        none.as_array().unwrap().len(),
        0,
        "repo 없는 보드는 빈 목록"
    );
    let (status, _) = get(&f.state, "/api/prs?board=nope").await;
    assert_eq!(status, 400);
    let (_, health) = get(&f.state, "/api/health").await;
    assert_eq!(health["prWatch"]["available"], true);
    assert_eq!(health["prWatch"]["repos"][0], "o/r");
    assert_eq!(health["prWatch"]["rateLimit"]["cost"], 3, "상세 한 번뿐");
    assert_eq!(health["prWatch"]["rateLimit"]["remaining"], 4797);
    assert!(health["prWatch"].get("pausedUntil").is_none());

    // 다음 tick 에 구독이 걷힌 1 의 스냅숏도 걷힌다 — 남는 건 구독한 2 뿐.
    tick(&f.state, &runner, &notifier, true).await;
    let (_, all) = get(&f.state, "/api/prs").await;
    assert_eq!(
        all.as_array()
            .unwrap()
            .iter()
            .map(|p| p["number"].as_i64().unwrap())
            .collect::<Vec<_>>(),
        vec![2]
    );
}

#[tokio::test]
async fn a_failing_repo_is_reported_in_health_and_does_not_break_others() {
    let f = fx();
    sub(&f, "o/gone", 1, None);
    sub(&f, "o/r", 5, None);
    let responses = Arc::new(Mutex::new(
        json!({ "r": [pr(5, "OPEN", "SUCCESS", "CLEAN", "main")] }),
    ));
    let runner = fake_gh(responses, Arc::new(Mutex::new(Vec::new())));
    let (notifier, seen) = capture();
    let events = tick(&f.state, &runner, &notifier, false).await.events;
    assert_eq!(events.len(), 2, "o/r 은 정상");
    assert!(
        seen.lock().unwrap().is_empty(),
        "notify=false 면 알리지 않는다"
    );
    let status = f.state.pr_watch();
    assert!(!status.available);
    assert!(status.reason.as_deref().unwrap().contains("o/gone"));
    assert!(status.reason.as_deref().unwrap().contains("404"));
    assert_eq!(status.repos, vec!["o/gone".to_string(), "o/r".to_string()]);
}

#[tokio::test]
async fn no_subscriptions_means_a_quiet_tick() {
    let f = fx();
    let runner: Runner =
        Arc::new(|_, _, _| Box::pin(async { CmdOutput::failure("should not run") }));
    let (notifier, _) = capture();
    let outcome = tick(&f.state, &runner, &notifier, true).await;
    assert!(outcome.events.is_empty() && outcome.pause.is_none());
    let status = f.state.pr_watch();
    assert!(status.available && status.repos.is_empty() && status.last_tick.is_some());
}

/// 구독 — REST 는 로컬 전용·모양 검증, 같은 PR 을 다시 구독하면 맡은 세션만 바뀐다(넘겨받기), 해지하면
/// 다음 tick 부터 묻지도 남기지도 않는다.
#[tokio::test]
async fn subscriptions_are_local_validated_and_unsubscribing_stops_watching() {
    let f = fx();
    let (status, body) = post(
        &f.state,
        "/api/prs/subscriptions",
        json!({ "repo": "o/r", "number": 4, "sessionId": "a" }),
    )
    .await;
    assert_eq!(status, 201, "{body}");
    assert_eq!(body["sessionId"], "a");
    let (_, body) = post(
        &f.state,
        "/api/prs/subscriptions",
        json!({ "repo": "O/R", "number": 4, "sessionId": "b" }),
    )
    .await;
    assert_eq!(body["sessionId"], "b", "다른 세션이 넘겨받는다");
    assert_eq!(
        body["repo"], "o/r",
        "레포 표기는 처음 것을 지킨다(대소문자 무시)"
    );
    let (_, list) = get(&f.state, "/api/prs/subscriptions").await;
    assert_eq!(list.as_array().unwrap().len(), 1);
    for bad in [
        json!({ "repo": "nope", "number": 1 }),
        json!({ "repo": "o/r", "number": 0 }),
        json!({ "repo": "o/r/x", "number": 1 }),
    ] {
        let (status, _) = post(&f.state, "/api/prs/subscriptions", bad).await;
        assert_eq!(status, 400);
    }
    let remote = ReqOptions {
        headers: vec![("x-forwarded-for", "10.0.0.2")],
        ..ReqOptions::default()
    };
    let (status, _) = call(
        &f.state,
        "POST",
        "/api/prs/subscriptions",
        Some(json!({ "repo": "o/r", "number": 5 })),
        remote,
    )
    .await;
    assert_eq!(status, 403);

    let responses = Arc::new(Mutex::new(
        json!({ "r": [pr(4, "OPEN", "SUCCESS", "CLEAN", "main")] }),
    ));
    let calls = Arc::new(Mutex::new(Vec::new()));
    let runner = fake_gh(responses, calls.clone());
    let (notifier, _) = capture();
    tick(&f.state, &runner, &notifier, false).await;
    assert_eq!(f.store.list_prs(Some("o/r"), false).unwrap().len(), 1);
    let (status, body) = call(
        &f.state,
        "DELETE",
        "/api/prs/subscriptions?repo=o/r&number=4",
        None,
        ReqOptions::default(),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["removed"], true);
    let before = calls.lock().unwrap().len();
    tick(&f.state, &runner, &notifier, false).await;
    assert_eq!(
        calls.lock().unwrap().len(),
        before,
        "구독이 없으면 묻지 않는다"
    );
    assert!(f.store.list_prs(Some("o/r"), false).unwrap().is_empty());
}

/// 닫힘도 끝이다 — 알린 뒤 구독을 걷고, 그다음부터는 묻지 않는다.
#[tokio::test]
async fn a_closed_pr_is_unsubscribed_after_the_transition() {
    let f = fx();
    sub(&f, "o/r", 7, None);
    let responses = Arc::new(Mutex::new(
        json!({ "r": [pr(7, "OPEN", "SUCCESS", "CLEAN", "main")] }),
    ));
    let calls = Arc::new(Mutex::new(Vec::new()));
    let runner = fake_gh(responses.clone(), calls.clone());
    let (notifier, _) = capture();
    tick(&f.state, &runner, &notifier, false).await;
    *responses.lock().unwrap() = json!({ "r": [pr(7, "CLOSED", "SUCCESS", "UNKNOWN", "main")] });
    let events = tick(&f.state, &runner, &notifier, false).await.events;
    assert_eq!(
        events
            .iter()
            .map(|e| (e.number, e.kind))
            .collect::<Vec<_>>(),
        vec![(7, PrEventKind::Closed)]
    );
    assert!(f.store.pr_subscriptions().unwrap().is_empty());
    let before = calls.lock().unwrap().len();
    tick(&f.state, &runner, &notifier, false).await;
    assert_eq!(calls.lock().unwrap().len(), before);
}

/// 보드의 `repo` 는 감시 대상을 정하지 않는다 — 보드에 레포를 붙여도 구독이 없으면 묻지 않는다.
#[tokio::test]
async fn a_board_repo_alone_does_not_start_watching() {
    let f = fx();
    f.store.ensure_board("rocky", None, "tester").unwrap();
    f.store.set_board_repo("rocky", "o/r", "tester").unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let runner = fake_gh(
        Arc::new(Mutex::new(
            json!({ "r": [pr(1, "OPEN", "SUCCESS", "CLEAN", "main")] }),
        )),
        calls.clone(),
    );
    let (notifier, _) = capture();
    tick(&f.state, &runner, &notifier, false).await;
    assert!(calls.lock().unwrap().is_empty());
    assert!(f.state.pr_watch().repos.is_empty());
}

/// 한도에 걸리면 남은 레포는 묻지 않고(똑같이 실패한다) 리셋까지 쉬라고 돌려준다 — 데몬이
/// 3분마다 실패를 반복하며 사용자의 gh 까지 막던 것을 끊는다.
#[tokio::test]
async fn a_rate_limit_error_stops_the_tick_and_pauses_until_reset() {
    let f = fx();
    sub(&f, "o/a", 1, None);
    sub(&f, "o/b", 1, None);
    let responses = Arc::new(Mutex::new(json!({
        "a": "RATE_LIMIT",
        "b": [pr(1, "OPEN", "SUCCESS", "CLEAN", "main")]
    })));
    let calls = Arc::new(Mutex::new(Vec::new()));
    let runner = fake_gh(responses, calls.clone());
    let (notifier, _) = capture();
    let outcome = tick(&f.state, &runner, &notifier, false).await;
    assert!(outcome.events.is_empty());
    assert_eq!(calls.lock().unwrap().len(), 1, "o/b 는 묻지 않는다");
    let pause = outcome.pause.expect("쉰다");
    assert_eq!(
        pause,
        Duration::from_secs(15 * 60),
        "리셋 시각을 모르면 고정 길이"
    );
    let status = f.state.pr_watch();
    assert!(!status.available);
    let reason = status.reason.unwrap();
    assert!(
        reason.contains("o/a") && reason.contains("레이트 리밋"),
        "{reason}"
    );
    assert!(status.paused_until.is_some());
    let (_, health) = get(&f.state, "/api/health").await;
    assert!(health["prWatch"]["pausedUntil"].is_string());
}

/// 잔여가 바닥(`RATE_LIMIT_FLOOR`) 밑으로 내려가면 한도에 닿기 전에 멈춘다 — 나머지 1/5 은
/// 세션·터미널의 gh 몫이다.
#[tokio::test]
async fn a_low_budget_stops_the_tick_before_the_limit() {
    let f = fx();
    sub(&f, "o/a", 1, None);
    sub(&f, "o/b", 2, None);
    let responses = Arc::new(Mutex::new(json!({
        "a": [pr(1, "OPEN", "SUCCESS", "CLEAN", "main")],
        "a:remaining": 903,
        "b": [pr(2, "OPEN", "SUCCESS", "CLEAN", "main")]
    })));
    let calls = Arc::new(Mutex::new(Vec::new()));
    let runner = fake_gh(responses, calls.clone());
    let (notifier, _) = capture();
    let outcome = tick(&f.state, &runner, &notifier, false).await;
    assert_eq!(
        outcome.events.iter().map(|e| e.number).collect::<Vec<_>>(),
        vec![1, 1],
        "o/a 의 스냅숏은 정상 반영(opened·ready)"
    );
    assert_eq!(calls.lock().unwrap().len(), 1, "o/b 는 다음으로 미룬다");
    let pause = outcome.pause.expect("쉰다");
    assert_eq!(
        pause,
        Duration::from_secs(61 * 60),
        "리셋(2099)까지는 상한(한도 창 + 여유)으로 자른다"
    );
    let status = f.state.pr_watch();
    assert!(!status.available);
    assert!(status.reason.unwrap().contains("잔여 900"));
    assert_eq!(status.rate_limit.unwrap().remaining, 900);
}

/// 알림 브릿지 — 등록된 argv 그대로, stdin 에 전이 JSON. 실패는 exit code 로만 알고 나머지
/// 알림기는 그대로 받는다.
#[tokio::test]
async fn bridge_notifier_runs_the_command_with_the_transition_on_stdin() {
    use rockyd::prwatch::{bridge_notifier, compose_notifiers};
    type Calls = Arc<Mutex<Vec<(Vec<String>, String, Duration)>>>;
    let calls: Calls = Arc::new(Mutex::new(Vec::new()));
    let sink = calls.clone();
    let runner: Runner = Arc::new(move |cmd, stdin, timeout| {
        let sink = sink.clone();
        Box::pin(async move {
            sink.lock().unwrap().push((cmd, stdin, timeout));
            CmdOutput::failure("telegram: 토큰이 없다 — --op REF")
        })
    });
    let bridge = rocky_core::config::CommandBridge {
        name: "telegram".into(),
        command: vec![
            "bun".into(),
            "/x/bridges/telegram/notify.ts".into(),
            "--chat".into(),
            "1".into(),
        ],
        timeout_ms: Some(15_000),
    };
    let (capture, seen) = capture();
    let both = compose_notifiers(vec![capture, bridge_notifier(runner, bridge)]);
    let snap = PrSnapshot {
        repo: "o/r".into(),
        number: 7,
        title: "PR 7".into(),
        url: "https://github.com/o/r/pull/7".into(),
        state: "OPEN".into(),
        is_draft: false,
        base: "main".into(),
        head: "0123456".into(),
        merge_state: "CLEAN".into(),
        ci: rocky_core::prwatch::CiState::Pass,
        unhandled: 0,
        unhandled_ids: vec![],
        decision: 0,
        ready: true,
        updated_at: "2026-09-28T10:00:00Z".into(),
        author: None,
    };
    let events = diff(&[], &[snap]);
    let ready = events
        .iter()
        .find(|e| e.kind == PrEventKind::Ready)
        .unwrap();
    both(ready);
    // 브릿지는 tokio::spawn 으로 보낸다 — 잠깐 양보.
    for _ in 0..50 {
        if !calls.lock().unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let calls = calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    let (argv, stdin, timeout) = &calls[0];
    assert_eq!(argv[0], "bun");
    assert_eq!(argv[3], "1", "등록한 argv 그대로 — 셸 없음");
    assert_eq!(*timeout, Duration::from_millis(15_000));
    let payload: Value = serde_json::from_str(stdin).unwrap();
    assert_eq!(payload["kind"], "ready");
    assert_eq!(payload["number"], 7);
    assert_eq!(payload["url"], "https://github.com/o/r/pull/7");
    assert_eq!(payload["text"], "#7 머지 후보 — PR 7");
    assert_eq!(seen.lock().unwrap().len(), 1, "다른 알림기도 받는다");
}

/// 세션 알림 — 훅이 등록한 받은편지함 소켓 중 **그 PR 을 구독한 세션** 하나에만 한 줄을 쓴다(더 최근 세션이
/// 있어도). 실제 유닉스 소켓으로 끝까지 본다. 등록 라우트는 로컬 전용·경로 검증.
#[cfg(unix)]
#[tokio::test]
async fn session_notifier_writes_one_line_to_the_subscribed_session_only() {
    use rockyd::prwatch::session_notifier;
    use std::io::Read;
    use std::os::unix::net::UnixListener;

    let f = fx();
    f.store.ensure_board("rocky", None, "tester").unwrap();
    f.store.set_board_repo("rocky", "o/r", "tester").unwrap();
    let dir = std::path::PathBuf::from(format!("/tmp/cc-socks-rockytest-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let old_sock = dir.join("100.sock");
    let new_sock = dir.join("200.sock");
    let old_listener = UnixListener::bind(&old_sock).unwrap();
    let new_listener = UnixListener::bind(&new_sock).unwrap();
    old_listener.set_nonblocking(true).unwrap();

    // 등록 — 원격은 404(존재를 숨긴다), 받은편지함 모양이 아닌 경로는 400.
    let remote = call(
        &f.state,
        "POST",
        "/api/sessions/inbox",
        Some(json!({ "sessionId": "x", "socket": new_sock.to_str().unwrap(), "cwd": "/w/rocky" })),
        ReqOptions {
            peer: Some("100.64.0.1"),
            ..ReqOptions::default()
        },
    )
    .await;
    assert_eq!(remote.0, 404);
    let bad = post(
        &f.state,
        "/api/sessions/inbox",
        json!({ "sessionId": "x", "socket": "/var/run/docker.sock", "cwd": "/w/rocky" }),
    )
    .await;
    assert_eq!(bad.0, 400);
    for (id, sock) in [("old", &old_sock), ("new", &new_sock)] {
        let (status, _) = post(
            &f.state,
            "/api/sessions/inbox",
            json!({ "sessionId": id, "socket": sock.to_str().unwrap(), "cwd": "/w/rocky" }),
        )
        .await;
        assert_eq!(status, 204);
        // 같은 초에 등록되면 순서를 가를 수 없다 — 나중 등록이 확실히 더 최근이게.
        tokio::time::sleep(Duration::from_millis(1100)).await;
    }

    // 옛 세션이 이 PR 을 만들어 구독했다 — 더 최근 세션(new)이 있어도 옛 세션이 받는다.
    sub(&f, "o/r", 7, Some("old"));
    old_listener.set_nonblocking(false).unwrap();
    new_listener.set_nonblocking(true).unwrap();
    let notify = session_notifier(f.state.clone());
    let snap = PrSnapshot {
        repo: "o/r".into(),
        number: 7,
        title: "PR 7".into(),
        url: "https://github.com/o/r/pull/7".into(),
        state: "OPEN".into(),
        is_draft: false,
        base: "main".into(),
        head: "0123456".into(),
        merge_state: "CLEAN".into(),
        ci: rocky_core::prwatch::CiState::Pass,
        unhandled: 0,
        unhandled_ids: vec![],
        decision: 0,
        ready: true,
        updated_at: "2026-09-28T10:00:00Z".into(),
        author: None,
    };
    let events = diff(&[], &[snap]);
    let ready = events
        .iter()
        .find(|e| e.kind == PrEventKind::Ready)
        .unwrap();
    notify(ready);

    let received = tokio::task::spawn_blocking(move || {
        let (mut conn, _) = old_listener.accept().unwrap();
        let mut text = String::new();
        conn.read_to_string(&mut text).unwrap();
        text
    })
    .await
    .unwrap();
    let v: Value = serde_json::from_str(received.trim_end()).unwrap();
    assert_eq!(v["type"], "user");
    assert!(v["message"]["content"]
        .as_str()
        .unwrap()
        .starts_with("rocky: o/r #7 머지 후보"));
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        new_listener.accept().is_err(),
        "구독하지 않은 세션에는 가지 않는다"
    );

    // 구독한 세션이 사라지면(소켓 없음) 등록을 걷고, 다른 세션으로 넘기지 않는다.
    std::fs::remove_file(&old_sock).unwrap();
    notify(ready);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!f.state.inboxes().iter().any(|r| r.session_id == "old"));
    assert!(new_listener.accept().is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

/// 구독한 세션이 등록돼 있지 않으면(끝났다) 아무에게도 보내지 않고 "못 보냄" 을 전달 기록에 남긴다. 구독이
/// 없는 PR · 세션 없이 지켜보기만 하는 구독은 아무도 깨우지 않는다 — 예전엔 보드의 가장 최근 세션에 레포의
/// PR 이 전부 쏟아졌다.
#[cfg(unix)]
#[tokio::test]
async fn no_live_subscriber_means_nobody_is_woken() {
    use rockyd::prwatch::session_notifier;
    use std::os::unix::net::UnixListener;

    let f = fx();
    f.store.ensure_board("rocky", None, "tester").unwrap();
    f.store.set_board_repo("rocky", "o/r", "tester").unwrap();
    let dir = std::path::PathBuf::from(format!("/tmp/cc-socks-rockyfb-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let live = dir.join("300.sock");
    let listener = UnixListener::bind(&live).unwrap();
    listener.set_nonblocking(true).unwrap();
    let (status, _) = post(
        &f.state,
        "/api/sessions/inbox",
        json!({ "sessionId": "live", "socket": live.to_str().unwrap(), "cwd": "/w/rocky" }),
    )
    .await;
    assert_eq!(status, 204);
    sub(&f, "o/r", 8, Some("gone"));
    sub(&f, "o/r", 11, None);
    let event = |number: i64| rocky_core::prwatch::PrEvent {
        kind: PrEventKind::Conflict,
        repo: "o/r".into(),
        number,
        title: format!("PR {number}"),
        url: format!("https://github.com/o/r/pull/{number}"),
        quiet: false,
        author: None,
    };
    let notify = session_notifier(f.state.clone());
    for n in [8, 10, 11] {
        notify(&event(n));
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        listener.accept().is_err(),
        "살아 있는 다른 세션에 넘기지 않는다"
    );
    let (_, deliveries) = get(&f.state, "/api/deliveries").await;
    let recent = deliveries["recent"].as_array().unwrap();
    assert_eq!(
        recent.len(),
        1,
        "구독 세션이 끝난 #8 만 기록 — 구독 없는 #10·지켜보기 #11 은 조용하다"
    );
    assert_eq!(recent[0]["sessionId"], "gone");
    assert_eq!(recent[0]["ok"], false);
    let _ = std::fs::remove_dir_all(&dir);
}

/// 리뷰 도착 — 그 레포 보드의 autoResolve 가 켜졌을 때만 세션에 review-fix 를 시킨다. 꺼진 보드는 조용하다.
#[cfg(unix)]
#[tokio::test]
async fn review_events_reach_the_session_only_when_auto_resolve_is_on() {
    use rockyd::prwatch::session_notifier;
    use std::io::Read;
    use std::os::unix::net::UnixListener;

    let f = fx();
    f.store.ensure_board("rocky", None, "tester").unwrap();
    f.store.set_board_repo("rocky", "o/r", "tester").unwrap();
    let dir = std::path::PathBuf::from(format!("/tmp/cc-socks-rockyar-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let sock = dir.join("500.sock");
    let listener = UnixListener::bind(&sock).unwrap();
    listener.set_nonblocking(true).unwrap();
    let (status, _) = post(
        &f.state,
        "/api/sessions/inbox",
        json!({ "sessionId": "s", "socket": sock.to_str().unwrap(), "cwd": "/w/rocky" }),
    )
    .await;
    assert_eq!(status, 204);
    sub(&f, "o/r", 9, Some("s"));
    let review = rocky_core::prwatch::PrEvent {
        kind: PrEventKind::Review,
        repo: "o/r".into(),
        number: 9,
        title: "PR 9".into(),
        url: "https://github.com/o/r/pull/9".into(),
        quiet: false,
        author: None,
    };
    // 꺼진 보드(기본) — 아무것도 안 간다.
    session_notifier(f.state.clone())(&review);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(listener.accept().is_err(), "autoResolve 가 꺼진 레포");
    // 그 레포의 세션이 자기 보드를 켰다 — review-fix 를 시킨다.
    let (status, _) = patch(
        &f.state,
        "/api/boards/rocky",
        json!({ "autoResolve": true }),
    )
    .await;
    assert_eq!(status, 200);
    listener.set_nonblocking(false).unwrap();
    session_notifier(f.state.clone())(&review);
    let received = tokio::task::spawn_blocking(move || {
        let (mut conn, _) = listener.accept().unwrap();
        let mut text = String::new();
        conn.read_to_string(&mut text).unwrap();
        text
    })
    .await
    .unwrap();
    assert!(received.contains("/rocky:review-fix 9"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// 세션 전달 현황 — 세션마다 구독한 PR(`repo#N`)과 최근 보낸 기록이 보이고, "보내지 않기" 를 켠 세션에는
/// 보내지 않는다(다른 세션으로 넘기지도 않는다). 다시 켜면 받는다. 현황·조작은 로컬 전용.
#[cfg(unix)]
#[tokio::test]
async fn muting_a_session_skips_it_and_the_delivery_log_shows_it() {
    use rockyd::prwatch::session_notifier;
    use std::io::Read;
    use std::os::unix::net::UnixListener;

    let f = fx();
    let dir = std::path::PathBuf::from(format!("/tmp/cc-socks-rockymute-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mine = dir.join("500.sock");
    let other = dir.join("600.sock");
    let mine_l = UnixListener::bind(&mine).unwrap();
    let other_l = UnixListener::bind(&other).unwrap();
    mine_l.set_nonblocking(true).unwrap();
    other_l.set_nonblocking(true).unwrap();
    for (id, sock) in [("mine", &mine), ("other", &other)] {
        post(
            &f.state,
            "/api/sessions/inbox",
            json!({ "sessionId": id, "socket": sock.to_str().unwrap(), "cwd": "/w/rocky" }),
        )
        .await;
    }
    sub(&f, "o/r", 9, Some("mine"));
    let (_, before) = get(&f.state, "/api/deliveries").await;
    let receives = |v: &Value, id: &str| {
        v["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["sessionId"] == id)
            .map(|s| s["receivesPrFor"].clone())
    };
    assert_eq!(receives(&before, "mine"), Some(json!(["o/r#9"])));
    assert_eq!(receives(&before, "other"), Some(json!([])));

    let (status, _) = post(
        &f.state,
        "/api/deliveries/mute",
        json!({ "sessionId": "mine", "muted": true }),
    )
    .await;
    assert_eq!(status, 200);
    let conflict = rocky_core::prwatch::PrEvent {
        kind: PrEventKind::Conflict,
        repo: "o/r".into(),
        number: 9,
        title: "PR 9".into(),
        url: "https://github.com/o/r/pull/9".into(),
        quiet: false,
        author: None,
    };
    session_notifier(f.state.clone())(&conflict);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        mine_l.accept().is_err(),
        "보내지 않기인 세션에는 보내지 않는다"
    );
    assert!(other_l.accept().is_err(), "다른 세션으로 넘기지도 않는다");

    // 다시 받기로 — 이제 간다.
    post(
        &f.state,
        "/api/deliveries/mute",
        json!({ "sessionId": "mine", "muted": false }),
    )
    .await;
    mine_l.set_nonblocking(false).unwrap();
    session_notifier(f.state.clone())(&conflict);
    let got = tokio::task::spawn_blocking(move || {
        let (mut conn, _) = mine_l.accept().unwrap();
        let mut text = String::new();
        conn.read_to_string(&mut text).unwrap();
        text
    })
    .await
    .unwrap();
    assert!(got.contains("#9"));
    tokio::time::sleep(Duration::from_millis(100)).await;
    let (_, after) = get(&f.state, "/api/deliveries").await;
    let recent = &after["recent"][0];
    assert_eq!(recent["kind"], "pr-conflict");
    assert_eq!(recent["sessionId"], "mine");
    assert_eq!(recent["ok"], true);

    // 로컬 전용 — 원격에는 현황도 조작도 없다.
    let remote = ReqOptions {
        headers: vec![("x-forwarded-for", "10.0.0.2")],
        ..ReqOptions::default()
    };
    let (status, _) = call(&f.state, "GET", "/api/deliveries", None, remote).await;
    assert_eq!(status, 403);
    let _ = std::fs::remove_dir_all(&dir);
}

/// 보드의 PR 작성자 필터 — `@me`(gh 로그인 계정)의 PR 전이만 세션·배너로 간다. 남의 PR 은 전이로 기록되지만
/// `quiet` 라 알림기에 가지 않고, 스냅숏(`rocky pr`)에는 그대로 남는다.
#[tokio::test]
async fn pr_author_filter_wakes_only_for_my_prs() {
    let f = fx();
    f.store.ensure_board("rocky", None, "tester").unwrap();
    f.store.set_board_repo("rocky", "o/r", "tester").unwrap();
    f.store
        .update_board(
            "rocky",
            &rocky_core::types::BoardPatch {
                pr_authors: Some(vec!["@me".into()]),
                ..Default::default()
            },
            "tester",
        )
        .unwrap();
    sub(&f, "o/r", 1, None);
    sub(&f, "o/r", 2, None);
    let mut mine = pr(1, "OPEN", "SUCCESS", "CLEAN", "main");
    mine["author"] = json!({ "login": "Me" }); // 대소문자는 가리지 않는다
    let mut theirs = pr(2, "OPEN", "SUCCESS", "CLEAN", "main");
    theirs["author"] = json!({ "login": "other" });
    let responses = Arc::new(Mutex::new(json!({ "r": [mine, theirs] })));
    let calls = Arc::new(Mutex::new(Vec::new()));
    let runner = fake_gh(responses, calls);
    let (notifier, seen) = capture();
    let events = tick(&f.state, &runner, &notifier, true).await.events;
    assert!(events
        .iter()
        .any(|e| e.number == 2 && e.kind == PrEventKind::Ready && e.quiet));
    let texts: Vec<String> = seen
        .lock()
        .unwrap()
        .iter()
        .map(|(_, b)| b.clone())
        .collect();
    assert_eq!(
        texts,
        vec!["#1 머지 후보 — PR 1".to_string()],
        "내 PR 만 깨운다"
    );
    assert_eq!(
        f.store.list_prs(Some("o/r"), true).unwrap().len(),
        2,
        "보기는 넓게"
    );
}

/// `PATCH /api/boards/:key {"prAuthors"}` — 로컬 전용, 배열 아니면 400, 모양이 틀린 login 은 400, null 은 지우기.
#[tokio::test]
async fn board_pr_authors_patch_is_local_and_validated() {
    let f = fx();
    f.store.ensure_board("rocky", None, "tester").unwrap();
    let (status, b) = patch(
        &f.state,
        "/api/boards/rocky",
        json!({ "prAuthors": ["@me"] }),
    )
    .await;
    assert_eq!(status, 200, "{b}");
    assert_eq!(b["prAuthors"], json!(["@me"]));
    let (status, _) = patch(&f.state, "/api/boards/rocky", json!({ "prAuthors": "@me" })).await;
    assert_eq!(status, 400);
    let (status, _) = patch(
        &f.state,
        "/api/boards/rocky",
        json!({ "prAuthors": ["no spaces"] }),
    )
    .await;
    assert_eq!(status, 400);
    let remote = ReqOptions {
        headers: vec![("x-forwarded-for", "10.0.0.2")],
        ..ReqOptions::default()
    };
    let (status, _) = call(
        &f.state,
        "PATCH",
        "/api/boards/rocky",
        Some(json!({ "prAuthors": ["x"] })),
        remote,
    )
    .await;
    assert_eq!(status, 403);
    let (_, b) = patch(&f.state, "/api/boards/rocky", json!({ "prAuthors": null })).await;
    assert!(b.get("prAuthors").is_none(), "지우면 응답에서 빠진다: {b}");
}

/// 필터는 보드 속성이다 — 같은 레포를 둔 두 보드 중 `@me` 보드에는 남의 PR 전이가 `quiet` 로, 필터 없는
/// 보드에는 그대로 기록된다(훅 주입이 보드별로 갈린다). 배너·브릿지(레포 단위)는 하나라도 통과시키면 알린다.
#[tokio::test]
async fn pr_author_filter_is_per_board() {
    let f = fx();
    for key in ["mine", "team"] {
        f.store.ensure_board(key, None, "tester").unwrap();
        f.store.set_board_repo(key, "o/r", "tester").unwrap();
    }
    f.store
        .update_board(
            "mine",
            &rocky_core::types::BoardPatch {
                pr_authors: Some(vec!["@me".into()]),
                ..Default::default()
            },
            "tester",
        )
        .unwrap();
    sub(&f, "o/r", 2, None);
    let mut theirs = pr(2, "OPEN", "SUCCESS", "CLEAN", "main");
    theirs["author"] = json!({ "login": "other" });
    let responses = Arc::new(Mutex::new(json!({ "r": [theirs] })));
    let runner = fake_gh(responses, Arc::new(Mutex::new(Vec::new())));
    let (notifier, seen) = capture();
    let events = tick(&f.state, &runner, &notifier, true).await.events;
    assert!(
        events.iter().all(|e| !e.quiet),
        "레포 단위로는 team 보드가 통과시킨다"
    );
    assert_eq!(seen.lock().unwrap().len(), 1, "배너·세션 알림기는 한 번");
    let quiet_on = |key: &str| {
        let id = f.store.board_id_of(key).unwrap().unwrap();
        f.store
            .list_history(&rocky_core::types::ListHistoryFilter {
                entity_id: Some(id),
                ..Default::default()
            })
            .unwrap()
            .iter()
            .filter(|h| h.action == "pr-ready")
            .all(|h| h.changes.as_ref().and_then(|c| c.get("quiet")) == Some(&json!(true)))
    };
    assert!(quiet_on("mine"), "@me 보드에는 남의 PR 이 조용히 기록된다");
    assert!(!quiet_on("team"), "필터 없는 보드에는 그대로");
}

/// 필터 구독 — 데몬이 tick 마다 검색해 걸린 열린 PR 을 그 세션 구독으로 넣고, 같은 tick 에 감시한다. 이미 다른 세션이
/// 직접 구독한 PR 은 빼앗지 않는다. 검색어는 `-f` 로 넘긴다(`-F` 는 `@me` 를 파일로 읽는다). 해지하면 같이 걷힌다.
#[tokio::test]
async fn a_filter_subscription_pulls_matching_prs_into_watch() {
    let f = fx();
    sub(&f, "o/r", 2, Some("other"));
    let (status, filter) = post(
        &f.state,
        "/api/prs/filters",
        json!({ "query": "@me repo:o/r", "sessionId": "s1" }),
    )
    .await;
    assert_eq!(status, 201, "{filter}");
    let responses = Arc::new(Mutex::new(json!({
        "search": [
            { "number": 1, "repository": { "nameWithOwner": "o/r" } },
            { "number": 2, "repository": { "nameWithOwner": "o/r" } }
        ],
        "r": [pr(1, "OPEN", "SUCCESS", "CLEAN", "main"), pr(2, "OPEN", "SUCCESS", "CLEAN", "main")]
    })));
    let calls = Arc::new(Mutex::new(Vec::new()));
    let runner = fake_gh(responses, calls.clone());
    let (notifier, _) = capture();
    let events = tick(&f.state, &runner, &notifier, false).await.events;
    assert!(
        events.iter().any(|e| e.number == 1),
        "같은 tick 에 감시까지"
    );
    let search_call = calls.lock().unwrap()[0].clone();
    let at = search_call
        .iter()
        .position(|a| a.starts_with("q="))
        .unwrap();
    assert_eq!(search_call[at - 1], "-f");
    assert_eq!(
        search_call[at],
        "q=is:pr is:open sort:updated-desc @me repo:o/r"
    );
    let s1 = f.store.pr_subscription("o/r", 1).unwrap().unwrap();
    assert_eq!(s1.session_id.as_deref(), Some("s1"));
    assert_eq!(
        f.store
            .pr_subscription("o/r", 2)
            .unwrap()
            .unwrap()
            .session_id
            .as_deref(),
        Some("other"),
        "직접 구독은 빼앗지 않는다"
    );
    let (status, _) = post(&f.state, "/api/prs/filters", json!({ "query": "" })).await;
    assert_eq!(status, 400);
    let remote = ReqOptions {
        headers: vec![("x-forwarded-for", "10.0.0.2")],
        ..ReqOptions::default()
    };
    let (status, _) = call(
        &f.state,
        "POST",
        "/api/prs/filters",
        Some(json!({ "query": "repo:o/r" })),
        remote,
    )
    .await;
    assert_eq!(status, 403);
    let id = filter["id"].as_str().unwrap();
    let (status, body) = call(
        &f.state,
        "DELETE",
        &format!("/api/prs/filters?id={id}"),
        None,
        ReqOptions::default(),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["prs"], 1);
    assert!(f.store.pr_subscription("o/r", 1).unwrap().is_none());
    assert!(f.store.pr_subscription("o/r", 2).unwrap().is_some());
}
