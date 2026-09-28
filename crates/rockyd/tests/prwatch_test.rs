//! PR 감시 잡 — 가짜 gh 러너로 한 tick: 스냅숏 저장·전이·알림·health·`/api/prs`·예산.

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::*;
use rocky_core::prwatch::PrEventKind;
use rockyd::prwatch::{tick, Notifier};
use rockyd::runner::{CmdOutput, Runner};
use serde_json::{json, Value};

fn pr(number: i64, state: &str, rollup: &str, merge_state: &str, base: &str) -> Value {
    json!({
        "number": number, "title": format!("PR {number}"), "url": format!("https://github.com/o/r/pull/{number}"),
        "state": state, "isDraft": false, "headRefOid": "0123456789abcdef", "mergeStateStatus": merge_state,
        "baseRefName": base, "updatedAt": "2026-09-28T10:00:00Z",
        "commits": { "nodes": [ { "commit": { "statusCheckRollup": { "state": rollup } } } ] },
        "reviewThreads": { "nodes": [] }
    })
}

/// 목록 창 밖으로 밀린 PR — 목록에는 안 나오고 상세로 물으면 나온다.
fn hidden(mut pr: Value) -> Value {
    pr["__hidden"] = json!(true);
    pr
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
                json!({ "rateLimit": rate_limit(remaining - 3, 3), "repository": repository })
            } else {
                let visible = pool.iter().filter(|p| p["__hidden"] != true);
                let (open, recent): (Vec<_>, Vec<_>) = visible.partition(|p| p["state"] == "OPEN");
                json!({
                    "rateLimit": rate_limit(remaining, 7),
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
        Arc::new(move |title, body| sink.lock().unwrap().push((title, body))),
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
async fn a_tick_queries_each_watched_repo_and_notifies_only_ready_and_conflict() {
    let f = fx();
    f.store.ensure_board("rocky", None, "tester").unwrap();
    f.store.set_board_repo("rocky", "o/r", "tester").unwrap();
    f.store.ensure_board("norepo", None, "tester").unwrap();
    let responses = Arc::new(Mutex::new(json!({
        "r": [pr(1, "OPEN", "SUCCESS", "CLEAN", "main"), pr(2, "OPEN", "PENDING", "CLEAN", "main")]
    })));
    let calls = Arc::new(Mutex::new(Vec::new()));
    let runner = fake_gh(responses.clone(), calls.clone());
    let (notifier, seen) = capture();

    let events = tick(&f.state, &runner, &notifier, true).await.events;
    assert_eq!(list_calls(&calls), 1, "repo 있는 보드만, 레포당 목록 한 번");
    assert_eq!(
        calls.lock().unwrap().len(),
        2,
        "열린 PR 둘의 상세는 별칭 배치 한 번"
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
    assert_eq!(seen.lock().unwrap()[0].1, "#1 확인·머지해도 된다 — PR 1");
    let status = f.state.pr_watch();
    assert!(status.available && status.repos == vec!["o/r".to_string()]);

    // 다음 tick: 1 은 머지, 2 는 충돌 → 알림은 충돌만.
    *responses.lock().unwrap() = json!({
        "r": [pr(1, "MERGED", "SUCCESS", "UNKNOWN", "main"), pr(2, "OPEN", "SUCCESS", "DIRTY", "main")]
    });
    let events = tick(&f.state, &runner, &notifier, true).await.events;
    let mut kinds: Vec<(i64, PrEventKind)> = events.iter().map(|e| (e.number, e.kind)).collect();
    kinds.sort_by_key(|(n, _)| *n); // 열린 것이 먼저, 닫힌 것이 뒤 — 순서는 계약이 아니다
    assert_eq!(
        kinds,
        vec![(1, PrEventKind::Merged), (2, PrEventKind::Conflict)]
    );
    assert_eq!(seen.lock().unwrap().len(), 2);
    assert_eq!(seen.lock().unwrap()[1].1, "#2 충돌 — PR 2");

    // 전이는 보드 히스토리에, 그리고 /api/prs 로 읽힌다.
    let (status, body) = get(&f.state, "/api/prs?board=rocky&open=true").await;
    assert_eq!(status, 200);
    assert_eq!(body.as_array().unwrap().len(), 1);
    assert_eq!(body[0]["number"], 2);
    assert_eq!(body[0]["mergeState"], "DIRTY");
    let (_, all) = get(&f.state, "/api/prs").await;
    assert_eq!(all.as_array().unwrap().len(), 2);
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
    assert_eq!(
        health["prWatch"]["rateLimit"]["cost"], 10,
        "tick 누계 — 목록 7 + 상세 3"
    );
    assert_eq!(
        health["prWatch"]["rateLimit"]["remaining"], 4797,
        "마지막 응답의 잔여"
    );
    assert!(health["prWatch"].get("pausedUntil").is_none());
}

#[tokio::test]
async fn a_failing_repo_is_reported_in_health_and_does_not_break_others() {
    let f = fx();
    f.store.ensure_board("a", None, "tester").unwrap();
    f.store.set_board_repo("a", "o/gone", "tester").unwrap();
    f.store.ensure_board("b", None, "tester").unwrap();
    f.store.set_board_repo("b", "o/r", "tester").unwrap();
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
async fn no_repo_boards_means_a_quiet_tick() {
    let f = fx();
    let runner: Runner =
        Arc::new(|_, _, _| Box::pin(async { CmdOutput::failure("should not run") }));
    let (notifier, _) = capture();
    let outcome = tick(&f.state, &runner, &notifier, true).await;
    assert!(outcome.events.is_empty() && outcome.pause.is_none());
    let status = f.state.pr_watch();
    assert!(status.available && status.repos.is_empty() && status.last_tick.is_some());
}

/// 열린 PR 이 없는 레포는 목록 한 번으로 끝난다 — 상세 쿼리를 부르지 않는다(비용).
#[tokio::test]
async fn a_repo_without_open_prs_costs_one_list_query() {
    let f = fx();
    f.store.ensure_board("rocky", None, "tester").unwrap();
    f.store.set_board_repo("rocky", "o/r", "tester").unwrap();
    let responses = Arc::new(Mutex::new(
        json!({ "r": [pr(3, "MERGED", "SUCCESS", "UNKNOWN", "main")] }),
    ));
    let calls = Arc::new(Mutex::new(Vec::new()));
    let runner = fake_gh(responses, calls.clone());
    let (notifier, _) = capture();
    let events = tick(&f.state, &runner, &notifier, false).await.events;
    assert!(events.is_empty(), "처음 보는 닫힌 PR 은 전이가 아니다");
    assert_eq!(calls.lock().unwrap().len(), 1);
    assert_eq!(f.state.pr_watch().rate_limit.unwrap().cost, 7);
}

/// 닫힌 지 오래돼 최근 창(30건) 밖으로 밀린 옛 열린 PR — 상세에 끼워 머지 전이를 잃지 않는다.
#[tokio::test]
async fn an_open_pr_that_fell_out_of_the_window_is_queried_in_the_detail_batch() {
    let f = fx();
    f.store.ensure_board("rocky", None, "tester").unwrap();
    f.store.set_board_repo("rocky", "o/r", "tester").unwrap();
    let responses = Arc::new(Mutex::new(
        json!({ "r": [pr(7, "OPEN", "SUCCESS", "CLEAN", "main")] }),
    ));
    let calls = Arc::new(Mutex::new(Vec::new()));
    let runner = fake_gh(responses.clone(), calls.clone());
    let (notifier, _) = capture();
    tick(&f.state, &runner, &notifier, false).await;
    assert_eq!(detail_calls_for(&calls, 7), 1);
    // 다음 tick: 목록에 7 이 없다(열린 것도 최근 닫힌 것도 아님) → 상세로 → MERGED.
    *responses.lock().unwrap() = json!({
        "r": [hidden(pr(7, "MERGED", "SUCCESS", "UNKNOWN", "main"))]
    });
    let events = tick(&f.state, &runner, &notifier, false).await.events;
    assert_eq!(
        events
            .iter()
            .map(|e| (e.number, e.kind))
            .collect::<Vec<_>>(),
        vec![(7, PrEventKind::Merged)]
    );
    assert_eq!(detail_calls_for(&calls, 7), 2);
    assert!(f.state.pr_watch().available);
    // 이제 OPEN 이 아니니 다시 묻지 않는다.
    let before = calls.lock().unwrap().len();
    tick(&f.state, &runner, &notifier, false).await;
    assert_eq!(calls.lock().unwrap().len(), before + 1, "목록 쿼리 하나뿐");
}

/// 보드에서 repo 를 떼면 그 레포의 스냅숏은 걷히고, 전역 목록에도 보이지 않는다.
#[tokio::test]
async fn snapshots_of_a_repo_no_longer_on_any_board_disappear() {
    let f = fx();
    f.store.ensure_board("rocky", None, "tester").unwrap();
    f.store.set_board_repo("rocky", "o/r", "tester").unwrap();
    let responses = Arc::new(Mutex::new(
        json!({ "r": [pr(1, "OPEN", "SUCCESS", "CLEAN", "main")] }),
    ));
    let runner = fake_gh(responses, Arc::new(Mutex::new(Vec::new())));
    let (notifier, _) = capture();
    tick(&f.state, &runner, &notifier, false).await;
    assert_eq!(
        get(&f.state, "/api/prs?open=true")
            .await
            .1
            .as_array()
            .unwrap()
            .len(),
        1
    );
    // repo 를 다른 것으로 바꿈 — 다음 tick 전에도 전역 목록은 비고, tick 이 돌면 행도 걷힌다.
    f.store
        .set_board_repo("rocky", "o/other", "tester")
        .unwrap();
    assert_eq!(
        get(&f.state, "/api/prs?open=true")
            .await
            .1
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert_eq!(f.store.list_prs(Some("o/r"), false).unwrap().len(), 1);
    tick(&f.state, &runner, &notifier, false).await;
    assert_eq!(f.store.list_prs(Some("o/r"), false).unwrap().len(), 0);
}

/// 한도에 걸리면 남은 레포는 묻지 않고(똑같이 실패한다) 리셋까지 쉬라고 돌려준다 — 데몬이
/// 3분마다 실패를 반복하며 사용자의 gh 까지 막던 것을 끊는다.
#[tokio::test]
async fn a_rate_limit_error_stops_the_tick_and_pauses_until_reset() {
    let f = fx();
    f.store.ensure_board("a", None, "tester").unwrap();
    f.store.set_board_repo("a", "o/a", "tester").unwrap();
    f.store.ensure_board("b", None, "tester").unwrap();
    f.store.set_board_repo("b", "o/b", "tester").unwrap();
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
    f.store.ensure_board("a", None, "tester").unwrap();
    f.store.set_board_repo("a", "o/a", "tester").unwrap();
    f.store.ensure_board("b", None, "tester").unwrap();
    f.store.set_board_repo("b", "o/b", "tester").unwrap();
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
    assert_eq!(list_calls(&calls), 1, "o/b 는 다음으로 미룬다");
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
