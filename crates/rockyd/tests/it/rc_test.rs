//! `GET /api/rc/servers` — 가짜 러너로 프로브 순서·응답 모양을 고정한다.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use rocky_core::config::RcConfig;
use rockyd::rc::{cached_rc, probe, rc_handles};
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
        supervise: false,
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
        Some("ps 실패(종료 코드 1): 10000ms 안에 끝나지 않았다")
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
async fn unconfigured_probes_only_agy() {
    // rc 블록이 없는 기기 — claude rc 쪽(ps·lsof·claude)은 돌리지 않고, agy 줄은 설치 여부를 따른다.
    let calls = Arc::new(Mutex::new(Vec::new()));
    let runner = fake_runner(calls.clone(), true);
    let status = probe(&runner, None, "/home/u").await;
    assert!(!status.configured);
    assert!(status.servers.is_empty());
    assert_eq!(
        status.antigravity.unwrap().state.as_deref(),
        Some("running")
    );
    assert_eq!(
        *calls.lock().unwrap(),
        vec!["agy remote-control status".to_string()]
    );
    let runner = fake_runner(Arc::new(Mutex::new(Vec::new())), false);
    assert_eq!(probe(&runner, None, "/home/u").await.antigravity, None);

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

#[tokio::test]
async fn silent_lsof_failure_names_the_pids_and_exit_code() {
    let runner: Runner = Arc::new(|argv: Vec<String>, _stdin, _timeout| {
        let result = match argv[0].as_str() {
            "ps" => CmdOutput {
                code: 0,
                stdout: PS.to_string(),
                stderr: String::new(),
            },
            "lsof" => CmdOutput {
                code: 1,
                stdout: String::new(),
                stderr: String::new(),
            },
            _ => CmdOutput::failure("No such file or directory"),
        };
        Box::pin(async move { result })
    });
    let status = probe(&runner, Some(&config()), "/home/u").await;
    assert_eq!(
        status.probe_error.as_deref(),
        Some("lsof -p 100,200 실패(종료 코드 1): stderr 없음")
    );
}

/// agy 를 켜고 끄는 가짜 — 상태를 기억하고, `fail` 이면 start 가 실패한다. 실측(agy 1.2.14)처럼 끈 직후 첫
/// 조회는 launchd 의 중간값(`Daemon state = SIGTERMed`)을, 그 뒤로는 state 줄 없는 `Daemon status: not running` 을 낸다.
fn agy_runner(calls: Arc<Mutex<Vec<String>>>, fail: bool) -> Runner {
    let running = Arc::new(Mutex::new(true));
    let just_stopped = Arc::new(Mutex::new(false));
    Arc::new(move |argv: Vec<String>, _stdin, _timeout| {
        calls.lock().unwrap().push(argv.join(" "));
        let mut up = running.lock().unwrap();
        let mut fresh_stop = just_stopped.lock().unwrap();
        let ok = |stdout: String| CmdOutput {
            code: 0,
            stdout,
            stderr: String::new(),
        };
        let result = match argv.get(2).map(String::as_str) {
            Some("start") if fail => CmdOutput {
                code: 1,
                stdout: String::new(),
                stderr: "not logged in".into(),
            },
            Some("start") => {
                *up = true;
                ok(String::new())
            }
            Some("stop") => {
                *up = false;
                *fresh_stop = true;
                ok(String::new())
            }
            _ => {
                let state = if *up {
                    "Daemon state = running"
                } else if std::mem::take(&mut *fresh_stop) {
                    "Daemon state = SIGTERMed"
                } else {
                    "Daemon status: not running"
                };
                ok(format!("{state}\nInstance name: mac-1 (x)\n"))
            }
        };
        Box::pin(async move { result })
    })
}

#[tokio::test]
async fn agy_stop_and_start_refresh_the_cached_status() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let f = fx();
    let (status, control) = rc_handles(
        agy_runner(calls.clone(), false),
        None,
        "/home/u".into(),
        Duration::from_secs(600),
    );
    let provider = status.clone();
    let state = rebuild(&f, move |o| {
        o.rc = Some(status);
        o.agy_control = Some(control);
    });
    let (_, body) = get(&state, "/api/rc/servers").await;
    assert_eq!(body["antigravity"]["state"], "running");

    let (code, body) = post(&state, "/api/rc/antigravity/stop", json!({})).await;
    assert_eq!(code, 200, "{body}");
    // 끈 직후의 중간값(SIGTERMed)을 내지 않고 자리 잡은 값(state 줄 없음)까지 기다린다.
    assert_eq!(body["antigravity"]["state"], json!(null), "{body}");
    // TTL 이 길어도 캐시가 새 값이다 — 끈 직후 화면이 5초 동안 옛 값을 보이지 않게.
    assert_eq!(provider().await.antigravity.unwrap().state, None);
    let (code, body) = post(&state, "/api/rc/antigravity/start", json!({})).await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body["antigravity"]["state"], "running");
    assert_eq!(
        calls
            .lock()
            .unwrap()
            .iter()
            .filter(|c| !c.ends_with("status"))
            .cloned()
            .collect::<Vec<_>>(),
        vec!["agy remote-control stop", "agy remote-control start"]
    );
}

#[tokio::test]
async fn agy_control_is_local_only_and_named() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let f = fx();
    let (status, control) = rc_handles(
        agy_runner(calls.clone(), false),
        None,
        "/home/u".into(),
        Duration::ZERO,
    );
    let state = rebuild(&f, move |o| {
        o.rc = Some(status);
        o.agy_control = Some(control);
    });
    // 프록시를 거친(노출된) 요청은 이 기계의 원격 접속 데몬을 바꾸지 못한다.
    let (code, _) = call(
        &state,
        "POST",
        "/api/rc/antigravity/stop",
        Some(json!({})),
        ReqOptions {
            headers: vec![("x-forwarded-for", "203.0.113.7")],
            ..ReqOptions::default()
        },
    )
    .await;
    assert_eq!(code, 403);
    // start·stop 밖의 하위 명령은 넘기지 않는다.
    let (code, _) = post(&state, "/api/rc/antigravity/serve", json!({})).await;
    assert_eq!(code, 404);
    assert!(calls.lock().unwrap().is_empty(), "아무 명령도 돌지 않았다");

    // 손잡이가 없는 서버는 404.
    let (code, _) = post(&f.state, "/api/rc/antigravity/start", json!({})).await;
    assert_eq!(code, 404);
}

#[tokio::test]
async fn failed_agy_start_says_what_failed() {
    let f = fx();
    let (status, control) = rc_handles(
        agy_runner(Arc::new(Mutex::new(Vec::new())), true),
        None,
        "/home/u".into(),
        Duration::ZERO,
    );
    let state = rebuild(&f, move |o| {
        o.rc = Some(status);
        o.agy_control = Some(control);
    });
    let (code, body) = post(&state, "/api/rc/antigravity/start", json!({})).await;
    assert_eq!(code, 502);
    assert!(
        body.to_string()
            .contains("agy remote-control start 실패(종료 코드 1): not logged in"),
        "{body}"
    );
}
