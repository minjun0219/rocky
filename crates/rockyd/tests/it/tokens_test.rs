//! 토큰 색인 라우트 — 트랜스크립트를 색인한 logs.db 를 요약·세션 상세·현재 세션으로 읽는다.

use std::io::Write;

use crate::common::*;
use rocky_core::logindex::LogIndex;
use serde_json::json;

/// 세션 하나(`/repo/app` 에서 두 턴)를 색인한 logs.db 경로.
pub fn indexed_logs(tmp: &std::path::Path) -> std::path::PathBuf {
    let file = tmp.join("projects").join("-repo-app").join("s-1.jsonl");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    let mut f = std::fs::File::create(&file).unwrap();
    let prompt = |uuid: &str, ts: &str| {
        json!({ "type": "user", "uuid": uuid, "sessionId": "s-1", "timestamp": ts, "cwd": "/repo/app",
                "gitBranch": "main", "message": { "role": "user", "content": "해 줘" } })
    };
    let reply = |id: &str, ts: &str, model: &str, effort: &str, out: u64| {
        json!({ "type": "assistant", "uuid": format!("a-{id}"), "sessionId": "s-1", "timestamp": ts,
                "cwd": "/repo/app", "gitBranch": "main", "effort": effort,
                "message": { "id": id, "model": model, "stop_reason": "end_turn",
                             "content": [{ "type": "text", "text": "ok" }],
                             "usage": { "input_tokens": 1, "output_tokens": out,
                                        "cache_read_input_tokens": 0, "cache_creation_input_tokens": 0 } } })
    };
    for line in [
        prompt("p1", "2099-01-01T00:00:00Z"),
        reply("m1", "2099-01-01T00:00:01Z", "claude-opus-5-5", "high", 100),
        prompt("p2", "2099-01-01T00:01:00Z"),
        reply(
            "m2",
            "2099-01-01T00:01:01Z",
            "claude-opus-5-5",
            "medium",
            40,
        ),
    ] {
        writeln!(f, "{line}").unwrap();
    }
    let db = tmp.join("logs.db");
    LogIndex::open(&db)
        .unwrap()
        .ingest_transcripts(&tmp.join("projects"))
        .unwrap();
    db
}

#[tokio::test]
async fn summary_groups_by_model_and_effort() {
    let f = fx();
    let tmp = tempfile::tempdir().unwrap();
    let db = indexed_logs(tmp.path());
    let state = rebuild(&f, |o| o.logs_db = Some(db.clone()));

    let (status, body) = get(&state, "/api/tokens/summary?from=2099-01-01").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["groupBy"], "model,effort");
    let rows = body["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["effort"], "high");
    assert_eq!(rows[0]["outputTokens"], 100);
    assert_eq!(rows[0]["turns"], 1);

    let (_, body) = get(
        &state,
        "/api/tokens/summary?from=2099-01-01&groupBy=session",
    )
    .await;
    assert_eq!(body["rows"][0]["sessionId"], "s-1");
    assert_eq!(body["rows"][0]["turns"], 2);

    let (status, _) = get(&state, "/api/tokens/summary?groupBy=planet").await;
    assert_eq!(status, 400);
}

#[tokio::test]
async fn session_and_current_routes_return_turns_and_effort_changes() {
    let f = fx();
    let tmp = tempfile::tempdir().unwrap();
    let db = indexed_logs(tmp.path());
    let state = rebuild(&f, |o| o.logs_db = Some(db.clone()));

    let (status, body) = get(&state, "/api/tokens/sessions/s-1").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["session"]["cwd"], "/repo/app");
    assert_eq!(body["turns"].as_array().unwrap().len(), 2);
    assert_eq!(body["effortChanges"][0]["from"], "high");
    assert_eq!(body["effortChanges"][0]["to"], "medium");

    let (status, body) = get(&state, "/api/tokens/current?cwd=/repo").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["session"]["sessionId"], "s-1");

    let (status, _) = get(&state, "/api/tokens/sessions/nope").await;
    assert_eq!(status, 404);
    let (status, _) = get(&state, "/api/tokens/current?cwd=/elsewhere").await;
    assert_eq!(status, 404);
    let (status, _) = get(&state, "/api/tokens/current").await;
    assert_eq!(status, 400);
}

#[tokio::test]
async fn token_routes_are_empty_without_an_index() {
    let f = fx();
    let (status, body) = get(&f.state, "/api/tokens/summary").await;
    assert_eq!(status, 200);
    assert_eq!(body["rows"], json!([]));
}

