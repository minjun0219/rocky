//! 로그 색인 — JSONL 을 증분으로 옮기고 다시 읽어도 중복이 없다.

use std::io::Write;
use std::path::Path;

use rocky_core::logindex::{todo_ref_from_tags, LogIndex, WorklogQuery};

fn line(id: &str, ts: &str, kind: &str, content: &str, tags: &[&str]) -> String {
    serde_json::json!({ "id": id, "timestamp": ts, "kind": kind, "content": content, "tags": tags })
        .to_string()
        + "\n"
}

fn append(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    f.write_all(text.as_bytes()).unwrap();
}

fn all(index: &LogIndex) -> Vec<String> {
    index
        .worklog(&WorklogQuery {
            limit: 100,
            ..Default::default()
        })
        .unwrap()
        .into_iter()
        .map(|e| e.id)
        .collect()
}

#[test]
fn worklog_is_ingested_incrementally_and_idempotently() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("worklog");
    let file = root.join("rocky-proj").join("worklog.jsonl");
    let mut index = LogIndex::open(&tmp.path().join("logs.db")).unwrap();

    append(
        &file,
        &line(
            "1-a",
            "2026-10-02T00:00:00.000Z",
            "turn",
            "첫 턴",
            &["turn"],
        ),
    );
    assert_eq!(index.ingest_worklog_root(&root).unwrap().worklog, 1);
    // 같은 내용을 다시 훑어도 늘지 않는다
    assert_eq!(index.ingest_worklog_root(&root).unwrap().worklog, 0);

    // 쓰는 중인 줄(끝에 \n 없음)은 다음 바퀴로 미룬다
    let second = line(
        "2-b",
        "2026-10-02T00:01:00.000Z",
        "decision",
        "결정",
        &["todo:rocky-12"],
    );
    append(&file, &second[..second.len() - 10]);
    assert_eq!(index.ingest_worklog_root(&root).unwrap().worklog, 0);
    append(&file, &second[second.len() - 10..]);
    assert_eq!(index.ingest_worklog_root(&root).unwrap().worklog, 1);
    assert_eq!(all(&index), vec!["2-b", "1-a"], "최신순");

    let held = index
        .worklog(&WorklogQuery {
            todo_ref: Some("rocky-12".into()),
            limit: 10,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(held.len(), 1);
    assert_eq!(held[0].project_key, "rocky-proj");
    assert_eq!(held[0].todo_ref.as_deref(), Some("rocky-12"));
}

/// 파일이 읽은 위치보다 작아지면(갈아엎음) 처음부터 — id 가 기본키라 겹치는 줄은 다시 들어가지 않는다.
#[test]
fn a_shrunken_file_is_read_again_from_the_start() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("worklog");
    let file = root.join("p-1").join("worklog.jsonl");
    let mut index = LogIndex::open(&tmp.path().join("logs.db")).unwrap();
    append(
        &file,
        &line("1-a", "2026-10-01T00:00:00.000Z", "turn", "a", &[]),
    );
    append(
        &file,
        &line("2-b", "2026-10-01T00:01:00.000Z", "turn", "b", &[]),
    );
    index.ingest_worklog_root(&root).unwrap();
    std::fs::write(
        &file,
        line("3-c", "2026-10-01T00:02:00.000Z", "turn", "c", &[]),
    )
    .unwrap();
    assert_eq!(index.ingest_worklog_root(&root).unwrap().worklog, 1);
    assert_eq!(all(&index), vec!["3-c", "2-b", "1-a"]);
}

#[test]
fn worklog_query_filters_by_project_kind_text_and_before() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("worklog");
    append(
        &root.join("a-1").join("worklog.jsonl"),
        &line("1", "2026-10-01T00:00:00.000Z", "turn", "100% 완료_됨", &[]),
    );
    append(
        &root.join("a-1").join("worklog.jsonl"),
        &line("2", "2026-10-01T00:01:00.000Z", "decision", "정함", &[]),
    );
    append(
        &root.join("b-2").join("worklog.jsonl"),
        &line("3", "2026-10-01T00:02:00.000Z", "turn", "다른 레포", &[]),
    );
    // 파싱 안 되는 줄은 건너뛴다
    append(&root.join("b-2").join("worklog.jsonl"), "{깨진 줄\n");
    let mut index = LogIndex::open(&tmp.path().join("logs.db")).unwrap();
    index.ingest_worklog_root(&root).unwrap();

    let q = |f: &dyn Fn(&mut WorklogQuery)| {
        let mut query = WorklogQuery {
            limit: 10,
            ..Default::default()
        };
        f(&mut query);
        index
            .worklog(&query)
            .unwrap()
            .into_iter()
            .map(|e| e.id)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        q(&|w| w.project_keys = Some(vec!["a-1".into()])),
        vec!["2", "1"]
    );
    assert_eq!(q(&|w| w.project_keys = Some(vec![])), Vec::<String>::new());
    assert_eq!(q(&|w| w.kind = Some("decision".into())), vec!["2"]);
    // LIKE 의 % · _ 는 글자 그대로
    assert_eq!(q(&|w| w.text = Some("100%".into())), vec!["1"]);
    assert_eq!(q(&|w| w.text = Some("료_".into())), vec!["1"]);
    assert_eq!(
        q(&|w| w.before = Some("2026-10-01T00:01:00.000Z".into())),
        vec!["1"]
    );
}

