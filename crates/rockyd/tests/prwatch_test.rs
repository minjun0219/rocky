//! PR 감시 잡 — 가짜 gh 러너로 한 tick: 스냅숏 저장·전이·알림·health·`/api/prs`·예산.

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::*;
use rocky_core::prwatch::{diff, notification_text, PrEventKind, PrSnapshot};
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
        rocket: 0,
        ready: true,
        updated_at: "2026-09-28T10:00:00Z".into(),
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
    assert_eq!(payload["text"], "#7 확인·머지해도 된다 — PR 7");
    assert_eq!(seen.lock().unwrap().len(), 1, "다른 알림기도 받는다");
}

/// 세션 알림 — 훅이 등록한 받은편지함 소켓에, 그 레포 보드에서 일하는 가장 최근 세션 하나에만
/// 한 줄을 쓴다. 실제 유닉스 소켓으로 끝까지 본다. 등록 라우트는 로컬 전용·경로 검증.
#[cfg(unix)]
#[tokio::test]
async fn session_notifier_writes_one_line_to_the_latest_session_inbox() {
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
        rocket: 0,
        ready: true,
        updated_at: "2026-09-28T10:00:00Z".into(),
    };
    let events = diff(&[], &[snap]);
    let ready = events
        .iter()
        .find(|e| e.kind == PrEventKind::Ready)
        .unwrap();
    notify(ready);

    let received = tokio::task::spawn_blocking(move || {
        let (mut conn, _) = new_listener.accept().unwrap();
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
        .starts_with("rocky: o/r #7 확인·머지해도 된다"));
    // 옛 세션에는 보내지 않는다 — 한 곳에만.
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(old_listener.accept().is_err(), "가장 최근 세션 하나에만");

    // 받는 이가 사라지면(소켓 없음) 등록을 걷는다.
    drop(old_listener);
    std::fs::remove_file(&new_sock).unwrap();
    notify(ready);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!f.state.inboxes().iter().any(|r| r.session_id == "new"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Codex 지적 회귀 — 가장 최근 세션이 이미 끝났으면(소켓 없음) 그다음 세션이 받는다. 전이는 한 번만
/// 나므로 여기서 놓치면 살아 있는 세션은 영영 모른다.
#[cfg(unix)]
#[tokio::test]
async fn session_notifier_falls_back_when_the_newest_session_is_gone() {
    use rockyd::prwatch::session_notifier;
    use std::io::Read;
    use std::os::unix::net::UnixListener;

    let f = fx();
    f.store.ensure_board("rocky", None, "tester").unwrap();
    f.store.set_board_repo("rocky", "o/r", "tester").unwrap();
    let dir = std::path::PathBuf::from(format!("/tmp/cc-socks-rockyfb-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let live = dir.join("300.sock");
    let gone = dir.join("400.sock"); // 만들지 않는다 — 세션이 끝났다
    let listener = UnixListener::bind(&live).unwrap();
    for (id, sock) in [("live", &live), ("gone", &gone)] {
        let (status, _) = post(
            &f.state,
            "/api/sessions/inbox",
            json!({ "sessionId": id, "socket": sock.to_str().unwrap(), "cwd": "/w/rocky" }),
        )
        .await;
        assert_eq!(status, 204);
        tokio::time::sleep(Duration::from_millis(1100)).await;
    }
    let conflict = rocky_core::prwatch::PrEvent {
        kind: PrEventKind::Conflict,
        repo: "o/r".into(),
        number: 8,
        title: "PR 8".into(),
        url: "https://github.com/o/r/pull/8".into(),
    };
    session_notifier(f.state.clone())(&conflict);
    let received = tokio::task::spawn_blocking(move || {
        let (mut conn, _) = listener.accept().unwrap();
        let mut text = String::new();
        conn.read_to_string(&mut text).unwrap();
        text
    })
    .await
    .unwrap();
    assert!(received.contains("#8 충돌"));
    tokio::time::sleep(Duration::from_millis(100)).await;
    let ids: Vec<String> = f
        .state
        .inboxes()
        .into_iter()
        .map(|r| r.session_id)
        .collect();
    assert!(
        !ids.contains(&"gone".to_string()),
        "끝난 세션의 등록은 걷는다"
    );
    assert!(ids.contains(&"live".to_string()));
    let _ = std::fs::remove_dir_all(&dir);
}

/// 리뷰 도착 — 그 레포 보드의 autoResolve 가 켜졌을 때만 세션에 resolve-reviews 를 시킨다. 꺼진 보드는 조용하다.
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
    let review = rocky_core::prwatch::PrEvent {
        kind: PrEventKind::Review,
        repo: "o/r".into(),
        number: 9,
        title: "PR 9".into(),
        url: "https://github.com/o/r/pull/9".into(),
    };
    // 꺼진 보드(기본) — 아무것도 안 간다.
    session_notifier(f.state.clone())(&review);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(listener.accept().is_err(), "autoResolve 가 꺼진 레포");
    // 그 레포의 세션이 자기 보드를 켰다 — resolve-reviews 를 시킨다.
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
    assert!(received.contains("/rocky:resolve-reviews 9"));
    let _ = std::fs::remove_dir_all(&dir);
}
