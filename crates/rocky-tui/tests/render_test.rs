//! 렌더 — TestBackend 로 실제 화면을 그려 핵심 줄이 있는지 본다. 픽셀 단위 스냅샷은 두지
//! 않는다(레이아웃 폭이 바뀔 때마다 깨진다) — 무엇이 어디에 나오는지만 고정한다.

use ratatui::backend::TestBackend;
use ratatui::Terminal;
use rocky_core::doing::DoingState;
use rocky_core::inbox::{InboxItem, InboxResponse, InboxSourceResult};
use rocky_core::refs::TodoView;
use rocky_core::types::{Board, Section, Todo, TodoLink, TodoPriority, TodoStatus};
use rocky_tui::api::SessionOut;
use rocky_tui::app::{App, Picker, Tab};
use rocky_tui::ui;

fn todo(n: i64, title: &str, status: TodoStatus, section: Option<&str>) -> TodoView {
    TodoView {
        todo: Todo {
            id: format!("id{n}"),
            number: n,
            board_id: "b1".into(),
            section_id: section.map(str::to_string),
            parent_id: None,
            title: title.into(),
            description: "설명 첫 줄\n둘째 줄".into(),
            status,
            priority: TodoPriority::P1,
            due: Some("2026-10-01".into()),
            labels: vec!["ui".into()],
            links: vec![TodoLink {
                url: "https://github.com/minjun0219/rocky/pull/143".into(),
                title: Some("PR #143".into()),
            }],
            doing_by: (status == TodoStatus::Doing).then(|| "claude-code".to_string()),
            doing_since: None,
            doing_session_id: None,
            position: n,
            created_at: "2026-09-27T00:00:00Z".into(),
            updated_at: "2026-09-27T00:00:00Z".into(),
            completed_at: None,
            archived_at: None,
        },
        r#ref: format!("rocky-{n}"),
        comment_count: 2,
        last_comment_at: None,
        doing_state: (status == TodoStatus::Doing).then_some(DoingState::Idle),
    }
}

/// 버퍼를 줄 문자열로. 폭 2 글자(한글·이모지) 뒤의 이어짐 셀은 건너뛴다 — 안 그러면 "설계" 가
/// "설 계" 로 조립된다.
fn lines(terminal: &Terminal<TestBackend>) -> Vec<String> {
    use unicode_width::UnicodeWidthStr;
    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|y| {
            let mut line = String::new();
            let mut skip = 0usize;
            for x in 0..buffer.area.width {
                if skip > 0 {
                    skip -= 1;
                    continue;
                }
                let symbol = buffer[(x, y)].symbol();
                line.push_str(symbol);
                skip = symbol.width().saturating_sub(1);
            }
            line
        })
        .collect()
}

fn has(lines: &[String], needle: &str) -> bool {
    lines.iter().any(|l| l.contains(needle))
}

#[test]
fn board_screen_shows_tabs_rows_detail_and_help() {
    let mut app = App::new("rocky".into());
    app.boards = vec![
        Board {
            id: "b1".into(),
            key: "rocky".into(),
            title: "rocky".into(),
            description: None,
            repo: None,
            path: None,
            created_at: String::new(),
            previous_keys: None,
            review_fix: false,
            archived_at: None,
            pr_authors: Vec::new(),
        },
        Board {
            id: "b2".into(),
            key: "tally".into(),
            title: "tally".into(),
            description: None,
            repo: None,
            path: None,
            created_at: String::new(),
            previous_keys: None,
            review_fix: false,
            archived_at: None,
            pr_authors: Vec::new(),
        },
    ];
    app.connected = true;
    app.set_board_data(
        vec![Section {
            id: "s1".into(),
            board_id: "b1".into(),
            title: "설계".into(),
            position: 1,
            archived_at: None,
        }],
        vec![
            todo(20, "버전 무관 진입점", TodoStatus::Done, None),
            todo(21, "TUI 기본", TodoStatus::Doing, Some("s1")),
        ],
    );

    let mut terminal = Terminal::new(TestBackend::new(100, 14)).unwrap();
    terminal.draw(|f| ui::render(f, &app)).unwrap();
    let out = lines(&terminal);

    assert!(
        has(&out, "[rocky]") && has(&out, " tally "),
        "탭: {:?}",
        out[0]
    );
    assert!(has(&out, "rocky · 2개"), "목록 제목");
    assert!(has(&out, "✓ 20"), "done 글리프");
    assert!(has(&out, "# 설계"), "섹션 머리글");
    assert!(has(&out, "◐ 21"), "doing idle 글리프");
    assert!(has(&out, "[ui]") && has(&out, "💬2"), "라벨·댓글 배지");
    // 첫 todo 가 선택돼 상세에 뜬다.
    assert!(
        has(&out, "rocky-20") && has(&out, "due 2026-10-01"),
        "상세 머리"
    );
    assert!(has(&out, "↗ PR #143"), "링크");
    assert!(has(&out, "설명 첫 줄"), "설명");
    assert!(
        has(&out, "j/k 이동") && has(&out, "h 핸드오프"),
        "도움말 줄"
    );

    // GitHub 요약과 핸드오프 대기가 상세에 붙는다.
    app.set_gh(
        "https://github.com/minjun0219/rocky/pull/143".into(),
        Some("PR #143 · merged".into()),
    );
    app.handoffs = vec![serde_json::json!({"todoId": "id20", "status": "pending"})];
    terminal.draw(|f| ui::render(f, &app)).unwrap();
    let out = lines(&terminal);
    assert!(has(&out, "PR #143 · merged"), "gh 요약");
    assert!(has(&out, "핸드오프 대기 1"), "핸드오프 표시");
    assert!(has(&out, "⇢1"), "목록 배지");
}

