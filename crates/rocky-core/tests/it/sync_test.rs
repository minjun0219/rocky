//! 싱크 순번(`seq`) — 마이그레이션 19 의 트리거와 `TodoStore::sync_rows_since`.

use rocky_core::store::TodoStore;
use rocky_core::sync::{needs_full, SyncHead, Synced, FULL_HISTORY_LIMIT, SYNC_TABLES};
use rocky_core::types::*;
use rusqlite::Connection;

struct Fx {
    _dir: tempfile::TempDir,
    db_path: std::path::PathBuf,
    store: TodoStore,
}

fn fx() -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("todo.db");
    let store = TodoStore::open(&db_path).unwrap();
    Fx {
        _dir: dir,
        db_path,
        store,
    }
}

fn todo(store: &TodoStore, board: &str, title: &str) -> Todo {
    store
        .create_todo(
            &CreateTodoInput {
                board: board.into(),
                title: title.into(),
                ..Default::default()
            },
            "you",
        )
        .unwrap()
}

fn todo_ids(rows: &[Synced<Todo>]) -> Vec<String> {
    rows.iter().map(|s| s.row.id.clone()).collect()
}

#[test]
fn every_write_gets_a_seq_and_shows_up_after_the_cursor() {
    let f = fx();
    let start = f.store.sync_head().unwrap();
    let created = todo(&f.store, "sync", "first");

    let after_create = f
        .store
        .sync_rows_since(Some(start.seq), Some(&start.epoch))
        .unwrap();
    assert!(!after_create.full);
    assert!(after_create.seq > start.seq);
    assert_eq!(todo_ids(&after_create.rows.todos), vec![created.id.clone()]);
    // 보드는 할 일과 함께 저절로 생기고, 히스토리 줄도 같이 온다.
    assert!(after_create.rows.boards.iter().any(|b| b.row.key == "sync"));
    assert!(!after_create.rows.history.is_empty());

    let cursor = after_create.seq;
    f.store
        .update_todo(
            &created.id,
            &UpdateTodoPatch {
                title: Some("renamed".into()),
                ..Default::default()
            },
            "you",
            None,
        )
        .unwrap();
    let after_update = f
        .store
        .sync_rows_since(Some(cursor), Some(&start.epoch))
        .unwrap();
    assert_eq!(after_update.rows.todos.len(), 1);
    assert_eq!(after_update.rows.todos[0].row.title, "renamed");
    assert!(after_update.rows.todos[0].seq > cursor);
    // 바뀌지 않은 보드는 다시 오지 않는다.
    assert!(after_update.rows.boards.is_empty());
}

#[test]
fn nothing_comes_after_the_head() {
    let f = fx();
    todo(&f.store, "sync", "a");
    let head = f.store.sync_head().unwrap();
    let rows = f
        .store
        .sync_rows_since(Some(head.seq), Some(&head.epoch))
        .unwrap();
    assert!(!rows.full);
    assert!(rows.rows.is_empty());
    assert_eq!(rows.seq, head.seq);
}

#[test]
fn full_copy_when_the_client_has_nothing_or_another_db() {
    let f = fx();
    let t = todo(&f.store, "sync", "a");
    let head = f.store.sync_head().unwrap();
    let cases: [(Option<i64>, Option<&str>); 4] = [
        (None, None),
        (Some(head.seq), None),
        (Some(head.seq), Some("not-this-db")),
        (Some(head.seq + 5), Some(head.epoch.as_str())),
    ];
    for (since, epoch) in cases {
        let rows = f.store.sync_rows_since(since, epoch).unwrap();
        assert!(rows.full, "since={since:?} epoch={epoch:?}");
        assert_eq!(todo_ids(&rows.rows.todos), vec![t.id.clone()]);
    }
}

