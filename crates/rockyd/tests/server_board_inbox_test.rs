//! 보드 수집함 설정 — 어댑터 칸 목록·등록·삭제, 등록한 소스가 재기동 없이 수집함에 나오는지.

mod common;

use std::sync::{Arc, Mutex};

use common::*;
use rocky_core::config::InboxSource;
use rockyd::runner::{CmdOutput, Runner};
use serde_json::json;

const DESCRIBE: &str = r#"{"title":"보드","params":[{"flag":"--project","label":"보드","required":true},{"flag":"--filter","label":"필터","required":false}]}"#;

/// `--describe` 면 칸 목록, 아니면 받은 argv 를 기록하고 항목 하나를 낸다.
fn scripted(seen: Arc<Mutex<Vec<Vec<String>>>>) -> Runner {
    Arc::new(move |cmd, _stdin, _timeout| {
        let out = if cmd.last().map(String::as_str) == Some("--describe") {
            CmdOutput {
                code: 0,
                stdout: DESCRIBE.into(),
                stderr: String::new(),
            }
        } else {
            seen.lock().unwrap().push(cmd.clone());
            CmdOutput {
                code: 0,
                stdout: r#"{"items":[{"id":"1","title":"버그","url":"https://x/1"}]}"#.into(),
                stderr: String::new(),
            }
        };
        Box::pin(async move { out })
    })
}

fn adapter() -> InboxSource {
    InboxSource {
        name: "gh-project".into(),
        command: vec!["adapter".into()],
        timeout_ms: None,
    }
}

fn fx_board(seen: Arc<Mutex<Vec<Vec<String>>>>) -> Fx {
    let f = fx_with(|o| {
        o.gh_runner = Some(scripted(seen));
        o.inbox_adapters = vec![adapter()];
        o.inbox_sources = vec![InboxSource {
            name: "todoist".into(),
            command: vec!["adapter".into(), "--config".into()],
            timeout_ms: None,
        }];
    });
    f.state.store.ensure_board("web", None, "tester").unwrap();
    f.state.store.ensure_board("api", None, "tester").unwrap();
    f
}

#[tokio::test]
async fn adapters_list_their_describe_fields() {
    let f = fx_board(Arc::default());
    let (status, body) = get(&f.state, "/api/inbox/adapters").await;
    assert_eq!(status, 200);
    assert_eq!(body[0]["name"], "gh-project");
    assert_eq!(body[0]["params"][0]["flag"], "--project");
}

