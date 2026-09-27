//! GET /api/inbox — 수집함 어댑터 실행·캐시·실패 격리.

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use common::*;
use rocky_core::config::InboxSource;
use rockyd::inbox_exec::cached_inbox;
use rockyd::runner::{default_runner, CmdOutput, Runner};

fn source(name: &str, argv0: &str) -> InboxSource {
    InboxSource {
        name: name.into(),
        command: vec![argv0.into()],
        timeout_ms: None,
    }
}

/// argv[0] 로 결과를 고르는 가짜 러너 — 호출 횟수를 센다.
fn scripted(counter: Arc<AtomicUsize>) -> Runner {
    Arc::new(move |cmd, _stdin, _timeout| {
        counter.fetch_add(1, Ordering::SeqCst);
        let out = match cmd[0].as_str() {
            "ok" => CmdOutput {
                code: 0,
                stdout: r#"{"items":[{"id":"1","title":"첫 항목","url":"https://x/1"}]}"#.into(),
                stderr: String::new(),
            },
            "fails" => CmdOutput {
                code: 1,
                stdout: String::new(),
                stderr: "token expired\nsecond line".into(),
            },
            "garbage" => CmdOutput {
                code: 0,
                stdout: "<html>".into(),
                stderr: String::new(),
            },
            "silent-fail" => CmdOutput {
                code: 3,
                stdout: String::new(),
                stderr: String::new(),
            },
            other => CmdOutput::failure(format!("unexpected {other}")),
        };
        Box::pin(async move { out })
    })
}

#[tokio::test]
async fn no_sources_is_empty_list() {
    let f = fx();
    let (status, body) = get(&f.state, "/api/inbox").await;
    assert_eq!(status, 200);
    assert_eq!(body["sources"], serde_json::json!([]));
}

#[tokio::test]
async fn failures_are_isolated_per_source_and_order_follows_config() {
    let counter = Arc::new(AtomicUsize::new(0));
    let provider = cached_inbox(
        scripted(counter.clone()),
        vec![
            source("ok", "ok"),
            source("fails", "fails"),
            source("garbage", "garbage"),
            source("silent", "silent-fail"),
        ],
        Duration::from_secs(60),
    );
    let f = fx_with(|o| o.inbox = Some(provider));
    let (status, body) = get(&f.state, "/api/inbox").await;
    assert_eq!(status, 200);
    let sources = body["sources"].as_array().unwrap();
    let names: Vec<&str> = sources
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["ok", "fails", "garbage", "silent"]);

    assert_eq!(sources[0]["available"], true);
    assert_eq!(sources[0]["items"][0]["title"], "첫 항목");
    assert!(sources[0].get("reason").is_none());
    assert!(sources[0]["fetchedAt"].as_str().unwrap().ends_with('Z'));

    assert_eq!(sources[1]["available"], false);
    assert_eq!(sources[1]["reason"], "exit 1: token expired"); // stderr 첫 줄만
    assert_eq!(sources[1]["items"], serde_json::json!([]));

    assert_eq!(sources[2]["available"], false);
    assert!(sources[2]["reason"].as_str().unwrap().contains("JSON"));

    assert_eq!(sources[3]["reason"], "exit 3");
    assert_eq!(counter.load(Ordering::SeqCst), 4);
}

#[tokio::test]
async fn cache_hits_within_ttl_and_refresh_bypasses() {
    let counter = Arc::new(AtomicUsize::new(0));
    let provider = cached_inbox(
        scripted(counter.clone()),
        vec![source("ok", "ok"), source("fails", "fails")],
        Duration::from_secs(60),
    );
    let f = fx_with(|o| o.inbox = Some(provider));
    get(&f.state, "/api/inbox").await;
    get(&f.state, "/api/inbox").await;
    // 실패한 소스도 캐시된다 — 죽은 어댑터를 매 요청마다 다시 때리지 않는다.
    assert_eq!(counter.load(Ordering::SeqCst), 2);
    let (status, body) = get(&f.state, "/api/inbox?refresh=true").await;
    assert_eq!(status, 200);
    assert_eq!(body["sources"].as_array().unwrap().len(), 2);
    assert_eq!(counter.load(Ordering::SeqCst), 4);
}