#[test]
fn full_copy_includes_rows_from_before_the_migration() {
    let f = fx();
    let t = todo(&f.store, "sync", "old");
    // 마이그레이션 전부터 있던 행은 순번이 0 이다. seq 를 바꾸는 갱신이라 트리거는 건너뛴다.
    let raw = Connection::open(&f.db_path).unwrap();
    raw.execute("UPDATE todos SET seq = 0 WHERE id = ?1", [&t.id])
        .unwrap();
    let rows = f.store.sync_rows_since(None, None).unwrap();
    assert!(rows
        .rows
        .todos
        .iter()
        .any(|s| s.row.id == t.id && s.seq == 0));
}

#[test]
fn writes_without_history_still_get_a_seq() {
    // 세션 귀속은 히스토리를 남기지 않는다 — 순번은 트리거가 매긴다.
    let f = fx();
    let t = todo(&f.store, "sync", "claimed");
    f.store
        .set_todo_status(&t.id, StatusAction::Start, "claude-code", None)
        .unwrap();
    let head = f.store.sync_head().unwrap();
    let claimed = f
        .store
        .claim_doing_session(&t.id, "session-1", None)
        .unwrap();
    assert!(claimed.is_some(), "귀속이 붙어야 이 테스트가 뜻이 있다");
    let rows = f
        .store
        .sync_rows_since(Some(head.seq), Some(&head.epoch))
        .unwrap();
    assert_eq!(todo_ids(&rows.rows.todos), vec![t.id.clone()]);
    assert!(rows.rows.history.is_empty());
}

#[test]
fn comments_notes_and_sections_get_seqs_in_commit_order() {
    let f = fx();
    let t = todo(&f.store, "sync", "with comment");
    let head = f.store.sync_head().unwrap();
    let comment = f.store.add_comment(&t.id, "hello", "you", None).unwrap();
    let note = f
        .store
        .create_note(
            &CreateNoteInput {
                board: Some("sync".into()),
                title: "memo".into(),
                content: None,
            },
            "you",
        )
        .unwrap();
    let sectioned = f
        .store
        .create_todo(
            &CreateTodoInput {
                board: "sync".into(),
                title: "in a section".into(),
                section: Some("later".into()),
                ..Default::default()
            },
            "you",
        )
        .unwrap();
    let rows = f
        .store
        .sync_rows_since(Some(head.seq), Some(&head.epoch))
        .unwrap();
    let comment_ids: Vec<_> = rows.rows.comments.iter().map(|s| &s.row.id).collect();
    assert_eq!(comment_ids, vec![&comment.id]);
    let note_ids: Vec<_> = rows.rows.notes.iter().map(|s| &s.row.id).collect();
    assert_eq!(note_ids, vec![&note.id]);
    assert!(rows.rows.sections.iter().any(|s| s.row.title == "later"));
    assert_eq!(todo_ids(&rows.rows.todos), vec![sectioned.id.clone()]);
    let comment_seq = rows.rows.comments[0].seq;
    let note_seq = rows.rows.notes[0].seq;
    assert!(comment_seq < note_seq && note_seq < rows.rows.todos[0].seq);
    let history: Vec<i64> = rows.rows.history.iter().map(|h| h.seq).collect();
    assert!(history.windows(2).all(|w| w[0] < w[1]));
}

#[test]
fn every_sync_table_has_both_triggers() {
    let f = fx();
    let raw = Connection::open(&f.db_path).unwrap();
    for table in SYNC_TABLES {
        for kind in ["insert", "update"] {
            let name = format!("{table}_sync_{kind}");
            let found: i64 = raw
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'trigger' AND name = ?1",
                    [&name],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(found, 1, "missing trigger {name}");
        }
    }
}