#[test]
fn inbox_tab_and_picker_overlay() {
    let mut app = App::new("rocky".into());
    app.tab = Tab::Inbox;
    app.set_inbox(InboxResponse {
        sources: vec![
            InboxSourceResult {
                name: "gtasks".into(),
                board: None,
                available: true,
                reason: None,
                fetched_at: "t".into(),
                items: vec![InboxItem {
                    id: "a".into(),
                    title: "폰에서 적은 것".into(),
                    url: Some("https://x/a".into()),
                    note: Some("본문 메모".into()),
                    due: Some("2026-10-02".into()),
                    created_at: None,
                    promoted: false,
                }],
            },
            InboxSourceResult {
                name: "broken".into(),
                board: None,
                available: false,
                reason: Some("exit 1: token expired".into()),
                fetched_at: "t".into(),
                items: vec![],
            },
        ],
    });
    let mut terminal = Terminal::new(TestBackend::new(100, 14)).unwrap();
    terminal.draw(|f| ui::render(f, &app)).unwrap();
    let out = lines(&terminal);
    assert!(has(&out, "[수집함]"), "탭");
    assert!(has(&out, "# gtasks (1)"), "소스 머리글");
    assert!(
        has(&out, "# broken") && has(&out, "exit 1: token expired"),
        "실패 소스 사유"
    );
    assert!(
        has(&out, "○ 폰에서 적은 것") && has(&out, "2026-10-02"),
        "항목"
    );
    assert!(has(&out, "p — rocky 보드 백로그로 올린다"), "올리기 안내");
    assert!(has(&out, "본문 메모"), "메모");
    assert!(has(&out, "p 보드로 올리기"), "도움말");

    app.tab = Tab::Board;
    app.picker = Some(Picker {
        todo_ref: "rocky-9".into(),
        choices: vec![SessionOut {
            session_id: "abc".into(),
            name: "rocky-1e".into(),
            cwd: "/w/rocky".into(),
            status: "idle".into(),
            matched: true,
        }],
        selected: 0,
    });
    terminal.draw(|f| ui::render(f, &app)).unwrap();
    let out = lines(&terminal);
    assert!(has(&out, "rocky-9 를 넘길 세션"), "피커 제목");
    assert!(
        has(&out, "* rocky-1e") && has(&out, "/w/rocky"),
        "피커 후보"
    );
}

#[test]
fn daemon_down_and_empty_board_are_visible() {
    let mut app = App::new("rocky".into());
    app.daemon_ok = false;
    app.notice = Some("/api/todos: connection refused".into());
    let mut terminal = Terminal::new(TestBackend::new(80, 8)).unwrap();
    terminal.draw(|f| ui::render(f, &app)).unwrap();
    let out = lines(&terminal);
    assert!(has(&out, "데몬 없음"), "상단 경고");
    assert!(has(&out, "connection refused"), "하단 notice");
    assert!(has(&out, "항목을 고르면"), "빈 상세");

    app.daemon_ok = true;
    app.connected = false;
    app.notice = None;
    terminal.draw(|f| ui::render(f, &app)).unwrap();
    let out = lines(&terminal);
    assert!(has(&out, "SSE 끊김"), "재연결 표시");
}
