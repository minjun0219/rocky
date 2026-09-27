//! 순수 상태 — 키 매핑, 섹션 묶기, 선택 유지, 보드 고르기. 터미널·HTTP 없음.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rocky_core::inbox::{InboxItem, InboxResponse, InboxSourceResult};
use rocky_core::refs::TodoView;
use rocky_core::types::TodoLink;
use rocky_core::types::{Board, Section, Todo, TodoPriority, TodoStatus};
use rocky_tui::api::SessionOut;
use rocky_tui::app::{
    build_inbox_rows, build_rows, key_to_action, pick_board, promote_body, Action, App, InboxRow,
    Picker, PickerOutcome, Row, Tab,
};

fn todo(n: i64, title: &str, section: Option<&str>, position: i64) -> TodoView {
    TodoView {
        todo: Todo {
            id: format!("id{n}"),
            number: n,
            board_id: "b1".into(),
            section_id: section.map(str::to_string),
            parent_id: None,
            title: title.into(),
            description: String::new(),
            status: TodoStatus::Todo,
            priority: TodoPriority::P2,
            due: None,
            labels: vec![],
            links: vec![],
            doing_by: None,
            doing_since: None,
            doing_session_id: None,
            position,
            created_at: "2026-09-27T00:00:00Z".into(),
            updated_at: "2026-09-27T00:00:00Z".into(),
            completed_at: None,
            archived_at: None,
        },
        r#ref: format!("rocky-{n}"),
        comment_count: 0,
        last_comment_at: None,
        doing_state: None,
    }
}

fn section(id: &str, title: &str, position: i64) -> Section {
    Section {
        id: id.into(),
        board_id: "b1".into(),
        title: title.into(),
        position,
        archived_at: None,
    }
}

