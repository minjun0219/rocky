//! 화면 상태와 키 매핑 — 순수. 터미널도 HTTP 도 모른다(테스트가 그대로 돌린다).

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rocky_core::actor::{board_key_from, BoardKeySources};
use rocky_core::doing::DoingState;
use rocky_core::inbox::{InboxItem, InboxResponse, InboxSourceResult};
use rocky_core::refs::TodoView;
use rocky_core::statusline::{board_key_for_cwd, BoardLocation};
use rocky_core::types::{Board, Section, TodoStatus};
use serde_json::Value;

use crate::api::{SessionOut, TodoDetail};
use crate::github::{github_ref, GH_CACHE_TTL};

/// 목록의 한 줄 — 섹션 머리글이거나 `todos` 의 인덱스.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    Header(String),
    Todo(usize),
}

/// 수집함 탭의 한 줄 — 소스 머리글이거나 (소스, 항목) 인덱스.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InboxRow {
    Source(usize),
    Item(usize, usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Board,
    Inbox,
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
    ToggleTab,
    Refresh,
    /// `POST /api/todos/:ref/status` 의 action 문자열.
    Status(&'static str),
    /// 수집함 항목을 보드로 올린다.
    Promote,
    /// 실행 중인 세션에 넘긴다(후보가 여럿이면 피커).
    Handoff,
    /// 새 워크트리 세션을 띄운다.
    Spawn,
    /// GitHub 이슈로 만든다.
    Issue,
}

/// 키 → 액션. vim 식(`j`/`k`)과 화살표 둘 다. 보드 전환은 `[`/`]`, 탭 전환은 Tab.
pub fn key_to_action(key: KeyEvent) -> Action {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return Action::Quit;
    }
    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => Action::Quit,
        KeyCode::Char('j') | KeyCode::Down => Action::Down,
        KeyCode::Char('k') | KeyCode::Up => Action::Up,
        KeyCode::Char(']') => Action::NextBoard,
        KeyCode::Char('[') => Action::PrevBoard,
        KeyCode::Tab | KeyCode::BackTab => Action::ToggleTab,
        KeyCode::Char('r') => Action::Refresh,
        KeyCode::Char('s') => Action::Status("start"),
        KeyCode::Char('x') => Action::Status("stop"),
        KeyCode::Char('d') => Action::Status("done"),
        KeyCode::Char('o') => Action::Status("reopen"),
        KeyCode::Char('a') => Action::Status("archive"),
        KeyCode::Char('p') => Action::Promote,
        KeyCode::Char('h') => Action::Handoff,
        KeyCode::Char('n') => Action::Spawn,
        KeyCode::Char('i') => Action::Issue,
        _ => Action::None,
    }
}

/// 이 탭에서 실행해도 되는 액션인가 — 보드 항목을 바꾸는 키는 보드 탭에서만, `p` 는 수집함에서만.
/// 수집함으로 넘어가도 보드 선택은 남아 있어서, 막지 않으면 **안 보이는** todo 를 바꾸게 된다.
pub fn action_allowed(action: Action, tab: Tab) -> bool {
    match action {
        Action::Status(_) | Action::Handoff | Action::Spawn | Action::Issue => tab == Tab::Board,
        Action::Promote => tab == Tab::Inbox,
        _ => true,
    }
}

/// 피커가 열려 있을 때의 키 결과.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickerOutcome {
    None,
    Cancel,
    /// (todo ref, session id)
    Confirm(String, String),
}

/// 핸드오프 대상 고르기 — 후보가 정확히 1개가 아닐 때만 열린다(데몬 계약과 같은 기준).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picker {
    pub todo_ref: String,
    pub choices: Vec<SessionOut>,
    pub selected: usize,
}

