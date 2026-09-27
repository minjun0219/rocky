//! 화면 상태와 키 매핑 — 순수. 터미널도 HTTP 도 모른다(테스트가 그대로 돌린다).

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rocky_core::actor::{board_key_from, BoardKeySources};
use rocky_core::doing::DoingState;
use rocky_core::refs::TodoView;
use rocky_core::statusline::{board_key_for_cwd, BoardLocation};
use rocky_core::types::{Board, Section, TodoStatus};

use crate::api::TodoDetail;

/// 목록의 한 줄 — 섹션 머리글이거나 `todos` 의 인덱스.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    Header(String),
    Todo(usize),
}

/// 키 하나가 뜻하는 것. 실제 HTTP 호출은 `main` 이 한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    None,
    Quit,
    Down,
    Up,
    NextBoard,
    PrevBoard,
    Refresh,
    /// `POST /api/todos/:ref/status` 의 action 문자열.
    Status(&'static str),
}

/// 키 → 액션. vim 식(`j`/`k`)과 화살표 둘 다.
pub fn key_to_action(key: KeyEvent) -> Action {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return Action::Quit;
    }
    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => Action::Quit,
        KeyCode::Char('j') | KeyCode::Down => Action::Down,
        KeyCode::Char('k') | KeyCode::Up => Action::Up,
        KeyCode::Tab => Action::NextBoard,
        KeyCode::BackTab => Action::PrevBoard,
        KeyCode::Char('r') => Action::Refresh,
        KeyCode::Char('s') => Action::Status("start"),
        KeyCode::Char('x') => Action::Status("stop"),
        KeyCode::Char('d') => Action::Status("done"),
        KeyCode::Char('o') => Action::Status("reopen"),
        KeyCode::Char('a') => Action::Status("archive"),
        _ => Action::None,
    }
}

/// 상태 표시용 한 글자. doing 은 세션 판정까지 반영한다.
pub fn status_glyph(todo: &TodoView) -> &'static str {
    match todo.todo.status {
        TodoStatus::Todo => "○",
        TodoStatus::Done => "✓",
        TodoStatus::Doing => match todo.doing_state {
            Some(DoingState::Live) => "●",
            Some(DoingState::Idle) => "◐",
            Some(DoingState::Gone) => "◌",
            Some(DoingState::Unknown) | None => "◍",
        },
    }
}

#[derive(Debug, Default)]
pub struct App {
    pub boards: Vec<Board>,
    /// 현재 보드 key.
    pub board: String,
    pub sections: Vec<Section>,
    pub todos: Vec<TodoView>,
    pub rows: Vec<Row>,
    /// `rows` 의 인덱스 — 항상 `Row::Todo` 를 가리키거나 None.
    pub selected: Option<usize>,
    pub detail: Option<TodoDetail>,
    /// SSE 가 붙어 있나.
    pub connected: bool,
    /// 마지막 REST 호출이 성공했나 — 아니면 상단에 "데몬 없음".
    pub daemon_ok: bool,
    /// 하단 한 줄 — 마지막 에러나 안내.
    pub notice: Option<String>,
    pub should_quit: bool,
}

impl App {
    pub fn new(board: String) -> Self {
        App {
            board,
            daemon_ok: true,
            ..Default::default()
        }
    }