fn board(key: &str, path: Option<&str>) -> Board {
    Board {
        id: format!("id-{key}"),
        key: key.into(),
        title: key.into(),
        description: None,
        repo: None,
        path: path.map(str::to_string),
        created_at: "2026-09-27T00:00:00Z".into(),
        previous_keys: None,
        archived_at: None,
    }
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

#[test]
fn keys_map_to_actions() {
    assert_eq!(key_to_action(key(KeyCode::Char('q'))), Action::Quit);
    assert_eq!(key_to_action(key(KeyCode::Esc)), Action::Quit);
    assert_eq!(
        key_to_action(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
        Action::Quit
    );
    assert_eq!(key_to_action(key(KeyCode::Char('j'))), Action::Down);
    assert_eq!(key_to_action(key(KeyCode::Up)), Action::Up);
    assert_eq!(key_to_action(key(KeyCode::Char(']'))), Action::NextBoard);
    assert_eq!(key_to_action(key(KeyCode::Char('['))), Action::PrevBoard);
    assert_eq!(key_to_action(key(KeyCode::Tab)), Action::ToggleTab);
    assert_eq!(key_to_action(key(KeyCode::Char('p'))), Action::Promote);
    assert_eq!(key_to_action(key(KeyCode::Char('h'))), Action::Handoff);
    assert_eq!(key_to_action(key(KeyCode::Char('n'))), Action::Spawn);
    assert_eq!(key_to_action(key(KeyCode::Char('i'))), Action::Issue);
    assert_eq!(
        key_to_action(key(KeyCode::Char('s'))),
        Action::Status("start")
    );
    assert_eq!(
        key_to_action(key(KeyCode::Char('d'))),
        Action::Status("done")
    );
    assert_eq!(
        key_to_action(key(KeyCode::Char('a'))),
        Action::Status("archive")
    );
    assert_eq!(key_to_action(key(KeyCode::Char('z'))), Action::None);
}

#[test]
fn rows_put_unsectioned_first_then_sections_by_position() {
    let sections = vec![
        section("s2", "백로그", 2),
        section("s1", "설계", 1),
        section("s9", "빈 섹션", 9),
    ];
    let todos = vec![
        todo(3, "설계 항목", Some("s1"), 30),
        todo(1, "섹션 없음 뒤", None, 20),
        todo(2, "섹션 없음 앞", None, 10),
        todo(4, "백로그 항목", Some("s2"), 40),
    ];
    let rows = build_rows(&sections, &todos);
    assert_eq!(
        rows,
        vec![
            Row::Todo(2),
            Row::Todo(1),
            Row::Header("설계".into()),
            Row::Todo(0),
            Row::Header("백로그".into()),
            Row::Todo(3),
        ]
    );
    // 항목 없는 섹션은 머리글도 안 낸다.
    assert!(!rows.contains(&Row::Header("빈 섹션".into())));
}

#[test]
fn selection_skips_headers_stops_at_ends_and_survives_refetch() {
    let mut app = App::new("rocky".into());
    let sections = vec![section("s1", "설계", 1)];
    app.set_board_data(
        sections.clone(),
        vec![
            todo(1, "a", None, 1),
            todo(2, "b", Some("s1"), 2),
            todo(3, "c", Some("s1"), 3),
        ],
    );
    // 첫 todo 가 자동 선택.
    assert_eq!(app.selected_todo().map(|t| t.todo.number), Some(1));
    app.move_selection(1); // 머리글을 건너뛰어 2
    assert_eq!(app.selected_todo().map(|t| t.todo.number), Some(2));
    app.move_selection(1);
    app.move_selection(1); // 끝에서 멈춤
    assert_eq!(app.selected_todo().map(|t| t.todo.number), Some(3));
    app.move_selection(-1);
    app.move_selection(-1); // 머리글 건너 1
    app.move_selection(-1); // 처음에서 멈춤
    assert_eq!(app.selected_todo().map(|t| t.todo.number), Some(1));

    // refetch 로 순서가 바뀌어도 ref 로 선택을 유지한다.
    app.move_selection(1);
    assert_eq!(
        app.selected_todo().map(|t| t.r#ref.as_str()),
        Some("rocky-2")
    );
    app.set_board_data(
        sections,
        vec![
            todo(3, "c", Some("s1"), 1),
            todo(2, "b", Some("s1"), 2),
            todo(1, "a", None, 3),
        ],
    );
    assert_eq!(
        app.selected_todo().map(|t| t.r#ref.as_str()),
        Some("rocky-2")
    );
    // 선택하던 항목이 사라지면 첫 항목으로.
    app.set_board_data(vec![], vec![todo(9, "z", None, 1)]);
    assert_eq!(app.selected_todo().map(|t| t.todo.number), Some(9));
    app.set_board_data(vec![], vec![]);
    assert!(app.selected_todo().is_none());
}

#[test]
fn board_cycling_wraps_and_resets_selection() {
    let mut app = App::new("tally".into());
    app.boards = vec![
        board("rocky", None),
        board("tally", None),
        board("mdwire", None),
    ];
    app.set_board_data(vec![], vec![todo(1, "a", None, 1)]);
    assert_eq!(app.cycle_board(1).as_deref(), Some("mdwire"));
    assert!(app.selected.is_none());
    assert_eq!(app.cycle_board(1).as_deref(), Some("rocky"));
    assert_eq!(app.cycle_board(-1).as_deref(), Some("mdwire"));
    // 목록에 없는 key 에서 시작하면 첫 보드로.
    app.board = "ghost".into();
    assert_eq!(app.cycle_board(1).as_deref(), Some("rocky"));
    app.boards.clear();
    assert_eq!(app.cycle_board(1), None);
}

#[test]
fn detail_needed_tracks_selection() {
    let mut app = App::new("rocky".into());
    assert_eq!(app.detail_needed(), None);
    app.set_board_data(vec![], vec![todo(1, "a", None, 1)]);
    assert_eq!(app.detail_needed().as_deref(), Some("rocky-1"));
}

#[test]
fn pick_board_prefers_explicit_then_path_then_key_segment_then_git() {
    let boards = vec![
        board("tally", Some("/w/money/tally-repo")),
        board("rocky", None),
    ];
    assert_eq!(pick_board(Some(" x "), &boards, None, None, None), "x");
    // boards.path 하위 — 디렉터리 이름이 key 와 달라도 잡는다(워크트리 규약).
    assert_eq!(
        pick_board(
            None,
            &boards,
            Some("/w/money/tally-repo/.claude/worktrees/todo-3"),
            None,
            None
        ),
        "tally"
    );
    // key 가 경로 세그먼트.
    assert_eq!(
        pick_board(None, &boards, Some("/w/rocky/sub"), None, None),
        "rocky"
    );
    // 둘 다 아니면 git remote 에서 유추.
    assert_eq!(
        pick_board(
            None,
            &boards,
            Some("/w/other"),
            Some("git@github.com:me/forses.git"),
            Some("/w/other")
        ),
        "forses"
    );
}

fn item(id: &str, title: &str, url: Option<&str>) -> InboxItem {
    InboxItem {
        id: id.into(),
        title: title.into(),
        url: url.map(str::to_string),
        note: Some("메모".into()),
        due: Some("2026-10-01".into()),
        created_at: None,
    }
}

fn inbox() -> InboxResponse {
    InboxResponse {
        sources: vec![
            InboxSourceResult {
                name: "gtasks".into(),
                available: true,
                reason: None,
                fetched_at: "t".into(),
                items: vec![item("a", "첫", Some("https://x/a")), item("b", "둘", None)],
            },
            InboxSourceResult {
                name: "broken".into(),
                available: false,
                reason: Some("exit 1".into()),
                fetched_at: "t".into(),
                items: vec![],
            },
            InboxSourceResult {
                name: "file".into(),
                available: true,
                reason: None,
                fetched_at: "t".into(),
                items: vec![item("c", "셋", Some("https://x/c"))],
            },
        ],
    }
}

#[test]
fn inbox_rows_and_selection_skip_source_headers() {
    let rows = build_inbox_rows(&inbox());
    assert_eq!(
        rows,
        vec![
            InboxRow::Source(0),
            InboxRow::Item(0, 0),
            InboxRow::Item(0, 1),
            InboxRow::Source(1),
            InboxRow::Source(2),
            InboxRow::Item(2, 0),
        ]
    );
    let mut app = App::new("rocky".into());
    app.tab = Tab::Inbox;
    app.set_inbox(inbox());
    assert_eq!(
        app.selected_inbox_item().map(|(_, i)| i.id.as_str()),
        Some("a")
    );
    app.move_selection(1);
    app.move_selection(1); // 머리글 둘을 건너 c
    assert_eq!(
        app.selected_inbox_item()
            .map(|(s, i)| (s.name.as_str(), i.id.as_str())),
        Some(("file", "c"))
    );
    app.move_selection(1); // 끝
    assert_eq!(
        app.selected_inbox_item().map(|(_, i)| i.id.as_str()),
        Some("c")
    );
    // refetch 로 순서가 바뀌어도 (소스, id) 로 유지.
    let mut swapped = inbox();
    swapped.sources.rotate_left(2);
    app.set_inbox(swapped);
    assert_eq!(
        app.selected_inbox_item().map(|(_, i)| i.id.as_str()),
        Some("c")
    );
}

#[test]
fn promoted_is_judged_by_board_links_and_url_presence() {
    let mut app = App::new("rocky".into());
    let mut t = todo(1, "올라간 것", None, 1);
    t.todo.links = vec![TodoLink {
        url: "https://x/a".into(),
        title: Some("gtasks: 첫".into()),
    }];
    app.set_board_data(vec![], vec![t]);
    assert!(app.is_promoted(&item("a", "첫", Some("https://x/a"))));
    assert!(!app.is_promoted(&item("c", "셋", Some("https://x/c"))));
    assert!(!app.is_promoted(&item("b", "둘", None))); // url 없으면 판정 불가
}

#[test]
fn promote_body_shape() {
    let body = promote_body("rocky", "gtasks", &item("a", "첫", Some("https://x/a")));
    assert_eq!(body["board"], "rocky");
    assert_eq!(body["title"], "첫");
    assert_eq!(body["section"], "백로그");
    assert_eq!(body["description"], "메모");
    assert_eq!(body["due"], "2026-10-01");
    assert_eq!(body["links"][0]["url"], "https://x/a");
    assert_eq!(body["links"][0]["title"], "gtasks: 첫");
    let bare = promote_body("rocky", "gtasks", &item("b", "둘", None));
    assert!(bare.get("links").is_none());
}

#[test]
fn picker_moves_confirms_and_cancels() {
    let sess = |id: &str, matched: bool| SessionOut {
        session_id: id.into(),
        name: id.into(),
        cwd: "/w".into(),
        status: "idle".into(),
        matched,
    };
    let mut picker = Picker {
        todo_ref: "rocky-1".into(),
        choices: vec![sess("s1", true), sess("s2", false)],
        selected: 0,
    };
    assert_eq!(
        picker.handle_key(key(KeyCode::Char('k'))),
        PickerOutcome::None
    );
    assert_eq!(picker.selected, 0);
    assert_eq!(
        picker.handle_key(key(KeyCode::Char('j'))),
        PickerOutcome::None
    );
    assert_eq!(
        picker.handle_key(key(KeyCode::Char('j'))),
        PickerOutcome::None
    );
    assert_eq!(picker.selected, 1); // 끝에서 멈춤
    assert_eq!(
        picker.handle_key(key(KeyCode::Enter)),
        PickerOutcome::Confirm("rocky-1".into(), "s2".into())
    );
    assert_eq!(picker.handle_key(key(KeyCode::Esc)), PickerOutcome::Cancel);
}

#[test]
fn open_handoffs_are_counted_per_todo() {
    let mut app = App::new("rocky".into());
    app.handoffs = vec![
        serde_json::json!({"todoId": "id1", "status": "pending"}),
        serde_json::json!({"todoId": "id1", "status": "delivered"}),
        serde_json::json!({"todoId": "id2", "status": "pending"}),
    ];
    assert_eq!(app.open_handoffs_for("id1"), 2);
    assert_eq!(app.open_handoffs_for("id9"), 0);
}

#[test]
fn gh_needed_filters_non_github_pending_and_cached() {
    let mut app = App::new("rocky".into());
    let mut t = todo(1, "x", None, 1);
    t.todo.links = vec![
        TodoLink {
            url: "https://github.com/o/r/pull/1".into(),
            title: None,
        },
        TodoLink {
            url: "https://github.com/o/r/issues/2".into(),
            title: None,
        },
        TodoLink {
            url: "https://github.com/o/r/blob/main/x.rs".into(),
            title: None,
        },
        TodoLink {
            url: "https://example.com".into(),
            title: None,
        },
    ];
    app.set_board_data(vec![], vec![t]);
    assert_eq!(
        app.gh_needed(),
        vec![
            "https://github.com/o/r/pull/1",
            "https://github.com/o/r/issues/2"
        ]
    );
    app.gh_pending
        .insert("https://github.com/o/r/pull/1".into());
    app.set_gh(
        "https://github.com/o/r/issues/2".into(),
        Some("이슈 #2 · open".into()),
    );
    assert!(app.gh_needed().is_empty());
    assert_eq!(
        app.gh_line("https://github.com/o/r/issues/2"),
        Some("이슈 #2 · open")
    );
}