/// `s-2` 세션(`/repo/chat`)에 짧은 턴 `n` 개를 덧붙인다 — 도구 없음, 출력 200.
fn append_turns(projects: &std::path::Path, start: usize, n: usize, effort: &str) {
    let file = projects.join("-repo-chat").join("s-2.jsonl");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file)
        .unwrap();
    for i in start..start + n {
        let ts = |s: u32| format!("2099-02-01T{:02}:{:02}:{s:02}Z", i / 60, i % 60);
        let p = json!({ "type": "user", "uuid": format!("p{i}"), "sessionId": "s-2", "timestamp": ts(0),
                        "cwd": "/repo/chat", "message": { "role": "user", "content": "질문" } });
        let a = json!({ "type": "assistant", "uuid": format!("a{i}"), "sessionId": "s-2", "timestamp": ts(1),
                        "cwd": "/repo/chat", "effort": effort,
                        "message": { "id": format!("m{i}"), "model": "claude-opus-5-5", "stop_reason": "end_turn",
                                     "content": [{ "type": "text", "text": "답" }],
                                     "usage": { "input_tokens": 1, "output_tokens": 200 } } });
        writeln!(f, "{p}\n{a}").unwrap();
    }
}

#[tokio::test]
async fn recommendation_route_and_current_session_carry_suggestions() {
    let f = fx();
    let tmp = tempfile::tempdir().unwrap();
    let projects = tmp.path().join("projects");
    append_turns(&projects, 0, 6, "xhigh");
    let db = tmp.path().join("logs.db");
    LogIndex::open(&db)
        .unwrap()
        .ingest_transcripts(&projects)
        .unwrap();
    let state = rebuild(&f, |o| o.logs_db = Some(db.clone()));

    let (status, body) = get(&state, "/api/tokens/recommendation?sessionId=s-2").await;
    assert_eq!(status, 200, "{body}");
    let rules: Vec<&str> = body["suggestions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["rule"].as_str().unwrap())
        .collect();
    assert_eq!(rules, vec!["lower-effort", "switch-to-sonnet"]);
    assert_eq!(body["evidence"]["turns"], 6);
    assert_eq!(body["evidence"]["avgOutputTokens"], 200);

    let (_, by_cwd) = get(&state, "/api/tokens/recommendation?cwd=/repo/chat").await;
    assert_eq!(by_cwd["sessionId"], "s-2");
    let (_, current) = get(&state, "/api/tokens/current?cwd=/repo/chat").await;
    assert_eq!(
        current["recommendation"]["suggestions"][0]["rule"],
        "lower-effort"
    );
    assert_eq!(current["turns"].as_array().unwrap().len(), 6);

    let (status, _) = get(&state, "/api/tokens/recommendation").await;
    assert_eq!(status, 400);
    let (status, _) = get(&state, "/api/tokens/recommendation?sessionId=nope").await;
    assert_eq!(status, 404);
}

#[test]
fn feed_seeds_on_first_pass_and_pushes_only_rule_changes() {
    use rockyd::logindex::RecommendationFeed;
    let tmp = tempfile::tempdir().unwrap();
    let projects = tmp.path().join("projects");
    let (tx, mut rx) = tokio::sync::broadcast::channel::<String>(8);
    let mut feed = RecommendationFeed::new(tx, Default::default());
    let mut index = LogIndex::open(&tmp.path().join("logs.db")).unwrap();

    // 첫 바퀴 — 과거 가져오기: 추천이 있어도 기준선만.
    append_turns(&projects, 0, 6, "xhigh");
    let touched = index.ingest_transcripts(&projects).unwrap();
    assert_eq!(feed.publish(&index, &touched), 0);

    // 턴이 늘어도 낸 규칙이 같으면 다시 알리지 않는다.
    append_turns(&projects, 6, 1, "xhigh");
    let touched = index.ingest_transcripts(&projects).unwrap();
    assert_eq!(feed.publish(&index, &touched), 0);

    // effort 를 medium 으로 내리면 lower-effort 가 빠진다 — 바뀌었으니 민다.
    append_turns(&projects, 7, 1, "medium");
    let touched = index.ingest_transcripts(&projects).unwrap();
    assert_eq!(feed.publish(&index, &touched), 1);
    let pushed: serde_json::Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
    assert_eq!(pushed["sessionId"], "s-2");
    assert_eq!(pushed["suggestions"][0]["rule"], "switch-to-sonnet");
}

#[tokio::test]
async fn token_events_stream_uses_a_named_event() {
    let f = fx();
    let request = axum::http::Request::builder()
        .method("GET")
        .uri("/api/tokens/events")
        .body(axum::body::Body::empty())
        .unwrap();
    let response = rockyd::server::handle_api(&f.state, request, Some("127.0.0.1".into())).await;
    assert_eq!(response.headers()["content-type"], "text/event-stream");
    let _ = f
        .state
        .token_events
        .send("{\"sessionId\":\"s\"}".to_string());
    let mut body = response.into_body().into_data_stream();
    use tokio_stream::StreamExt;
    let mut seen = String::new();
    while !seen.contains("sessionId") {
        match tokio::time::timeout(std::time::Duration::from_millis(500), body.next()).await {
            Ok(Some(Ok(chunk))) => seen.push_str(&String::from_utf8_lossy(&chunk)),
            _ => break,
        }
    }
    assert!(
        seen.contains("event: tokens.recommendation\ndata: {\"sessionId\":\"s\"}\n\n"),
        "{seen}"
    );
}
