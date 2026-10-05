//! `GET /api/rc/servers` — 가짜 러너로 프로브 순서·응답 모양을 고정한다.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use rocky_core::config::RcConfig;
use rockyd::rc::{cached_rc, probe};
use rockyd::runner::{CmdOutput, Runner};
use serde_json::json;

use crate::common::*;

const PS: &str = "\
    1     0 30-00:00:00 /sbin/launchd
  100     1    10:00 claude rc --name repo-a
  101   100    05:00 /x/claude --print --sdk-url https://api.example.com/v1/code/sessions/cse_1
  200     1 1-00:00:00 claude rc --name old-name
  300   999    00:01 zsh -c claude rc --name repo-b
";

fn fake_runner(calls: Arc<Mutex<Vec<String>>>, agy_installed: bool) -> Runner {
    Arc::new(move |argv: Vec<String>, _stdin, _timeout| {
        calls.lock().unwrap().push(argv.join(" "));
        let out = |code: i32, stdout: &str| CmdOutput {
            code,
            stdout: stdout.to_string(),
            stderr: String::new(),
        };
        let result = match argv[0].as_str() {
            "ps" => out(0, PS),
            "lsof" => out(0, "p100\nfcwd\nn/w/repo-a\np200\nfcwd\nn/w/old-name\n"),
            "claude" => out(0, r#"{"loggedIn": true}"#),
            "agy" if agy_installed => out(
                0,
                "Daemon state = running\nDaemon pid = 7\nInstance name: mac-1 (x)\n",
            ),
            _ => CmdOutput::failure("No such file or directory"),
        };
        // 한 번 양보한다 — 겹친 요청이 실제로 같은 순간에 프로브 안에 있게.
        Box::pin(async move {
            tokio::task::yield_now().await;
            result
        })
    })
}

fn config() -> RcConfig {
    RcConfig {
        root: Some("/w".into()),
        pinned: vec!["repo-a".into()],
        targets: vec!["repo-b".into()],
    }
}

#[tokio::test]
async fn route_reports_targets_strays_auth_and_agy() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let f = fx();
    let runner = fake_runner(calls.clone(), true);
    let state = rebuild(&f, move |o| {
        o.rc = Some(cached_rc(
            runner,
            Some(config()),
            "/home/u".into(),
            Duration::ZERO,
        ));
    });
    let (status, body) = get(&state, "/api/rc/servers").await;
    assert_eq!(status, 200);
    assert_eq!(
        body,
        json!({
            "configured": true,
            "servers": [
                {"label": "repo-a", "dir": "/w/repo-a", "pinned": true, "running": true,
                 "pid": 100, "uptimeSecs": 600, "sessions": 1},
                {"label": "repo-b", "dir": "/w/repo-b", "pinned": false, "running": false, "sessions": 0}
            ],
            "strays": [
                {"label": "old-name", "dir": "/w/old-name", "pid": 200, "uptimeSecs": 86400, "sessions": 0}
            ],
            "auth": "in",
            "antigravity": {"state": "running", "pid": 7, "instance": "mac-1"}
        })
    );
    // lsof 는 서버 pid 를 모아 한 번만 — 셸 래퍼(300)는 서버가 아니다.
    let calls = calls.lock().unwrap();
    assert_eq!(
        calls
            .iter()
            .filter(|c| c.starts_with("lsof"))
            .cloned()
            .collect::<Vec<_>>(),
        vec!["lsof -a -d cwd -p 100,200 -F pn".to_string()]
    );
}

#[tokio::test]
async fn failed_ps_is_reported_not_read_as_stopped() {
    let runner: Runner = Arc::new(|argv: Vec<String>, _stdin, _timeout| {
        let result = if argv[0] == "ps" {
            CmdOutput::failure("10000ms 안에 끝나지 않았다")
        } else {
            CmdOutput::failure("No such file or directory")
        };
        Box::pin(async move { result })
    });
    let status = probe(&runner, Some(&config()), "/home/u").await;
    assert_eq!(
        status.probe_error.as_deref(),
        Some("ps 실패: 10000ms 안에 끝나지 않았다")
    );
    assert!(status.servers.iter().all(|s| !s.running));
}

#[tokio::test]
async fn missing_agy_is_null() {
    let runner = fake_runner(Arc::new(Mutex::new(Vec::new())), false);
    let status = probe(&runner, Some(&config()), "/home/u").await;
    assert_eq!(status.antigravity, None);
}

#[tokio::test]
async fn unconfigured_runs_nothing() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let runner = fake_runner(calls.clone(), true);
    let status = probe(&runner, None, "/home/u").await;
    assert!(!status.configured);
    assert!(calls.lock().unwrap().is_empty());

    // 주입이 없는 서버도 같은 모양을 낸다.
    let f = fx();
    let (code, body) = get(&f.state, "/api/rc/servers").await;
    assert_eq!(code, 200);
    assert_eq!(body["configured"], json!(false));
}

#[tokio::test]
async fn cache_reuses_within_ttl() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let provider = cached_rc(
        fake_runner(calls.clone(), true),
        Some(config()),
        "/home/u".into(),
        Duration::from_secs(60),
    );
    provider().await;
    provider().await;
    assert_eq!(
        calls
            .lock()
            .unwrap()
            .iter()
            .filter(|c| c.starts_with("ps"))
            .count(),
        1
    );
}

#[tokio::test]
async fn concurrent_misses_share_one_probe() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let provider = cached_rc(
        fake_runner(calls.clone(), true),
        Some(config()),
        "/home/u".into(),
        Duration::from_secs(60),
    );
    let (a, b, c) = tokio::join!(provider(), provider(), provider());
    assert_eq!(a, b);
    assert_eq!(b, c);
    assert_eq!(
        calls
            .lock()
            .unwrap()
            .iter()
            .filter(|c| c.starts_with("ps"))
            .count(),
        1
    );
}
