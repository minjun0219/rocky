//! `POST /api/rc/reload`(`rocky rc reload` · `rocky rc targets add|rm`) — 떠 있는 데몬이 rocky.json 의 대상 목록을 다시
//! 읽는다. 현황 캐시와 기동기가 같은 설정을 보는지, 못 읽은 파일 · 기능 스위치는 그대로 두는지 고정한다.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use rocky_core::config::read_rc_block;
use rockyd::rc::{cached_rc, RcCommand, RcController, RcRefusal, RcSettings};
use rockyd::runner::{CmdOutput, Runner};
use rockyd::server::ServerState;
use serde_json::{json, Value};

use crate::common::*;

/// `repo-a` 에만 서버가 떠 있는 기기.
fn runner() -> Runner {
    Arc::new(|argv: Vec<String>, _stdin, _timeout| {
        let out = match argv[0].as_str() {
            "ps" => "    1     0 30-00:00:00 /sbin/launchd\n  100     1    10:00 claude rc --name repo-a\n",
            "lsof" => "p100\nfcwd\nn/w/repo-a\n",
            "claude" => r#"{"loggedIn": true}"#,
            _ => return Box::pin(async { CmdOutput::failure("없음") }),
        };
        Box::pin(async move {
            CmdOutput {
                code: 0,
                stdout: out.to_string(),
                stderr: String::new(),
            }
        })
    })
}

fn write(path: &Path, rc: Value) {
    std::fs::write(path, json!({ "todo": {}, "rc": rc }).to_string()).unwrap();
}

struct Fixture {
    f: Fx,
    path: std::path::PathBuf,
    settings: RcSettings,
    control: Arc<RcController>,
    state: Arc<ServerState>,
}

/// 기동 때의 설정 파일(`rc`)로 데몬을 꾸린다 — 현황은 TTL 1분(다시 읽기가 캐시를 넘기는지 보려고), 기동기 · 라우트는 같은 설정.
fn fixture(rc: Value) -> Fixture {
    let f = fx();
    let path = f._dir.path().join("rocky.json");
    write(&path, rc);
    let settings = RcSettings::new(
        read_rc_block(&path).unwrap(),
        path.clone(),
        "/home/u".into(),
    );
    let control = Arc::new(RcController::new(
        settings.clone(),
        "/home/u".into(),
        f._dir.path().join("rc"),
        runner(),
        rockyd::rc::default_ops(),
    ));
    let state = rebuild(&f, {
        let (settings, control) = (settings.clone(), control.clone());
        move |o| {
            o.rc = Some(cached_rc(
                runner(),
                settings.clone(),
                "/home/u".into(),
                Duration::from_secs(60),
            ));
            o.rc_control = Some(control);
            o.rc_settings = Some(settings);
        }
    });
    Fixture {
        f,
        path,
        settings,
        control,
        state,
    }
}

async fn labels(state: &Arc<ServerState>) -> Vec<String> {
    let (status, body) = get(state, "/api/rc/servers").await;
    assert_eq!(status, 200, "{body}");
    body["servers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["label"].as_str().unwrap().to_string())
        .collect()
}

fn events(fx: &Fixture) -> Vec<Value> {
    std::fs::read_to_string(fx.f._dir.path().join("rc/events.jsonl"))
        .unwrap_or_default()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn base() -> Value {
    json!({ "root": "/w", "pinned": ["repo-a"], "targets": ["repo-b"] })
}

#[tokio::test]
async fn a_target_added_to_the_file_is_startable_after_reload() {
    let fx = fixture(base());
    assert_eq!(labels(&fx.state).await, ["repo-a", "repo-b"]);
    assert!(matches!(
        fx.control.begin("ogpeek", RcCommand::Start),
        Err(RcRefusal::NotFound(m)) if m == "rc 대상이 아니다: ogpeek"
    ));

    write(
        &fx.path,
        json!({ "root": "/w", "pinned": ["repo-a"], "targets": ["repo-b", "ogpeek"] }),
    );
    // 다시 읽기 전에는 기동 때 목록 그대로다(현황은 캐시 안).
    assert_eq!(labels(&fx.state).await, ["repo-a", "repo-b"]);

    let (status, body) = post(&fx.state, "/api/rc/reload", json!({})).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        body,
        json!({
            "active": true, "supervise": false, "applied": true, "added": ["ogpeek"], "removed": [],
            "pinned": [], "unpinned": [], "restartNeeded": [],
            "path": fx.path.to_string_lossy(),
        })
    );
    // 현황은 TTL 안이어도 새로 잰다 — 막 더한 대상이 바로 보인다.
    assert_eq!(labels(&fx.state).await, ["repo-a", "repo-b", "ogpeek"]);
    let target = fx
        .control
        .begin("ogpeek", RcCommand::Start)
        .expect("다시 읽은 목록으로 받는다");
    assert_eq!(target.dir, "/w/ogpeek");

    let reloads: Vec<Value> = events(&fx)
        .into_iter()
        .filter(|e| e["event"] == "reload")
        .collect();
    assert_eq!(reloads.len(), 1, "다시 읽기는 이벤트로 남는다");
    assert_eq!(reloads[0]["fields"]["added"], json!(["ogpeek"]));
}

#[tokio::test]
async fn a_removed_target_is_refused_after_reload() {
    let fx = fixture(base());
    write(&fx.path, json!({ "root": "/w", "pinned": ["repo-a"] }));
    let (status, body) = post(&fx.state, "/api/rc/reload", json!({})).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["removed"], json!(["repo-b"]));
    assert_eq!(labels(&fx.state).await, ["repo-a"]);
    assert!(matches!(
        fx.control.begin("repo-b", RcCommand::Start),
        Err(RcRefusal::NotFound(_))
    ));
}