    /// 보드 데이터를 갈아끼운다. 선택은 **ref 로** 유지한다 — refetch 마다 커서가 튀면 못 쓴다.
    pub fn set_board_data(&mut self, sections: Vec<Section>, todos: Vec<TodoView>) {
        let keep = self.selected_todo().map(|t| t.r#ref.clone());
        self.rows = build_rows(&sections, &todos);
        self.sections = sections;
        self.todos = todos;
        self.selected = keep
            .and_then(|r| self.row_index_of(&r))
            .or_else(|| self.first_todo_row());
        if self.selected.is_none() {
            self.detail = None;
        }
    }

    fn row_index_of(&self, todo_ref: &str) -> Option<usize> {
        self.rows.iter().position(|row| match row {
            Row::Todo(i) => self.todos[*i].r#ref == todo_ref,
            Row::Header(_) => false,
        })
    }

    fn first_todo_row(&self) -> Option<usize> {
        self.rows.iter().position(|r| matches!(r, Row::Todo(_)))
    }

    pub fn selected_todo(&self) -> Option<&TodoView> {
        match self.selected.and_then(|i| self.rows.get(i)) {
            Some(Row::Todo(t)) => self.todos.get(*t),
            _ => None,
        }
    }

    /// 머리글을 건너뛰며 한 칸. 끝에서는 멈춘다(순환하지 않는다 — 목록이 길면 순환이 더 헷갈린다).
    pub fn move_selection(&mut self, delta: i32) {
        let Some(mut i) = self.selected else {
            self.selected = self.first_todo_row();
            return;
        };
        loop {
            let next = i as i64 + delta as i64;
            if next < 0 || next >= self.rows.len() as i64 {
                return;
            }
            i = next as usize;
            if matches!(self.rows[i], Row::Todo(_)) {
                self.selected = Some(i);
                return;
            }
        }
    }

    /// 보드 목록에서 앞뒤로. 현재 보드가 목록에 없으면(옛 key 등) 첫 보드로.
    pub fn cycle_board(&mut self, delta: i32) -> Option<String> {
        if self.boards.is_empty() {
            return None;
        }
        let n = self.boards.len() as i64;
        let current = self.boards.iter().position(|b| b.key == self.board);
        let next = match current {
            Some(i) => (i as i64 + delta as i64).rem_euclid(n) as usize,
            None => 0,
        };
        let key = self.boards[next].key.clone();
        if key == self.board {
            return None;
        }
        self.board = key.clone();
        self.selected = None;
        self.detail = None;
        Some(key)
    }

    /// 상세가 현재 선택과 맞지 않으면 그 ref — 호출자가 가져와 `detail` 에 넣는다.
    pub fn detail_needed(&self) -> Option<String> {
        let todo = self.selected_todo()?;
        match &self.detail {
            Some(d) if d.todo.r#ref == todo.r#ref => None,
            _ => Some(todo.r#ref.clone()),
        }
    }
}

/// 섹션 없는 항목 먼저(위치 순), 그다음 섹션 위치 순으로 머리글 + 항목. CLI `ls` 와 같은 모양.
pub fn build_rows(sections: &[Section], todos: &[TodoView]) -> Vec<Row> {
    let mut order: Vec<usize> = (0..todos.len()).collect();
    order.sort_by_key(|&i| todos[i].todo.position);
    let mut rows: Vec<Row> = order
        .iter()
        .copied()
        .filter(|&i| todos[i].todo.section_id.is_none())
        .map(Row::Todo)
        .collect();
    let mut sorted_sections: Vec<&Section> = sections
        .iter()
        .filter(|s| s.archived_at.is_none())
        .collect();
    sorted_sections.sort_by_key(|s| s.position);
    for section in sorted_sections {
        let members: Vec<usize> = order
            .iter()
            .copied()
            .filter(|&i| todos[i].todo.section_id.as_deref() == Some(section.id.as_str()))
            .collect();
        if members.is_empty() {
            continue;
        }
        rows.push(Row::Header(section.title.clone()));
        rows.extend(members.into_iter().map(Row::Todo));
    }
    rows
}

/// 기동 시 보드 고르기 — `--board` > `boards.path` 하위 / key 경로 세그먼트 > git remote 유추.
/// CLI 와 statusline 이 쓰는 규약 그대로.
pub fn pick_board(
    explicit: Option<&str>,
    boards: &[Board],
    cwd: Option<&str>,
    remote_url: Option<&str>,
    toplevel: Option<&str>,
) -> String {
    if let Some(key) = explicit.map(str::trim).filter(|k| !k.is_empty()) {
        return key.to_string();
    }
    let locations: Vec<BoardLocation> = boards
        .iter()
        .map(|b| BoardLocation {
            key: b.key.clone(),
            path: b.path.clone(),
        })
        .collect();
    if let Some(key) = board_key_for_cwd(&locations, cwd) {
        return key;
    }
    board_key_from(&BoardKeySources {
        remote_url,
        toplevel,
        cwd,
    })
}