#[tokio::test]
async fn a_registered_source_runs_with_its_values_and_only_shows_on_its_board() {
    let seen: Arc<Mutex<Vec<Vec<String>>>> = Arc::default();
    let f = fx_board(seen.clone());
    let (status, created) = post(
        &f.state,
        "/api/inbox/sources",
        json!({ "board": "web", "name": "gh-bugs", "adapter": "gh-project",
                "params": { "--project": "acme/7", "--filter": "type:Bug" } }),
    )
    .await;
    assert_eq!(status, 200, "{created}");
    assert_eq!(created["board"], "web");

    // 재기동 없이 수집함에 나오고, 명령은 어댑터 argv + 칸 목록 순서의 값이다.
    let (_, inbox) = get(&f.state, "/api/inbox?board=web").await;
    let names: Vec<&str> = inbox["sources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["todoist", "gh-bugs"]);
    assert_eq!(inbox["sources"][1]["board"], "web");
    assert!(seen.lock().unwrap().contains(&vec![
        "adapter".to_string(),
        "--project".into(),
        "acme/7".into(),
        "--filter".into(),
        "type:Bug".into()
    ]));

    // 다른 보드에서는 공통 소스만.
    let (_, other) = get(&f.state, "/api/inbox?board=api").await;
    assert_eq!(other["sources"].as_array().unwrap().len(), 1);

    // 지우면 다음 조회부터 빠진다.
    let id = created["id"].as_str().unwrap();
    let (status, _) = call(
        &f.state,
        "DELETE",
        &format!("/api/inbox/sources/{id}"),
        None,
        ReqOptions::default(),
    )
    .await;
    assert_eq!(status, 200);
    let (_, after) = get(&f.state, "/api/inbox?board=web").await;
    assert_eq!(after["sources"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn registration_rejects_unknown_flags_missing_required_and_name_clashes() {
    let f = fx_board(Arc::default());
    for (body, reason) in [
        (
            json!({ "board": "web", "name": "a", "adapter": "gh-project",
                    "params": { "--project": "acme/7", "--from": "/etc/passwd" } }),
            "받지 않는 칸",
        ),
        (
            json!({ "board": "web", "name": "a", "adapter": "gh-project", "params": {} }),
            "비었다",
        ),
        (
            json!({ "board": "web", "name": "a", "adapter": "gh-project",
                    "params": { "--project": "--describe" } }),
            "- 로 시작",
        ),
        (
            json!({ "board": "web", "name": "todoist", "adapter": "gh-project",
                    "params": { "--project": "acme/7" } }),
            "이름이 겹친다",
        ),
        (
            json!({ "board": "web", "name": "a", "adapter": "nope",
                    "params": { "--project": "acme/7" } }),
            "없는 어댑터",
        ),
    ] {
        let (status, error) = post(&f.state, "/api/inbox/sources", body).await;
        assert_eq!(status, 400, "{error}");
        assert!(error.to_string().contains(reason), "{error} ∌ {reason}");
    }
}

#[tokio::test]
async fn settings_writes_are_local_only() {
    let f = fx_board(Arc::default());
    let remote = || ReqOptions {
        headers: vec![("x-forwarded-for", "10.0.0.2")],
        ..ReqOptions::default()
    };
    let (status, _) = call(
        &f.state,
        "POST",
        "/api/inbox/sources",
        Some(
            json!({ "board": "web", "name": "a", "adapter": "gh-project",
                     "params": { "--project": "acme/7" } }),
        ),
        remote(),
    )
    .await;
    assert_eq!(status, 403);
    let (status, _) = call(&f.state, "GET", "/api/inbox/adapters", None, remote()).await;
    assert_eq!(status, 403);
    let (status, _) = call(&f.state, "DELETE", "/api/inbox/sources/x", None, remote()).await;
    assert_eq!(status, 403);
    // 목록 읽기는 막지 않는다.
    let (status, _) = call(&f.state, "GET", "/api/inbox/sources", None, remote()).await;
    assert_eq!(status, 200);
}

/// 지우고 같은 이름으로 다른 값을 다시 등록하면 옛 값으로 가져온 캐시 결과가 나오면 안 된다 — 캐시 키가
/// 이름뿐이던 때의 회귀. (negative control: `inbox_exec::cache_key` 를 `source.name.clone()` 으로 되돌리면
/// 둘째 조회가 옛 argv 결과를 돌려주어 이 테스트가 실패한다 — 작성 때 확인.)
#[tokio::test]
async fn re_registering_a_name_with_new_values_does_not_serve_the_old_cache() {
    let seen: Arc<Mutex<Vec<Vec<String>>>> = Arc::default();
    let f = fx_board(seen.clone());
    let register = |project: &'static str| {
        let state = f.state.clone();
        async move {
            let (status, created) = post(
                &state,
                "/api/inbox/sources",
                json!({ "board": "web", "name": "gh-bugs", "adapter": "gh-project",
                        "params": { "--project": project } }),
            )
            .await;
            assert_eq!(status, 200, "{created}");
            created["id"].as_str().unwrap().to_string()
        }
    };
    let id = register("acme/7").await;
    get(&f.state, "/api/inbox?board=web").await;
    call(
        &f.state,
        "DELETE",
        &format!("/api/inbox/sources/{id}"),
        None,
        ReqOptions::default(),
    )
    .await;
    register("acme/9").await;
    get(&f.state, "/api/inbox?board=web").await;
    let runs = seen.lock().unwrap().clone();
    assert!(
        runs.iter().any(|argv| argv.contains(&"acme/9".to_string())),
        "새 값으로 다시 실행해야 한다: {runs:?}"
    );
}

#[tokio::test]
async fn archived_boards_and_aliases() {
    let f = fx_board(Arc::default());
    // 별칭으로 등록해도 응답은 현재 key, 별칭으로 걸러도 나온다.
    let (status, _) = patch(&f.state, "/api/boards/web", json!({ "key": "front" })).await;
    assert_eq!(status, 200);
    let (status, created) = post(
        &f.state,
        "/api/inbox/sources",
        json!({ "board": "web", "name": "gh-bugs", "adapter": "gh-project",
                "params": { "--project": "acme/7" } }),
    )
    .await;
    assert_eq!(status, 200, "{created}");
    assert_eq!(created["board"], "front");
    let (_, inbox) = get(&f.state, "/api/inbox?board=web").await;
    assert!(inbox["sources"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["name"] == "gh-bugs"));
    // 요약·statusline 처럼 보드를 모르면 공통 소스만, 보드 없이 부르면 전부.
    let (_, all) = get(&f.state, "/api/inbox").await;
    assert_eq!(all["sources"].as_array().unwrap().len(), 2);
    let (_, summary) = get(&f.state, "/api/summary?cwd=/nowhere").await;
    let from: Vec<&str> = summary["collectItems"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["source"].as_str().unwrap())
        .collect();
    assert_eq!(from, vec!["todoist"], "{summary}");
}

#[tokio::test]
async fn params_must_be_an_object() {
    let f = fx_board(Arc::default());
    let (status, error) = post(
        &f.state,
        "/api/inbox/sources",
        json!({ "board": "web", "name": "a", "adapter": "gh-project", "params": ["acme/7"] }),
    )
    .await;
    assert_eq!(status, 400);
    assert!(error.to_string().contains("객체"), "{error}");
}
