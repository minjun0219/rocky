//! PR 감시 잡 — 가짜 gh 러너로 한 tick: 스냅숏 저장·전이·알림·health·`/api/prs`.

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

/// 레포별 응답을 정해 두는 가짜 `gh` — 호출 argv 도 남긴다.
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
            let body = responses.lock().unwrap().get(&name).cloned();
            match body {
                Some(data) => CmdOutput {
                    code: 0,
                    stdout: json!({ "data": data }).to_string(),
                    stderr: String::new(),
                },
                None => CmdOutput::failure("gh: HTTP 404: Not Found"),
            }
        })
    })
}

fn capture() -> (Notifier, Arc<Mutex<Vec<(String, String)>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = seen.clone();
    (
        Arc::new(move |title, body| sink.lock().unwrap().push((title, body))),
        seen,
    )
}

fn data(prs: Vec<Value>) -> Value {
    json!({
        "viewer": { "login": "minjun0219" },
        "repository": { "defaultBranchRef": { "name": "main" }, "pullRequests": { "nodes": prs } }
    })
}

#[tokio::test]
async fn a_tick_queries_each_watched_repo_and_notifies_only_ready_and_conflict() {
    let f = fx();
    f.store.ensure_board("rocky", None, "tester").unwrap();
    f.store.set_board_repo("rocky", "o/r", "tester").unwrap();
    f.store.ensure_board("norepo", None, "tester").unwrap();
    let responses = Arc::new(Mutex::new(json!({
        "r": data(vec![pr(1, "OPEN", "SUCCESS", "CLEAN", "main"), pr(2, "OPEN", "PENDING", "CLEAN", "main")])
    })));
    let calls = Arc::new(Mutex::new(Vec::new()));
    let runner = fake_gh(responses.clone(), calls.clone());
    let (notifier, seen) = capture();

    let events = tick(&f.state, &runner, &notifier, true).await;
    assert_eq!(
        calls.lock().unwrap().len(),
        1,
        "repo 있는 보드만, 레포당 한 번"
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
        "r": data(vec![pr(1, "MERGED", "SUCCESS", "UNKNOWN", "main"), pr(2, "OPEN", "SUCCESS", "DIRTY", "main")])
    });
    let events = tick(&f.state, &runner, &notifier, true).await;
    assert_eq!(
        events
            .iter()
            .map(|e| (e.number, e.kind))
            .collect::<Vec<_>>(),
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
}

#[tokio::test]
async fn a_failing_repo_is_reported_in_health_and_does_not_break_others() {
    let f = fx();
    f.store.ensure_board("a", None, "tester").unwrap();
    f.store.set_board_repo("a", "o/gone", "tester").unwrap();
    f.store.ensure_board("b", None, "tester").unwrap();
    f.store.set_board_repo("b", "o/r", "tester").unwrap();
    let responses = Arc::new(Mutex::new(
        json!({ "r": data(vec![pr(5, "OPEN", "SUCCESS", "CLEAN", "main")]) }),
    ));
    let runner = fake_gh(responses, Arc::new(Mutex::new(Vec::new())));
    let (notifier, seen) = capture();
    let events = tick(&f.state, &runner, &notifier, false).await;
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
    let events = tick(&f.state, &runner, &notifier, true).await;
    assert!(events.is_empty());
    let status = f.state.pr_watch();
    assert!(status.available && status.repos.is_empty() && status.last_tick.is_some());
    let _ = Duration::from_secs(0);
}