#[test]
fn reopening_renews_the_epoch_and_keeps_seq() {
    // 데몬이 다시 뜨면(백업에서 되살린 DB 포함) 클라이언트는 다른 epoch 를 보고 통째로 다시 받는다.
    let f = fx();
    todo(&f.store, "sync", "a");
    let before = f.store.sync_head().unwrap();
    assert_eq!(before.epoch.len(), 16);
    assert!(before.epoch.chars().all(|c| c.is_ascii_hexdigit()));
    let reopened = TodoStore::open(&f.db_path).unwrap();
    let after = reopened.sync_head().unwrap();
    assert_ne!(after.epoch, before.epoch);
    assert_eq!(after.seq, before.seq);
    let rows = reopened
        .sync_rows_since(Some(before.seq), Some(&before.epoch))
        .unwrap();
    assert!(rows.full);
}

#[test]
fn rerunning_the_migration_keeps_seq_and_triggers() {
    // 버전 올리기가 실패하면 다음 기동이 19 를 다시 돈다 — 순번은 이어지고 트리거는 하나씩이다.
    let f = fx();
    todo(&f.store, "sync", "a");
    let before = f.store.sync_head().unwrap();
    let raw = Connection::open(&f.db_path).unwrap();
    raw.execute_batch("PRAGMA user_version = 18;").unwrap();
    drop(raw);
    let reopened = TodoStore::open(&f.db_path).unwrap();
    assert_eq!(reopened.sync_head().unwrap().seq, before.seq);
    let t = todo(&reopened, "sync", "b");
    let rows = reopened.sync_rows_since(None, None).unwrap();
    let seq = rows
        .rows
        .todos
        .iter()
        .find(|s| s.row.id == t.id)
        .unwrap()
        .seq;
    assert!(seq > before.seq);
}

/// 쓰기 하나가 `table` 에 순번이 오른 행을 남기는지.
fn bumps(store: &TodoStore, table: &str, write: impl FnOnce()) {
    let head = store.sync_head().unwrap();
    write();
    let rows = store
        .sync_rows_since(Some(head.seq), Some(&head.epoch))
        .unwrap();
    let seqs: Vec<i64> = match table {
        "boards" => rows.rows.boards.iter().map(|s| s.seq).collect(),
        "sections" => rows.rows.sections.iter().map(|s| s.seq).collect(),
        "todos" => rows.rows.todos.iter().map(|s| s.seq).collect(),
        "comments" => rows.rows.comments.iter().map(|s| s.seq).collect(),
        "notes" => rows.rows.notes.iter().map(|s| s.seq).collect(),
        "history" => rows.rows.history.iter().map(|s| s.seq).collect(),
        "handoffs" => rows.rows.handoffs.iter().map(|s| s.seq).collect(),
        other => panic!("unknown table {other}"),
    };
    assert!(
        seqs.iter().any(|&seq| seq > head.seq),
        "{table} 에 새 순번이 없다"
    );
}

#[test]
fn every_table_gets_a_seq_on_insert_and_update() {
    let f = fx();
    let s = &f.store;
    let t = todo(s, "sync", "anchor");
    bumps(s, "boards", || {
        todo(s, "other", "makes a board");
    });
    bumps(s, "boards", || {
        s.update_board(
            "sync",
            &BoardPatch {
                title: Some("Sync".into()),
                ..Default::default()
            },
            "you",
        )
        .unwrap();
    });
    let mut section_id = String::new();
    bumps(s, "sections", || {
        let sectioned = s
            .create_todo(
                &CreateTodoInput {
                    board: "sync".into(),
                    title: "sectioned".into(),
                    section: Some("later".into()),
                    ..Default::default()
                },
                "you",
            )
            .unwrap();
        section_id = sectioned.section_id.unwrap();
    });
    bumps(s, "sections", || {
        s.archive_section(&section_id, "you").unwrap()
    });
    let mut comment_id = String::new();
    bumps(s, "comments", || {
        comment_id = s.add_comment(&t.id, "hi", "you", None).unwrap().id;
    });
    bumps(s, "comments", || {
        s.update_comment(&comment_id, "hello", "you").unwrap();
    });
    bumps(s, "comments", || {
        s.set_comment_archived(&comment_id, true, "you").unwrap();
    });
    let mut note_id = String::new();
    bumps(s, "notes", || {
        note_id = s
            .create_note(
                &CreateNoteInput {
                    board: Some("sync".into()),
                    title: "memo".into(),
                    content: None,
                },
                "you",
            )
            .unwrap()
            .id;
    });
    bumps(s, "notes", || {
        s.update_note(
            &note_id,
            &UpdateNotePatch {
                title: Some("memo 2".into()),
                ..Default::default()
            },
            "you",
            None,
        )
        .unwrap();
    });
    bumps(s, "notes", || {
        s.set_note_pinned(&note_id, true, "you", None).unwrap();
    });
    bumps(s, "notes", || {
        s.archive_note(&note_id, "you", None).unwrap();
    });
    let mut handoff_id = String::new();
    bumps(s, "handoffs", || {
        handoff_id = s
            .create_handoff(&CreateHandoffInput {
                todo_ref: t.id.clone(),
                session_id: "sess-1".into(),
                session_name: None,
                session_cwd: None,
                note: None,
                actor: "you".into(),
                current_board_id: None,
            })
            .unwrap()
            .id;
    });
    bumps(s, "handoffs", || {
        s.cancel_handoff(&handoff_id, "you").unwrap();
    });
    bumps(s, "history", || {
        s.set_todo_status(&t.id, StatusAction::Done, "you", None)
            .unwrap();
    });
}

