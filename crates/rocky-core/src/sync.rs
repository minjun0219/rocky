//! 보드 싱크 — 행 순번(`seq`)으로 "마지막으로 받은 뒤 바뀐 행"을 주고받는 프로토콜의 순수 부분.
//!
//! 순번은 SQLite 트리거가 매긴다(`migrations::add_sync_seq`). 쓰기 경로는 순번을 모른다 — 새 쓰기를 더해도 순번은
//! 저절로 붙는다. 읽기는 `TodoStore::sync_head` · `TodoStore::sync_rows_since`.
//! 설계: `docs/design/specs/2026-10-15-dual-structure-sync-design.md`, 규칙: `docs/features/sync.md`.

use serde::{Deserialize, Deserializer, Serialize};

use crate::ids::ID_LENGTH;
use crate::types::{
    Board, Comment, Handoff, HistoryEntry, Note, Section, StatusAction, Todo, TodoLink,
    TodoPriority,
};

/// 순번을 매기는 보드 테이블. 늘리면 새 마이그레이션으로 그 테이블의 `seq` 열과 트리거를 더하고(마이그레이션 19 는
/// 고치지 않는다), `SyncTables`·`TodoStore::sync_rows_since` 를 함께 고친다.
pub const SYNC_TABLES: [&str; 7] = [
    "boards", "sections", "todos", "comments", "notes", "history", "handoffs",
];

/// 통째 사본에 싣는 히스토리 줄 수(최근 것부터). 히스토리는 쓰기마다 늘어 끝이 없어서 통째 사본이 커지지 않게 자른다.
/// 그보다 오래된 줄은 REST 로 받는다. 이어 받기(`since` 이후)는 자르지 않는다.
pub const FULL_HISTORY_LIMIT: i64 = 2000;

/// DB 의 싱크 머리 — 마지막으로 매긴 순번과 `epoch`. `epoch` 는 스토어가 DB 를 열 때마다 새로 정한다 — 데몬이 다시
/// 뜨면(DB 를 백업에서 되살린 경우 포함) 클라이언트가 옛 사본에 순번을 이어 붙이지 않고 통째로 다시 받는다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SyncHead {
    pub epoch: String,
    pub seq: i64,
}

/// 클라이언트 사본을 통째로 바꿔야 하나 — 처음 받거나(`since`·`epoch` 없음), 다른 DB 의 사본이거나(`epoch` 다름),
/// 이 DB 보다 앞선 순번을 들고 왔으면(DB 를 되돌림) 그렇다.
pub fn needs_full(since: Option<i64>, epoch: Option<&str>, head: &SyncHead) -> bool {
    match (since, epoch) {
        (Some(since), Some(epoch)) => epoch != head.epoch || since < 0 || since > head.seq,
        _ => true,
    }
}

/// 행 하나와 그 순번. JSON 은 행의 필드에 `seq` 를 더한 모양이다.
#[derive(Debug, Clone, Serialize)]
pub struct Synced<T> {
    #[serde(flatten)]
    pub row: T,
    pub seq: i64,
}

/// 테이블별로 모은 행.
#[derive(Debug, Clone, Default, Serialize)]
pub struct SyncTables {
    pub boards: Vec<Synced<Board>>,
    pub sections: Vec<Synced<Section>>,
    pub todos: Vec<Synced<Todo>>,
    pub comments: Vec<Synced<Comment>>,
    pub notes: Vec<Synced<Note>>,
    pub history: Vec<Synced<HistoryEntry>>,
    pub handoffs: Vec<Synced<Handoff>>,
}