#[test]
fn usage_logs_are_ingested_by_file_and_offset() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("usage");
    let event = |name: &str, ok: bool| {
        serde_json::json!({ "ts": "2026-10-02T00:00:00.000Z", "source": "rest", "name": name, "ok": ok, "ms": 12 })
            .to_string()
            + "\n"
    };
    append(&dir.join("2026-10.jsonl"), &event("GET /api/todos", true));
    append(&dir.join("2026-10.jsonl"), &event("GET /api/todos", false));
    append(&dir.join("notes.txt"), "무시");
    let mut index = LogIndex::open(&tmp.path().join("logs.db")).unwrap();
    assert_eq!(index.ingest_usage_dir(&dir).unwrap().usage, 2);
    // 같은 두 줄(내용이 같아도 위치가 다르다)이 다시 들어가지 않는다
    assert_eq!(index.ingest_usage_dir(&dir).unwrap().usage, 0);
    assert_eq!(index.usage_count().unwrap(), 2);
}

#[test]
fn the_todo_tag_names_the_held_todo() {
    let tags = |t: &[&str]| t.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(
        todo_ref_from_tags(&tags(&["turn", "todo:rocky-12"])).as_deref(),
        Some("rocky-12")
    );
    assert_eq!(todo_ref_from_tags(&tags(&["turn", "todo:"])), None);
    assert_eq!(todo_ref_from_tags(&tags(&["turn"])), None);
}

#[test]
fn stats_count_turns_by_project_and_todo_and_reuse_the_usage_report() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("worklog");
    append(
        &root.join("rocky-1").join("worklog.jsonl"),
        &line(
            "1",
            "2026-10-02T01:00:00.000Z",
            "turn",
            "a",
            &["turn", "todo:rocky-3"],
        ),
    );
    append(
        &root.join("rocky-1").join("worklog.jsonl"),
        &line(
            "2",
            "2026-10-02T02:00:00.000Z",
            "turn",
            "b",
            &["turn", "todo:rocky-3"],
        ),
    );
    append(
        &root.join("mdwire-2").join("worklog.jsonl"),
        &line("3", "2026-10-02T03:00:00.000Z", "turn", "c", &["turn"]),
    );
    append(
        &root.join("mdwire-2").join("worklog.jsonl"),
        &line("4", "2026-10-02T04:00:00.000Z", "decision", "d", &[]),
    );
    // 기간 밖
    append(
        &root.join("mdwire-2").join("worklog.jsonl"),
        &line("5", "2026-09-01T00:00:00.000Z", "turn", "e", &[]),
    );
    let usage = tmp.path().join("usage");
    let event = |name: &str, ok: bool, ms: u64| {
        serde_json::json!({ "ts": "2026-10-02T05:00:00.000Z", "source": "rest", "name": name, "ok": ok, "ms": ms }).to_string() + "\n"
    };
    append(
        &usage.join("2026-10.jsonl"),
        &event("GET /api/todos", true, 10),
    );
    append(
        &usage.join("2026-10.jsonl"),
        &event("GET /api/todos", false, 30),
    );
    let mut index = LogIndex::open(&tmp.path().join("logs.db")).unwrap();
    index.ingest_worklog_root(&root).unwrap();
    index.ingest_usage_dir(&usage).unwrap();

    let stats = index
        .stats("2026-10-01T00:00:00.000Z", "2026-10-03T00:00:00.000Z")
        .unwrap();
    assert_eq!(stats.worklog.turns, 3, "기간 안의 turn 만(decision 제외)");
    assert_eq!(
        stats.worklog.by_project,
        vec![("rocky-1".to_string(), 2), ("mdwire-2".to_string(), 1)]
    );
    assert_eq!(stats.worklog.by_todo, vec![("rocky-3".to_string(), 2)]);
    assert_eq!(stats.worklog.by_weekday.iter().sum::<u64>(), 3);
    let todos = stats
        .usage
        .surfaces
        .iter()
        .find(|s| s.name == "GET /api/todos")
        .unwrap();
    assert_eq!((todos.count, todos.errors), (2, 1));
    assert!(!stats.usage.unused.is_empty(), "안 쓴 표면도 같은 집계로");
}