#[test]
fn full_copy_carries_only_recent_history() {
    let f = fx();
    let t = todo(&f.store, "sync", "busy");
    let raw = Connection::open(&f.db_path).unwrap();
    raw.execute_batch("BEGIN").unwrap();
    for i in 0..(FULL_HISTORY_LIMIT + 50) {
        raw.execute(
            "INSERT INTO history (entity, entity_id, actor, action, at) VALUES ('todo', ?1, 'you', 'update', ?2)",
            rusqlite::params![t.id, format!("2026-10-15T00:00:{:02}.000Z", i % 60)],
        )
        .unwrap();
    }
    raw.execute_batch("COMMIT").unwrap();
    let newest: i64 = raw
        .query_row("SELECT MAX(id) FROM history", [], |r| r.get(0))
        .unwrap();
    let full = f.store.sync_rows_since(None, None).unwrap();
    assert_eq!(full.rows.history.len() as i64, FULL_HISTORY_LIMIT);
    assert_eq!(full.rows.history.last().unwrap().row.id, newest);
    // 이어 받기는 자르지 않는다.
    let since = full.seq - (FULL_HISTORY_LIMIT + 10);
    let incremental = f
        .store
        .sync_rows_since(Some(since), Some(&full.epoch))
        .unwrap();
    assert_eq!(
        incremental.rows.history.len() as i64,
        FULL_HISTORY_LIMIT + 10
    );
}

#[test]
fn rows_serialize_with_seq_beside_the_row_fields() {
    let f = fx();
    let t = todo(&f.store, "sync", "json");
    let value = serde_json::to_value(f.store.sync_rows_since(None, None).unwrap()).unwrap();
    assert_eq!(value["full"], true);
    assert!(value["epoch"].is_string());
    let row = value["rows"]["todos"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["id"] == t.id.as_str())
        .unwrap();
    assert_eq!(row["title"], "json");
    assert_eq!(row["boardId"], t.board_id.as_str());
    assert!(row["seq"].as_i64().unwrap() > 0);
}

#[test]
fn needs_full_only_for_a_fresh_or_foreign_copy() {
    let head = SyncHead {
        epoch: "e".into(),
        seq: 10,
    };
    assert!(needs_full(None, None, &head));
    assert!(needs_full(Some(3), None, &head));
    assert!(needs_full(None, Some("e"), &head));
    assert!(needs_full(Some(3), Some("other"), &head));
    assert!(needs_full(Some(11), Some("e"), &head));
    assert!(needs_full(Some(-1), Some("e"), &head));
    assert!(!needs_full(Some(10), Some("e"), &head));
    assert!(!needs_full(Some(0), Some("e"), &head));
}