impl Picker {
    pub fn handle_key(&mut self, key: KeyEvent) -> PickerOutcome {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => PickerOutcome::Cancel,
            KeyCode::Char('j') | KeyCode::Down => {
                if self.selected + 1 < self.choices.len() {
                    self.selected += 1;
                }
                PickerOutcome::None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.selected = self.selected.saturating_sub(1);
                PickerOutcome::None
            }
            KeyCode::Enter => match self.choices.get(self.selected) {
                Some(choice) => {
                    PickerOutcome::Confirm(self.todo_ref.clone(), choice.session_id.clone())
                }
                None => PickerOutcome::Cancel,
            },
            _ => PickerOutcome::None,
        }
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
    pub tab: Tab,
    pub boards: Vec<Board>,
    /// 현재 보드 key.
    pub board: String,
    pub sections: Vec<Section>,
    pub todos: Vec<TodoView>,
    pub rows: Vec<Row>,
    /// `rows` 의 인덱스 — 항상 `Row::Todo` 를 가리키거나 None.
    pub selected: Option<usize>,
    pub detail: Option<TodoDetail>,
    /// 현재 보드 todos 의 `links[].url` — 방금 올린 항목을 다음 수집함 조회 전에도 ✓ 로 보이게.
    /// 판정의 정본은 데몬이 전 보드의 링크로 채운 `InboxItem::promoted` 다.
    pub promoted_urls: HashSet<String>,
    /// 열린 핸드오프(`GET /api/handoffs?open=true&board=`) — todo_id 별 대기 표시용.
    pub handoffs: Vec<Value>,
    pub inbox: Option<InboxResponse>,
    pub inbox_rows: Vec<InboxRow>,
    /// `inbox_rows` 의 인덱스 — 항상 `InboxRow::Item` 이거나 None.
    pub inbox_selected: Option<usize>,
    pub picker: Option<Picker>,
    /// GitHub 링크 → (조회 시각, 한 줄 요약. None 은 실패 — 줄을 비운다).
    pub gh: HashMap<String, (Instant, Option<String>)>,
    /// 조회 스레드가 도는 중인 링크.
    pub gh_pending: HashSet<String>,
    /// SSE 가 붙어 있나.
    pub connected: bool,
    /// 마지막 REST 호출이 성공했나 — 아니면 상단에 "데몬 없음".
    pub daemon_ok: bool,
    /// 하단 한 줄 — 마지막 에러나 안내.
    pub notice: Option<String>,
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
        self.promoted_urls = todos
            .iter()
            .flat_map(|t| t.todo.links.iter().map(|l| l.url.clone()))
            .collect();
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
        match self.tab {
            Tab::Board => {
                let is_item = |row: &Row| matches!(row, Row::Todo(_));
                self.selected = step(&self.rows, self.selected, delta, is_item);
            }
            Tab::Inbox => {
                let is_item = |row: &InboxRow| matches!(row, InboxRow::Item(_, _));
                self.inbox_selected = step(&self.inbox_rows, self.inbox_selected, delta, is_item);
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
        self.handoffs.clear();
        Some(key)
    }

    pub fn toggle_tab(&mut self) {
        self.tab = match self.tab {
            Tab::Board => Tab::Inbox,
            Tab::Inbox => Tab::Board,
        };
    }

    /// 상세가 현재 선택과 맞지 않으면 그 ref — 호출자가 가져와 `detail` 에 넣는다.
    pub fn detail_needed(&self) -> Option<String> {
        let todo = self.selected_todo()?;
        match &self.detail {
            Some(d) if d.todo.r#ref == todo.r#ref => None,
            _ => Some(todo.r#ref.clone()),
        }
    }

    // ── 수집함 ──

    pub fn set_inbox(&mut self, response: InboxResponse) {
        let keep = self
            .selected_inbox_item()
            .map(|(s, i)| (s.name.clone(), i.id.clone()));
        self.inbox_rows = build_inbox_rows(&response);
        self.inbox = Some(response);
        self.inbox_selected = keep
            .and_then(|(name, id)| {
                self.inbox_rows.iter().position(|row| match row {
                    InboxRow::Item(s, i) => {
                        let source = &self.inbox.as_ref().unwrap().sources[*s];
                        source.name == name && source.items[*i].id == id
                    }
                    InboxRow::Source(_) => false,
                })
            })
            .or_else(|| {
                self.inbox_rows
                    .iter()
                    .position(|r| matches!(r, InboxRow::Item(_, _)))
            });
    }

    pub fn selected_inbox_item(&self) -> Option<(&InboxSourceResult, &InboxItem)> {
        let inbox = self.inbox.as_ref()?;
        match self.inbox_selected.and_then(|i| self.inbox_rows.get(i)) {
            Some(InboxRow::Item(s, i)) => {
                let source = inbox.sources.get(*s)?;
                Some((source, source.items.get(*i)?))
            }
            _ => None,
        }
    }

    /// 이미 보드에 올라간 항목인가 — 데몬 판정(전 보드·보관 포함의 링크, `rocky today` 요약과 같은 값)이
    /// 먼저고, 현재 보드 todos 의 링크는 방금 올린 것을 캐시가 돌기 전에 보이게 하는 보충이다.
    /// url 없으면 판정 불가.
    pub fn is_promoted(&self, item: &InboxItem) -> bool {
        item.promoted
            || item
                .url
                .as_deref()
                .is_some_and(|u| self.promoted_urls.contains(u))
    }

    // ── 핸드오프 ──

    /// 이 todo 앞으로 열린 핸드오프 수(pending + 미수락 delivered).
    pub fn open_handoffs_for(&self, todo_id: &str) -> usize {
        self.handoffs
            .iter()
            .filter(|h| h.get("todoId").and_then(Value::as_str) == Some(todo_id))
            .count()
    }

    // ── GitHub ──

    /// 선택 항목의 GitHub 링크 중 지금 조회해야 할 것 — 캐시 만료 · 미조회 · 미진행.
    pub fn gh_needed(&self) -> Vec<String> {
        let Some(todo) = self.selected_todo() else {
            return Vec::new();
        };
        todo.todo
            .links
            .iter()
            .map(|l| l.url.clone())
            .filter(|u| github_ref(u).is_some())
            .filter(|u| !self.gh_pending.contains(u))
            .filter(|u| match self.gh.get(u) {
                Some((at, _)) => at.elapsed() >= GH_CACHE_TTL,
                None => true,
            })
            .collect()
    }

    pub fn set_gh(&mut self, url: String, summary: Option<String>) {
        self.gh_pending.remove(&url);
        self.gh.insert(url, (Instant::now(), summary));
    }

    /// 링크의 한 줄 요약(캐시). 조회 전·실패면 None.
    pub fn gh_line(&self, url: &str) -> Option<&str> {
        self.gh.get(url).and_then(|(_, s)| s.as_deref())
    }
}

/// 머리글을 건너뛰며 delta 만큼. 선택이 없으면 첫 항목.
fn step<R>(
    rows: &[R],
    current: Option<usize>,
    delta: i32,
    is_item: impl Fn(&R) -> bool,
) -> Option<usize> {
    let Some(mut i) = current else {
        return rows.iter().position(&is_item);
    };
    loop {
        let next = i as i64 + delta as i64;
        if next < 0 || next >= rows.len() as i64 {
            return Some(i);
        }
        i = next as usize;
        if is_item(&rows[i]) {
            return Some(i);
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

/// 소스마다 머리글 + 항목. 실패한 소스는 머리글만(사유는 렌더가 붙인다).
pub fn build_inbox_rows(inbox: &InboxResponse) -> Vec<InboxRow> {
    let mut rows = Vec::new();
    for (s, source) in inbox.sources.iter().enumerate() {
        rows.push(InboxRow::Source(s));
        rows.extend((0..source.items.len()).map(|i| InboxRow::Item(s, i)));
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

/// 수집함 항목 → `POST /api/todos` 본문. 링크 제목은 `<소스>: <제목>`, 섹션은 백로그.
pub fn promote_body(board: &str, source: &str, item: &InboxItem) -> Value {
    let mut body = serde_json::json!({
        "board": board,
        "title": item.title,
        "section": "백로그",
    });
    if let Some(note) = item.note.as_deref().filter(|n| !n.trim().is_empty()) {
        body["description"] = Value::String(note.to_string());
    }
    if let Some(due) = &item.due {
        body["due"] = Value::String(due.clone());
    }
    if let Some(url) = &item.url {
        body["links"] =
            serde_json::json!([{ "url": url, "title": format!("{source}: {}", item.title) }]);
    }
    body
}
