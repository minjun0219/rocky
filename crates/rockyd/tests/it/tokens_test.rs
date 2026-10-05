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