impl SyncTables {
    /// 모든 테이블의 행 수.
    pub fn len(&self) -> usize {
        self.boards.len()
            + self.sections.len()
            + self.todos.len()
            + self.comments.len()
            + self.notes.len()
            + self.history.len()
            + self.handoffs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// `sync_rows_since` 의 결과. `seq` 는 이 행들을 읽은 순간의 머리 순번이다 — 다음에는 `since=seq` 로 묻는다.
#[derive(Debug, Clone, Serialize)]
pub struct SyncRows {
    pub epoch: String,
    pub seq: i64,
    /// 참이면 `rows` 가 DB 의 보드 행 전부다. 클라이언트는 사본을 통째로 바꾼다.
    pub full: bool,
    pub rows: SyncTables,
}

/// 클라이언트가 정한 id 인가 — 데몬의 `new_id` 와 같은 모양(8자 base36). 낙관적으로 그린 행과 확정된 행이 같은 id 를
/// 쓰고, 응답을 잃어 다시 보낸 만들기가 두 번 만들지 않는다.
pub fn is_client_id(id: &str) -> bool {
    id.len() == ID_LENGTH
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || b.is_ascii_lowercase())
}

/// 싱크 트랜잭션(`{t:"tx", ops:[…]}`)의 쓰기 하나. 어휘는 원격에서도 이미 허용된 보드 쓰기뿐이다 — 세션 띄우기·이슈·
/// 핸드오프·보드의 `path`·`repo`·`reviewFix`·`prAuthors`·수집함처럼 실행으로 이어지는 쓰기는 없다(그런 쓰기는 로컬
/// 요청 전용 REST 로만 한다). 적용은 `TodoStore::apply_sync_op` 이 지금 스토어 메서드로 한다.
///
/// 지우기가 되는 칸(할 일의 `due`·`section`·`parentId`, 보드의 `description`)은 `null` 이면 지우고, 빠지면 그대로 둔다.
/// 할 일 설명은 빈 문자열로 비운다.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "op", rename_all_fields = "camelCase")]
pub enum SyncOp {
    #[serde(rename = "todo.create")]
    TodoCreate {
        id: String,
        board: String,
        title: String,
        description: Option<String>,
        section: Option<String>,
        parent_id: Option<String>,
        priority: Option<TodoPriority>,
        due: Option<String>,
        labels: Option<Vec<String>>,
        links: Option<Vec<TodoLink>>,
    },
    #[serde(rename = "todo.update")]
    TodoUpdate {
        id: String,
        title: Option<String>,
        description: Option<String>,
        priority: Option<TodoPriority>,
        #[serde(default, deserialize_with = "set_or_clear")]
        due: Option<Option<String>>,
        labels: Option<Vec<String>>,
        links: Option<Vec<TodoLink>>,
        #[serde(default, deserialize_with = "set_or_clear")]
        section: Option<Option<String>>,
        #[serde(default, deserialize_with = "set_or_clear")]
        parent_id: Option<Option<String>>,
    },
    #[serde(rename = "todo.status")]
    TodoStatus { id: String, action: StatusAction },
    /// 같은 보드 안에서 `before` 앞으로 옮긴다. REST 처럼 `before` 는 명시해야 한다 — `null` 이면 맨 뒤.
    #[serde(rename = "todo.move")]
    TodoMove {
        id: String,
        #[serde(default, deserialize_with = "set_or_clear")]
        before: Option<Option<String>>,
    },
    #[serde(rename = "comment.create")]
    CommentCreate {
        id: String,
        todo_id: String,
        body: String,
    },
    #[serde(rename = "comment.update")]
    CommentUpdate { id: String, body: String },
    #[serde(rename = "comment.archive")]
    CommentArchive { id: String },
    #[serde(rename = "comment.unarchive")]
    CommentUnarchive { id: String },
    /// 이름으로 만든다(같은 이름이 있으면 그것) — 섹션 id 는 데몬이 정한다.
    #[serde(rename = "section.create")]
    SectionCreate { board: String, title: String },
    #[serde(rename = "section.archive")]
    SectionArchive { id: String },
    /// 본문은 처음 한 번만 넣는다. 그 뒤 본문 편집은 노트 문서 프레임(CRDT)으로 한다.
    #[serde(rename = "note.create")]
    NoteCreate {
        id: String,
        board: Option<String>,
        title: String,
        content: Option<String>,
    },
    #[serde(rename = "note.update")]
    NoteUpdate { id: String, title: String },
    #[serde(rename = "note.archive")]
    NoteArchive { id: String },
    #[serde(rename = "note.unarchive")]
    NoteUnarchive { id: String },
    #[serde(rename = "note.pin")]
    NotePin { id: String },
    #[serde(rename = "note.unpin")]
    NoteUnpin { id: String },
    /// 보드는 제목과 설명만 — 키·경로·레포 같은 기계 설정은 로컬 REST 로만 바꾼다.
    #[serde(rename = "board.update")]
    BoardUpdate {
        key: String,
        title: Option<String>,
        #[serde(default, deserialize_with = "set_or_clear")]
        description: Option<Option<String>>,
    },
}

/// 빠지면 `None`(그대로), `null` 이면 `Some(None)`(지우기), 값이면 `Some(Some(v))`. `#[serde(default)]` 와 같이 쓴다.
fn set_or_clear<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}
