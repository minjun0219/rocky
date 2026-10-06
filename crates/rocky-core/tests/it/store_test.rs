//! TS 원본 `src/store.test.ts` 포팅.

use rocky_core::peer_inbox::ClearedAction;
use rocky_core::refs::ref_of;
use rocky_core::store::{StoreResult, TodoStore};
use rocky_core::types::*;
use rusqlite::Connection;
use std::path::PathBuf;

struct Fx {
    _dir: tempfile::TempDir,
    db_path: PathBuf,
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

impl Fx {
    /// 데몬 재기동 시뮬레이션 — 같은 파일로 스토어를 다시 연다.
    fn reload(&mut self) {
        let fresh = TodoStore::open(&self.db_path).unwrap();
        self.store = fresh;
    }

    /// 테스트 전용 raw 연결 — malformed 레거시 행 주입용.
    fn raw(&self) -> Connection {
        Connection::open(&self.db_path).unwrap()
    }
}

fn todo_input(board: &str, title: &str) -> CreateTodoInput {
    CreateTodoInput {
        board: board.into(),
        title: title.into(),
        ..Default::default()
    }
}

fn create(store: &TodoStore, board: &str, title: &str, actor: &str) -> Todo {
    store.create_todo(&todo_input(board, title), actor).unwrap()
}

fn err_of<T: std::fmt::Debug>(result: StoreResult<T>) -> String {
    result.expect_err("expected error").to_string()
}

/// id prefix 테스트용 — 알파벳이 하나 이상 들어간 prefix (전부 숫자면 번호 분기로 샌다).
fn id_prefix(id: &str) -> String {
    match id.find(|c: char| c.is_ascii_lowercase()) {
        None => id.to_string(),
        Some(at) => id[..std::cmp::max(4, at + 1)].to_string(),
    }
}

// ── boards ──────────────────────────────────────────────────────────────────

#[test]
fn ensure_board_creates_and_is_idempotent_by_key() {
    let f = fx();
    let a = f.store.ensure_board("rocky", None, "tester").unwrap();
    let b = f.store.ensure_board("rocky", None, "tester").unwrap();
    assert_eq!(a.id, b.id);
    assert_eq!(a.title, "rocky");
    assert_eq!(f.store.list_boards(false).unwrap().len(), 1);
}

#[test]
fn ensure_board_accepts_explicit_title_on_first_creation() {
    let f = fx();
    let board = f
        .store
        .ensure_board("rocky", Some("Rocky Board"), "tester")
        .unwrap();
    assert_eq!(board.title, "Rocky Board");
    // 두 번째 호출의 title 은 무시된다 — 이미 있는 보드를 그대로 돌려준다.
    let again = f
        .store
        .ensure_board("rocky", Some("다른 이름"), "tester")
        .unwrap();
    assert_eq!(again.title, "Rocky Board");
}

#[test]
fn ensure_board_rejects_whitespace_key() {
    let f = fx();
    let msg = err_of(f.store.ensure_board("my repo", None, "tester"));
    assert!(msg.contains("whitespace"), "{msg}");
}

#[test]
fn ensure_board_rejects_hash_key() {
    let f = fx();
    let msg = err_of(f.store.ensure_board("a#b", None, "tester"));
    assert!(msg.contains('#'), "{msg}");
}

#[test]
fn ensure_board_accepts_note_key_and_items_fall_back_to_raw_id() {
    let f = fx();
    let todo = create(&f.store, "note", "note 보드 항목", "tester");
    // `note` 는 ref-safe 하지 않다 — refOf 가 raw id 로 폴백한다.
    let r = ref_of(&f.store, Some(&todo.board_id), todo.number, &todo.id).unwrap();
    assert_eq!(r, todo.id);
}

#[test]
fn ensure_board_allows_keys_starting_with_note() {
    let f = fx();
    let todo = create(&f.store, "notes-vault", "항목", "tester");
    let r = ref_of(&f.store, Some(&todo.board_id), todo.number, &todo.id).unwrap();
    assert_eq!(r, format!("notes-vault-{}", todo.number));
}

#[test]
fn ensure_board_rejects_empty_key() {
    let f = fx();
    let msg = err_of(f.store.ensure_board("", None, "tester"));
    assert!(msg.contains("empty"), "{msg}");
}

#[test]
fn ensure_board_accepts_route_colliding_keys() {
    let f = fx();
    assert!(f.store.ensure_board("api", None, "tester").is_ok());
    assert!(f.store.ensure_board("mcp", None, "tester").is_ok());
}

#[test]
fn ensure_board_returns_preexisting_malformed_board_unchanged() {
    let mut f = fx();
    {
        let raw = f.raw();
        raw.execute(
            "INSERT INTO boards (id, key, title, created_at) VALUES ('b1', 'my repo', 'my repo', '2026-07-01T00:00:00.000Z')",
            [],
        )
        .unwrap();
    }
    f.reload();
    let board = f.store.ensure_board("my repo", None, "tester").unwrap();
    assert_eq!(board.id, "b1");
    assert_eq!(f.store.list_boards(false).unwrap().len(), 1);
}

#[test]
fn set_board_repo_stores_slug_and_survives_reload() {
    let mut f = fx();
    f.store.ensure_board("rocky", None, "tester").unwrap();
    f.store
        .set_board_repo("rocky", "minjun0219/rocky", "tester")
        .unwrap();
    f.reload();
    assert_eq!(
        f.store.get_board("rocky").unwrap().unwrap().repo.as_deref(),
        Some("minjun0219/rocky")
    );
}

#[test]
fn set_board_repo_does_not_create_board() {
    let f = fx();
    let msg = err_of(f.store.set_board_repo("nope", "o/n", "tester"));
    assert!(msg.contains("board not found"), "{msg}");
    assert!(f.store.list_boards(false).unwrap().is_empty());
}

#[test]
fn set_board_repo_trims_slug() {
    let mut f = fx();
    let board = f.store.ensure_board("rocky", None, "tester").unwrap();
    f.store
        .set_board_repo("rocky", "  o/n  ", "tester")
        .unwrap();
    f.reload();
    assert_eq!(
        f.store.get_board("rocky").unwrap().unwrap().repo.as_deref(),
        Some("o/n")
    );
    let history = f
        .store
        .list_history(&ListHistoryFilter {
            entity: Some(HistoryEntity::Board),
            entity_id: Some(board.id),
            ..Default::default()
        })
        .unwrap();
    let entry = history.iter().find(|h| h.action == "update").unwrap();
    assert_eq!(
        entry.changes.as_ref().unwrap()["repo"],
        serde_json::json!([null, "o/n"])
    );
}

#[test]
fn set_board_repo_is_noop_when_unchanged() {
    let f = fx();
    let board = f.store.ensure_board("rocky", None, "tester").unwrap();
    f.store.set_board_repo("rocky", "o/n", "tester").unwrap();
    let before = f
        .store
        .list_history(&ListHistoryFilter {
            entity: Some(HistoryEntity::Board),
            entity_id: Some(board.id.clone()),
            ..Default::default()
        })
        .unwrap()
        .len();
    f.store.set_board_repo("rocky", "o/n", "tester").unwrap();
    let after = f
        .store
        .list_history(&ListHistoryFilter {
            entity: Some(HistoryEntity::Board),
            entity_id: Some(board.id),
            ..Default::default()
        })
        .unwrap()
        .len();
    assert_eq!(before, after);
}

#[test]
fn set_board_repo_records_real_change() {
    let f = fx();
    let board = f.store.ensure_board("rocky", None, "tester").unwrap();
    f.store.set_board_repo("rocky", "o/n", "tester").unwrap();
    f.store
        .set_board_repo("rocky", "o/other", "tester")
        .unwrap();
    let history = f
        .store
        .list_history(&ListHistoryFilter {
            entity: Some(HistoryEntity::Board),
            entity_id: Some(board.id),
            ..Default::default()
        })
        .unwrap();
    let updates: Vec<_> = history.iter().filter(|h| h.action == "update").collect();
    assert_eq!(updates.len(), 2);
}

#[test]
fn board_by_id_unknown_returns_none() {
    let f = fx();
    assert!(f.store.board_by_id("nope").unwrap().is_none());
}

#[test]
fn set_board_path_stores_and_survives_reload() {
    let mut f = fx();
    f.store.ensure_board("rocky", None, "tester").unwrap();
    f.store
        .set_board_path("rocky", "/dev/rocky", "tester")
        .unwrap();
    f.reload();
    assert_eq!(
        f.store.get_board("rocky").unwrap().unwrap().path.as_deref(),
        Some("/dev/rocky")
    );
}

#[test]
fn set_board_path_trims_and_skips_history_when_unchanged() {
    let f = fx();
    let board = f.store.ensure_board("rocky", None, "tester").unwrap();
    f.store
        .set_board_path("rocky", "  /dev/rocky  ", "tester")
        .unwrap();
    assert_eq!(
        f.store.get_board("rocky").unwrap().unwrap().path.as_deref(),
        Some("/dev/rocky")
    );
    let before = f
        .store
        .list_history(&ListHistoryFilter {
            entity: Some(HistoryEntity::Board),
            entity_id: Some(board.id.clone()),
            ..Default::default()
        })
        .unwrap()
        .len();
    f.store
        .set_board_path("rocky", "/dev/rocky", "tester")
        .unwrap();
    let after = f
        .store
        .list_history(&ListHistoryFilter {
            entity: Some(HistoryEntity::Board),
            entity_id: Some(board.id),
            ..Default::default()
        })
        .unwrap()
        .len();
    assert_eq!(before, after);
}

#[test]
fn set_board_path_requires_existing_board() {
    let f = fx();
    let msg = err_of(f.store.set_board_path("nope", "/x", "tester"));
    assert!(msg.contains("board not found"), "{msg}");
}

// ── updateBoard ─────────────────────────────────────────────────────────────

fn board_history(store: &TodoStore, board_id: &str) -> Vec<HistoryEntry> {
    store
        .list_history(&ListHistoryFilter {
            entity: Some(HistoryEntity::Board),
            entity_id: Some(board_id.to_string()),
            ..Default::default()
        })
        .unwrap()
}

#[test]
fn update_board_patches_all_fields_with_single_history_entry() {
    let f = fx();
    let board = f.store.ensure_board("gotgan", None, "logan").unwrap();
    let before = board_history(&f.store, &board.id).len();

    let updated = f
        .store
        .update_board(
            "gotgan",
            &BoardPatch {
                title: Some("Tally".into()),
                description: Some(Some("가계부".into())),
                repo: Some(Some("minjun0219/tally".into())),
                path: Some(Some("/dev/tally".into())),
                ..Default::default()
            },
            "logan",
        )
        .unwrap();

    assert_eq!(updated.title, "Tally");
    assert_eq!(updated.description.as_deref(), Some("가계부"));
    assert_eq!(updated.repo.as_deref(), Some("minjun0219/tally"));
    assert_eq!(updated.path.as_deref(), Some("/dev/tally"));
    let history = board_history(&f.store, &board.id);
    assert_eq!(history.len(), before + 1);
    let mut keys: Vec<_> = history[0]
        .changes
        .as_ref()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    keys.sort();
    assert_eq!(keys, vec!["description", "path", "repo", "title"]);
}

#[test]
fn update_board_noop_records_nothing() {
    let f = fx();
    let board = f.store.ensure_board("rocky", None, "logan").unwrap();
    let before = board_history(&f.store, &board.id).len();
    f.store
        .update_board(
            "rocky",
            &BoardPatch {
                title: Some("rocky".into()),
                description: Some(None),
                ..Default::default()
            },
            "logan",
        )
        .unwrap();
    assert_eq!(board_history(&f.store, &board.id).len(), before);
}

#[test]
fn update_board_null_and_blank_clear() {
    let f = fx();
    f.store.ensure_board("rocky", None, "logan").unwrap();
    f.store
        .update_board(
            "rocky",
            &BoardPatch {
                description: Some(Some("설명".into())),
                repo: Some(Some("o/n".into())),
                ..Default::default()
            },
            "logan",
        )
        .unwrap();
    let cleared = f
        .store
        .update_board(
            "rocky",
            &BoardPatch {
                description: Some(None),
                repo: Some(Some("  ".into())),
                ..Default::default()
            },
            "logan",
        )
        .unwrap();
    assert!(cleared.description.is_none());
    assert!(cleared.repo.is_none());
}

#[test]
fn update_board_rejects_blank_title() {
    let f = fx();
    f.store.ensure_board("rocky", None, "logan").unwrap();
    let msg = err_of(f.store.update_board(
        "rocky",
        &BoardPatch {
            title: Some("   ".into()),
            ..Default::default()
        },
        "logan",
    ));
    assert!(msg.contains("title"), "{msg}");
}

#[test]
fn update_board_does_not_create_missing_board() {
    let f = fx();
    let msg = err_of(f.store.update_board(
        "nope",
        &BoardPatch {
            title: Some("X".into()),
            ..Default::default()
        },
        "logan",
    ));
    assert!(msg.contains("board not found"), "{msg}");
    assert!(f.store.list_boards(false).unwrap().is_empty());
}

#[test]
fn rename_keeps_old_refs_and_board_args_alive() {
    let f = fx();
    let board = f.store.ensure_board("gotgan", None, "logan").unwrap();
    let todo = create(&f.store, "gotgan", "이월 정산", "logan");

    let renamed = f
        .store
        .update_board(
            "gotgan",
            &BoardPatch {
                key: Some("tally".into()),
                ..Default::default()
            },
            "logan",
        )
        .unwrap();
    assert_eq!(renamed.key, "tally");
    assert_eq!(renamed.previous_keys, Some(vec!["gotgan".to_string()]));

    // 참조 해석: 옛 ref 도 새 ref 도 같은 항목
    assert_eq!(
        f.store.get_todo("gotgan-1", None).unwrap().unwrap().id,
        todo.id
    );
    assert_eq!(
        f.store.get_todo("tally-1", None).unwrap().unwrap().id,
        todo.id
    );
    assert_eq!(
        f.store.get_todo("gotgan#1", None).unwrap().unwrap().id,
        todo.id
    );
    assert_eq!(
        f.store.board_id_of("gotgan").unwrap().as_deref(),
        Some(board.id.as_str())
    );
    // 옛 key 로 항목을 더해도 새 보드가 생기지 않는다.
    create(&f.store, "gotgan", "두번째", "logan");
    assert_eq!(f.store.list_boards(false).unwrap().len(), 1);
}

#[test]
fn rename_exports_new_key_only() {
    let f = fx();
    f.store.ensure_board("gotgan", None, "logan").unwrap();
    let todo = create(&f.store, "gotgan", "이월 정산", "logan");
    f.store
        .update_board(
            "gotgan",
            &BoardPatch {
                key: Some("tally".into()),
                ..Default::default()
            },
            "logan",
        )
        .unwrap();
    assert_eq!(
        ref_of(&f.store, Some(&todo.board_id), todo.number, &todo.id).unwrap(),
        "tally-1"
    );
}

#[test]
fn rename_cannot_steal_other_board_name_or_alias() {
    let f = fx();
    f.store.ensure_board("gotgan", None, "logan").unwrap();
    f.store.ensure_board("tally", None, "logan").unwrap();
    let msg = err_of(f.store.update_board(
        "gotgan",
        &BoardPatch {
            key: Some("tally".into()),
            ..Default::default()
        },
        "logan",
    ));
    assert!(msg.contains("already in use"), "{msg}");

    f.store.ensure_board("other", None, "logan").unwrap();
    f.store
        .update_board(
            "other",
            &BoardPatch {
                key: Some("renamed".into()),
                ..Default::default()
            },
            "logan",
        )
        .unwrap();
    let msg = err_of(f.store.update_board(
        "gotgan",
        &BoardPatch {
            key: Some("other".into()),
            ..Default::default()
        },
        "logan",
    ));
    assert!(msg.contains("already in use"), "{msg}");
    assert_eq!(f.store.get_board("gotgan").unwrap().unwrap().key, "gotgan");
}

#[test]
fn rename_back_cleans_self_alias() {
    let f = fx();
    f.store.ensure_board("gotgan", None, "logan").unwrap();
    f.store
        .update_board(
            "gotgan",
            &BoardPatch {
                key: Some("tally".into()),
                ..Default::default()
            },
            "logan",
        )
        .unwrap();
    let back = f
        .store
        .update_board(
            "tally",
            &BoardPatch {
                key: Some("gotgan".into()),
                ..Default::default()
            },
            "logan",
        )
        .unwrap();
    assert_eq!(back.key, "gotgan");
    assert_eq!(back.previous_keys, Some(vec!["tally".to_string()]));
}

#[test]
fn rename_uses_creation_key_validation() {
    let f = fx();
    f.store.ensure_board("rocky", None, "logan").unwrap();
    assert!(err_of(f.store.update_board(
        "rocky",
        &BoardPatch {
            key: Some("my repo".into()),
            ..Default::default()
        },
        "logan"
    ))
    .contains("whitespace"));
    assert!(err_of(f.store.update_board(
        "rocky",
        &BoardPatch {
            key: Some("a#b".into()),
            ..Default::default()
        },
        "logan"
    ))
    .contains('#'));
    assert!(err_of(f.store.update_board(
        "rocky",
        &BoardPatch {
            key: Some("   ".into()),
            ..Default::default()
        },
        "logan"
    ))
    .contains("empty"));
    assert_eq!(f.store.get_board("rocky").unwrap().unwrap().key, "rocky");
}

#[test]
fn alias_can_be_used_to_update_board() {
    let f = fx();
    f.store.ensure_board("gotgan", None, "logan").unwrap();
    f.store
        .update_board(
            "gotgan",
            &BoardPatch {
                key: Some("tally".into()),
                ..Default::default()
            },
            "logan",
        )
        .unwrap();
    let updated = f
        .store
        .update_board(
            "gotgan",
            &BoardPatch {
                description: Some(Some("가계부".into())),
                ..Default::default()
            },
            "logan",
        )
        .unwrap();
    assert_eq!(updated.key, "tally");
    assert_eq!(updated.description.as_deref(), Some("가계부"));
}

// ── todos ───────────────────────────────────────────────────────────────────

#[test]
fn create_todo_applies_defaults_and_lists_by_board() {
    let f = fx();
    let todo = create(&f.store, "rocky", "기본값", "tester");
    assert_eq!(todo.status, TodoStatus::Todo);
    assert_eq!(todo.priority, TodoPriority::P4);
    assert_eq!(todo.description, "");
    let listed = f
        .store
        .list_todos(&ListTodosFilter {
            board: Some("rocky".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, todo.id);
}

#[test]
fn create_todo_full_metadata_round_trips() {
    let f = fx();
    let todo = f
        .store
        .create_todo(
            &CreateTodoInput {
                board: "rocky".into(),
                title: "풀 메타".into(),
                description: Some("상세".into()),
                section: Some("설계".into()),
                priority: Some(TodoPriority::P2),
                due: Some("2026-08-01".into()),
                labels: Some(vec!["ui".into(), "a11y".into()]),
                links: Some(vec![TodoLink {
                    url: "https://github.com/x/y/issues/1".into(),
                    title: Some("이슈".into()),
                }]),
                ..Default::default()
            },
            "tester",
        )
        .unwrap();
    let loaded = f.store.get_todo(&todo.id, None).unwrap().unwrap();
    assert_eq!(loaded.description, "상세");
    assert_eq!(loaded.priority, TodoPriority::P2);
    assert_eq!(loaded.due.as_deref(), Some("2026-08-01"));
    assert_eq!(loaded.labels, vec!["ui", "a11y"]);
    assert_eq!(loaded.links.len(), 1);
    assert_eq!(loaded.links[0].url, "https://github.com/x/y/issues/1");
    assert!(loaded.section_id.is_some());
}

#[test]
fn section_is_upserted_by_name_within_board() {
    let f = fx();
    let a = f
        .store
        .create_todo(
            &CreateTodoInput {
                section: Some("설계".into()),
                ..todo_input("rocky", "a")
            },
            "tester",
        )
        .unwrap();
    let b = f
        .store
        .create_todo(
            &CreateTodoInput {
                section: Some("설계".into()),
                ..todo_input("rocky", "b")
            },
            "tester",
        )
        .unwrap();
    assert_eq!(a.section_id, b.section_id);
    let board_id = f.store.board_id_of("rocky").unwrap().unwrap();
    assert_eq!(f.store.list_sections(&board_id, false).unwrap().len(), 1);
}

#[test]
fn section_null_detaches() {
    let f = fx();
    let todo = f
        .store
        .create_todo(
            &CreateTodoInput {
                section: Some("설계".into()),
                ..todo_input("rocky", "a")
            },
            "tester",
        )
        .unwrap();
    let updated = f
        .store
        .update_todo(
            &todo.id,
            &UpdateTodoPatch {
                section: Some(None),
                ..Default::default()
            },
            "tester",
            None,
        )
        .unwrap();
    assert!(updated.section_id.is_none());
}

#[test]
fn create_todo_does_not_create_blank_section() {
    let f = fx();
    let todo = f
        .store
        .create_todo(
            &CreateTodoInput {
                section: Some("   ".into()),
                ..todo_input("rocky", "a")
            },
            "tester",
        )
        .unwrap();
    assert!(todo.section_id.is_none());
    let board_id = f.store.board_id_of("rocky").unwrap().unwrap();
    assert!(f.store.list_sections(&board_id, false).unwrap().is_empty());
}

#[test]
fn create_todo_trims_section_name() {
    let f = fx();
    let a = f
        .store
        .create_todo(
            &CreateTodoInput {
                section: Some(" 설계 ".into()),
                ..todo_input("rocky", "a")
            },
            "tester",
        )
        .unwrap();
    let b = f
        .store
        .create_todo(
            &CreateTodoInput {
                section: Some("설계".into()),
                ..todo_input("rocky", "b")
            },
            "tester",
        )
        .unwrap();
    assert_eq!(a.section_id, b.section_id);
}

#[test]
fn blank_section_in_update_detaches_instead_of_creating() {
    let f = fx();
    let todo = f
        .store
        .create_todo(
            &CreateTodoInput {
                section: Some("설계".into()),
                ..todo_input("rocky", "a")
            },
            "tester",
        )
        .unwrap();
    let updated = f
        .store
        .update_todo(
            &todo.id,
            &UpdateTodoPatch {
                section: Some(Some("  ".into())),
                ..Default::default()
            },
            "tester",
            None,
        )
        .unwrap();
    assert!(updated.section_id.is_none());
    let board_id = f.store.board_id_of("rocky").unwrap().unwrap();
    assert_eq!(f.store.list_sections(&board_id, false).unwrap().len(), 1); // 빈 이름 섹션이 안 생겼다
}

#[test]
fn archiving_section_returns_items_to_unsectioned() {
    let f = fx();
    let todo = f
        .store
        .create_todo(
            &CreateTodoInput {
                section: Some("설계".into()),
                ..todo_input("rocky", "a")
            },
            "tester",
        )
        .unwrap();
    let section_id = todo.section_id.clone().unwrap();
    f.store.archive_section(&section_id, "tester").unwrap();
    let after = f.store.get_todo(&todo.id, None).unwrap().unwrap();
    assert!(after.section_id.is_none());
    let board_id = f.store.board_id_of("rocky").unwrap().unwrap();
    assert!(f.store.list_sections(&board_id, false).unwrap().is_empty());
    assert_eq!(f.store.list_sections(&board_id, true).unwrap().len(), 1);
}

#[test]
fn archiving_section_records_history_on_each_todo() {
    let f = fx();
    let todo = f
        .store
        .create_todo(
            &CreateTodoInput {
                section: Some("설계".into()),
                ..todo_input("rocky", "a")
            },
            "tester",
        )
        .unwrap();
    let section_id = todo.section_id.clone().unwrap();
    f.store.archive_section(&section_id, "tester").unwrap();
    let history = f
        .store
        .list_history(&ListHistoryFilter {
            entity_id: Some(todo.id),
            ..Default::default()
        })
        .unwrap();
    let entry = history.iter().find(|h| h.action == "update").unwrap();
    assert_eq!(
        entry.changes.as_ref().unwrap()["section"],
        serde_json::json!([section_id, null])
    );
}

#[test]
fn hierarchy_child_references_parent() {
    let f = fx();
    let parent = create(&f.store, "rocky", "부모", "tester");
    let child = f
        .store
        .create_todo(
            &CreateTodoInput {
                parent_id: Some(parent.id.clone()),
                ..todo_input("rocky", "자식")
            },
            "tester",
        )
        .unwrap();
    assert_eq!(child.parent_id.as_deref(), Some(parent.id.as_str()));
}

#[test]
fn create_todo_rejects_unknown_parent() {
    let f = fx();
    let msg = err_of(f.store.create_todo(
        &CreateTodoInput {
            parent_id: Some("nosuchid".into()),
            ..todo_input("rocky", "자식")
        },
        "tester",
    ));
    assert!(msg.contains("parent todo not found"), "{msg}");
}

#[test]
fn create_todo_accepts_bare_number_parent_in_own_board() {
    let f = fx();
    let parent = create(&f.store, "rocky", "부모", "tester");
    let child = f
        .store
        .create_todo(
            &CreateTodoInput {
                parent_id: Some(parent.number.to_string()),
                ..todo_input("rocky", "자식")
            },
            "tester",
        )
        .unwrap();
    assert_eq!(child.parent_id.as_deref(), Some(parent.id.as_str()));
}

#[test]
fn create_todo_bare_number_parent_does_not_leak_across_boards() {
    let f = fx();
    create(&f.store, "alpha", "다른 보드 1번", "tester");
    let msg = err_of(f.store.create_todo(
        &CreateTodoInput {
            parent_id: Some("1".into()),
            ..todo_input("beta", "자식")
        },
        "tester",
    ));
    // beta 보드에는 1번이 없다 — alpha 의 1번으로 새면 안 된다.
    assert!(msg.contains("parent todo not found"), "{msg}");
}

#[test]
fn update_todo_patches_and_bumps_updated_at() {
    let f = fx();
    let todo = create(&f.store, "rocky", "이전", "tester");
    std::thread::sleep(std::time::Duration::from_millis(2));
    let updated = f
        .store
        .update_todo(
            &todo.id,
            &UpdateTodoPatch {
                title: Some("이후".into()),
                priority: Some(TodoPriority::P1),
                ..Default::default()
            },
            "tester",
            None,
        )
        .unwrap();
    assert_eq!(updated.title, "이후");
    assert_eq!(updated.priority, TodoPriority::P1);
    assert!(updated.updated_at > todo.updated_at);
}

#[test]
fn get_todo_resolves_unique_id_prefix() {
    let f = fx();
    let todo = create(&f.store, "rocky", "대상", "tester");
    let prefix = id_prefix(&todo.id);
    assert_eq!(
        f.store.get_todo(&prefix, None).unwrap().unwrap().id,
        todo.id
    );
}

#[test]
fn update_todo_accepts_bare_number_parent_in_own_board() {
    let f = fx();
    let parent = create(&f.store, "rocky", "부모", "tester");
    let child = create(&f.store, "rocky", "자식", "tester");
    let updated = f
        .store
        .update_todo(
            &child.id,
            &UpdateTodoPatch {
                parent_id: Some(Some(parent.number.to_string())),
                ..Default::default()
            },
            "tester",
            None,
        )
        .unwrap();
    assert_eq!(updated.parent_id.as_deref(), Some(parent.id.as_str()));
}

#[test]
fn update_todo_bare_number_parent_does_not_leak_across_boards() {
    let f = fx();
    create(&f.store, "alpha", "다른 보드 1번", "tester");
    let child = create(&f.store, "beta", "자식", "tester");
    // child 자신이 beta-1 이므로 자기 부모가 될 수 없다는 에러가 맞다 —
    // alpha-1 로 새지 않는다는 것이 핵심이다.
    let msg = err_of(f.store.update_todo(
        &child.id,
        &UpdateTodoPatch {
            parent_id: Some(Some("1".into())),
            ..Default::default()
        },
        "tester",
        None,
    ));
    assert!(msg.contains("own parent"), "{msg}");
}

// ── status transitions ──────────────────────────────────────────────────────

#[test]
fn start_marks_doing_and_stop_reverts() {
    let f = fx();
    let todo = create(&f.store, "rocky", "작업", "tester");
    let started = f
        .store
        .set_todo_status(&todo.id, StatusAction::Start, "claude-code", None)
        .unwrap();
    assert_eq!(started.status, TodoStatus::Doing);
    assert_eq!(started.doing_by.as_deref(), Some("claude-code"));
    assert!(started.doing_since.is_some());

    let stopped = f
        .store
        .set_todo_status(&todo.id, StatusAction::Stop, "claude-code", None)
        .unwrap();
    assert_eq!(stopped.status, TodoStatus::Todo);
    assert!(stopped.doing_by.is_none());
    assert!(stopped.doing_since.is_none());
}

#[test]
fn done_sets_completed_at_and_reopen_reverts() {
    let f = fx();
    let todo = create(&f.store, "rocky", "작업", "tester");
    f.store
        .set_todo_status(&todo.id, StatusAction::Start, "claude-code", None)
        .unwrap();
    let finished = f
        .store
        .set_todo_status(&todo.id, StatusAction::Done, "claude-code", None)
        .unwrap();
    assert_eq!(finished.status, TodoStatus::Done);
    assert!(finished.completed_at.is_some());
    assert!(finished.doing_by.is_none());

    let reopened = f
        .store
        .set_todo_status(&todo.id, StatusAction::Reopen, "tester", None)
        .unwrap();
    assert_eq!(reopened.status, TodoStatus::Todo);
    assert!(reopened.completed_at.is_none());
}

#[test]
fn archive_hides_from_default_listing() {
    let f = fx();
    let todo = create(&f.store, "rocky", "작업", "tester");
    f.store
        .set_todo_status(&todo.id, StatusAction::Archive, "tester", None)
        .unwrap();
    let filter = ListTodosFilter {
        board: Some("rocky".into()),
        ..Default::default()
    };
    assert!(f.store.list_todos(&filter).unwrap().is_empty());
    let with_archived = ListTodosFilter {
        board: Some("rocky".into()),
        include_archived: true,
        ..Default::default()
    };
    assert_eq!(f.store.list_todos(&with_archived).unwrap().len(), 1);

    f.store
        .set_todo_status(&todo.id, StatusAction::Unarchive, "tester", None)
        .unwrap();
    assert_eq!(f.store.list_todos(&filter).unwrap().len(), 1);
}

#[test]
fn list_todos_filters_status_label_and_all_boards() {
    let f = fx();
    let a = create(&f.store, "alpha", "a", "tester");
    f.store
        .create_todo(
            &CreateTodoInput {
                labels: Some(vec!["ui".into()]),
                ..todo_input("beta", "b")
            },
            "tester",
        )
        .unwrap();
    f.store
        .set_todo_status(&a.id, StatusAction::Start, "tester", None)
        .unwrap();

    let all = f.store.list_todos(&ListTodosFilter::default()).unwrap();
    assert_eq!(all.len(), 2);
    let doing = f
        .store
        .list_todos(&ListTodosFilter {
            status: Some(TodoStatus::Doing),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(doing.len(), 1);
    assert_eq!(doing[0].id, a.id);
    let labeled = f
        .store
        .list_todos(&ListTodosFilter {
            label: Some("ui".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(labeled.len(), 1);
    assert_eq!(labeled[0].title, "b");
    let unknown_board = f
        .store
        .list_todos(&ListTodosFilter {
            board: Some("nope".into()),
            ..Default::default()
        })
        .unwrap();
    assert!(unknown_board.is_empty());
}

// ── notes ───────────────────────────────────────────────────────────────────

#[test]
fn note_lifecycle_create_edit_append_archive() {
    let f = fx();
    let note = f
        .store
        .create_note(
            &CreateNoteInput {
                board: Some("rocky".into()),
                title: "메모".into(),
                content: Some("첫 줄".into()),
            },
            "tester",
        )
        .unwrap();
    assert_eq!(note.content, "첫 줄");

    let set = f
        .store
        .update_note(
            &note.id,
            &UpdateNotePatch {
                content: Some("교체".into()),
                ..Default::default()
            },
            "tester",
            None,
        )
        .unwrap();
    assert_eq!(set.content, "교체");

    let appended = f
        .store
        .update_note(
            &note.id,
            &UpdateNotePatch {
                content: Some("이어붙임".into()),
                mode: NoteContentMode::Append,
                ..Default::default()
            },
            "tester",
            None,
        )
        .unwrap();
    assert_eq!(appended.content, "교체\n이어붙임");

    let archived = f.store.archive_note(&note.id, "tester", None).unwrap();
    assert!(archived.archived_at.is_some());
    assert!(f
        .store
        .list_notes(&ListNotesFilter {
            board: Some("rocky".into()),
            ..Default::default()
        })
        .unwrap()
        .is_empty());
    let restored = f.store.unarchive_note(&note.id, "tester", None).unwrap();
    assert!(restored.archived_at.is_none());
}

#[test]
fn pinning_a_note_is_idempotent_and_keeps_its_order() {
    let f = fx();
    let note = f
        .store
        .create_note(
            &CreateNoteInput {
                board: Some("rocky".into()),
                title: "고정".into(),
                ..Default::default()
            },
            "tester",
        )
        .unwrap();
    assert!(note.pinned_at.is_none());

    let pinned = f
        .store
        .set_note_pinned(&note.id, true, "tester", None)
        .unwrap();
    let first = pinned.pinned_at.clone().expect("고정 시각");
    // 다시 고정해도 시각이 밀리지 않고 히스토리도 늘지 않는다.
    let again = f
        .store
        .set_note_pinned(&note.id, true, "tester", None)
        .unwrap();
    assert_eq!(again.pinned_at.as_deref(), Some(first.as_str()));
    let actions = |f: &Fx| {
        f.store
            .list_history(&ListHistoryFilter {
                entity_id: Some(note.id.clone()),
                ..Default::default()
            })
            .unwrap()
            .into_iter()
            .map(|h| h.action)
            .filter(|a| a == "pin" || a == "unpin")
            .count()
    };
    assert_eq!(actions(&f), 1);

    let unpinned = f
        .store
        .set_note_pinned(&note.id, false, "tester", None)
        .unwrap();
    assert!(unpinned.pinned_at.is_none());
    assert_eq!(actions(&f), 2);
}

#[test]
fn global_note_has_no_board() {
    let f = fx();
    let note = f
        .store
        .create_note(
            &CreateNoteInput {
                title: "글로벌".into(),
                ..Default::default()
            },
            "tester",
        )
        .unwrap();
    assert!(note.board_id.is_none());
    let globals = f
        .store
        .list_notes(&ListNotesFilter {
            global: true,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(globals.len(), 1);
}

// ── history ─────────────────────────────────────────────────────────────────

#[test]
fn mutations_are_recorded_with_actor_action_diff() {
    let f = fx();
    let todo = create(&f.store, "rocky", "이전", "logan");
    f.store
        .update_todo(
            &todo.id,
            &UpdateTodoPatch {
                title: Some("이후".into()),
                ..Default::default()
            },
            "claude-code",
            None,
        )
        .unwrap();
    let history = f
        .store
        .list_history(&ListHistoryFilter {
            entity_id: Some(todo.id),
            ..Default::default()
        })
        .unwrap();
    let update = history.iter().find(|h| h.action == "update").unwrap();
    assert_eq!(update.actor, "claude-code");
    assert_eq!(
        update.changes.as_ref().unwrap()["title"],
        serde_json::json!(["이전", "이후"])
    );
    assert!(history.iter().any(|h| h.action == "create"));
}

#[test]
fn note_edits_are_recorded() {
    let f = fx();
    let note = f
        .store
        .create_note(
            &CreateNoteInput {
                title: "메모".into(),
                ..Default::default()
            },
            "tester",
        )
        .unwrap();
    f.store
        .update_note(
            &note.id,
            &UpdateNotePatch {
                content: Some("내용".into()),
                ..Default::default()
            },
            "tester",
            None,
        )
        .unwrap();
    let history = f
        .store
        .list_history(&ListHistoryFilter {
            entity_id: Some(note.id),
            ..Default::default()
        })
        .unwrap();
    assert!(history
        .iter()
        .any(|h| h.action == "update" && h.entity == HistoryEntity::Note));
}

#[test]
fn exclude_actions_filters_at_query() {
    let f = fx();
    let todo = create(&f.store, "rocky", "작업", "logan");
    f.store
        .add_comment(&todo.id, "댓글", "logan", None)
        .unwrap();
    let all = f
        .store
        .list_history(&ListHistoryFilter {
            entity_id: Some(todo.id.clone()),
            ..Default::default()
        })
        .unwrap();
    assert!(all.iter().any(|h| h.action == "comment"));
    let filtered = f
        .store
        .list_history(&ListHistoryFilter {
            entity_id: Some(todo.id),
            exclude_actions: DETAIL_HISTORY_EXCLUDED
                .iter()
                .map(|s| s.to_string())
                .collect(),
            ..Default::default()
        })
        .unwrap();
    assert!(!filtered.iter().any(|h| h.action == "comment"));
    assert!(filtered.iter().any(|h| h.action == "create"));
}

// ── listChangesSince ────────────────────────────────────────────────────────

#[test]
fn changes_since_resolves_titles_and_board_key_oldest_first() {
    let f = fx();
    let todo = create(&f.store, "rocky", "피드 작업", "claude-code");
    let base = f.store.list_changes_since(0, None).unwrap();
    assert!(!base.entries.is_empty());

    f.store
        .update_todo(
            &todo.id,
            &UpdateTodoPatch {
                title: Some("피드 작업 v2".into()),
                ..Default::default()
            },
            "logan",
            None,
        )
        .unwrap();
    let note = f
        .store
        .create_note(
            &CreateNoteInput {
                board: Some("rocky".into()),
                title: "피드 메모".into(),
                content: None,
            },
            "logan",
        )
        .unwrap();

    let feed = f.store.list_changes_since(base.last_id, None).unwrap();
    assert!(feed.last_id > base.last_id);
    assert_eq!(feed.entries.len(), 2);
    assert_eq!(feed.entries[0].history.action, "update");
    assert_eq!(feed.entries[0].title, "피드 작업 v2");
    assert_eq!(feed.entries[0].board_key.as_deref(), Some("rocky"));
    assert_eq!(feed.entries[1].history.entity, HistoryEntity::Note);
    assert_eq!(feed.entries[1].title, "피드 메모");
    assert_eq!(feed.entries[1].history.entity_id, note.id);
}

#[test]
fn changes_since_no_new_changes() {
    let f = fx();
    create(&f.store, "rocky", "x", "tester");
    let last_id = f.store.list_changes_since(0, None).unwrap().last_id;
    let feed = f.store.list_changes_since(last_id, None).unwrap();
    assert!(feed.entries.is_empty());
    assert_eq!(feed.last_id, last_id);
}

// ── change events ───────────────────────────────────────────────────────────

#[test]
fn subscribe_receives_events_for_every_mutation() {
    use std::sync::{Arc, Mutex};
    let f = fx();
    let events: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = events.clone();
    let sub = f.store.subscribe(move |e| {
        sink.lock()
            .unwrap()
            .push(format!("{}:{}", e.entity.as_str(), e.action));
    });

    let todo = create(&f.store, "rocky", "evt", "tester");
    f.store
        .set_todo_status(&todo.id, StatusAction::Start, "tester", None)
        .unwrap();
    let note = f
        .store
        .create_note(
            &CreateNoteInput {
                title: "n".into(),
                content: Some("".into()),
                ..Default::default()
            },
            "tester",
        )
        .unwrap();
    f.store
        .update_note(
            &note.id,
            &UpdateNotePatch {
                content: Some("x".into()),
                ..Default::default()
            },
            "tester",
            None,
        )
        .unwrap();

    {
        let seen = events.lock().unwrap();
        assert!(seen.contains(&"todo:create".to_string()));
        assert!(seen.contains(&"todo:start".to_string()));
        assert!(seen.contains(&"note:create".to_string()));
        assert!(seen.contains(&"note:update".to_string()));
    }

    f.store.unsubscribe(sub);
    create(&f.store, "rocky", "evt2", "tester");
    let seen = events.lock().unwrap();
    assert_eq!(seen.iter().filter(|e| *e == "todo:create").count(), 1);
}

// ── number 발급 ─────────────────────────────────────────────────────────────

#[test]
fn numbers_are_sequential_within_board() {
    let f = fx();
    let a = create(&f.store, "alpha", "첫째", "tester");
    let b = create(&f.store, "alpha", "둘째", "tester");
    assert_eq!(a.number, 1);
    assert_eq!(b.number, 2);
}

#[test]
fn number_spaces_are_per_board() {
    let f = fx();
    create(&f.store, "alpha", "첫째", "tester");
    let other = create(&f.store, "beta", "다른 보드 첫째", "tester");
    assert_eq!(other.number, 1);
}

#[test]
fn archive_does_not_reclaim_numbers() {
    let f = fx();
    let a = create(&f.store, "alpha", "첫째", "tester");
    f.store
        .set_todo_status(&a.id, StatusAction::Archive, "tester", None)
        .unwrap();
    let b = create(&f.store, "alpha", "둘째", "tester");
    assert_eq!(b.number, 2);
}

#[test]
fn notes_get_numbers_per_board() {
    let f = fx();
    let n = f
        .store
        .create_note(
            &CreateNoteInput {
                board: Some("alpha".into()),
                title: "메모".into(),
                content: None,
            },
            "tester",
        )
        .unwrap();
    assert_eq!(n.number, 1);
}

#[test]
fn global_notes_have_independent_number_space() {
    let f = fx();
    f.store
        .create_note(
            &CreateNoteInput {
                board: Some("alpha".into()),
                title: "보드 메모".into(),
                content: None,
            },
            "tester",
        )
        .unwrap();
    let g = f
        .store
        .create_note(
            &CreateNoteInput {
                title: "글로벌 메모".into(),
                ..Default::default()
            },
            "tester",
        )
        .unwrap();
    assert_eq!(g.number, 1);
}

// ── 참조 해석 ───────────────────────────────────────────────────────────────

#[test]
fn legacy_scoped_ref_resolves() {
    let f = fx();
    let t = create(&f.store, "alpha", "대상", "tester");
    assert_eq!(
        f.store
            .get_todo(&format!("alpha#{}", t.number), None)
            .unwrap()
            .unwrap()
            .id,
        t.id
    );
}

#[test]
fn bare_and_hash_numbers_resolve_in_current_board() {
    let f = fx();
    let board = f.store.ensure_board("alpha", None, "tester").unwrap();
    let t = create(&f.store, "alpha", "대상", "tester");
    assert_eq!(
        f.store
            .get_todo(&format!("#{}", t.number), Some(&board.id))
            .unwrap()
            .unwrap()
            .id,
        t.id
    );
    assert_eq!(
        f.store
            .get_todo(&t.number.to_string(), Some(&board.id))
            .unwrap()
            .unwrap()
            .id,
        t.id
    );
}

#[test]
fn eight_char_base36_is_id_not_number() {
    let f = fx();
    let t = create(&f.store, "alpha", "대상", "tester");
    assert_eq!(f.store.get_todo(&t.id, None).unwrap().unwrap().id, t.id);
}

#[test]
fn short_string_resolves_as_id_prefix() {
    let f = fx();
    let t = create(&f.store, "alpha", "대상", "tester");
    let prefix = id_prefix(&t.id);
    assert_eq!(f.store.get_todo(&prefix, None).unwrap().unwrap().id, t.id);
}

#[test]
fn hash_number_without_board_context_errors() {
    let f = fx();
    create(&f.store, "alpha", "대상", "tester");
    let msg = err_of(f.store.get_todo("#1", None));
    assert!(msg.to_lowercase().contains("board"), "{msg}");
}

#[test]
fn missing_number_is_none() {
    let f = fx();
    create(&f.store, "alpha", "대상", "tester");
    assert!(f.store.get_todo("alpha#999", None).unwrap().is_none());
}

#[test]
fn hash_number_is_number_regardless_of_length() {
    let f = fx();
    create(&f.store, "alpha", "대상", "tester");
    // '#1234567' — '#' 가 붙으면 무조건 번호다(길이 게이트가 '#' 포함 길이로 재던 버그 회귀).
    let msg = err_of(f.store.get_todo("#1234567", None));
    assert!(msg.to_lowercase().contains("board"), "{msg}");
}

#[test]
fn hash_number_with_board_context_stays_in_number_branch() {
    let f = fx();
    let board = f.store.ensure_board("alpha", None, "tester").unwrap();
    let t = create(&f.store, "alpha", "대상", "tester");
    assert_eq!(
        f.store
            .get_todo(&format!("#{}", t.number), Some(&board.id))
            .unwrap()
            .unwrap()
            .id,
        t.id
    );
    // 존재하지 않는 큰 번호도 번호 분기로 라우팅 — id 매칭으로 새지 않고 조회만 실패.
    assert!(f
        .store
        .get_todo("#1234567", Some(&board.id))
        .unwrap()
        .is_none());
}

#[test]
fn exactly_eight_digit_number_is_treated_as_id() {
    let f = fx();
    assert!(f.store.get_todo("00000012", None).unwrap().is_none());
}

#[test]
fn global_note_resolves_without_board_context() {
    let f = fx();
    let g = f
        .store
        .create_note(
            &CreateNoteInput {
                title: "글로벌 메모".into(),
                content: Some("".into()),
                ..Default::default()
            },
            "tester",
        )
        .unwrap();
    assert_eq!(
        f.store
            .get_note(&format!("#{}", g.number), None)
            .unwrap()
            .unwrap()
            .id,
        g.id
    );
    assert_eq!(
        f.store
            .get_note(&g.number.to_string(), None)
            .unwrap()
            .unwrap()
            .id,
        g.id
    );
}

#[test]
fn board_and_global_notes_sharing_number_resolve_to_different_rows() {
    let f = fx();
    let board = f.store.ensure_board("alpha", None, "tester").unwrap();
    let board_note = f
        .store
        .create_note(
            &CreateNoteInput {
                board: Some("alpha".into()),
                title: "보드 메모".into(),
                content: None,
            },
            "tester",
        )
        .unwrap();
    let global_note = f
        .store
        .create_note(
            &CreateNoteInput {
                title: "글로벌 메모".into(),
                ..Default::default()
            },
            "tester",
        )
        .unwrap();
    assert_eq!(board_note.number, global_note.number);

    assert_eq!(
        f.store
            .get_note(&format!("#{}", global_note.number), None)
            .unwrap()
            .unwrap()
            .id,
        global_note.id
    );
    assert_eq!(
        f.store
            .get_note(&format!("#{}", board_note.number), Some(&board.id))
            .unwrap()
            .unwrap()
            .id,
        board_note.id
    );
}

#[test]
fn todos_have_no_global_number_space() {
    let f = fx();
    create(&f.store, "alpha", "대상", "tester");
    let msg = err_of(f.store.get_todo("#1", None));
    assert!(msg.to_lowercase().contains("board"), "{msg}");
}

#[test]
fn empty_and_blank_refs_are_rejected() {
    let f = fx();
    create(&f.store, "alpha", "유일한 항목", "tester");
    assert!(f.store.get_todo("", None).is_err());
    assert!(f.store.get_todo("   ", None).is_err());
}

#[test]
fn like_wildcards_in_id_prefix_are_rejected() {
    let f = fx();
    let t = create(&f.store, "alpha", "대상", "tester");
    let wildcard_prefix = format!("_{}", &t.id[1..]);
    assert!(err_of(f.store.get_todo(&wildcard_prefix, None)).contains("invalid id prefix"));
    assert!(err_of(f.store.get_todo("%", None)).contains("invalid id prefix"));
}

#[test]
fn case_mismatched_board_ref_is_none_not_other_board() {
    let f = fx();
    f.store.ensure_board("rocky", None, "tester").unwrap();
    create(&f.store, "rocky", "대상", "tester");
    assert!(f.store.get_todo("ROCKY#1", None).unwrap().is_none());
}

#[test]
fn uppercase_board_key_legacy_ref_resolves() {
    let f = fx();
    f.store.ensure_board("MyProject", None, "tester").unwrap();
    let t = create(&f.store, "MyProject", "대상", "tester");
    assert_eq!(
        f.store.get_todo("MyProject#1", None).unwrap().unwrap().id,
        t.id
    );
}

#[test]
fn underscore_board_key_legacy_ref_resolves_without_wildcard_guard() {
    let f = fx();
    f.store.ensure_board("_private", None, "tester").unwrap();
    let t = create(&f.store, "_private", "대상", "tester");
    assert_eq!(
        f.store.get_todo("_private#1", None).unwrap().unwrap().id,
        t.id
    );
}

#[test]
fn uppercase_board_key_dashed_ref_resolves() {
    let f = fx();
    f.store.ensure_board("MyProject", None, "tester").unwrap();
    let t = create(&f.store, "MyProject", "대상", "tester");
    assert_eq!(
        f.store.get_todo("MyProject-1", None).unwrap().unwrap().id,
        t.id
    );
}

#[test]
fn underscore_board_key_dashed_ref_resolves_without_wildcard_guard() {
    let f = fx();
    f.store.ensure_board("_private", None, "tester").unwrap();
    let t = create(&f.store, "_private", "대상", "tester");
    assert_eq!(
        f.store.get_todo("_private-1", None).unwrap().unwrap().id,
        t.id
    );
}

#[test]
fn dashed_ref_resolves() {
    let f = fx();
    let t = create(&f.store, "rocky", "신규 표기", "tester");
    assert_eq!(
        f.store
            .get_todo(&format!("rocky-{}", t.number), None)
            .unwrap()
            .unwrap()
            .id,
        t.id
    );
}

#[test]
fn dashed_ref_splits_at_rightmost_dash() {
    let f = fx();
    let t = create(&f.store, "rocky-todo", "하이픈 보드", "tester");
    assert_eq!(
        f.store
            .get_todo(&format!("rocky-todo-{}", t.number), None)
            .unwrap()
            .unwrap()
            .id,
        t.id
    );
}

#[test]
fn dashed_ref_to_missing_board_is_none() {
    let f = fx();
    create(&f.store, "rocky", "있음", "tester");
    assert!(f.store.get_todo("no-such-board-1", None).unwrap().is_none());
}

#[test]
fn underscore_board_both_notations_return_none_without_throwing() {
    let f = fx();
    create(&f.store, "rocky", "있음", "tester");
    assert!(f.store.get_todo("my_board-1", None).unwrap().is_none());
    assert!(f.store.get_todo("my_board#1", None).unwrap().is_none());
}

#[test]
fn note_prefix_is_global_and_ignores_board_context() {
    let f = fx();
    let board = f.store.ensure_board("rocky", None, "tester").unwrap();
    let global_note = f
        .store
        .create_note(
            &CreateNoteInput {
                title: "전역 메모".into(),
                ..Default::default()
            },
            "tester",
        )
        .unwrap();
    assert_eq!(
        f.store
            .get_note(&format!("note-{}", global_note.number), None)
            .unwrap()
            .unwrap()
            .id,
        global_note.id
    );
    assert_eq!(
        f.store
            .get_note(&format!("note-{}", global_note.number), Some(&board.id))
            .unwrap()
            .unwrap()
            .id,
        global_note.id
    );
}

#[test]
fn note_prefix_does_not_resolve_todos() {
    let f = fx();
    create(&f.store, "rocky", "있음", "tester");
    assert!(f.store.get_todo("note-1", None).unwrap().is_none());
}

#[test]
fn legacy_notation_still_resolves() {
    let f = fx();
    let t = create(&f.store, "rocky", "구 표기", "tester");
    assert_eq!(
        f.store
            .get_todo(&format!("rocky#{}", t.number), None)
            .unwrap()
            .unwrap()
            .id,
        t.id
    );
}

// ── comments ────────────────────────────────────────────────────────────────

#[test]
fn add_comment_stores_and_records_history_on_parent() {
    let f = fx();
    let todo = create(&f.store, "rocky", "작업", "logan");
    let comment = f
        .store
        .add_comment(&todo.id, "  진행 중입니다  ", "claude-code", None)
        .unwrap();

    assert_eq!(comment.todo_id, todo.id);
    assert_eq!(comment.actor, "claude-code");
    assert_eq!(comment.body, "진행 중입니다");
    assert!(comment.archived_at.is_none());

    let history = f
        .store
        .list_history(&ListHistoryFilter {
            entity_id: Some(todo.id),
            ..Default::default()
        })
        .unwrap();
    let entry = history.iter().find(|h| h.action == "comment").unwrap();
    assert_eq!(entry.entity, HistoryEntity::Todo);
    assert_eq!(entry.actor, "claude-code");
    assert_eq!(
        entry.changes.as_ref().unwrap()["comment"],
        serde_json::json!([null, "진행 중입니다"])
    );
}

#[test]
fn add_comment_accepts_board_scoped_ref() {
    let f = fx();
    let todo = create(&f.store, "rocky", "작업", "logan");
    let comment = f
        .store
        .add_comment(
            &format!("rocky#{}", todo.number),
            "참조로 달기",
            "logan",
            None,
        )
        .unwrap();
    assert_eq!(comment.todo_id, todo.id);
}

#[test]
fn add_comment_rejects_blank_body() {
    let f = fx();
    let todo = create(&f.store, "rocky", "작업", "logan");
    let msg = err_of(f.store.add_comment(&todo.id, "   \n  ", "logan", None));
    assert!(msg.contains("body is required"), "{msg}");
}

#[test]
fn list_comments_oldest_first_hides_archived_by_default() {
    let f = fx();
    let todo = create(&f.store, "rocky", "작업", "logan");
    let first = f
        .store
        .add_comment(&todo.id, "첫째", "logan", None)
        .unwrap();
    let second = f
        .store
        .add_comment(&todo.id, "둘째", "claude-code", None)
        .unwrap();

    let bodies: Vec<_> = f
        .store
        .list_comments(&todo.id, false)
        .unwrap()
        .into_iter()
        .map(|c| c.body)
        .collect();
    assert_eq!(bodies, vec!["첫째", "둘째"]);

    f.store
        .set_comment_archived(&first.id, true, "logan")
        .unwrap();
    let ids: Vec<_> = f
        .store
        .list_comments(&todo.id, false)
        .unwrap()
        .into_iter()
        .map(|c| c.id)
        .collect();
    assert_eq!(ids, vec![second.id.clone()]);
    let all: Vec<_> = f
        .store
        .list_comments(&todo.id, true)
        .unwrap()
        .into_iter()
        .map(|c| c.id)
        .collect();
    assert_eq!(all, vec![first.id, second.id]);
}

#[test]
fn same_millisecond_comments_keep_insertion_order() {
    let f = fx();
    let todo = create(&f.store, "rocky", "작업", "logan");
    let at = "2026-07-26T01:00:00.000Z";
    {
        // id 사전순을 삽입 순서와 반대로 심는다 — rowid 타이브레이크가 아니면 반드시 실패.
        let raw = f.raw();
        raw.execute(
            "INSERT INTO comments (id, todo_id, actor, body, created_at, updated_at) VALUES ('zzzzzzzz', ?1, 'logan', '먼저 쓴 댓글', ?2, ?2)",
            rusqlite::params![todo.id, at],
        )
        .unwrap();
        raw.execute(
            "INSERT INTO comments (id, todo_id, actor, body, created_at, updated_at) VALUES ('aaaaaaaa', ?1, 'logan', '나중에 쓴 댓글', ?2, ?2)",
            rusqlite::params![todo.id, at],
        )
        .unwrap();
    }
    let bodies: Vec<_> = f
        .store
        .list_comments(&todo.id, false)
        .unwrap()
        .into_iter()
        .map(|c| c.body)
        .collect();
    assert_eq!(bodies, vec!["먼저 쓴 댓글", "나중에 쓴 댓글"]);
}

#[test]
fn update_comment_rewrites_body_and_records_edit() {
    let f = fx();
    let todo = create(&f.store, "rocky", "작업", "logan");
    let comment = f
        .store
        .add_comment(&todo.id, "오타 있음", "logan", None)
        .unwrap();
    let updated = f
        .store
        .update_comment(&comment.id, "오타 고침", "logan")
        .unwrap();

    assert_eq!(updated.body, "오타 고침");
    let history = f
        .store
        .list_history(&ListHistoryFilter {
            entity_id: Some(todo.id),
            ..Default::default()
        })
        .unwrap();
    let entry = history.iter().find(|h| h.action == "comment-edit").unwrap();
    assert_eq!(
        entry.changes.as_ref().unwrap()["comment"],
        serde_json::json!(["오타 있음", "오타 고침"])
    );
}

#[test]
fn set_comment_archived_toggles_and_records_history() {
    let f = fx();
    let todo = create(&f.store, "rocky", "작업", "logan");
    let comment = f
        .store
        .add_comment(&todo.id, "잘못 달았다", "logan", None)
        .unwrap();

    let archived = f
        .store
        .set_comment_archived(&comment.id, true, "logan")
        .unwrap();
    assert!(archived.archived_at.is_some());
    let restored = f
        .store
        .set_comment_archived(&comment.id, false, "logan")
        .unwrap();
    assert!(restored.archived_at.is_none());

    let actions: Vec<_> = f
        .store
        .list_history(&ListHistoryFilter {
            entity_id: Some(todo.id),
            ..Default::default()
        })
        .unwrap()
        .into_iter()
        .map(|h| h.action)
        .collect();
    assert!(actions.contains(&"comment-archive".to_string()));
    assert!(actions.contains(&"comment-unarchive".to_string()));
}

#[test]
fn unknown_comment_id_is_not_found() {
    let f = fx();
    let msg = err_of(f.store.update_comment("nosuchid", "본문", "logan"));
    assert!(msg.contains("comment not found"), "{msg}");
}

#[test]
fn comment_stats_counts_only_unarchived() {
    let f = fx();
    let todo = create(&f.store, "rocky", "작업", "logan");
    let empty = f.store.comment_stats_of(&todo.id).unwrap();
    assert_eq!(empty.count, 0);
    assert!(empty.last_at.is_none());

    let first = f
        .store
        .add_comment(&todo.id, "첫째", "logan", None)
        .unwrap();
    let second = f
        .store
        .add_comment(&todo.id, "둘째", "logan", None)
        .unwrap();
    let stats = f.store.comment_stats_of(&todo.id).unwrap();
    assert_eq!(stats.count, 2);
    assert_eq!(stats.last_at.as_deref(), Some(second.created_at.as_str()));

    f.store
        .set_comment_archived(&second.id, true, "logan")
        .unwrap();
    let after = f.store.comment_stats_of(&todo.id).unwrap();
    assert_eq!(after.count, 1);
    assert_eq!(after.last_at.as_deref(), Some(first.created_at.as_str()));
}

// ── handoffs ────────────────────────────────────────────────────────────────

fn handoff_input(todo_ref: &str, session_id: &str, actor: &str) -> CreateHandoffInput {
    CreateHandoffInput {
        todo_ref: todo_ref.into(),
        session_id: session_id.into(),
        session_name: None,
        session_cwd: None,
        note: None,
        actor: actor.into(),
        current_board_id: None,
    }
}

#[test]
fn create_handoff_is_pending_and_recorded() {
    let f = fx();
    let todo = create(&f.store, "rocky-todo", "핸드오프 대상", "logan");
    let handoff = f
        .store
        .create_handoff(&CreateHandoffInput {
            session_name: Some("eelpout-a3".into()),
            session_cwd: Some("/w/rocky-todo/eelpout".into()),
            note: Some("테스트부터".into()),
            ..handoff_input(&todo.id, "sess-1", "logan")
        })
        .unwrap();

    assert_eq!(handoff.status, HandoffStatus::Pending);
    assert_eq!(handoff.todo_id, todo.id);
    assert_eq!(handoff.note, "테스트부터");
    let history = f
        .store
        .list_history(&ListHistoryFilter {
            entity_id: Some(todo.id),
            ..Default::default()
        })
        .unwrap();
    assert!(history.iter().any(|h| h.action == "handoff"));
}

#[test]
fn pending_handoff_of_returns_existing() {
    let f = fx();
    let todo = create(&f.store, "rocky-todo", "x", "logan");
    let first = f
        .store
        .create_handoff(&handoff_input(&todo.id, "sess-1", "logan"))
        .unwrap();
    assert_eq!(
        f.store.pending_handoff_of(&todo.id).unwrap().unwrap().id,
        first.id
    );
}

#[test]
fn cannot_handoff_archived_todo() {
    let f = fx();
    let todo = create(&f.store, "rocky-todo", "x", "logan");
    f.store
        .set_todo_status(&todo.id, StatusAction::Archive, "logan", None)
        .unwrap();
    let msg = err_of(
        f.store
            .create_handoff(&handoff_input(&todo.id, "s", "logan")),
    );
    assert!(msg.to_lowercase().contains("archived"), "{msg}");
}

#[test]
fn claim_takes_oldest_and_reports_remaining() {
    let f = fx();
    let a = create(&f.store, "rocky-todo", "첫째", "logan");
    let b = create(&f.store, "rocky-todo", "둘째", "logan");
    f.store
        .create_handoff(&handoff_input(&a.id, "sess-1", "logan"))
        .unwrap();
    f.store
        .create_handoff(&handoff_input(&b.id, "sess-1", "logan"))
        .unwrap();

    let claimed = f
        .store
        .claim_handoff("sess-1", HandoffVia::Stop)
        .unwrap()
        .unwrap();
    assert_eq!(claimed.todo_title, "첫째");
    assert_eq!(claimed.todo_ref, "rocky-todo-1");
    assert_eq!(claimed.remaining, 1);
    assert_eq!(claimed.handoff.status, HandoffStatus::Delivered);
    assert_eq!(claimed.handoff.delivered_via, Some(HandoffVia::Stop));

    let second = f
        .store
        .claim_handoff("sess-1", HandoffVia::Prompt)
        .unwrap()
        .unwrap();
    assert_eq!(second.todo_title, "둘째");
    assert_eq!(second.remaining, 0);

    assert!(f
        .store
        .claim_handoff("sess-1", HandoffVia::Stop)
        .unwrap()
        .is_none());
}

#[test]
fn claim_skips_pending_handoff_of_archived_todo() {
    let f = fx();
    let archived = create(&f.store, "rocky-todo", "접은 일", "logan");
    let live = create(&f.store, "rocky-todo", "산 일", "logan");
    f.store
        .create_handoff(&handoff_input(&archived.id, "sess-1", "logan"))
        .unwrap();
    f.store
        .create_handoff(&handoff_input(&live.id, "sess-1", "logan"))
        .unwrap();
    f.store
        .set_todo_status(&archived.id, StatusAction::Archive, "logan", None)
        .unwrap();

    // 더 오래된 요청이지만 todo 가 보관됐으니 건너뛰고, remaining 에도 세지 않는다.
    let claimed = f
        .store
        .claim_handoff("sess-1", HandoffVia::Stop)
        .unwrap()
        .unwrap();
    assert_eq!(claimed.todo_title, "산 일");
    assert_eq!(claimed.remaining, 0);
    assert!(f
        .store
        .claim_handoff("sess-1", HandoffVia::Stop)
        .unwrap()
        .is_none());

    // 보관을 풀면 그 요청은 그대로 pending 이라 다시 집힌다.
    f.store
        .set_todo_status(&archived.id, StatusAction::Unarchive, "logan", None)
        .unwrap();
    let again = f
        .store
        .claim_handoff("sess-1", HandoffVia::Prompt)
        .unwrap()
        .unwrap();
    assert_eq!(again.todo_title, "접은 일");
}

#[test]
fn claim_does_not_take_other_sessions_requests() {
    let f = fx();
    let todo = create(&f.store, "rocky-todo", "x", "logan");
    f.store
        .create_handoff(&handoff_input(&todo.id, "sess-1", "logan"))
        .unwrap();
    assert!(f
        .store
        .claim_handoff("sess-2", HandoffVia::Stop)
        .unwrap()
        .is_none());
}

#[test]
fn cancel_makes_cancelled_and_unclaimable() {
    let f = fx();
    let todo = create(&f.store, "rocky-todo", "x", "logan");
    let handoff = f
        .store
        .create_handoff(&handoff_input(&todo.id, "sess-1", "logan"))
        .unwrap();
    let cancelled = f.store.cancel_handoff(&handoff.id, "logan").unwrap();
    assert_eq!(cancelled.status, HandoffStatus::Cancelled);
    assert!(f
        .store
        .claim_handoff("sess-1", HandoffVia::Stop)
        .unwrap()
        .is_none());
    assert!(f.store.pending_handoff_of(&todo.id).unwrap().is_none());
}

#[test]
fn delivered_handoff_cannot_be_cancelled() {
    let f = fx();
    let todo = create(&f.store, "rocky-todo", "x", "logan");
    let handoff = f
        .store
        .create_handoff(&handoff_input(&todo.id, "sess-1", "logan"))
        .unwrap();
    f.store.claim_handoff("sess-1", HandoffVia::Stop).unwrap();
    let msg = err_of(f.store.cancel_handoff(&handoff.id, "logan"));
    assert!(msg.to_lowercase().contains("pending"), "{msg}");
}

#[test]
fn list_handoffs_open_includes_pending_and_undone_delivered() {
    let f = fx();
    let waiting = create(&f.store, "rocky-todo", "대기", "logan");
    let in_flight = create(&f.store, "rocky-todo", "진행", "logan");
    let finished = create(&f.store, "rocky-todo", "완료", "logan");
    f.store
        .create_handoff(&handoff_input(&waiting.id, "sess-1", "logan"))
        .unwrap();
    f.store
        .create_handoff(&handoff_input(&in_flight.id, "sess-2", "logan"))
        .unwrap();
    f.store
        .create_handoff(&handoff_input(&finished.id, "sess-3", "logan"))
        .unwrap();
    f.store.claim_handoff("sess-2", HandoffVia::Stop).unwrap();
    f.store.claim_handoff("sess-3", HandoffVia::Stop).unwrap();
    f.store
        .set_todo_status(&finished.id, StatusAction::Done, "claude-code", None)
        .unwrap();

    let board_id = f.store.board_id_of("rocky-todo").unwrap();
    let open = f
        .store
        .list_handoffs(&ListHandoffsFilter {
            board_id,
            open: true,
            ..Default::default()
        })
        .unwrap();
    let mut todo_ids: Vec<_> = open.iter().map(|h| h.todo_id.clone()).collect();
    todo_ids.sort();
    let mut expected = vec![in_flight.id, waiting.id];
    expected.sort();
    assert_eq!(todo_ids, expected);
}

#[test]
fn list_handoffs_open_excludes_archived_todo() {
    let f = fx();
    let kept = create(&f.store, "rocky-todo", "산 것", "logan");
    let archived = create(&f.store, "rocky-todo", "보관될 것", "logan");
    f.store
        .create_handoff(&handoff_input(&kept.id, "sess-1", "logan"))
        .unwrap();
    f.store
        .create_handoff(&handoff_input(&archived.id, "sess-2", "logan"))
        .unwrap();
    // 배달됐지만 미착수인 채로 todo 를 보관 — 옛 테스트 핸드오프가 이 모양으로 영원히 남았다.
    f.store.claim_handoff("sess-2", HandoffVia::Stop).unwrap();
    f.store
        .set_todo_status(&archived.id, StatusAction::Archive, "logan", None)
        .unwrap();

    let open = f
        .store
        .list_handoffs(&ListHandoffsFilter {
            open: true,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        open.iter().map(|h| h.todo_id.as_str()).collect::<Vec<_>>(),
        vec![kept.id.as_str()]
    );
    // open 없이 보면 여전히 보인다 — 기록은 지우지 않는다.
    let all = f
        .store
        .list_handoffs(&ListHandoffsFilter::default())
        .unwrap();
    assert_eq!(all.len(), 2);

    // 보관 해제하면 다시 열린다.
    f.store
        .set_todo_status(&archived.id, StatusAction::Unarchive, "logan", None)
        .unwrap();
    let open = f
        .store
        .list_handoffs(&ListHandoffsFilter {
            open: true,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(open.len(), 2);
}

#[test]
fn list_handoffs_filters_by_board() {
    let f = fx();
    let mine = create(&f.store, "rocky-todo", "x", "logan");
    let other = create(&f.store, "forses", "y", "logan");
    f.store
        .create_handoff(&handoff_input(&mine.id, "s1", "logan"))
        .unwrap();
    f.store
        .create_handoff(&handoff_input(&other.id, "s2", "logan"))
        .unwrap();

    let board_id = f.store.board_id_of("rocky-todo").unwrap();
    let listed = f
        .store
        .list_handoffs(&ListHandoffsFilter {
            board_id,
            status: Some(HandoffStatus::Pending),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].todo_id, mine.id);
}

#[test]
fn handoff_actions_are_excluded_from_changes_feed() {
    let f = fx();
    let todo = create(&f.store, "rocky-todo", "x", "logan");
    let before = f.store.list_changes_since(0, None).unwrap().last_id;
    f.store
        .create_handoff(&handoff_input(&todo.id, "sess-1", "logan"))
        .unwrap();

    let feed = f.store.list_changes_since(before, None).unwrap();
    assert!(!feed
        .entries
        .iter()
        .any(|e| e.history.action.starts_with("handoff")));
    // 커서는 그래도 전진해야 한다 — 아니면 같은 항목을 영원히 다시 읽는다.
    assert!(feed.last_id > before);
}

fn spawned_input(todo_ref: &str) -> CreateSpawnedHandoffInput {
    CreateSpawnedHandoffInput {
        todo_ref: todo_ref.into(),
        session_id: "5acaaaeb".into(),
        session_name: "rocky-todo-16".into(),
        session_cwd: "/repo/.claude/worktrees/todo-16".into(),
        note: None,
        actor: "logan".into(),
        current_board_id: None,
    }
}

#[test]
fn spawned_handoff_is_delivered_via_spawn() {
    let f = fx();
    let todo = create(&f.store, "rocky-todo", "세션 띄우기", "logan");
    let handoff = f
        .store
        .create_spawned_handoff(&CreateSpawnedHandoffInput {
            note: Some("테스트부터".into()),
            ..spawned_input(&todo.id)
        })
        .unwrap();
    assert_eq!(handoff.status, HandoffStatus::Delivered);
    assert_eq!(handoff.delivered_via, Some(HandoffVia::Spawn));
    assert!(handoff.delivered_at.is_some());
    assert_eq!(
        handoff.session_cwd.as_deref(),
        Some("/repo/.claude/worktrees/todo-16")
    );
}

#[test]
fn spawned_handoff_is_not_claimable() {
    let f = fx();
    let todo = create(&f.store, "rocky-todo", "x", "logan");
    f.store
        .create_spawned_handoff(&spawned_input(&todo.id))
        .unwrap();
    assert!(f.store.pending_handoff_of(&todo.id).unwrap().is_none());
    assert!(f
        .store
        .claim_handoff("5acaaaeb", HandoffVia::Stop)
        .unwrap()
        .is_none());
}

#[test]
fn spawned_handoff_records_handoff_spawn_history() {
    let f = fx();
    let todo = create(&f.store, "rocky-todo", "x", "logan");
    f.store
        .create_spawned_handoff(&spawned_input(&todo.id))
        .unwrap();
    let actions: Vec<_> = f
        .store
        .list_history(&ListHistoryFilter {
            entity_id: Some(todo.id),
            ..Default::default()
        })
        .unwrap()
        .into_iter()
        .map(|h| h.action)
        .collect();
    assert!(actions.contains(&"handoff-spawn".to_string()));
}

#[test]
fn spawned_handoff_excluded_from_changes_feed() {
    let f = fx();
    let todo = create(&f.store, "rocky-todo", "x", "logan");
    let before = f.store.list_changes_since(0, None).unwrap().last_id;
    f.store
        .create_spawned_handoff(&spawned_input(&todo.id))
        .unwrap();

    let feed = f.store.list_changes_since(before, None).unwrap();
    assert!(!feed
        .entries
        .iter()
        .any(|e| e.history.action.starts_with("handoff")));
    assert!(feed.last_id > before);
}

#[test]
fn spawned_handoff_rejects_archived_todo() {
    let f = fx();
    let todo = create(&f.store, "rocky-todo", "x", "logan");
    f.store
        .set_todo_status(&todo.id, StatusAction::Archive, "logan", None)
        .unwrap();
    let msg = err_of(f.store.create_spawned_handoff(&spawned_input(&todo.id)));
    assert!(msg.contains("archived"), "{msg}");
}

// ── 라이프사이클 — start/done 귀속 ──────────────────────────────────────────

/// 배달까지 마친 핸드오프 하나 — 라이프사이클 테스트의 공통 출발점.
fn delivered(f: &Fx, session_id: &str) -> (Todo, String) {
    let todo = create(&f.store, "rocky-todo", "작업", "logan");
    let handoff = f
        .store
        .create_handoff(&handoff_input(&todo.id, session_id, "logan"))
        .unwrap();
    f.store.claim_handoff(session_id, HandoffVia::Stop).unwrap();
    (todo, handoff.id)
}

fn handoff_by_id(f: &Fx, id: &str) -> Handoff {
    f.store
        .list_handoffs(&ListHandoffsFilter::default())
        .unwrap()
        .into_iter()
        .find(|h| h.id == id)
        .unwrap()
}

#[test]
fn start_stamps_accepted_and_attributes_session() {
    let f = fx();
    let (todo, handoff_id) = delivered(&f, "sess-1");

    let started = f
        .store
        .set_todo_status(&todo.id, StatusAction::Start, "claude-code", None)
        .unwrap();

    assert_eq!(started.doing_session_id.as_deref(), Some("sess-1"));
    let h = handoff_by_id(&f, &handoff_id);
    assert!(h.accepted_at.is_some());
    assert!(h.completed_at.is_none());
}

#[test]
fn done_stamps_completed_and_clears_attribution() {
    let f = fx();
    let (todo, handoff_id) = delivered(&f, "sess-1");
    f.store
        .set_todo_status(&todo.id, StatusAction::Start, "claude-code", None)
        .unwrap();

    let finished = f
        .store
        .set_todo_status(&todo.id, StatusAction::Done, "claude-code", None)
        .unwrap();

    assert!(finished.doing_session_id.is_none());
    assert!(handoff_by_id(&f, &handoff_id).completed_at.is_some());
}

#[test]
fn done_without_start_stamps_accepted_too() {
    let f = fx();
    let (todo, handoff_id) = delivered(&f, "sess-1");

    f.store
        .set_todo_status(&todo.id, StatusAction::Done, "claude-code", None)
        .unwrap();

    let after = handoff_by_id(&f, &handoff_id);
    assert!(after.accepted_at.is_some());
    assert_eq!(after.accepted_at, after.completed_at);
}

#[test]
fn human_start_does_not_attribute() {
    let f = fx();
    let (todo, handoff_id) = delivered(&f, "sess-1");

    let started = f
        .store
        .set_todo_status(&todo.id, StatusAction::Start, "logan", None)
        .unwrap();

    assert!(started.doing_session_id.is_none());
    assert!(handoff_by_id(&f, &handoff_id).accepted_at.is_none());
}

#[test]
fn stop_clears_attribution_but_keeps_accepted() {
    let f = fx();
    let (todo, handoff_id) = delivered(&f, "sess-1");
    f.store
        .set_todo_status(&todo.id, StatusAction::Start, "claude-code", None)
        .unwrap();

    let stopped = f
        .store
        .set_todo_status(&todo.id, StatusAction::Stop, "claude-code", None)
        .unwrap();

    assert!(stopped.doing_session_id.is_none());
    let h = handoff_by_id(&f, &handoff_id);
    assert!(h.accepted_at.is_some());
    assert!(h.completed_at.is_none());
}

#[test]
fn multiple_delivered_attributes_oldest() {
    let f = fx();
    let todo = create(&f.store, "rocky-todo", "x", "logan");
    let first = f
        .store
        .create_handoff(&handoff_input(&todo.id, "sess-1", "logan"))
        .unwrap();
    f.store.claim_handoff("sess-1", HandoffVia::Stop).unwrap();
    let second = f
        .store
        .create_handoff(&handoff_input(&todo.id, "sess-2", "logan"))
        .unwrap();
    f.store.claim_handoff("sess-2", HandoffVia::Stop).unwrap();

    f.store
        .set_todo_status(&todo.id, StatusAction::Start, "claude-code", None)
        .unwrap();

    assert!(handoff_by_id(&f, &first.id).accepted_at.is_some());
    assert!(handoff_by_id(&f, &second.id).accepted_at.is_none());
}

#[test]
fn done_closes_in_flight_leaving_unaccepted_untouched() {
    let f = fx();
    let todo = create(&f.store, "rocky-todo", "x", "logan");
    let first = f
        .store
        .create_handoff(&handoff_input(&todo.id, "sess-1", "logan"))
        .unwrap();
    f.store.claim_handoff("sess-1", HandoffVia::Stop).unwrap();
    f.store
        .set_todo_status(&todo.id, StatusAction::Start, "claude-code", None)
        .unwrap();
    let second = f
        .store
        .create_handoff(&handoff_input(&todo.id, "sess-2", "logan"))
        .unwrap();
    f.store.claim_handoff("sess-2", HandoffVia::Stop).unwrap();

    f.store
        .set_todo_status(&todo.id, StatusAction::Done, "claude-code", None)
        .unwrap();

    assert!(handoff_by_id(&f, &first.id).completed_at.is_some());
    let h2 = handoff_by_id(&f, &second.id);
    assert!(h2.accepted_at.is_none());
    assert!(h2.completed_at.is_none());
}

#[test]
fn start_without_delivered_behaves_as_before() {
    let f = fx();
    let todo = create(&f.store, "rocky-todo", "x", "logan");

    let started = f
        .store
        .set_todo_status(&todo.id, StatusAction::Start, "claude-code", None)
        .unwrap();

    assert_eq!(started.status, TodoStatus::Doing);
    assert_eq!(started.doing_by.as_deref(), Some("claude-code"));
    assert!(started.doing_session_id.is_none());
}

#[test]
fn reopen_does_not_revert_completion() {
    let f = fx();
    let (todo, handoff_id) = delivered(&f, "sess-1");
    f.store
        .set_todo_status(&todo.id, StatusAction::Done, "claude-code", None)
        .unwrap();

    f.store
        .set_todo_status(&todo.id, StatusAction::Reopen, "logan", None)
        .unwrap();

    assert!(handoff_by_id(&f, &handoff_id).completed_at.is_some());
}

// ── moveTodo ────────────────────────────────────────────────────────────────

fn move_setup(f: &Fx) -> (Todo, Todo, Todo) {
    let a = create(&f.store, "rocky-todo", "a", "t");
    let b = create(&f.store, "rocky-todo", "b", "t");
    let c = create(&f.store, "rocky-todo", "c", "t");
    (a, b, c)
}

fn titles(f: &Fx, include_archived: bool) -> Vec<String> {
    f.store
        .list_todos(&ListTodosFilter {
            board: Some("rocky-todo".into()),
            include_archived,
            ..Default::default()
        })
        .unwrap()
        .into_iter()
        .map(|t| t.title)
        .collect()
}

#[test]
fn move_before_target() {
    let f = fx();
    let (a, _b, c) = move_setup(&f);
    f.store.move_todo(&c.id, Some(&a.id), "t", None).unwrap();
    assert_eq!(titles(&f, false), vec!["c", "a", "b"]);
}

#[test]
fn move_keeps_archived_relative_order() {
    let f = fx();
    let (a, b, c) = move_setup(&f);
    f.store
        .set_todo_status(&b.id, StatusAction::Archive, "t", None)
        .unwrap();
    f.store.move_todo(&c.id, Some(&a.id), "t", None).unwrap();
    assert_eq!(titles(&f, true), vec!["c", "a", "b"]);
}

#[test]
fn move_before_null_is_end() {
    let f = fx();
    let (a, _b, _c) = move_setup(&f);
    f.store.move_todo(&a.id, None, "t", None).unwrap();
    assert_eq!(titles(&f, false), vec!["b", "c", "a"]);
}

#[test]
fn move_before_self_is_noop() {
    let f = fx();
    let (_a, b, _c) = move_setup(&f);
    f.store.move_todo(&b.id, Some(&b.id), "t", None).unwrap();
    assert_eq!(titles(&f, false), vec!["a", "b", "c"]);
}

#[test]
fn move_rejects_target_in_other_board() {
    let f = fx();
    let (a, _b, _c) = move_setup(&f);
    let other = create(&f.store, "other", "x", "t");
    assert!(f
        .store
        .move_todo(&a.id, Some(&other.id), "t", None)
        .is_err());
}

#[test]
fn move_records_reorder_history() {
    let f = fx();
    let (a, _b, c) = move_setup(&f);
    f.store.move_todo(&c.id, Some(&a.id), "t", None).unwrap();
    let entry = &f
        .store
        .list_history(&ListHistoryFilter {
            entity_id: Some(c.id),
            ..Default::default()
        })
        .unwrap()[0];
    assert_eq!(entry.action, "reorder");
}

// ── moveTodoToBoard ─────────────────────────────────────────────────────────

#[test]
fn move_to_board_issues_new_number() {
    let f = fx();
    create(&f.store, "target", "t1", "t");
    let moving = create(&f.store, "origin", "m", "t");
    let moved = f
        .store
        .move_todo_to_board(&moving.id, "target", "t", None)
        .unwrap();
    assert_eq!(moved.number, 2); // target 의 MAX+1
    let target_titles: Vec<_> = f
        .store
        .list_todos(&ListTodosFilter {
            board: Some("target".into()),
            ..Default::default()
        })
        .unwrap()
        .into_iter()
        .map(|t| t.title)
        .collect();
    assert!(target_titles.contains(&"m".to_string()));
    assert!(f
        .store
        .list_todos(&ListTodosFilter {
            board: Some("origin".into()),
            ..Default::default()
        })
        .unwrap()
        .is_empty());
}

#[test]
fn move_to_board_links_same_named_section_only() {
    let f = fx();
    f.store.ensure_board("origin", None, "t").unwrap();
    let target = f.store.ensure_board("target", None, "t").unwrap();
    f.store.ensure_section(&target.id, "설계", "t").unwrap();
    let with_match = f
        .store
        .create_todo(
            &CreateTodoInput {
                section: Some("설계".into()),
                ..todo_input("origin", "a")
            },
            "t",
        )
        .unwrap();
    let without_match = f
        .store
        .create_todo(
            &CreateTodoInput {
                section: Some("백로그".into()),
                ..todo_input("origin", "b")
            },
            "t",
        )
        .unwrap();
    let moved_a = f
        .store
        .move_todo_to_board(&with_match.id, "target", "t", None)
        .unwrap();
    let moved_b = f
        .store
        .move_todo_to_board(&without_match.id, "target", "t", None)
        .unwrap();
    let target_sections = f.store.list_sections(&target.id, false).unwrap();
    let section_titles: Vec<_> = target_sections.iter().map(|s| s.title.clone()).collect();
    assert_eq!(section_titles, vec!["설계"]); // 몰래 만들지 않는다
    assert_eq!(
        moved_a.section_id.as_deref(),
        Some(target_sections[0].id.as_str())
    );
    assert!(moved_b.section_id.is_none());
}

#[test]
fn move_to_board_rejects_todo_with_children() {
    let f = fx();
    let parent = create(&f.store, "origin", "p", "t");
    f.store
        .create_todo(
            &CreateTodoInput {
                parent_id: Some(parent.id.clone()),
                ..todo_input("origin", "c")
            },
            "t",
        )
        .unwrap();
    let msg = err_of(f.store.move_todo_to_board(&parent.id, "target", "t", None));
    assert!(msg.contains("children"), "{msg}");
}

#[test]
fn move_to_board_severs_parent_link() {
    let f = fx();
    let parent = create(&f.store, "origin", "p", "t");
    let child = f
        .store
        .create_todo(
            &CreateTodoInput {
                parent_id: Some(parent.id.clone()),
                ..todo_input("origin", "c")
            },
            "t",
        )
        .unwrap();
    let moved = f
        .store
        .move_todo_to_board(&child.id, "target", "t", None)
        .unwrap();
    assert!(moved.parent_id.is_none());
}

#[test]
fn move_to_board_records_history_with_board_and_number() {
    let f = fx();
    let moving = create(&f.store, "origin", "m", "t");
    f.store
        .move_todo_to_board(&moving.id, "target", "t", None)
        .unwrap();
    let entry = &f
        .store
        .list_history(&ListHistoryFilter {
            entity_id: Some(moving.id),
            ..Default::default()
        })
        .unwrap()[0];
    assert_eq!(entry.action, "move-board");
    assert_eq!(
        entry.changes.as_ref().unwrap()["board"],
        serde_json::json!(["origin", "target"])
    );
}

// ── 노트 CRDT 문서 ───────────────────────────────────────────────────────────

fn note_with(f: &Fx, content: &str) -> Note {
    f.store
        .create_note(
            &CreateNoteInput {
                board: Some("rocky".into()),
                title: "패드".into(),
                content: Some(content.into()),
            },
            "tester",
        )
        .unwrap()
}

/// 웹(yjs)이 고치는 동안 에이전트가 set 으로 다른 자리를 고쳐도 둘 다 남는다 — 설계의 핵심
/// 약속. 클라이언트 쪽은 같은 `NoteDoc` 으로 흉내 낸다.
#[test]
fn a_web_edit_and_an_agent_set_on_the_same_note_both_survive() {
    use rocky_core::note_doc::NoteDoc;
    let f = fx();
    let note = note_with(&f, "## 계획\n- a\n\n## 메모\n");
    let state = f.store.note_doc_state(&note.id, None, None).unwrap();
    let web = NoteDoc::open(Some(&state.update), &note.content);
    web.set_text("## 계획\n- a\n- b\n\n## 메모\n");
    // 에이전트가 먼저 set(전체 본문) — 둘째 절에 문장 추가.
    f.store
        .update_note(
            &note.id,
            &UpdateNotePatch {
                content: Some("## 계획\n- a\n\n## 메모\n에이전트\n".into()),
                ..Default::default()
            },
            "agent",
            None,
        )
        .unwrap();
    // 웹의 변경이 뒤늦게 도착.
    let diff = web.diff_since(&state.state_vector).unwrap();
    let applied = f
        .store
        .apply_note_update(&note.id, &diff, "logan", None, None)
        .unwrap();
    assert!(applied.changed);
    assert_eq!(
        applied.note.content,
        "## 계획\n- a\n- b\n\n## 메모\n에이전트\n"
    );
    // 목록도 같은 본문(content 가 읽는 쪽의 진실).
    let listed = f.store.get_note(&note.id, None).unwrap().unwrap();
    assert_eq!(listed.content, applied.note.content);
}

#[test]
fn a_known_update_changes_nothing_and_records_nothing() {
    let f = fx();
    let note = note_with(&f, "x");
    let state = f.store.note_doc_state(&note.id, None, None).unwrap();
    let before = f
        .store
        .list_history(&ListHistoryFilter {
            entity_id: Some(note.id.clone()),
            ..Default::default()
        })
        .unwrap()
        .len();
    let applied = f
        .store
        .apply_note_update(&note.id, &state.update, "logan", None, None)
        .unwrap();
    assert!(!applied.changed);
    let after = f
        .store
        .list_history(&ListHistoryFilter {
            entity_id: Some(note.id.clone()),
            ..Default::default()
        })
        .unwrap()
        .len();
    assert_eq!(before, after);
}

/// 글자마다 히스토리 한 줄이면 잡음이다 — 같은 actor 의 연속 편집은 한 줄로 묶인다.
#[test]
fn web_edits_by_the_same_actor_coalesce_into_one_history_row() {
    use rocky_core::note_doc::NoteDoc;
    let f = fx();
    let note = note_with(&f, "");
    let state = f.store.note_doc_state(&note.id, None, None).unwrap();
    let web = NoteDoc::open(Some(&state.update), "");
    let mut sv = state.state_vector.clone();
    for piece in ["ㄱ", "ㄴ", "ㄷ"] {
        web.append(piece);
        let diff = web.diff_since(&sv).unwrap();
        let applied = f
            .store
            .apply_note_update(&note.id, &diff, "logan", None, None)
            .unwrap();
        assert!(applied.changed);
        sv = f
            .store
            .note_doc_state(&note.id, None, None)
            .unwrap()
            .state_vector;
    }
    let history = f
        .store
        .list_history(&ListHistoryFilter {
            entity_id: Some(note.id.clone()),
            ..Default::default()
        })
        .unwrap();
    let edits: Vec<_> = history.iter().filter(|h| h.action == "edit").collect();
    assert_eq!(edits.len(), 1, "{history:?}");
    assert_eq!(edits[0].actor, "logan");
    // 전역 이벤트도 글자마다는 아니다 — 첫 편집이 흘린 뒤 2초 안의 편집은 조용하다.
    let seen = std::sync::Arc::new(std::sync::Mutex::new(0u32));
    let counter = seen.clone();
    let sub = f.store.subscribe(move |e| {
        if e.action == "edit" {
            *counter.lock().unwrap() += 1;
        }
    });
    web.append("ㅁ");
    let diff = web.diff_since(&sv).unwrap();
    f.store
        .apply_note_update(&note.id, &diff, "logan", None, None)
        .unwrap();
    assert_eq!(
        *seen.lock().unwrap(),
        0,
        "2초 안 — 히스토리도 이벤트도 없다"
    );
    f.store.unsubscribe(sub);
    sv = f
        .store
        .note_doc_state(&note.id, None, None)
        .unwrap()
        .state_vector;
    // 다른 actor 가 끼어들면 새 줄.
    let other = NoteDoc::open(
        Some(&f.store.note_doc_state(&note.id, None, None).unwrap().update),
        "",
    );
    other.append("ㄹ");
    let diff = other.diff_since(&sv).unwrap();
    f.store
        .apply_note_update(&note.id, &diff, "codex", None, None)
        .unwrap();
    let history = f
        .store
        .list_history(&ListHistoryFilter {
            entity_id: Some(note.id.clone()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(history.iter().filter(|h| h.action == "edit").count(), 2);
}

#[test]
fn doc_state_diff_since_and_bad_input() {
    let f = fx();
    let note = note_with(&f, "hello");
    let first = f.store.note_doc_state(&note.id, None, None).unwrap();
    f.store
        .update_note(
            &note.id,
            &UpdateNotePatch {
                content: Some("world".into()),
                mode: NoteContentMode::Append,
                ..Default::default()
            },
            "agent",
            None,
        )
        .unwrap();
    let diff = f
        .store
        .note_doc_state(&note.id, Some(&first.state_vector), None)
        .unwrap();
    assert!(
        diff.update.len()
            < f.store
                .note_doc_state(&note.id, None, None)
                .unwrap()
                .update
                .len()
    );
    assert!(f
        .store
        .note_doc_state(&note.id, Some(b"\xff"), None)
        .is_err());
    assert!(f
        .store
        .apply_note_update(&note.id, b"junk", "x", None, None)
        .is_err());
    assert!(f.store.note_doc_state("nope-1", None, None).is_err());
}

/// 본문이 그대로인 update(같은 글자를 지웠다 넣음)도 state 는 저장돼야 한다 — 안 그러면 그것에
/// 기대는 다음 update 가 영영 안 붙는다. `changed` 와 히스토리는 본문 기준 그대로.
#[test]
fn a_text_preserving_update_is_still_stored_so_later_updates_apply() {
    use rocky_core::note_doc::NoteDoc;
    let f = fx();
    let note = note_with(&f, "x");
    let state = f.store.note_doc_state(&note.id, None, None).unwrap();
    let web = NoteDoc::open(Some(&state.update), "x");
    web.set_text("");
    web.set_text("x");
    let first = web.diff_since(&state.state_vector).unwrap();
    let applied = f
        .store
        .apply_note_update(&note.id, &first, "logan", Some("c1"), None)
        .unwrap();
    assert!(!applied.changed);
    // 그 위에 얹은 편집 — 앞의 update 가 저장돼 있어야 붙는다.
    let sv = f
        .store
        .note_doc_state(&note.id, None, None)
        .unwrap()
        .state_vector;
    web.append("y");
    let second = web.diff_since(&sv).unwrap();
    let applied = f
        .store
        .apply_note_update(&note.id, &second, "logan", Some("c1"), None)
        .unwrap();
    assert!(applied.changed);
    assert_eq!(applied.note.content, "x\ny");
}

/// 어느 경로의 편집이든 문서 갱신 이벤트가 난다 — 서버가 노트별 SSE 로 방송할 재료.
#[test]
fn every_edit_path_emits_a_note_doc_event() {
    use rocky_core::note_doc::NoteDoc;
    let f = fx();
    let note = note_with(&f, "a");
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<NoteDocEvent>::new()));
    let sink = seen.clone();
    f.store
        .subscribe_note_docs(move |e| sink.lock().unwrap().push(e.clone()));
    // 에이전트 append.
    f.store
        .update_note(
            &note.id,
            &UpdateNotePatch {
                content: Some("b".into()),
                mode: NoteContentMode::Append,
                ..Default::default()
            },
            "codex",
            None,
        )
        .unwrap();
    // 웹 update.
    let state = f.store.note_doc_state(&note.id, None, None).unwrap();
    let web = NoteDoc::open(Some(&state.update), "a\nb");
    web.append("c");
    let diff = web.diff_since(&state.state_vector).unwrap();
    f.store
        .apply_note_update(&note.id, &diff, "logan", Some("c1"), None)
        .unwrap();
    let events = seen.lock().unwrap().clone();
    assert_eq!(events.len(), 2, "{events:?}");
    assert_eq!(
        (events[0].actor.as_str(), events[0].client.as_deref()),
        ("codex", None)
    );
    assert_eq!(
        (events[1].actor.as_str(), events[1].client.as_deref()),
        ("logan", Some("c1"))
    );
    // 방송된 조각을 처음 상태에 얹으면 서버 본문이 된다.
    let replay = NoteDoc::open(None, "a");
    let seeded =
        NoteDoc::from_state(&f.store.note_doc_state(&note.id, None, None).unwrap().update).unwrap();
    assert_eq!(seeded.text(), "a\nb\nc");
    drop(replay);
}

/// 역순 배치 — 뒤 배치가 먼저 와도 저장돼 있다가 선행 배치가 오면 본문이 된다(문서를
/// 요청마다 새로 여는 스토어에서 특히 중요하다).
#[test]
fn an_out_of_order_batch_is_kept_until_its_predecessor_arrives() {
    use rocky_core::note_doc::NoteDoc;
    let f = fx();
    let note = note_with(&f, "x");
    let state = f.store.note_doc_state(&note.id, None, None).unwrap();
    let web = NoteDoc::from_state(&state.update).unwrap();
    let sv0 = web.state_vector();
    web.append("first");
    let sv1 = web.state_vector();
    let batch1 = web.diff_since(&sv0).unwrap();
    web.append("second");
    let batch2 = web.diff_since(&sv1).unwrap();
    let applied = f
        .store
        .apply_note_update(&note.id, &batch2, "logan", Some("c1"), None)
        .unwrap();
    assert!(!applied.changed);
    let applied = f
        .store
        .apply_note_update(&note.id, &batch1, "logan", Some("c1"), None)
        .unwrap();
    assert!(applied.changed);
    assert_eq!(applied.note.content, "x\nfirst\nsecond");
}

// ── PR 감시 ─────────────────────────────────────────────────────────────────

/// 알림 판정 — 전부 알림(필터 없음).
const ALL: &dyn Fn(&rocky_core::prwatch::PrEvent, Option<&str>) -> bool = &|_, _| true;

/// 레포의 기준선을 먼저 잡는다 — 처음 보는 레포의 첫 스냅숏은 전이를 내지 않는다.
fn baseline(store: &TodoStore, repo: &str) {
    assert!(store
        .apply_pr_snapshot(repo, &[], "rocky", ALL)
        .unwrap()
        .is_empty());
}

fn pr_snap(
    number: i64,
    state: &str,
    ready: bool,
    merge_state: &str,
) -> rocky_core::prwatch::PrSnapshot {
    rocky_core::prwatch::PrSnapshot {
        repo: "o/r".into(),
        number,
        title: format!("PR {number}"),
        url: format!("https://x/pull/{number}"),
        state: state.into(),
        is_draft: false,
        base: "main".into(),
        head: "abcdef0".into(),
        merge_state: merge_state.into(),
        ci: rocky_core::prwatch::CiState::Pass,
        unhandled: 0,
        unhandled_ids: vec![],
        decision: 0,
        ready,
        updated_at: "2026-09-28T10:00:00Z".into(),
        author: None,
    }
}

/// 전이는 그 레포를 둔 보드의 히스토리로 남고(actor rocky, action pr-*), 같은 스냅숏을 다시
/// 넣으면 아무 전이도 없다 — 데몬 재기동 뒤 재알림 방지.
#[test]
fn pr_snapshots_are_remembered_and_transitions_land_in_board_history() {
    let f = fx();
    f.store.ensure_board("rocky", None, "tester").unwrap();
    f.store.set_board_repo("rocky", "o/r", "tester").unwrap();
    baseline(&f.store, "o/r");
    let events = f
        .store
        .apply_pr_snapshot(
            "o/r",
            &[
                pr_snap(1, "OPEN", true, "CLEAN"),
                pr_snap(2, "OPEN", false, "CLEAN"),
            ],
            "rocky",
            ALL,
        )
        .unwrap();
    assert_eq!(
        events
            .iter()
            .map(|e| (e.number, e.kind))
            .collect::<Vec<_>>(),
        vec![
            (1, rocky_core::prwatch::PrEventKind::Opened),
            (1, rocky_core::prwatch::PrEventKind::Ready),
            (2, rocky_core::prwatch::PrEventKind::Opened)
        ]
    );
    let again = f
        .store
        .apply_pr_snapshot(
            "o/r",
            &[
                pr_snap(1, "OPEN", true, "CLEAN"),
                pr_snap(2, "OPEN", false, "CLEAN"),
            ],
            "rocky",
            ALL,
        )
        .unwrap();
    assert!(again.is_empty());
    let merged = f
        .store
        .apply_pr_snapshot(
            "o/r",
            &[
                pr_snap(1, "MERGED", false, "UNKNOWN"),
                pr_snap(2, "OPEN", false, "DIRTY"),
            ],
            "rocky",
            ALL,
        )
        .unwrap();
    assert_eq!(merged.len(), 2);
    let board_id = f.store.board_id_of("rocky").unwrap().unwrap();
    let history = f
        .store
        .list_history(&ListHistoryFilter {
            entity_id: Some(board_id),
            ..Default::default()
        })
        .unwrap();
    let actions: Vec<&str> = history
        .iter()
        .filter(|h| h.action.starts_with("pr-"))
        .map(|h| h.action.as_str())
        .collect();
    assert!(
        actions.contains(&"pr-ready")
            && actions.contains(&"pr-merged")
            && actions.contains(&"pr-conflict"),
        "{actions:?}"
    );
    let ready_row = history.iter().find(|h| h.action == "pr-ready").unwrap();
    assert_eq!(ready_row.actor, "rocky");
    assert_eq!(
        ready_row.changes.as_ref().unwrap().get("number"),
        Some(&serde_json::json!(1))
    );
    // 목록: open 만 / 전부.
    assert_eq!(f.store.list_prs(Some("o/r"), true).unwrap().len(), 1);
    assert_eq!(f.store.list_prs(None, false).unwrap().len(), 2);
}

/// 그 레포를 둔 보드가 없으면 스냅숏은 기억하되 히스토리는 안 남는다(붙일 곳이 없다).
#[test]
fn pr_events_without_a_board_are_returned_but_not_recorded() {
    let f = fx();
    baseline(&f.store, "no/board");
    let events = f
        .store
        .apply_pr_snapshot(
            "no/board",
            &[pr_snap(1, "OPEN", true, "CLEAN")],
            "rocky",
            ALL,
        )
        .unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(f.store.list_prs(Some("no/board"), false).unwrap().len(), 1);
}

/// 보드에서 repo 를 떼면 그 레포의 스냅숏은 걷힌다 — 옛 열린 PR 이 "지금" 표에 남지 않게.
#[test]
fn snapshots_of_unwatched_repos_are_pruned() {
    let f = fx();
    f.store
        .apply_pr_snapshot("o/keep", &[pr_snap(1, "OPEN", true, "CLEAN")], "rocky", ALL)
        .unwrap();
    f.store
        .apply_pr_snapshot("o/gone", &[pr_snap(2, "OPEN", true, "CLEAN")], "rocky", ALL)
        .unwrap();
    assert_eq!(f.store.retain_pr_repos(&["o/keep".to_string()]).unwrap(), 1);
    assert_eq!(f.store.list_prs(None, false).unwrap().len(), 1);
    assert_eq!(f.store.retain_pr_repos(&["o/keep".to_string()]).unwrap(), 0);
}

/// 수집함 "이미 올라감" 판정 근거 — 보드를 가리지 않고, 보관된 todo 의 링크도 포함한다(끝내고 보관한
/// 항목이 외부에서 아직 열려 있어도 다시 미올림으로 돌아오지 않게).
#[test]
fn linked_urls_span_every_board_and_include_archived_todos() {
    let f = fx();
    let link = |url: &str| TodoLink {
        url: url.into(),
        title: None,
    };
    let with_links = |board: &str, url: &str| CreateTodoInput {
        links: Some(vec![link(url)]),
        ..todo_input(board, "올린 것")
    };
    f.store
        .create_todo(&with_links("a", "https://x/1"), "tester")
        .unwrap();
    let archived = f
        .store
        .create_todo(&with_links("b", "https://x/2"), "tester")
        .unwrap();
    f.store
        .set_todo_status(&archived.id, StatusAction::Archive, "tester", None)
        .unwrap();
    create(&f.store, "c", "링크 없음", "tester");
    let urls = f.store.linked_urls().unwrap();
    assert_eq!(urls.len(), 2);
    assert!(urls.contains("https://x/1") && urls.contains("https://x/2"));
}

/// 보드에 repo 를 처음 붙인 첫 tick 은 기준선만 — 이미 머지 가능·이미 실패 중인 PR 이 한꺼번에 쏟아지지 않는다.
/// 다음 스냅숏부터의 실제 변화만 전이다. 레포를 떼면 표시가 걷혀, 다시 붙이면 또 기준선부터.
#[test]
fn first_snapshot_of_a_new_repo_is_a_silent_baseline() {
    let f = fx();
    f.store.ensure_board("rocky", None, "tester").unwrap();
    f.store.set_board_repo("rocky", "o/r", "tester").unwrap();
    let first = f
        .store
        .apply_pr_snapshot("o/r", &[pr_snap(1, "OPEN", true, "CLEAN")], "rocky", ALL)
        .unwrap();
    assert!(first.is_empty(), "첫 tick 은 기준선만: {first:?}");
    assert_eq!(
        f.store.list_prs(Some("o/r"), true).unwrap().len(),
        1,
        "스냅숏은 적는다"
    );
    let next = f
        .store
        .apply_pr_snapshot(
            "o/r",
            &[
                pr_snap(1, "OPEN", true, "CLEAN"),
                pr_snap(2, "OPEN", true, "CLEAN"),
            ],
            "rocky",
            ALL,
        )
        .unwrap();
    assert_eq!(
        next.iter().filter(|e| e.number == 2).count(),
        2,
        "새 PR 은 전이"
    );
    assert!(next.iter().all(|e| e.number == 2));
    f.store.retain_pr_repos(&[]).unwrap();
    let again = f
        .store
        .apply_pr_snapshot("o/r", &[pr_snap(3, "OPEN", true, "CLEAN")], "rocky", ALL)
        .unwrap();
    assert!(again.is_empty(), "떼었다 다시 붙이면 기준선부터");
}

/// 작성자 필터에 걸린 전이는 기록은 남되 `quiet` — 세션·배너·훅은 건너뛴다.
#[test]
fn filtered_transitions_are_recorded_as_quiet() {
    let f = fx();
    f.store.ensure_board("rocky", None, "tester").unwrap();
    f.store.set_board_repo("rocky", "o/r", "tester").unwrap();
    baseline(&f.store, "o/r");
    let mut mine = pr_snap(1, "OPEN", true, "CLEAN");
    mine.author = Some("me".into());
    let mut theirs = pr_snap(2, "OPEN", true, "CLEAN");
    theirs.author = Some("other".into());
    let only_me: &dyn Fn(&rocky_core::prwatch::PrEvent, Option<&str>) -> bool =
        &|e, _| e.author.as_deref() == Some("me");
    let events = f
        .store
        .apply_pr_snapshot("o/r", &[mine, theirs], "rocky", only_me)
        .unwrap();
    assert!(events.iter().filter(|e| e.number == 1).all(|e| !e.quiet));
    assert!(events.iter().filter(|e| e.number == 2).all(|e| e.quiet));
    let board_id = f.store.board_id_of("rocky").unwrap().unwrap();
    let history = f
        .store
        .list_history(&ListHistoryFilter {
            entity_id: Some(board_id),
            ..Default::default()
        })
        .unwrap();
    let quiet_rows = history
        .iter()
        .filter(|h| h.action.starts_with("pr-"))
        .filter(|h| {
            h.changes.as_ref().and_then(|c| c.get("quiet")) == Some(&serde_json::json!(true))
        })
        .count();
    assert_eq!(
        quiet_rows, 2,
        "남의 PR 의 전이(열림·머지 후보)도 기록은 남는다"
    );
}

/// 보드의 prAuthors — 저장·지우기, 모양이 틀린 값은 거부.
#[test]
fn board_pr_authors_round_trip_and_validate() {
    let f = fx();
    f.store.ensure_board("rocky", None, "tester").unwrap();
    let set = |v: Vec<&str>| {
        f.store.update_board(
            "rocky",
            &BoardPatch {
                pr_authors: Some(v.into_iter().map(String::from).collect()),
                ..Default::default()
            },
            "tester",
        )
    };
    let b = set(vec!["@me", "octo-cat"]).unwrap();
    assert_eq!(
        b.pr_authors,
        vec!["@me".to_string(), "octo-cat".to_string()]
    );
    assert!(set(vec!["bad name"]).is_err());
    assert!(set(vec![]).unwrap().pr_authors.is_empty());
}

/// 업그레이드(마이그레이션 12) — 이미 보던 레포(PR 기록이 있거나 보드에 붙은 레포)는 기준선을 잡은 것으로
/// 채운다. 안 그러면 올린 직후 첫 tick 이 그 사이의 진짜 변화(새 PR·머지 후보)를 기준선으로 삼켜 버린다.
/// (negative control: 시딩 두 줄을 빼면 두 단언이 실패한다 — 작성 때 확인.)
#[test]
fn upgrading_seeds_baselines_for_repos_already_watched() {
    let mut f = fx();
    f.store.ensure_board("rocky", None, "tester").unwrap();
    f.store.set_board_repo("rocky", "o/r", "tester").unwrap();
    f.store.ensure_board("quiet", None, "tester").unwrap();
    f.store
        .set_board_repo("quiet", "o/empty", "tester")
        .unwrap();
    baseline(&f.store, "o/r");
    f.store
        .apply_pr_snapshot("o/r", &[pr_snap(1, "OPEN", false, "CLEAN")], "rocky", ALL)
        .unwrap();
    // 11 버전 DB 처럼 — 기준선 표시가 없다.
    {
        let raw = f.raw();
        raw.execute_batch("DELETE FROM pr_watch_repos; PRAGMA user_version = 11;")
            .unwrap();
    }
    f.reload();
    let with_rows = f
        .store
        .apply_pr_snapshot(
            "o/r",
            &[
                pr_snap(1, "OPEN", false, "CLEAN"),
                pr_snap(2, "OPEN", true, "CLEAN"),
            ],
            "rocky",
            ALL,
        )
        .unwrap();
    assert!(
        with_rows.iter().any(|e| e.number == 2),
        "PR 기록이 있던 레포: {with_rows:?}"
    );
    let empty = f
        .store
        .apply_pr_snapshot(
            "o/empty",
            &[pr_snap(3, "OPEN", true, "CLEAN")],
            "rocky",
            ALL,
        )
        .unwrap();
    assert!(
        !empty.is_empty(),
        "보드에 붙어 있던 레포(PR 기록 없음)도 이미 보던 레포다"
    );
}

#[test]
fn pr_subscriptions_round_trip_and_hand_over() {
    let f = fx();
    let a = f.store.subscribe_pr("o/r", 3, Some("s1")).unwrap();
    assert_eq!(a.session_id.as_deref(), Some("s1"));
    // 다른 세션이 다시 구독하면 넘겨받는다 — 레포 대소문자는 가리지 않고 처음 표기를 지킨다.
    let b = f.store.subscribe_pr("O/R", 3, Some("s2")).unwrap();
    assert_eq!(
        (b.repo.as_str(), b.session_id.as_deref()),
        ("o/r", Some("s2"))
    );
    f.store.subscribe_pr("o/r", 4, None).unwrap();
    assert_eq!(f.store.pr_subscriptions().unwrap().len(), 2);
    assert!(f.store.subscribe_pr("nope", 1, None).is_err());
    assert!(f.store.subscribe_pr("o/r", 0, None).is_err());
    assert!(f.store.unsubscribe_pr("O/r", 3).unwrap());
    assert!(!f.store.unsubscribe_pr("o/r", 3).unwrap());
    assert!(f.store.pr_subscription("o/r", 3).unwrap().is_none());
}

/// `/clear` 된 세션을 "새 세션으로 넘기기" 로 정하면 구독이 새 id 로 간다 — PR·필터·수집함(본 항목 포함). 새 세션이
/// 이미 같은 소스를 구독했으면 그쪽을 두고, 다른 세션의 구독은 건드리지 않는다. 옛 받은편지함 등록은 적을 때 지운다.
#[test]
fn hand_over_session_moves_every_subscription_to_the_new_id() {
    let f = fx();
    f.store.subscribe_pr("o/r", 3, Some("old")).unwrap();
    f.store.subscribe_pr("o/r", 4, Some("other")).unwrap();
    f.store
        .subscribe_pr_filter("repo:o/r author:@me", Some("old"))
        .unwrap();
    f.store
        .subscribe_inbox(
            "gh-bugs",
            "old",
            "/tmp/cc-socks/1.sock",
            "fp",
            &["a".into()],
        )
        .unwrap();
    f.store
        .subscribe_inbox(
            "todoist",
            "old",
            "/tmp/cc-socks/1.sock",
            "fp",
            &["x".into()],
        )
        .unwrap();
    f.store
        .subscribe_inbox(
            "todoist",
            "new",
            "/tmp/cc-socks/1.sock",
            "fp2",
            &["y".into()],
        )
        .unwrap();
    f.store
        .save_session_inbox(&rocky_core::peer_inbox::InboxRegistration {
            session_id: "old".into(),
            socket: "/tmp/cc-socks/1.sock".into(),
            cwd: "/w".into(),
            seen_at: 100,
            restored: false,
        })
        .unwrap();

    assert!(f.store.mark_session_cleared("old", "new", "/w").unwrap());
    assert_eq!(
        f.store
            .resolve_cleared_session("old", ClearedAction::Handover)
            .unwrap(),
        Some((3, "new".to_string()))
    );

    let prs: Vec<(i64, Option<String>)> = f
        .store
        .pr_subscriptions()
        .unwrap()
        .into_iter()
        .map(|s| (s.number, s.session_id))
        .collect();
    assert_eq!(
        prs,
        vec![(3, Some("new".into())), (4, Some("other".into()))]
    );
    assert_eq!(
        f.store.pr_filter_subscriptions().unwrap()[0]
            .session_id
            .as_deref(),
        Some("new")
    );
    let inbox: Vec<(String, String, String)> = f
        .store
        .inbox_subscriptions()
        .unwrap()
        .into_iter()
        .map(|s| (s.source, s.session_id, s.fingerprint))
        .collect();
    assert_eq!(
        inbox,
        vec![
            ("gh-bugs".into(), "new".into(), "fp".into()),
            ("todoist".into(), "new".into(), "fp2".into()),
        ],
        "겹친 소스는 새 세션 것을 둔다"
    );
    assert!(
        f.store
            .unseen_inbox_ids("gh-bugs", "new", &["a".into(), "b".into()])
            .unwrap()
            .eq(&vec!["b".to_string()]),
        "본 항목도 따라간다"
    );
    assert!(f.store.load_session_inboxes(0).unwrap().is_empty());
}

/// 새 세션이 같은 조건의 필터를 이미 가졌으면 합친다 — 같은 검색을 두 번 돌리지 않고, 해지 한 번으로 그 필터로 들어온
/// PR 구독이 다 걷힌다. 아직 안 집힌 핸드오프도 새 id 로 가서 다음 턴에 집힌다.
#[test]
fn hand_over_session_merges_duplicate_filters_and_moves_pending_handoffs() {
    let f = fx();
    let old_filter = f
        .store
        .subscribe_pr_filter("author:@me", Some("old"))
        .unwrap();
    let new_filter = f
        .store
        .subscribe_pr_filter("author:@me", Some("new"))
        .unwrap();
    f.store
        .subscribe_pr_filter("label:review", Some("old"))
        .unwrap();
    f.store.subscribe_pr("o/r", 9, Some("old")).unwrap();
    // 필터 검색이 넣은 구독처럼 — 출처를 옛 필터로.
    Connection::open(&f.db_path)
        .unwrap()
        .execute(
            "UPDATE pr_subscriptions SET filter_id = ?1 WHERE number = 9",
            [&old_filter.id],
        )
        .unwrap();
    let todo = create(&f.store, "rocky", "넘길 일", "logan");
    f.store
        .create_handoff(&handoff_input(&todo.id, "old", "logan"))
        .unwrap();

    f.store.mark_session_cleared("old", "new", "/w").unwrap();
    f.store
        .resolve_cleared_session("old", ClearedAction::Handover)
        .unwrap()
        .unwrap();

    let filters: Vec<(String, Option<String>)> = f
        .store
        .pr_filter_subscriptions()
        .unwrap()
        .into_iter()
        .map(|x| (x.query, x.session_id))
        .collect();
    assert_eq!(
        filters,
        vec![
            ("author:@me".into(), Some("new".into())),
            ("label:review".into(), Some("new".into())),
        ],
        "겹친 필터는 하나로"
    );
    assert_eq!(
        f.store.unsubscribe_pr_filter(&new_filter.id).unwrap(),
        Some(1),
        "옛 필터로 들어온 PR 구독도 합친 필터 소속이라 같이 걷힌다"
    );
    assert!(f
        .store
        .claim_handoff("new", HandoffVia::Prompt)
        .unwrap()
        .is_some());
}

/// `/clear` 를 적는 것은 구독이 남았을 때만 — 정할 게 없으면 기록하지 않는다. 연달아 `/clear` 하면 후계가 따라간다.
/// "지켜보기만" 은 PR·필터 구독의 세션만 떼고(감시는 이어진다) 수집함 구독은 지우고, "해지" 는 전부 지운다. 정하면
/// 기록이 사라지고 다시 정하면 None.
#[test]
fn cleared_sessions_wait_for_a_decision_and_resolve_three_ways() {
    let f = fx();
    assert!(!f.store.mark_session_cleared("bare", "next", "/w").unwrap());
    assert!(f.store.cleared_sessions().unwrap().is_empty());

    for id in ["watcher", "quitter"] {
        f.store
            .subscribe_pr("o/r", if id == "watcher" { 1 } else { 2 }, Some(id))
            .unwrap();
        f.store
            .subscribe_pr_filter(&format!("label:{id}"), Some(id))
            .unwrap();
        f.store
            .subscribe_inbox("gh-bugs", id, "/tmp/cc-socks/1.sock", "fp", &[])
            .unwrap();
    }
    assert!(f
        .store
        .mark_session_cleared("watcher", "mid", "/w/rocky")
        .unwrap());
    assert!(f
        .store
        .mark_session_cleared("quitter", "q2", "/w/rocky")
        .unwrap());
    // 후계(mid)가 또 /clear — 기다리는 기록의 후계도 따라간다.
    f.store
        .mark_session_cleared("mid", "last", "/w/rocky")
        .unwrap();
    let cleared = f.store.cleared_sessions().unwrap();
    assert_eq!(cleared.len(), 2);
    assert_eq!(cleared[0].session_id, "watcher");
    assert_eq!(cleared[0].successor_id, "last");
    assert_eq!(cleared[0].prs, vec!["o/r#1".to_string()]);
    assert_eq!(cleared[0].filters, vec!["label:watcher".to_string()]);
    assert_eq!(cleared[0].inbox, vec!["gh-bugs".to_string()]);
    assert!(f.store.is_session_cleared("watcher").unwrap());

    f.store
        .resolve_cleared_session("watcher", ClearedAction::Watch)
        .unwrap()
        .unwrap();
    let pr1 = f.store.pr_subscription("o/r", 1).unwrap().unwrap();
    assert_eq!(pr1.session_id, None, "지켜보기 — 감시는 남고 세션만 뗀다");
    assert!(f
        .store
        .pr_filter_subscriptions()
        .unwrap()
        .iter()
        .any(|x| x.query == "label:watcher" && x.session_id.is_none()));
    assert!(!f
        .store
        .has_inbox_subscription("gh-bugs", "watcher")
        .unwrap());

    f.store
        .resolve_cleared_session("quitter", ClearedAction::Unsubscribe)
        .unwrap()
        .unwrap();
    assert!(f.store.pr_subscription("o/r", 2).unwrap().is_none());
    assert!(!f
        .store
        .pr_filter_subscriptions()
        .unwrap()
        .iter()
        .any(|x| x.query == "label:quitter"));

    assert!(f.store.cleared_sessions().unwrap().is_empty());
    assert!(!f.store.is_session_cleared("watcher").unwrap());
    assert_eq!(
        f.store
            .resolve_cleared_session("watcher", ClearedAction::Handover)
            .unwrap(),
        None
    );
}

/// 기다리는 사이(GraphQL 검색·어댑터·소켓 쓰기) 구독 주인이 바뀐 경우 — 옛 스냅숏의 세션 id 로 쓰지 않는다: 필터로
/// 넣는 PR 구독은 넣는 순간의 필터 행에서 세션을 읽고(해지됐으면 넣지 않는다), 본 항목·기준선은 그 세션의 구독이
/// 아직 있을 때만 쓴다.
#[test]
fn late_writes_follow_the_current_owner_not_a_stale_snapshot() {
    let f = fx();
    let filter = f
        .store
        .subscribe_pr_filter("author:@me", Some("old"))
        .unwrap();
    f.store
        .subscribe_inbox("gh-bugs", "old", "/tmp/cc-socks/1.sock", "fp", &[])
        .unwrap();
    f.store.mark_session_cleared("old", "new", "/w").unwrap();
    f.store
        .resolve_cleared_session("old", ClearedAction::Handover)
        .unwrap()
        .unwrap();

    assert!(f
        .store
        .subscribe_pr_via_filter("o/r", 1, &filter.id)
        .unwrap());
    assert_eq!(
        f.store
            .pr_subscription("o/r", 1)
            .unwrap()
            .unwrap()
            .session_id
            .as_deref(),
        Some("new"),
        "검색이 끝난 뒤의 필터 주인"
    );
    f.store.unsubscribe_pr_filter(&filter.id).unwrap();
    assert!(!f
        .store
        .subscribe_pr_via_filter("o/r", 2, &filter.id)
        .unwrap());

    f.store
        .mark_inbox_seen("gh-bugs", "old", &["a".into()])
        .unwrap();
    assert!(!f
        .store
        .rebaseline_inbox_subscription("gh-bugs", "old", "fp2", &["b".into()])
        .unwrap());
    assert!(
        !f.store.has_inbox_subscription("gh-bugs", "old").unwrap(),
        "되살리지 않는다"
    );
    assert_eq!(
        f.store
            .unseen_inbox_ids("gh-bugs", "new", &["a".into()])
            .unwrap(),
        vec!["a".to_string()],
        "옛 id 로 적은 본 항목이 없다 — 새 세션은 a 를 아직 안 봤다"
    );
    assert!(f
        .store
        .rebaseline_inbox_subscription("gh-bugs", "new", "fp2", &["b".into()])
        .unwrap());
}

/// 기다리는 동안 남은 구독이 다 없어지면(머지로 PR 구독이 풀림 등) 기록도 걷힌다.
#[test]
fn a_cleared_session_with_nothing_left_drops_out() {
    let f = fx();
    f.store.subscribe_pr("o/r", 5, Some("old")).unwrap();
    assert!(f.store.mark_session_cleared("old", "new", "/w").unwrap());
    f.store.unsubscribe_pr("o/r", 5).unwrap();
    assert!(f.store.cleared_sessions().unwrap().is_empty());
    assert!(!f.store.is_session_cleared("old").unwrap());
}

#[test]
fn snapshots_of_unsubscribed_prs_are_dropped() {
    let f = fx();
    f.store.subscribe_pr("o/r", 1, None).unwrap();
    let snap = |n: i64| rocky_core::prwatch::PrSnapshot {
        repo: "o/r".into(),
        number: n,
        title: format!("PR {n}"),
        url: format!("https://github.com/o/r/pull/{n}"),
        state: "OPEN".into(),
        is_draft: false,
        base: "main".into(),
        head: "abc".into(),
        merge_state: "CLEAN".into(),
        ci: rocky_core::prwatch::CiState::Pass,
        unhandled: 0,
        unhandled_ids: vec![],
        decision: 0,
        ready: false,
        updated_at: "2026-10-01T00:00:00Z".into(),
        author: None,
    };
    f.store
        .apply_pr_snapshot("o/r", &[snap(1), snap(2)], "rocky", &|_, _| true)
        .unwrap();
    assert_eq!(f.store.list_prs(Some("o/r"), false).unwrap().len(), 2);
    assert_eq!(f.store.retain_subscribed_prs().unwrap(), 1);
    let left: Vec<i64> = f
        .store
        .list_prs(Some("o/r"), false)
        .unwrap()
        .iter()
        .map(|p| p.number)
        .collect();
    assert_eq!(left, vec![1]);
}

#[test]
fn filter_subscriptions_feed_pr_subscriptions_without_stealing() {
    let f = fx();
    let filter = f
        .store
        .subscribe_pr_filter("repo:o/r author:@me", Some("s1"))
        .unwrap();
    // 같은 세션·같은 조건은 하나.
    assert_eq!(
        f.store
            .subscribe_pr_filter(" repo:o/r author:@me ", Some("s1"))
            .unwrap()
            .id,
        filter.id
    );
    assert!(f.store.subscribe_pr_filter("", None).is_err());
    assert!(f.store.subscribe_pr_filter("a\nb", None).is_err());
    // 다른 세션이 직접 구독한 PR 은 빼앗지 않는다.
    f.store.subscribe_pr("o/r", 1, Some("other")).unwrap();
    assert!(!f
        .store
        .subscribe_pr_via_filter("o/r", 1, &filter.id)
        .unwrap());
    assert!(f
        .store
        .subscribe_pr_via_filter("o/r", 2, &filter.id)
        .unwrap());
    assert!(f
        .store
        .subscribe_pr_via_filter("o/r", 3, &filter.id)
        .unwrap());
    // 세션이 3 을 직접 맡으면 필터 출처가 지워진다 — 필터를 해지해도 남는다.
    f.store.subscribe_pr("o/r", 3, Some("s1")).unwrap();
    assert_eq!(f.store.unsubscribe_pr_filter(&filter.id).unwrap(), Some(1));
    let left: Vec<i64> = f
        .store
        .pr_subscriptions()
        .unwrap()
        .iter()
        .map(|s| s.number)
        .collect();
    assert_eq!(left, vec![1, 3], "필터로 들어온 2 만 걷힌다");
    assert_eq!(f.store.unsubscribe_pr_filter(&filter.id).unwrap(), None);
}

#[test]
fn quick_check_is_ok_on_a_healthy_db() {
    let f = fx();
    assert_eq!(f.store.quick_check().unwrap(), "ok");
}

/// 세션이 스스로 든 doing 에만 귀속이 붙는다 — 에이전트가 방금 start 했고 귀속이 없을 때. 사람이 든 것·핸드오프로 받은 것·
/// doing 이 아닌 것은 그대로다. stop/done 은 귀속과 표시를 같이 지운다.
#[test]
fn claim_doing_session_only_for_a_fresh_agent_start() {
    let f = fx();
    let mine = create(&f.store, "rocky-todo", "스스로", "logan");
    f.store
        .set_todo_status(&mine.id, StatusAction::Start, "claude-code", None)
        .unwrap();
    let claimed = f
        .store
        .claim_doing_session(&mine.id, "sess-1")
        .unwrap()
        .unwrap();
    assert_eq!(claimed.doing_session_id.as_deref(), Some("sess-1"));
    assert!(claimed.doing_session_claimed);
    // 이미 귀속이 있으면 덮지 않는다.
    assert!(f
        .store
        .claim_doing_session(&mine.id, "sess-2")
        .unwrap()
        .is_none());

    let human = create(&f.store, "rocky-todo", "사람", "logan");
    f.store
        .set_todo_status(&human.id, StatusAction::Start, "logan", None)
        .unwrap();
    assert!(f
        .store
        .claim_doing_session(&human.id, "sess-1")
        .unwrap()
        .is_none());

    let idle = create(&f.store, "rocky-todo", "안 든 것", "logan");
    assert!(f
        .store
        .claim_doing_session(&idle.id, "sess-1")
        .unwrap()
        .is_none());

    // 핸드오프로 받은 것은 핸드오프 귀속이고 표시가 없다.
    let handed = create(&f.store, "rocky-todo", "넘겨받음", "logan");
    f.store
        .create_handoff(&handoff_input(&handed.id, "sess-3", "logan"))
        .unwrap();
    f.store.claim_handoff("sess-3", HandoffVia::Stop).unwrap();
    let started = f
        .store
        .set_todo_status(&handed.id, StatusAction::Start, "claude-code", None)
        .unwrap();
    assert_eq!(started.doing_session_id.as_deref(), Some("sess-3"));
    assert!(!started.doing_session_claimed);

    let stopped = f
        .store
        .set_todo_status(&mine.id, StatusAction::Stop, "claude-code", None)
        .unwrap();
    assert!(stopped.doing_session_id.is_none() && !stopped.doing_session_claimed);
}