#[tokio::test]
async fn a_broken_file_keeps_the_running_list() {
    let fx = fixture(base());
    std::fs::write(&fx.path, r#"{ "rc": { "targets": ["repo-b", "#).unwrap();
    let (status, body) = post(&fx.state, "/api/rc/reload", json!({})).await;
    assert_eq!(status, 422);
    let err = body["error"].as_str().unwrap();
    assert!(err.starts_with("설정 파일이 JSON이 아니다("), "{err}");
    assert!(err.contains(&fx.path.display().to_string()), "{err}");

    std::fs::remove_file(&fx.path).unwrap();
    let (status, body) = post(&fx.state, "/api/rc/reload", json!({})).await;
    assert_eq!(status, 422);
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .starts_with("설정 파일을 읽지 못했다("),
        "{body}"
    );

    assert_eq!(labels(&fx.state).await, ["repo-a", "repo-b"]);
    assert!(fx.control.begin("repo-b", RcCommand::Start).is_ok());
    assert!(
        events(&fx).iter().all(|e| e["event"] != "reload"),
        "실패한 다시 읽기는 남기지 않는다"
    );
}

#[tokio::test]
async fn switches_wait_for_a_daemon_restart() {
    let fx = fixture(base());
    write(
        &fx.path,
        json!({ "root": "/w", "pinned": ["repo-a"], "targets": ["repo-b"],
                "supervise": true, "nightly": {} }),
    );
    let (status, body) = post(&fx.state, "/api/rc/reload", json!({})).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["applied"], false);
    assert_eq!(body["restartNeeded"], json!(["supervise", "nightly"]));
    let now = fx.settings.get().unwrap();
    assert!(
        !now.supervise && now.nightly.is_none(),
        "감시 · 야간은 기동 때 값"
    );

    // 끄는 것도 재시작 몫이다 — 그동안은 하던 목록으로 돈다.
    write(
        &fx.path,
        json!({ "enabled": false, "root": "/w", "targets": ["other"] }),
    );
    let (_, body) = post(&fx.state, "/api/rc/reload", json!({})).await;
    assert_eq!(
        (&body["active"], &body["restartNeeded"]),
        (&json!(true), &json!(["enabled"]))
    );
    assert_eq!(labels(&fx.state).await, ["repo-a", "repo-b"]);
}

#[tokio::test]
async fn rc_off_at_startup_asks_for_a_restart() {
    let f = fx();
    let path = f._dir.path().join("rocky.json");
    write(&path, base());
    let settings = RcSettings::new(None, path.clone(), "/home/u".into());
    let state = rebuild(&f, {
        let settings = settings.clone();
        move |o| o.rc_settings = Some(settings)
    });
    let (status, body) = post(&state, "/api/rc/reload", json!({})).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        (&body["active"], &body["applied"], &body["restartNeeded"]),
        (&json!(false), &json!(false), &json!(["enabled"]))
    );
    assert!(
        settings.get().is_none(),
        "기동기가 없는 데몬은 rc 를 켜지 않는다"
    );
}

#[tokio::test]
async fn reload_is_local_only() {
    let fx = fixture(base());
    for options in [
        ReqOptions {
            headers: vec![("x-forwarded-for", "203.0.113.7")],
            ..ReqOptions::default()
        },
        ReqOptions {
            peer: Some("100.64.0.1"),
            ..ReqOptions::default()
        },
    ] {
        let (status, _) = call(
            &fx.state,
            "POST",
            "/api/rc/reload",
            Some(json!({})),
            options,
        )
        .await;
        assert_eq!(status, 403);
    }
    // 설정이 연결되지 않은 데몬(테스트 픽스처 · 옛 배선).
    let (status, body) = post(&fx.f.state, "/api/rc/reload", json!({})).await;
    assert_eq!(status, 404);
    assert_eq!(body["error"], "rc 설정이 이 데몬에 연결돼 있지 않다");
}
