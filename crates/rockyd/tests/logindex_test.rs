//! 로그 색인 라우트 — 보드의 레포 작업로그만, 할 일·검색어로 거른다.

mod common;

use std::io::Write;

use common::*;
use rocky_core::logindex::LogIndex;
use rocky_core::worklog::{default_project_key, git_common_dir};

fn write_line(path: &std::path::Path, id: &str, ts: &str, content: &str, tags: &[&str]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    let line = serde_json::json!({ "id": id, "timestamp": ts, "kind": "turn", "content": content, "tags": tags });
    writeln!(f, "{line}").unwrap();
}

#[tokio::test]
async fn worklog_route_reads_the_boards_repo_from_the_index() {
    let f = fx();
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let repo = std::fs::canonicalize(&repo).unwrap();
    f.store.ensure_board("rocky", None, "logan").unwrap();
    f.store
        .set_board_path("rocky", &repo.to_string_lossy(), "logan")
        .unwrap();
    f.store.ensure_board("bare", None, "logan").unwrap();

    // 보드 레포의 작업로그 + 다른 레포의 작업로그
    let root = tmp.path().join("worklog");
    let key = default_project_key(&repo, &git_common_dir);
    write_line(
        &root.join(&key).join("worklog.jsonl"),
        "1",
        "2026-10-02T00:00:00.000Z",
        "보드 작업",
        &["turn"],
    );
    write_line(
        &root.join(&key).join("worklog.jsonl"),
        "2",
        "2026-10-02T00:01:00.000Z",
        "할 일 작업",
        &["turn", "todo:rocky-3"],
    );
    write_line(
        &root.join("other-00000000").join("worklog.jsonl"),
        "3",
        "2026-10-02T00:02:00.000Z",
        "남의 레포",
        &["turn"],
    );
    let db = tmp.path().join("logs.db");
    LogIndex::open(&db)
        .unwrap()
        .ingest_worklog_root(&root)
        .unwrap();
    let state = rebuild(&f, |o| o.logs_db = Some(db.clone()));

    let ids = |v: &serde_json::Value| {
        v["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["id"].as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    };
    let (status, body) = get(&state, "/api/logs/worklog?board=rocky").await;
    assert_eq!(status, 200);
    assert_eq!(ids(&body), vec!["2", "1"], "보드 레포만, 최신순");
    assert_eq!(body["entries"][0]["todoRef"], "rocky-3");

    let (_, body) = get(&state, "/api/logs/worklog").await;
    assert_eq!(ids(&body), vec!["3", "2", "1"], "전체");
    let (_, body) = get(&state, "/api/logs/worklog?todo=rocky-3").await;
    assert_eq!(ids(&body), vec!["2"]);
    let (_, body) = get(&state, "/api/logs/worklog?board=rocky&q=%EB%B3%B4%EB%93%9C").await;
    assert_eq!(ids(&body), vec!["1"], "검색어");

    // path 없는 보드는 고를 레포가 없다
    let (status, body) = get(&state, "/api/logs/worklog?board=bare").await;
    assert_eq!(status, 200);
    assert_eq!(body["unlinked"], true);
    let (status, _) = get(&state, "/api/logs/worklog?board=nope").await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn stats_route_reports_the_index() {
    let f = fx();
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("worklog");
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    write_line(
        &root.join("p-1").join("worklog.jsonl"),
        "1",
        &now,
        "턴",
        &["turn", "todo:rocky-1"],
    );
    let db = tmp.path().join("logs.db");
    LogIndex::open(&db)
        .unwrap()
        .ingest_worklog_root(&root)
        .unwrap();
    let state = rebuild(&f, |o| o.logs_db = Some(db.clone()));
    let (status, body) = get(&state, "/api/logs/stats?days=7").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["worklog"]["turns"], 1);
    assert_eq!(
        body["worklog"]["byTodo"][0],
        serde_json::json!(["rocky-1", 1])
    );
    assert!(body["usage"]["unused"].is_array());
}