#[tokio::test]
async fn expired_cache_refetches() {
    let counter = Arc::new(AtomicUsize::new(0));
    let provider = cached_inbox(
        scripted(counter.clone()),
        vec![source("ok", "ok")],
        Duration::from_millis(1),
    );
    let f = fx_with(|o| o.inbox = Some(provider));
    get(&f.state, "/api/inbox").await;
    tokio::time::sleep(Duration::from_millis(5)).await;
    get(&f.state, "/api/inbox").await;
    assert_eq!(counter.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn timeout_is_reported_as_unavailable() {
    // 실제 프로세스: `sleep 5` 를 50ms 상한으로 — default_runner 가 죽이고 실패로 돌린다.
    let provider = cached_inbox(
        default_runner(),
        vec![InboxSource {
            name: "slow".into(),
            command: vec!["sleep".into(), "5".into()],
            timeout_ms: Some(50),
        }],
        Duration::from_secs(60),
    );
    let f = fx_with(|o| o.inbox = Some(provider));
    let (_, body) = get(&f.state, "/api/inbox").await;
    assert_eq!(body["sources"][0]["available"], false);
    assert!(body["sources"][0]["reason"]
        .as_str()
        .unwrap()
        .contains("50ms"));
}

#[tokio::test]
async fn file_adapter_runs_for_real() {
    // bridges/file/inbox.sh 를 진짜로 실행한다 — 규약의 참조 구현이 규약을 지키는지.
    let dir = tempfile::tempdir().unwrap();
    let items = dir.path().join("inbox.json");
    std::fs::write(
        &items,
        r#"{"items":[{"id":"f1","title":"파일에서 온 항목","url":"https://example/f1"}]}"#,
    )
    .unwrap();
    let script = concat!(env!("CARGO_MANIFEST_DIR"), "/../../bridges/file/inbox.sh");
    let provider = cached_inbox(
        default_runner(),
        vec![
            InboxSource {
                name: "file".into(),
                command: vec![
                    "sh".into(),
                    script.into(),
                    items.to_string_lossy().to_string(),
                ],
                timeout_ms: None,
            },
            InboxSource {
                name: "missing".into(),
                command: vec!["sh".into(), script.into(), "/no/such/inbox.json".into()],
                timeout_ms: None,
            },
        ],
        Duration::from_secs(60),
    );
    let f = fx_with(|o| o.inbox = Some(provider));
    let (_, body) = get(&f.state, "/api/inbox").await;
    assert_eq!(body["sources"][0]["available"], true);
    assert_eq!(body["sources"][0]["items"][0]["id"], "f1");
    assert_eq!(body["sources"][1]["available"], false);
    assert_eq!(
        body["sources"][1]["reason"],
        "exit 1: no such file: /no/such/inbox.json"
    );
}

#[tokio::test]
async fn remote_callers_get_exit_code_only() {
    let counter = Arc::new(AtomicUsize::new(0));
    let provider = cached_inbox(
        scripted(counter),
        vec![
            source("ok", "ok"),
            source("fails", "fails"),
            source("garbage", "garbage"),
        ],
        Duration::from_secs(60),
    );
    let f = fx_with(|o| o.inbox = Some(provider));
    // 로컬(루프백 + 프록시 헤더 없음): 상세 그대로.
    let (_, body) = get(&f.state, "/api/inbox").await;
    assert_eq!(body["sources"][1]["reason"], "exit 1: token expired");
    // 원격 peer: exit code 만. 정상 소스의 items 는 그대로.
    let (status, body) = call(
        &f.state,
        "GET",
        "/api/inbox",
        None,
        ReqOptions {
            peer: Some("100.64.0.9"),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["sources"][0]["items"][0]["title"], "첫 항목");
    assert_eq!(body["sources"][1]["reason"], "exit 1");
    assert!(!body["sources"][2]["reason"]
        .as_str()
        .unwrap()
        .contains("JSON"));
    // tailscale serve 경유(루프백이지만 프록시 헤더): 원격으로 본다.
    let (_, body) = call(
        &f.state,
        "GET",
        "/api/inbox",
        None,
        ReqOptions {
            headers: vec![("x-forwarded-for", "100.64.0.9")],
            ..Default::default()
        },
    )
    .await;
    assert_eq!(body["sources"][1]["reason"], "exit 1");
}
