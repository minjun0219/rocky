//! 보드 싱크 — 행 순번(`seq`)으로 "마지막으로 받은 뒤 바뀐 행"을 주고받는 프로토콜의 순수 부분.
//!
//! 순번은 SQLite 트리거가 매긴다(`migrations::add_sync_seq`). 쓰기 경로는 순번을 모른다 — 새 쓰기를 더해도 순번은
//! 저절로 붙는다. 읽기는 `TodoStore::sync_head` · `TodoStore::sync_rows_since`.
//! 설계: `docs/design/specs/2026-10-15-dual-structure-sync-design.md`, 규칙: `docs/features/sync.md`.

use serde::Serialize;

use crate::types::{Board, Comment, Handoff, HistoryEntry, Note, Section, Todo};

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
