//! SQLite 스토어 — TS 원본 `src/store.ts`. 데몬 프로세스 안에서 단일 인스턴스로 쓰인다.
//!
//! 동작 동일성이 계약이다: 에러 **메시지**까지 계약이다(서버가 `/not found/i` 로 404 를
//! 가른다 — docs/rewrite/contract.md). 스레딩은 `Mutex<Connection>` 하나로 직렬화한다 —
//! 단일 사용자 로컬 데몬이라 동시성 이득보다 단순성이 크고, TS(단일 스레드)와 실행 모델이
//! 같아진다. 이벤트 발행은 conn 락을 놓은 뒤에 한다(리스너 안에서 스토어를 다시 부르는
//! 구독자가 있어도 데드락이 없다).

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Row};
use serde_json::{json, Value};

use crate::actors::is_agent_actor;
use crate::ids::{new_id, ID_LENGTH};
use crate::migrations::{run_migrations, RunMigrationsOptions};
use crate::note_doc::NoteDoc;
use crate::prwatch::{
    diff as pr_diff, event_changes, PrEvent, PrFilterSubscription, PrSnapshot, PrSubscription,
};
use crate::refs::GLOBAL_NOTE_PREFIX;
use crate::types::*;

/// TS 의 `throw new Error(message)` 에 대응 — 메시지가 곧 계약이다.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct StoreError(String);

impl StoreError {
    pub fn new(message: impl Into<String>) -> Self {
        StoreError(message.into())
    }
}

impl From<rusqlite::Error> for StoreError {
    fn from(error: rusqlite::Error) -> Self {
        StoreError(error.to_string())
    }
}

pub type StoreResult<T> = Result<T, StoreError>;

pub(crate) fn now_iso() -> String {
    chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string()
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS boards (
  id TEXT PRIMARY KEY,
  key TEXT NOT NULL UNIQUE,
  title TEXT NOT NULL,
  description TEXT,
  repo TEXT,
  path TEXT,
  auto_resolve INTEGER NOT NULL DEFAULT 0,
  pr_authors TEXT NOT NULL DEFAULT '[]',
  created_at TEXT NOT NULL,
  archived_at TEXT
);
CREATE TABLE IF NOT EXISTS board_aliases (
  key TEXT PRIMARY KEY,
  board_id TEXT NOT NULL REFERENCES boards(id),
  created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS sections (
  id TEXT PRIMARY KEY,
  board_id TEXT NOT NULL REFERENCES boards(id),
  title TEXT NOT NULL,
  position INTEGER NOT NULL DEFAULT 0,
  archived_at TEXT
);
CREATE TABLE IF NOT EXISTS todos (
  id TEXT PRIMARY KEY,
  board_id TEXT NOT NULL REFERENCES boards(id),
  section_id TEXT REFERENCES sections(id),
  parent_id TEXT REFERENCES todos(id),
  title TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  status TEXT NOT NULL DEFAULT 'todo' CHECK (status IN ('todo','doing','done')),
  priority TEXT NOT NULL DEFAULT 'p4' CHECK (priority IN ('p1','p2','p3','p4')),
  due TEXT,
  labels TEXT NOT NULL DEFAULT '[]',
  links TEXT NOT NULL DEFAULT '[]',
  doing_by TEXT,
  doing_since TEXT,
  doing_session_id TEXT,
  position INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  completed_at TEXT,
  archived_at TEXT
);
CREATE TABLE IF NOT EXISTS notes (
  id TEXT PRIMARY KEY,
  board_id TEXT REFERENCES boards(id),
  title TEXT NOT NULL,
  content TEXT NOT NULL DEFAULT '',
  position INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  archived_at TEXT
);
CREATE TABLE IF NOT EXISTS note_docs (
  note_id TEXT PRIMARY KEY REFERENCES notes(id),
  state BLOB NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS board_inbox_sources (
  id TEXT PRIMARY KEY,
  board_id TEXT NOT NULL REFERENCES boards(id),
  name TEXT NOT NULL UNIQUE,
  adapter TEXT NOT NULL,
  params TEXT NOT NULL DEFAULT '[]',
  created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS inbox_subscriptions (
  source TEXT NOT NULL,
  session_id TEXT NOT NULL,
  socket TEXT NOT NULL,
  fingerprint TEXT NOT NULL,
  created_at TEXT NOT NULL,
  PRIMARY KEY (source, session_id)
);
CREATE TABLE IF NOT EXISTS inbox_seen (
  source TEXT NOT NULL,
  session_id TEXT NOT NULL,
  item_id TEXT NOT NULL,
  seen_at TEXT NOT NULL,
  PRIMARY KEY (source, session_id, item_id)
);
CREATE TABLE IF NOT EXISTS session_inboxes (
  session_id TEXT PRIMARY KEY,
  socket TEXT NOT NULL,
  cwd TEXT NOT NULL,
  seen_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS pr_watch_repos (
  repo TEXT PRIMARY KEY,
  baselined_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS pr_watch (
  repo TEXT NOT NULL,
  number INTEGER NOT NULL,
  snapshot TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (repo, number)
);
CREATE TABLE IF NOT EXISTS history (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  entity TEXT NOT NULL CHECK (entity IN ('board','section','todo','note')),
  entity_id TEXT NOT NULL,
  actor TEXT NOT NULL,
  action TEXT NOT NULL,
  changes TEXT,
  at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS comments (
  id TEXT PRIMARY KEY,
  todo_id TEXT NOT NULL REFERENCES todos(id),
  actor TEXT NOT NULL,
  body TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  archived_at TEXT
);
CREATE TABLE IF NOT EXISTS handoffs (
  id            TEXT PRIMARY KEY,
  todo_id       TEXT NOT NULL REFERENCES todos(id),
  session_id    TEXT NOT NULL,
  session_name  TEXT,
  session_cwd   TEXT,
  note          TEXT NOT NULL DEFAULT '',
  actor         TEXT NOT NULL,
  status        TEXT NOT NULL CHECK (status IN ('pending','delivered','cancelled')),
  created_at    TEXT NOT NULL,
  delivered_at  TEXT,
  delivered_via TEXT,
  accepted_at   TEXT,
  completed_at  TEXT
);
CREATE INDEX IF NOT EXISTS idx_board_aliases_board ON board_aliases(board_id);
CREATE INDEX IF NOT EXISTS idx_todos_board ON todos(board_id);
CREATE INDEX IF NOT EXISTS idx_notes_board ON notes(board_id);
CREATE INDEX IF NOT EXISTS idx_history_entity ON history(entity_id);
CREATE INDEX IF NOT EXISTS idx_comments_todo ON comments(todo_id, created_at);
CREATE INDEX IF NOT EXISTS idx_handoffs_session ON handoffs(session_id, status, created_at);
CREATE INDEX IF NOT EXISTS idx_handoffs_todo ON handoffs(todo_id, status);
";

// ── row 매핑 ────────────────────────────────────────────────────────────────

fn conv_err(e: impl std::error::Error + Send + Sync + 'static) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
}

#[derive(Debug, thiserror::Error)]
#[error("invalid enum value: {0}")]
struct EnumParseError(String);

fn board_from_row(row: &Row) -> rusqlite::Result<Board> {
    Ok(Board {
        id: row.get("id")?,
        key: row.get("key")?,
        title: row.get("title")?,
        description: row.get("description")?,
        repo: row.get("repo")?,
        path: row.get("path")?,
        previous_keys: None,
        review_fix: row.get::<_, i64>("auto_resolve")? != 0,
        pr_authors: serde_json::from_str(&row.get::<_, String>("pr_authors")?).unwrap_or_default(),
        created_at: row.get("created_at")?,
        archived_at: row.get("archived_at")?,
    })
}

fn section_from_row(row: &Row) -> rusqlite::Result<Section> {
    Ok(Section {
        id: row.get("id")?,
        board_id: row.get("board_id")?,
        title: row.get("title")?,
        position: row.get("position")?,
        archived_at: row.get("archived_at")?,
    })
}

fn todo_from_row(row: &Row) -> rusqlite::Result<Todo> {
    let status: String = row.get("status")?;
    let priority: String = row.get("priority")?;
    let labels: String = row.get("labels")?;
    let links: String = row.get("links")?;
    Ok(Todo {
        id: row.get("id")?,
        number: row.get("number")?,
        board_id: row.get("board_id")?,
        section_id: row.get("section_id")?,
        parent_id: row.get("parent_id")?,
        title: row.get("title")?,
        description: row.get("description")?,
        status: TodoStatus::parse(&status).ok_or_else(|| conv_err(EnumParseError(status)))?,
        priority: TodoPriority::parse(&priority)
            .ok_or_else(|| conv_err(EnumParseError(priority)))?,
        due: row.get("due")?,
        labels: serde_json::from_str(&labels).map_err(conv_err)?,
        links: serde_json::from_str(&links).map_err(conv_err)?,
        doing_by: row.get("doing_by")?,
        doing_since: row.get("doing_since")?,
        doing_session_id: row.get("doing_session_id")?,
        position: row.get("position")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
        completed_at: row.get("completed_at")?,
        archived_at: row.get("archived_at")?,
    })
}

fn note_from_row(row: &Row) -> rusqlite::Result<Note> {
    Ok(Note {
        id: row.get("id")?,
        number: row.get("number")?,
        board_id: row.get("board_id")?,
        title: row.get("title")?,
        content: row.get("content")?,
        position: row.get("position")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
        archived_at: row.get("archived_at")?,
        pinned_at: row.get("pinned_at")?,
    })
}

fn comment_from_row(row: &Row) -> rusqlite::Result<Comment> {
    Ok(Comment {
        id: row.get("id")?,
        todo_id: row.get("todo_id")?,
        actor: row.get("actor")?,
        body: row.get("body")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
        archived_at: row.get("archived_at")?,
    })
}

fn handoff_from_row(row: &Row) -> rusqlite::Result<Handoff> {
    let status: String = row.get("status")?;
    let via: Option<String> = row.get("delivered_via")?;
    Ok(Handoff {
        id: row.get("id")?,
        todo_id: row.get("todo_id")?,
        session_id: row.get("session_id")?,
        session_name: row.get("session_name")?,
        session_cwd: row.get("session_cwd")?,
        note: row.get("note")?,
        actor: row.get("actor")?,
        status: HandoffStatus::parse(&status).ok_or_else(|| conv_err(EnumParseError(status)))?,
        created_at: row.get("created_at")?,
        delivered_at: row.get("delivered_at")?,
        // 알 수 없는 via 값은 None 으로 접는다 — TS 의 `as HandoffVia | null` 캐스트와 동일.
        delivered_via: via.and_then(|v| HandoffVia::parse(&v)),
        accepted_at: row.get("accepted_at")?,
        completed_at: row.get("completed_at")?,
    })
}

fn history_from_row(row: &Row) -> rusqlite::Result<HistoryEntry> {
    let entity: String = row.get("entity")?;
    let changes: Option<String> = row.get("changes")?;
    Ok(HistoryEntry {
        id: row.get("id")?,
        entity: HistoryEntity::parse(&entity).ok_or_else(|| conv_err(EnumParseError(entity)))?,
        entity_id: row.get("entity_id")?,
        actor: row.get("actor")?,
        action: row.get("action")?,
        changes: match changes {
            Some(raw) => Some(serde_json::from_str(&raw).map_err(conv_err)?),
            None => None,
        },
        at: row.get("at")?,
    })
}

/// board key 로 쓸 수 있는 모양인지 — 생성(`ensure_board`)과 이름 변경(`update_board`)이
/// 같은 규칙을 쓰는 단일 출처. 공백/`#` 만 막는 이유는 `src/store.ts` 의 장문 주석 참고
/// (요지: 공백은 구조적으로 못 읽고, `#` 는 읽히지만 ref-safe 게이트에 걸려 무용지물).
fn assert_usable_board_key(key: &str) -> StoreResult<()> {
    if key.is_empty() {
        return Err(StoreError::new("board key must not be empty"));
    }
    if key.chars().any(|c| c.is_whitespace()) {
        return Err(StoreError::new(format!(
            "board key must not contain whitespace: {}",
            serde_json::to_string(key).unwrap_or_else(|_| key.to_string())
        )));
    }
    if key.contains('#') {
        return Err(StoreError::new(format!(
            "board key must not contain '#': {}",
            serde_json::to_string(key).unwrap_or_else(|_| key.to_string())
        )));
    }
    Ok(())
}

/// 빈 값("지운다")을 NULL 로 접는다 — description/repo/path 가 공유하는 규칙.
fn blank_to_null(value: Option<&str>) -> Option<String> {
    let trimmed = value.unwrap_or("").trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// 댓글 집계 — 목록 배지용.
#[derive(Debug, Clone)]
pub struct CommentStats {
    pub count: i64,
    pub last_at: Option<String>,
}

type Listener = Box<dyn Fn(&ChangeEvent) + Send + Sync>;
type DocListener = Box<dyn Fn(&NoteDocEvent) + Send + Sync>;

/// rocky 스토어 — 데몬 프로세스 안에서 단일 인스턴스로 쓰인다.
pub struct TodoStore {
    conn: Mutex<Connection>,
    listeners: Mutex<Vec<(u64, Listener)>>,
    next_listener_id: AtomicU64,
    /// 노트별 마지막 `edit` 변경 이벤트 시각 — 웹 편집을 전역 이벤트로 흘릴 때의 조절용.
    note_edit_events: Mutex<HashMap<String, std::time::Instant>>,
    /// 노트 문서 갱신 구독 — 서버가 노트별 SSE 로 방송한다.
    doc_listeners: Mutex<Vec<(u64, DocListener)>>,
}

impl TodoStore {
    pub fn open(db_path: &Path) -> StoreResult<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| StoreError::new(format!("cannot create db dir: {e}")))?;
        }
        let conn = Connection::open(db_path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(SCHEMA)?;
        // backup_path 는 지정하지 않는다 — run_migrations 가 실제 시작 버전으로
        // `${dbPath}.bak-v<version>` 을 스스로 계산한다.
        run_migrations(
            &conn,
            RunMigrationsOptions {
                db_path: Some(db_path),
                ..Default::default()
            },
        )?;
        Ok(TodoStore {
            conn: Mutex::new(conn),
            listeners: Mutex::new(Vec::new()),
            next_listener_id: AtomicU64::new(1),
            note_edit_events: Mutex::new(HashMap::new()),
            doc_listeners: Mutex::new(Vec::new()),
        })
    }

    /// `PRAGMA quick_check` — 정상이면 "ok", 아니면 첫 문제들(줄바꿈으로 이어 붙임).
    pub fn quick_check(&self) -> StoreResult<String> {
        let conn = self.lock();
        let mut stmt = conn.prepare("PRAGMA quick_check(5)")?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows.join("\n"))
    }

    /// 테스트용 — 메모리 DB. 프로덕션 경로는 `open`.
    pub fn open_in_memory() -> StoreResult<Self> {
        let conn = Connection::open_in_memory()?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(SCHEMA)?;
        run_migrations(&conn, RunMigrationsOptions::default())?;
        Ok(TodoStore {
            conn: Mutex::new(conn),
            listeners: Mutex::new(Vec::new()),
            next_listener_id: AtomicU64::new(1),
            note_edit_events: Mutex::new(HashMap::new()),
            doc_listeners: Mutex::new(Vec::new()),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().expect("store mutex poisoned")
    }

    /// mutation 이벤트 구독 — SSE 허브가 쓴다. 반환된 id 로 `unsubscribe`.
    pub fn subscribe(&self, listener: impl Fn(&ChangeEvent) + Send + Sync + 'static) -> u64 {
        let id = self.next_listener_id.fetch_add(1, Ordering::Relaxed);
        self.listeners
            .lock()
            .expect("listeners mutex poisoned")
            .push((id, Box::new(listener)));
        id
    }

    pub fn unsubscribe(&self, id: u64) {
        self.listeners
            .lock()
            .expect("listeners mutex poisoned")
            .retain(|(lid, _)| *lid != id);
    }

    /// 노트 문서 갱신 구독 — 어느 경로의 편집이든(웹 update·MCP/CLI set/append) 한 건씩.
    pub fn subscribe_note_docs(
        &self,
        listener: impl Fn(&NoteDocEvent) + Send + Sync + 'static,
    ) -> u64 {
        let id = self.next_listener_id.fetch_add(1, Ordering::Relaxed);
        self.doc_listeners
            .lock()
            .expect("doc_listeners mutex poisoned")
            .push((id, Box::new(listener)));
        id
    }

    pub fn unsubscribe_note_docs(&self, id: u64) {
        self.doc_listeners
            .lock()
            .expect("doc_listeners mutex poisoned")
            .retain(|(lid, _)| *lid != id);
    }

    /// conn 락을 놓은 뒤 — `emit_all` 과 같은 규칙.
    fn emit_docs(&self, events: Vec<NoteDocEvent>) {
        if events.is_empty() {
            return;
        }
        let listeners = self
            .doc_listeners
            .lock()
            .expect("doc_listeners mutex poisoned");
        for event in &events {
            for (_, listener) in listeners.iter() {
                listener(event);
            }
        }
    }

    /// conn 락을 **놓은 뒤** 부른다 — 리스너가 스토어를 재진입해도 데드락이 없다.
    fn emit_all(&self, events: Vec<ChangeEvent>) {
        if events.is_empty() {
            return;
        }
        let listeners = self.listeners.lock().expect("listeners mutex poisoned");
        for event in &events {
            for (_, listener) in listeners.iter() {
                listener(event);
            }
        }
    }
}

// ── PR 감시 ─────────────────────────────────────────────────────────────────

impl TodoStore {
    /// 레포의 새 스냅숏을 기억하고 직전과의 전이를 돌려준다. 전이는 그 레포를 `repo` 로 둔
    /// 보드(들)의 히스토리에 actor `rocky`·action `pr-*` 로 남아 SSE·`/api/changes`·훅 주입을
    /// 탄다. 스냅숏 저장과 히스토리는 한 트랜잭션 — 사이에서 죽으면 같은 전이를 다시 알린다.
    pub fn apply_pr_snapshot(
        &self,
        repo: &str,
        snapshots: &[PrSnapshot],
        actor: &str,
        allow: &dyn Fn(&PrEvent, Option<&str>) -> bool,
    ) -> StoreResult<Vec<PrEvent>> {
        let mut events = Vec::new();
        // 행의 키는 인자의 `repo` 다 — 스냅숏의 repo 필드가 다른 표기(대소문자 등)여도 한 레포로 본다.
        let snapshots: Vec<PrSnapshot> = snapshots
            .iter()
            .cloned()
            .map(|mut s| {
                s.repo = repo.to_string();
                s
            })
            .collect();
        let snapshots = snapshots.as_slice();
        let result = {
            let conn = self.lock();
            let prev = list_prs_conn(&conn, Some(repo))?;
            // 처음 보는 레포(보드에 repo 를 막 붙였다)의 첫 tick 은 기준선만 — 이미 실패 중인 CI·이미 머지
            // 가능한 PR 은 "새 변화" 가 아니다. 표시는 `pr_watch_repos`(레포를 떼면 `retain_pr_repos` 가 걷는다).
            let baselined: bool = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM pr_watch_repos WHERE repo = ?1)",
                params![repo],
                |r| r.get(0),
            )?;
            let mut changes = if baselined {
                pr_diff(&prev, snapshots)
            } else {
                Vec::new()
            };
            for event in &mut changes {
                // 레포 단위(배너·브릿지) — 그 레포를 둔 보드 중 하나라도 통과시키면 알린다.
                event.quiet = !allow(event, None);
            }
            conn.execute_batch("BEGIN")?;
            let inner = (|| -> StoreResult<Vec<PrEvent>> {
                let now = now_iso();
                if !baselined {
                    conn.execute(
                        "INSERT OR IGNORE INTO pr_watch_repos (repo, baselined_at) VALUES (?1, ?2)",
                        params![repo, now],
                    )?;
                }
                for snap in snapshots {
                    conn.execute(
                        "INSERT INTO pr_watch (repo, number, snapshot, updated_at) VALUES (?1, ?2, ?3, ?4)\n\
                         ON CONFLICT(repo, number) DO UPDATE SET snapshot = excluded.snapshot, updated_at = excluded.updated_at",
                        params![
                            snap.repo,
                            snap.number,
                            serde_json::to_string(snap).map_err(|e| StoreError::new(e.to_string()))?,
                            now
                        ],
                    )?;
                }
                let board_ids: Vec<String> = {
                    let mut stmt = conn
                        .prepare("SELECT id FROM boards WHERE repo = ?1 AND archived_at IS NULL")?;
                    let ids = stmt
                        .query_map(params![repo], |r| r.get::<_, String>(0))?
                        .collect::<rusqlite::Result<Vec<_>>>()?;
                    ids
                };
                for event in &changes {
                    for board_id in &board_ids {
                        // 보드 단위(세션 훅 주입·채널) — 그 보드의 prAuthors 가 정한다.
                        let mut per_board = event.clone();
                        per_board.quiet = !allow(event, Some(board_id));
                        let changes_map = event_changes(&per_board);
                        record_history(
                            &conn,
                            &mut events,
                            HistoryEntity::Board,
                            board_id,
                            actor,
                            event.kind.action(),
                            Some(&changes_map),
                            Some(board_id),
                            true,
                        )?;
                    }
                }
                Ok(changes)
            })();
            match inner {
                Ok(v) => {
                    conn.execute_batch("COMMIT")?;
                    v
                }
                Err(error) => {
                    let _ = conn.execute_batch("ROLLBACK");
                    return Err(error);
                }
            }
        };
        self.emit_all(events);
        Ok(result)
    }

    /// 더는 보지 않는 레포의 스냅숏을 걷는다 — 보드에서 repo 를 떼거나 바꾼 뒤 옛 열린 PR 이 "지금"
    /// 표에 영영 남지 않게. 데몬이 tick 마다 지금 보는 레포 목록으로 부른다. 지운 행 수를 돌려준다.
    pub fn retain_pr_repos(&self, repos: &[String]) -> StoreResult<usize> {
        let conn = self.lock();
        let existing: Vec<String> = {
            let mut stmt = conn.prepare("SELECT DISTINCT repo FROM pr_watch")?;
            let rows = stmt
                .query_map([], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows
        };
        let mut removed = 0;
        for repo in existing.iter().filter(|r| !repos.contains(r)) {
            removed += conn.execute("DELETE FROM pr_watch WHERE repo = ?1", params![repo])?;
        }
        // 기준선 표시도 — 나중에 다시 붙이면 그때 다시 기준선부터.
        let marked: Vec<String> = {
            let mut stmt = conn.prepare("SELECT repo FROM pr_watch_repos")?;
            let rows = stmt
                .query_map([], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows
        };
        for repo in marked.iter().filter(|r| !repos.contains(r)) {
            conn.execute("DELETE FROM pr_watch_repos WHERE repo = ?1", params![repo])?;
        }
        Ok(removed)
    }

    /// PR 을 구독한다 — 이미 있으면 맡은 세션만 바꾼다(다른 세션이 넘겨받기). 직접 구독은 필터 출처를 지운다 —
    /// 필터를 해지해도 직접 맡은 PR 은 남는다. 레포는 대소문자를 가리지
    /// 않고 한 줄로 본다. 그 레포를 처음 보더라도 기준선으로 삼키지 않는다 — 구독은 "지금 상태부터 알려 달라" 다.
    pub fn subscribe_pr(
        &self,
        repo: &str,
        number: i64,
        session_id: Option<&str>,
    ) -> StoreResult<PrSubscription> {
        if !crate::prwatch::is_repo_slug(repo) || number <= 0 {
            return Err(StoreError::new(format!(
                "구독할 PR 이 이상하다: {repo}#{number} (owner/name 과 양수 번호)"
            )));
        }
        let conn = self.lock();
        let existing: Option<String> = conn
            .query_row(
                "SELECT repo FROM pr_subscriptions WHERE lower(repo) = lower(?1) AND number = ?2",
                params![repo, number],
                |r| r.get(0),
            )
            .optional()?;
        let key = existing.unwrap_or_else(|| repo.to_string());
        conn.execute(
            "INSERT INTO pr_subscriptions (repo, number, session_id, created_at) VALUES (?1, ?2, ?3, ?4)\n\
             ON CONFLICT (repo, number) DO UPDATE SET session_id = excluded.session_id, filter_id = NULL",
            params![key, number, session_id, now_iso()],
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO pr_watch_repos (repo, baselined_at) VALUES (?1, ?2)",
            params![key, now_iso()],
        )?;
        get_pr_subscription_conn(&conn, &key, number)?.ok_or_else(|| {
            StoreError::new(format!("구독을 넣었는데 다시 읽지 못했다: {key}#{number}"))
        })
    }

    /// 필터로 걸린 PR 을 구독한다 — 이미 누가(직접이든 다른 필터든) 구독한 PR 은 건드리지 않는다(빼앗지 않는다).
    /// 새로 넣었으면 true.
    pub fn subscribe_pr_via_filter(
        &self,
        repo: &str,
        number: i64,
        session_id: Option<&str>,
        filter_id: &str,
    ) -> StoreResult<bool> {
        if !crate::prwatch::is_repo_slug(repo) || number <= 0 {
            return Ok(false);
        }
        let conn = self.lock();
        if get_pr_subscription_conn(&conn, repo, number)?.is_some() {
            return Ok(false);
        }
        conn.execute(
            "INSERT INTO pr_subscriptions (repo, number, session_id, created_at, filter_id) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![repo, number, session_id, now_iso(), filter_id],
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO pr_watch_repos (repo, baselined_at) VALUES (?1, ?2)",
            params![repo, now_iso()],
        )?;
        Ok(true)
    }

    /// 필터를 구독한다 — 같은 세션이 같은 조건을 또 구독하면 있던 것을 돌려준다.
    pub fn subscribe_pr_filter(
        &self,
        query: &str,
        session_id: Option<&str>,
    ) -> StoreResult<PrFilterSubscription> {
        if !crate::prwatch::is_filter_query(query) {
            return Err(StoreError::new(format!(
                "구독할 검색 조건이 이상하다: {query:?} (한 줄, 256자 안)"
            )));
        }
        let query = query.trim();
        let conn = self.lock();
        let existing = conn
            .query_row(
                "SELECT id, query, session_id, created_at FROM pr_filter_subscriptions \
                 WHERE query = ?1 AND session_id IS ?2",
                params![query, session_id],
                pr_filter_from_row,
            )
            .optional()?;
        if let Some(found) = existing {
            return Ok(found);
        }
        let id = new_id();
        conn.execute(
            "INSERT INTO pr_filter_subscriptions (id, query, session_id, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![id, query, session_id, now_iso()],
        )?;
        Ok(conn.query_row(
            "SELECT id, query, session_id, created_at FROM pr_filter_subscriptions WHERE id = ?1",
            params![id],
            pr_filter_from_row,
        )?)
    }

    /// 필터를 해지한다 — 그 필터로 들어온 PR 구독도 같이 걷는다. 걷힌 PR 구독 수, 필터가 없었으면 None.
    pub fn unsubscribe_pr_filter(&self, id: &str) -> StoreResult<Option<usize>> {
        let conn = self.lock();
        let removed = conn.execute(
            "DELETE FROM pr_filter_subscriptions WHERE id = ?1",
            params![id],
        )?;
        if removed == 0 {
            return Ok(None);
        }
        Ok(Some(conn.execute(
            "DELETE FROM pr_subscriptions WHERE filter_id = ?1",
            params![id],
        )?))
    }

    pub fn pr_filter_subscriptions(&self) -> StoreResult<Vec<PrFilterSubscription>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, query, session_id, created_at FROM pr_filter_subscriptions ORDER BY created_at",
        )?;
        let rows = stmt
            .query_map([], pr_filter_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// 구독을 걷는다 — 있었으면 true.
    pub fn unsubscribe_pr(&self, repo: &str, number: i64) -> StoreResult<bool> {
        let conn = self.lock();
        let n = conn.execute(
            "DELETE FROM pr_subscriptions WHERE lower(repo) = lower(?1) AND number = ?2",
            params![repo, number],
        )?;
        Ok(n > 0)
    }

    pub fn pr_subscriptions(&self) -> StoreResult<Vec<PrSubscription>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT repo, number, session_id, created_at, filter_id FROM pr_subscriptions ORDER BY repo, number",
        )?;
        let rows = stmt
            .query_map([], pr_subscription_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn pr_subscription(&self, repo: &str, number: i64) -> StoreResult<Option<PrSubscription>> {
        let conn = self.lock();
        get_pr_subscription_conn(&conn, repo, number)
    }

    /// 구독하지 않은 PR 의 스냅숏을 걷는다 — "지금" 표·GitHub 탭은 구독한 PR 만 보인다. 지운 행 수.
    pub fn retain_subscribed_prs(&self) -> StoreResult<usize> {
        let conn = self.lock();
        Ok(conn.execute(
            "DELETE FROM pr_watch WHERE NOT EXISTS (SELECT 1 FROM pr_subscriptions s \
             WHERE lower(s.repo) = lower(pr_watch.repo) AND s.number = pr_watch.number)",
            [],
        )?)
    }

    /// 기억하고 있는 PR 들 — `repo` 로 좁히거나 전부. `open_only` 면 OPEN 만.
    pub fn list_prs(&self, repo: Option<&str>, open_only: bool) -> StoreResult<Vec<PrSnapshot>> {
        let conn = self.lock();
        let mut prs = list_prs_conn(&conn, repo)?;
        if open_only {
            prs.retain(|p| p.state == "OPEN");
        }
        Ok(prs)
    }
}

fn pr_subscription_from_row(row: &Row) -> rusqlite::Result<PrSubscription> {
    Ok(PrSubscription {
        repo: row.get(0)?,
        number: row.get(1)?,
        session_id: row.get(2)?,
        created_at: row.get(3)?,
        filter_id: row.get(4)?,
    })
}

fn pr_filter_from_row(row: &Row) -> rusqlite::Result<PrFilterSubscription> {
    Ok(PrFilterSubscription {
        id: row.get(0)?,
        query: row.get(1)?,
        session_id: row.get(2)?,
        created_at: row.get(3)?,
    })
}

fn get_pr_subscription_conn(
    conn: &Connection,
    repo: &str,
    number: i64,
) -> StoreResult<Option<PrSubscription>> {
    Ok(conn
        .query_row(
            "SELECT repo, number, session_id, created_at, filter_id FROM pr_subscriptions \
             WHERE lower(repo) = lower(?1) AND number = ?2",
            params![repo, number],
            pr_subscription_from_row,
        )
        .optional()?)
}

fn list_prs_conn(conn: &Connection, repo: Option<&str>) -> StoreResult<Vec<PrSnapshot>> {
    let sql = match repo {
        Some(_) => "SELECT snapshot FROM pr_watch WHERE repo = ?1 ORDER BY number",
        None => "SELECT snapshot FROM pr_watch ORDER BY repo, number",
    };
    let mut stmt = conn.prepare(sql)?;
    let rows: Vec<String> = match repo {
        Some(r) => stmt
            .query_map(params![r], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?,
        None => stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?,
    };
    // 옛 모양의 행(스키마가 바뀐 뒤)은 버린다 — 다음 tick 이 새로 쓴다.
    Ok(rows
        .iter()
        .filter_map(|raw| serde_json::from_str::<PrSnapshot>(raw).ok())
        .collect())
}

// ── conn 헬퍼 (락 안에서만 부른다) ──────────────────────────────────────────

/// history 한 줄 기록. 이벤트는 `events` 에 모아 락 해제 후 발행한다.
/// `emit=false` 는 배치에서 N개를 쏘지 않기 위한 것 — 배치 끝에 대표 이벤트 하나.
#[allow(clippy::too_many_arguments)]
fn record_history(
    conn: &Connection,
    events: &mut Vec<ChangeEvent>,
    entity: HistoryEntity,
    entity_id: &str,
    actor: &str,
    action: &str,
    changes: Option<&Changes>,
    board_id: Option<&str>,
    emit: bool,
) -> StoreResult<()> {
    let changes_json = match changes {
        Some(c) => Some(serde_json::to_string(c).map_err(|e| StoreError::new(e.to_string()))?),
        None => None,
    };
    conn.execute(
        "INSERT INTO history (entity, entity_id, actor, action, changes, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![entity.as_str(), entity_id, actor, action, changes_json, now_iso()],
    )?;
    if emit {
        events.push(ChangeEvent {
            entity,
            entity_id: entity_id.to_string(),
            action: action.to_string(),
            board_id: board_id.map(str::to_string),
        });
    }
    Ok(())
}

fn board_row_by_any_key(conn: &Connection, key: &str) -> StoreResult<Option<Board>> {
    let direct = conn
        .query_row(
            "SELECT * FROM boards WHERE key = ?1",
            params![key],
            board_from_row,
        )
        .optional()?;
    if direct.is_some() {
        return Ok(direct);
    }
    Ok(conn
        .query_row(
            "SELECT b.* FROM boards b JOIN board_aliases a ON a.board_id = b.id WHERE a.key = ?1",
            params![key],
            board_from_row,
        )
        .optional()?)
}

fn board_id_of_conn(conn: &Connection, key: &str) -> StoreResult<Option<String>> {
    Ok(board_row_by_any_key(conn, key)?.map(|b| b.id))
}

fn board_key_of_conn(conn: &Connection, board_id: &str) -> StoreResult<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT key FROM boards WHERE id = ?1",
            params![board_id],
            |r| r.get(0),
        )
        .optional()?)
}

/// 보드 하나에 별칭을 붙여 뷰 모델로 만든다.
fn hydrate_board(conn: &Connection, mut board: Board) -> StoreResult<Board> {
    let mut stmt =
        conn.prepare("SELECT key FROM board_aliases WHERE board_id = ?1 ORDER BY created_at, key")?;
    let aliases = stmt
        .query_map(params![board.id], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    // 빈 배열은 싣지 않는다 — "이름을 바꾼 적 있다"는 신호가 흐려진다.
    board.previous_keys = if aliases.is_empty() {
        None
    } else {
        Some(aliases)
    };
    Ok(board)
}

/// `ref_of` 의 conn 버전 — 락 안에서 재진입 없이 참조 문자열을 만든다.
fn ref_of_conn(
    conn: &Connection,
    board_id: Option<&str>,
    number: i64,
    id: &str,
) -> StoreResult<String> {
    let Some(board_id) = board_id else {
        return Ok(format!("{GLOBAL_NOTE_PREFIX}-{number}"));
    };
    let Some(key) = board_key_of_conn(conn, board_id)? else {
        return Err(StoreError::new(format!(
            "cannot build ref: board not found for boardId {board_id}"
        )));
    };
    if !crate::refs::is_ref_safe_board_key(&key) {
        return Ok(id.to_string());
    }
    Ok(format!("{key}-{number}"))
}

fn next_position(conn: &Connection, table: &str, board_id: Option<&str>) -> StoreResult<i64> {
    let max: Option<i64> = match board_id {
        None => conn.query_row(
            &format!("SELECT MAX(position) FROM {table} WHERE board_id IS NULL"),
            [],
            |r| r.get(0),
        )?,
        Some(id) => conn.query_row(
            &format!("SELECT MAX(position) FROM {table} WHERE board_id = ?1"),
            params![id],
            |r| r.get(0),
        )?,
    };
    Ok(max.unwrap_or(0) + 1)
}

/// 보드 안에서 다음 번호 — MAX(number)+1 이라 아카이브돼도 회수하지 않는다.
fn next_number(conn: &Connection, table: &str, board_id: Option<&str>) -> StoreResult<i64> {
    let max: Option<i64> = match board_id {
        None => conn.query_row(
            &format!("SELECT MAX(number) FROM {table} WHERE board_id IS NULL"),
            [],
            |r| r.get(0),
        )?,
        Some(id) => conn.query_row(
            &format!("SELECT MAX(number) FROM {table} WHERE board_id = ?1"),
            params![id],
            |r| r.get(0),
        )?,
    };
    Ok(max.unwrap_or(0) + 1)
}

// ── 참조 해석 ───────────────────────────────────────────────────────────────

fn parse_all_digits(s: &str) -> Option<i64> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse::<i64>().ok()
}

fn row_id_by_number(
    conn: &Connection,
    table: &str,
    board_id: Option<&str>,
    number: i64,
) -> StoreResult<Option<String>> {
    let found = match board_id {
        Some(bid) => conn
            .query_row(
                &format!("SELECT id FROM {table} WHERE board_id = ?1 AND number = ?2"),
                params![bid, number],
                |r| r.get::<_, String>(0),
            )
            .optional()?,
        None => conn
            .query_row(
                &format!("SELECT id FROM {table} WHERE board_id IS NULL AND number = ?1"),
                params![number],
                |r| r.get::<_, String>(0),
            )
            .optional()?,
    };
    Ok(found)
}

/// 참조 문자열을 행 id 로 해석한다 — TS `TodoStore.resolveRef`. 다섯 분기를 **이 순서로**:
///
/// 1. 레거시 스코프 `rocky#12` — 첫 `#` 에서 가르고, 앞부분에 공백/`#` 금지, 뒷부분 전부 숫자.
/// 2. 신규 스코프 `rocky-12` — **가장 오른쪽** `-` 에서 가른다(board key 에 `-` 가 흔하다).
///    `note-N` 은 예약 접두사라 board 조회보다 먼저 전역 note 공간으로 간다(todos 면
///    전역 번호 공간이 없으므로 None). 보드를 못 찾으면 **명시적으로 None** — 아래로
///    흘리면 `_` 든 key 가 wildcard 가드에 걸려 레거시 분기와 다른 에러를 낸다.
/// 3. 맨숫자 `12`/`#12` — `#` 존재 또는 길이 < ID_LENGTH 일 때만. currentBoard 없으면
///    notes 는 전역 공간, todos 는 에러.
/// 4. id 정확 일치.
/// 5. 유일한 id prefix — `%`/`_` 는 LIKE 와일드카드라 통째로 거부.
///
/// TS 와 달리 행 전체가 아니라 id 만 돌려준다 — 호출자가 id 로 다시 읽는다(정확 일치
/// 분기라 의미 동일). board 조회는 별칭도 푼다(`board_id_of_conn`).
fn resolve_ref_id(
    conn: &Connection,
    table: &str,
    raw: &str,
    current_board_id: Option<&str>,
) -> StoreResult<Option<String>> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        // 빈 ref 를 흘리면 LIKE 프리픽스가 '' 로 모든 행에 매치된다.
        return Err(StoreError::new("empty ref"));
    }

    // 1. 레거시 스코프 `^([^#\s]+)#(\d+)$`
    if let Some(pos) = trimmed.find('#') {
        if pos > 0 {
            let board_part = &trimmed[..pos];
            let digit_part = &trimmed[pos + 1..];
            if !board_part.chars().any(|c| c.is_whitespace()) {
                if let Some(number) = parse_all_digits(digit_part) {
                    let Some(board_id) = board_id_of_conn(conn, board_part)? else {
                        return Ok(None);
                    };
                    return row_id_by_number(conn, table, Some(&board_id), number);
                }
            }
        }
    }

    // 2. 신규 스코프 `^(\S+)-(\d+)$` — 가장 오른쪽 `-` 에서 가른다.
    if !trimmed.chars().any(|c| c.is_whitespace()) {
        if let Some(pos) = trimmed.rfind('-') {
            if pos > 0 {
                let board_part = &trimmed[..pos];
                let digit_part = &trimmed[pos + 1..];
                if let Some(number) = parse_all_digits(digit_part) {
                    if board_part == GLOBAL_NOTE_PREFIX {
                        // 예약 접두사가 board 조회보다 먼저다 — `note` 라는 보드가 있어도
                        // `note-3` 은 전역 메모다(결정론).
                        if table != "notes" {
                            // 전역 todo 번호 공간은 존재하지 않는다.
                            return Ok(None);
                        }
                        return row_id_by_number(conn, table, None, number);
                    }
                    return match board_id_of_conn(conn, board_part)? {
                        Some(board_id) => row_id_by_number(conn, table, Some(&board_id), number),
                        // 흘려보내지 않는다 — `my_board-1` 이 wildcard 가드에 걸려
                        // `my_board#1`(레거시, 바로 None) 과 다른 에러를 내는 모순 방지.
                        None => Ok(None),
                    };
                }
            }
        }
    }

    // 3. 맨숫자 `^(#)?(\d+)$` — `#` 존재 또는 길이 < ID_LENGTH.
    {
        let (hash, digit_part) = match trimmed.strip_prefix('#') {
            Some(rest) => (true, rest),
            None => (false, trimmed),
        };
        if let Some(number) = parse_all_digits(digit_part) {
            if hash || digit_part.len() < ID_LENGTH {
                return match current_board_id {
                    None => {
                        if table == "notes" {
                            row_id_by_number(conn, table, None, number)
                        } else {
                            Err(StoreError::new(format!(
                                "board context required to resolve {trimmed} — use board-number"
                            )))
                        }
                    }
                    Some(bid) => row_id_by_number(conn, table, Some(bid), number),
                };
            }
        }
    }

    // 4. id 정확 일치
    let exact: Option<String> = conn
        .query_row(
            &format!("SELECT id FROM {table} WHERE id = ?1"),
            params![trimmed],
            |r| r.get(0),
        )
        .optional()?;
    if exact.is_some() {
        return Ok(exact);
    }

    // 5. id prefix — LIKE 와일드카드는 통째로 거부(진짜 id 는 base36 뿐이라 잃는 기능 없음).
    if trimmed.contains('%') || trimmed.contains('_') {
        return Err(StoreError::new(format!("invalid id prefix: {trimmed}")));
    }
    let mut stmt = conn.prepare(&format!(
        "SELECT id FROM {table} WHERE id LIKE ?1 || '%' LIMIT 2"
    ))?;
    let matches = stmt
        .query_map(params![trimmed], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if matches.len() > 1 {
        return Err(StoreError::new(format!("ambiguous id prefix: {trimmed}")));
    }
    Ok(matches.into_iter().next())
}

fn get_todo_conn(
    conn: &Connection,
    todo_ref: &str,
    current_board_id: Option<&str>,
) -> StoreResult<Option<Todo>> {
    let Some(id) = resolve_ref_id(conn, "todos", todo_ref, current_board_id)? else {
        return Ok(None);
    };
    Ok(conn
        .query_row(
            "SELECT * FROM todos WHERE id = ?1",
            params![id],
            todo_from_row,
        )
        .optional()?)
}

fn must_get_todo_conn(
    conn: &Connection,
    todo_ref: &str,
    current_board_id: Option<&str>,
) -> StoreResult<Todo> {
    get_todo_conn(conn, todo_ref, current_board_id)?
        .ok_or_else(|| StoreError::new(format!("todo not found: {todo_ref}")))
}

fn get_note_conn(
    conn: &Connection,
    note_ref: &str,
    current_board_id: Option<&str>,
) -> StoreResult<Option<Note>> {
    let Some(id) = resolve_ref_id(conn, "notes", note_ref, current_board_id)? else {
        return Ok(None);
    };
    Ok(conn
        .query_row(
            "SELECT * FROM notes WHERE id = ?1",
            params![id],
            note_from_row,
        )
        .optional()?)
}

fn must_get_note_conn(
    conn: &Connection,
    note_ref: &str,
    current_board_id: Option<&str>,
) -> StoreResult<Note> {
    get_note_conn(conn, note_ref, current_board_id)?
        .ok_or_else(|| StoreError::new(format!("note not found: {note_ref}")))
}

/// 웹 편집의 히스토리 묶음 창(초) — 이 안의 같은 actor `edit` 는 한 줄로 친다.
pub const NOTE_EDIT_COALESCE_SECS: i64 = 60;
/// 히스토리 줄 없이도 전역 변경 이벤트를 흘리는 최소 간격 — 다른 화면이 따라오는 속도.
pub const NOTE_EDIT_EVENT_THROTTLE: std::time::Duration = std::time::Duration::from_secs(2);

/// 노트의 CRDT 문서를 연다 — `note_docs.state` 가 있으면 그것으로, 없거나 본문과 어긋나면
/// `notes.content` 로(`NoteDoc::open` 이 맞춘다). 기존 노트는 첫 열기에 content 로 씨앗을 심는다.
///
/// **씨앗을 심었으면(또는 맞췄으면) 그 자리에서 저장한다** — 읽기 경로(`note_doc_state`)여도.
/// 저장하지 않으면 열 때마다 다른 client id 의 새 문서가 생겨, 클라이언트가 받은 상태와
/// 다음 요청이 여는 문서가 서로 다른 히스토리가 된다(같은 글자가 두 번 들어간다).
fn load_note_doc_conn(conn: &Connection, note: &Note) -> StoreResult<NoteDoc> {
    let state: Option<Vec<u8>> = conn
        .query_row(
            "SELECT state FROM note_docs WHERE note_id = ?1",
            params![note.id],
            |row| row.get(0),
        )
        .optional()?;
    let doc = NoteDoc::open(state.as_deref(), &note.content);
    let seeded = match &state {
        None => true,
        Some(bytes) => doc.state() != *bytes,
    };
    if seeded {
        save_note_doc_conn(conn, &note.id, &doc)?;
    }
    Ok(doc)
}

fn save_note_doc_conn(conn: &Connection, note_id: &str, doc: &NoteDoc) -> StoreResult<()> {
    conn.execute(
        "INSERT INTO note_docs (note_id, state, updated_at) VALUES (?1, ?2, ?3)\n\
         ON CONFLICT(note_id) DO UPDATE SET state = excluded.state, updated_at = excluded.updated_at",
        params![note_id, doc.state(), now_iso()],
    )?;
    Ok(())
}

/// 같은 actor 의 `edit` 가 묶음 창 안에 이미 있나 — 히스토리 최신 한 줄만 본다(다른 actor 의
/// 편집이나 다른 action 이 끼어들면 새 줄을 남긴다: "누가 언제" 가 히스토리의 뜻이다).
fn recent_edit_exists_conn(
    conn: &Connection,
    note_id: &str,
    actor: &str,
    now: &str,
) -> StoreResult<bool> {
    let last: Option<(String, String, String)> = conn
        .query_row(
            "SELECT actor, action, at FROM history WHERE entity = 'note' AND entity_id = ?1 ORDER BY id DESC LIMIT 1",
            params![note_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let Some((last_actor, action, at)) = last else {
        return Ok(false);
    };
    if last_actor != actor || action != "edit" {
        return Ok(false);
    }
    let (Ok(then), Ok(now)) = (
        chrono::DateTime::parse_from_rfc3339(&at),
        chrono::DateTime::parse_from_rfc3339(now),
    ) else {
        return Ok(false);
    };
    Ok((now - then).num_seconds() < NOTE_EDIT_COALESCE_SECS)
}

// ── boards / sections ───────────────────────────────────────────────────────

impl TodoStore {
    /// 보드를 key 로 upsert 한다. 옛 key(별칭)도 이 보드로 푼다 — 읽기/쓰기 갈라짐 방지.
    /// 검증(공백/`#` 금지)은 **새 보드 생성**에만 적용한다 — 레거시 malformed 보드는
    /// 조회로 계속 살아남는다.
    pub fn ensure_board(&self, key: &str, title: Option<&str>, actor: &str) -> StoreResult<Board> {
        let mut events = Vec::new();
        let board = {
            let conn = self.lock();
            ensure_board_conn(&conn, &mut events, key, title, actor)?
        };
        self.emit_all(events);
        Ok(board)
    }

    pub fn list_boards(&self, include_archived: bool) -> StoreResult<Vec<Board>> {
        let conn = self.lock();
        let sql = if include_archived {
            "SELECT * FROM boards ORDER BY key"
        } else {
            "SELECT * FROM boards WHERE archived_at IS NULL ORDER BY key"
        };
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt
            .query_map([], board_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        // 별칭은 보드마다 조회하지 않는다 — statusline 라우트가 초당 이 함수를 부른다.
        let mut alias_stmt =
            conn.prepare("SELECT key, board_id FROM board_aliases ORDER BY created_at, key")?;
        let aliases = alias_stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut grouped: HashMap<String, Vec<String>> = HashMap::new();
        for (key, board_id) in aliases {
            grouped.entry(board_id).or_default().push(key);
        }
        Ok(rows
            .into_iter()
            .map(|mut board| {
                if let Some(keys) = grouped.get(&board.id) {
                    if !keys.is_empty() {
                        board.previous_keys = Some(keys.clone());
                    }
                }
                board
            })
            .collect())
    }

    /// 보드 key → boardId. 아카이브 포함, **별칭도 푼다**. 없으면 None.
    pub fn board_id_of(&self, key: &str) -> StoreResult<Option<String>> {
        let conn = self.lock();
        board_id_of_conn(&conn, key)
    }

    /// boardId → board key. 없으면 None (FK 깨짐 — 호출자가 실패시켜야 한다).
    pub fn board_key_of(&self, board_id: &str) -> StoreResult<Option<String>> {
        let conn = self.lock();
        board_key_of_conn(&conn, board_id)
    }

    /// boardId 로 보드 한 건 — 이슈 라우트가 todo → 보드 → repo 를 따라갈 때 쓴다.
    pub fn board_by_id(&self, board_id: &str) -> StoreResult<Option<Board>> {
        let conn = self.lock();
        let row = conn
            .query_row(
                "SELECT * FROM boards WHERE id = ?1",
                params![board_id],
                board_from_row,
            )
            .optional()?;
        match row {
            Some(board) => Ok(Some(hydrate_board(&conn, board)?)),
            None => Ok(None),
        }
    }

    /// 보드 key(옛 key 포함)로 보드 한 건. 없으면 None.
    pub fn get_board(&self, key: &str) -> StoreResult<Option<Board>> {
        let conn = self.lock();
        match board_row_by_any_key(&conn, key)? {
            Some(board) => Ok(Some(hydrate_board(&conn, board)?)),
            None => Ok(None),
        }
    }

    /// 보드 메타 부분 수정 — key(slug)·title·description·repo·path 를 **한 트랜잭션에**.
    /// key 변경은 옛 key 를 `board_aliases` 에 남긴다(입력 전용 별칭). 바뀐 필드가 없으면
    /// no-op(히스토리도 없다). 자세한 규칙은 TS 원본/contract.md 참고.
    pub fn update_board(&self, key: &str, patch: &BoardPatch, actor: &str) -> StoreResult<Board> {
        let mut events = Vec::new();
        let board = {
            let conn = self.lock();
            let existing = board_row_by_any_key(&conn, key)?
                .ok_or_else(|| StoreError::new(format!("board not found: {key}")))?;

            let mut changes = Changes::new();
            let mut sets: Vec<String> = Vec::new();
            let mut vals: Vec<Option<String>> = Vec::new();
            let mut stage = |column: &str, before: Option<&str>, after: Option<String>| {
                if before == after.as_deref() {
                    return;
                }
                changes.insert(column.to_string(), json!([before, after]));
                sets.push(format!("{column} = ?"));
                vals.push(after);
            };

            let mut rename: Option<(String, String)> = None;
            if let Some(next_key) = &patch.key {
                let next_key = next_key.trim();
                if next_key != existing.key {
                    assert_usable_board_key(next_key)?;
                    if let Some(owner) = board_row_by_any_key(&conn, next_key)? {
                        if owner.id != existing.id {
                            return Err(StoreError::new(format!(
                                "board key already in use: {next_key}"
                            )));
                        }
                    }
                    rename = Some((existing.key.clone(), next_key.to_string()));
                    stage("key", Some(&existing.key), Some(next_key.to_string()));
                }
            }
            if let Some(next_title) = &patch.title {
                let next_title = next_title.trim();
                if next_title.is_empty() {
                    return Err(StoreError::new("board title must not be empty"));
                }
                stage("title", Some(&existing.title), Some(next_title.to_string()));
            }
            if let Some(value) = &patch.description {
                stage(
                    "description",
                    existing.description.as_deref(),
                    blank_to_null(value.as_deref()),
                );
            }
            if let Some(value) = &patch.repo {
                stage(
                    "repo",
                    existing.repo.as_deref(),
                    blank_to_null(value.as_deref()),
                );
            }
            if let Some(value) = &patch.path {
                stage(
                    "path",
                    existing.path.as_deref(),
                    blank_to_null(value.as_deref()),
                );
            }
            if let Some(next) = patch.review_fix {
                if next != existing.review_fix {
                    // 히스토리엔 참/거짓으로 남긴다(문자열 "0"/"1" 이 아니라).
                    changes.insert("reviewFix".to_string(), json!([existing.review_fix, next]));
                    sets.push("auto_resolve = ?".to_string());
                    vals.push(Some(if next { "1" } else { "0" }.to_string()));
                }
            }
            if let Some(next) = &patch.pr_authors {
                if let Some(bad) = next.iter().find(|a| !crate::prwatch::is_pr_author(a)) {
                    return Err(StoreError::new(format!(
                        "prAuthors 는 @me 또는 GitHub login 이어야 한다: {bad:?}"
                    )));
                }
                if next != &existing.pr_authors {
                    changes.insert("prAuthors".to_string(), json!([existing.pr_authors, next]));
                    sets.push("pr_authors = ?".to_string());
                    vals.push(Some(
                        serde_json::to_string(next).map_err(|e| StoreError::new(e.to_string()))?,
                    ));
                }
            }

            if sets.is_empty() {
                hydrate_board(&conn, existing)?
            } else {
                // key 변경과 별칭 기록은 원자적이어야 한다.
                conn.execute_batch("BEGIN")?;
                let applied = (|| -> StoreResult<()> {
                    let sql = format!("UPDATE boards SET {} WHERE id = ?", sets.join(", "));
                    let mut all: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
                    for v in &vals {
                        all.push(Box::new(v.clone()));
                    }
                    all.push(Box::new(existing.id.clone()));
                    conn.execute(&sql, params_from_iter(all.iter().map(|b| b.as_ref())))?;
                    if let Some((from, to)) = &rename {
                        // 되돌리기로 자기 별칭을 다시 현재 이름으로 쓰는 경우 — 걷어내야
                        // "key 와 별칭이 같은 이름" 모순이 안 남는다.
                        conn.execute(
                            "DELETE FROM board_aliases WHERE board_id = ?1 AND key = ?2",
                            params![existing.id, to],
                        )?;
                        conn.execute(
                            "INSERT INTO board_aliases (key, board_id, created_at) VALUES (?1, ?2, ?3)",
                            params![from, existing.id, now_iso()],
                        )?;
                    }
                    conn.execute_batch("COMMIT")?;
                    Ok(())
                })();
                if let Err(error) = applied {
                    let _ = conn.execute_batch("ROLLBACK");
                    return Err(error);
                }
                record_history(
                    &conn,
                    &mut events,
                    HistoryEntity::Board,
                    &existing.id,
                    actor,
                    "update",
                    Some(&changes),
                    Some(&existing.id),
                    true,
                )?;
                let updated = conn
                    .query_row(
                        "SELECT * FROM boards WHERE id = ?1",
                        params![existing.id],
                        board_from_row,
                    )
                    .optional()?
                    .unwrap_or(existing);
                hydrate_board(&conn, updated)?
            }
        };
        self.emit_all(events);
        Ok(board)
    }

    /// {@link update_board} 의 좁은 입구 — GitHub 레포 설정.
    pub fn set_board_repo(&self, key: &str, repo: &str, actor: &str) -> StoreResult<Board> {
        self.update_board(
            key,
            &BoardPatch {
                repo: Some(Some(repo.to_string())),
                ..Default::default()
            },
            actor,
        )
    }

    /// {@link update_board} 의 좁은 입구 — 메인 레포 경로. 경로 실재 판정은 spawn 라우트 몫.
    pub fn set_board_path(&self, key: &str, path: &str, actor: &str) -> StoreResult<Board> {
        self.update_board(
            key,
            &BoardPatch {
                path: Some(Some(path.to_string())),
                ..Default::default()
            },
            actor,
        )
    }

    /// 보드 안에서 섹션을 이름으로 upsert — todo_write 의 section 인자가 쓰는 경로.
    pub fn ensure_section(&self, board_id: &str, title: &str, actor: &str) -> StoreResult<Section> {
        let mut events = Vec::new();
        let section = {
            let conn = self.lock();
            ensure_section_conn(&conn, &mut events, board_id, title, actor)?
        };
        self.emit_all(events);
        Ok(section)
    }

    pub fn list_sections(
        &self,
        board_id: &str,
        include_archived: bool,
    ) -> StoreResult<Vec<Section>> {
        let conn = self.lock();
        list_sections_conn(&conn, board_id, include_archived)
    }

    /// 섹션 보관 — 소속 항목은 미분류로 돌려놓는다(항목 자체는 건드리지 않는다).
    pub fn archive_section(&self, id: &str, actor: &str) -> StoreResult<()> {
        let mut events = Vec::new();
        {
            let conn = self.lock();
            let row = conn
                .query_row(
                    "SELECT * FROM sections WHERE id = ?1",
                    params![id],
                    section_from_row,
                )
                .optional()?
                .ok_or_else(|| StoreError::new(format!("section not found: {id}")))?;
            let at = now_iso();
            let affected: Vec<String> = {
                let mut stmt = conn.prepare("SELECT id FROM todos WHERE section_id = ?1")?;
                let ids = stmt
                    .query_map(params![id], |r| r.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                ids
            };
            conn.execute(
                "UPDATE todos SET section_id = NULL, updated_at = ?1 WHERE section_id = ?2",
                params![at, id],
            )?;
            // 각 todo 에도 이력을 남긴다 — 모든 mutation 은 history 기록. 이벤트는 내지
            // 않는다(아래 section archive 하나로 갈음 — 구독자는 refetch 만 한다).
            for todo_id in &affected {
                let mut changes = Changes::new();
                changes.insert("section".into(), json!([id, null]));
                record_history(
                    &conn,
                    &mut events,
                    HistoryEntity::Todo,
                    todo_id,
                    actor,
                    "update",
                    Some(&changes),
                    Some(&row.board_id),
                    false,
                )?;
            }
            conn.execute(
                "UPDATE sections SET archived_at = ?1 WHERE id = ?2",
                params![at, id],
            )?;
            record_history(
                &conn,
                &mut events,
                HistoryEntity::Section,
                id,
                actor,
                "archive",
                None,
                Some(&row.board_id),
                true,
            )?;
        }
        self.emit_all(events);
        Ok(())
    }
}

fn ensure_section_conn(
    conn: &Connection,
    events: &mut Vec<ChangeEvent>,
    board_id: &str,
    title: &str,
    actor: &str,
) -> StoreResult<Section> {
    let existing = conn
        .query_row(
            "SELECT * FROM sections WHERE board_id = ?1 AND title = ?2 AND archived_at IS NULL",
            params![board_id, title],
            section_from_row,
        )
        .optional()?;
    if let Some(section) = existing {
        return Ok(section);
    }
    let position = next_position(conn, "sections", Some(board_id))?;
    let section = Section {
        id: new_id(),
        board_id: board_id.to_string(),
        title: title.to_string(),
        position,
        archived_at: None,
    };
    conn.execute(
        "INSERT INTO sections (id, board_id, title, position) VALUES (?1, ?2, ?3, ?4)",
        params![section.id, board_id, title, position],
    )?;
    record_history(
        conn,
        events,
        HistoryEntity::Section,
        &section.id,
        actor,
        "create",
        None,
        Some(board_id),
        true,
    )?;
    Ok(section)
}

fn list_sections_conn(
    conn: &Connection,
    board_id: &str,
    include_archived: bool,
) -> StoreResult<Vec<Section>> {
    let sql = if include_archived {
        "SELECT * FROM sections WHERE board_id = ?1 ORDER BY position"
    } else {
        "SELECT * FROM sections WHERE board_id = ?1 AND archived_at IS NULL ORDER BY position"
    };
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt
        .query_map(params![board_id], section_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// `ensure_board` 의 conn 버전 — create_todo/create_note/move 가 락 안에서 부른다.
fn ensure_board_conn(
    conn: &Connection,
    events: &mut Vec<ChangeEvent>,
    key: &str,
    title: Option<&str>,
    actor: &str,
) -> StoreResult<Board> {
    if let Some(existing) = board_row_by_any_key(conn, key)? {
        return hydrate_board(conn, existing);
    }
    assert_usable_board_key(key)?;
    let board = Board {
        id: new_id(),
        key: key.to_string(),
        title: title.unwrap_or(key).to_string(),
        description: None,
        repo: None,
        path: None,
        previous_keys: None,
        review_fix: false,
        pr_authors: Vec::new(),
        created_at: now_iso(),
        archived_at: None,
    };
    conn.execute(
        "INSERT INTO boards (id, key, title, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![board.id, board.key, board.title, board.created_at],
    )?;
    record_history(
        conn,
        events,
        HistoryEntity::Board,
        &board.id,
        actor,
        "create",
        None,
        Some(&board.id),
        true,
    )?;
    Ok(board)
}

// ── todos ───────────────────────────────────────────────────────────────────

impl TodoStore {
    pub fn create_todo(&self, input: &CreateTodoInput, actor: &str) -> StoreResult<Todo> {
        let mut events = Vec::new();
        let todo = {
            let conn = self.lock();
            let board = ensure_board_conn(&conn, &mut events, &input.board, None, actor)?;
            // 공백뿐인 이름으로 섹션을 만들지 않고, 앞뒤 공백은 다듬는다(updateTodo 와 동일).
            let mut section_id: Option<String> = None;
            let section_title = input.section.as_deref().unwrap_or("").trim().to_string();
            if !section_title.is_empty() {
                section_id = Some(
                    ensure_section_conn(&conn, &mut events, &board.id, &section_title, actor)?.id,
                );
            }
            let mut parent_id: Option<String> = None;
            if let Some(parent_ref) = &input.parent_id {
                let parent = get_todo_conn(&conn, parent_ref, Some(&board.id))?;
                match parent {
                    Some(parent) if parent.board_id == board.id => parent_id = Some(parent.id),
                    _ => {
                        return Err(StoreError::new(format!(
                            "parent todo not found in board {}: {parent_ref}",
                            input.board
                        )))
                    }
                }
            }
            let now = now_iso();
            let todo = Todo {
                id: new_id(),
                number: next_number(&conn, "todos", Some(&board.id))?,
                board_id: board.id.clone(),
                section_id: section_id.clone(),
                parent_id: parent_id.clone(),
                title: input.title.clone(),
                description: input.description.clone().unwrap_or_default(),
                status: TodoStatus::Todo,
                priority: input.priority.unwrap_or(TodoPriority::P4),
                due: input.due.clone(),
                labels: input.labels.clone().unwrap_or_default(),
                links: input.links.clone().unwrap_or_default(),
                doing_by: None,
                doing_since: None,
                doing_session_id: None,
                position: next_position(&conn, "todos", Some(&board.id))?,
                created_at: now.clone(),
                updated_at: now,
                completed_at: None,
                archived_at: None,
            };
            conn.execute(
                "INSERT INTO todos (id, number, board_id, section_id, parent_id, title, description, status, priority, due, labels, links, position, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                params![
                    todo.id,
                    todo.number,
                    todo.board_id,
                    section_id,
                    parent_id,
                    todo.title,
                    todo.description,
                    todo.status.as_str(),
                    todo.priority.as_str(),
                    todo.due,
                    serde_json::to_string(&todo.labels).map_err(|e| StoreError::new(e.to_string()))?,
                    serde_json::to_string(&todo.links).map_err(|e| StoreError::new(e.to_string()))?,
                    todo.position,
                    todo.created_at,
                    todo.updated_at,
                ],
            )?;
            record_history(
                &conn,
                &mut events,
                HistoryEntity::Todo,
                &todo.id,
                actor,
                "create",
                None,
                Some(&board.id),
                true,
            )?;
            todo
        };
        self.emit_all(events);
        Ok(todo)
    }

    pub fn update_todo(
        &self,
        todo_ref: &str,
        patch: &UpdateTodoPatch,
        actor: &str,
        current_board_id: Option<&str>,
    ) -> StoreResult<Todo> {
        let mut events = Vec::new();
        let todo = {
            let conn = self.lock();
            let current = must_get_todo_conn(&conn, todo_ref, current_board_id)?;
            let mut changes = Changes::new();
            let mut sets: Vec<String> = Vec::new();
            let mut vals: Vec<Value> = Vec::new();

            // TS 의 apply — JSON 값 비교로 실변경만 스테이징. changes 에는 [old??null, new??null].
            let mut apply = |column: &str,
                             field: &str,
                             old_value: Value,
                             new_value: Value,
                             serialized: Value| {
                if old_value == new_value {
                    return;
                }
                changes.insert(field.to_string(), json!([old_value, new_value]));
                sets.push(format!("{column} = ?"));
                vals.push(serialized);
            };

            if let Some(title) = &patch.title {
                apply(
                    "title",
                    "title",
                    json!(current.title),
                    json!(title),
                    json!(title),
                );
            }
            if let Some(description) = &patch.description {
                apply(
                    "description",
                    "description",
                    json!(current.description),
                    json!(description),
                    json!(description),
                );
            }
            if let Some(priority) = patch.priority {
                apply(
                    "priority",
                    "priority",
                    json!(current.priority),
                    json!(priority),
                    json!(priority.as_str()),
                );
            }
            if let Some(due) = &patch.due {
                apply("due", "due", json!(current.due), json!(due), json!(due));
            }
            if let Some(labels) = &patch.labels {
                apply(
                    "labels",
                    "labels",
                    json!(current.labels),
                    json!(labels),
                    json!(serde_json::to_string(labels)
                        .map_err(|e| StoreError::new(e.to_string()))?),
                );
            }
            if let Some(links) = &patch.links {
                apply(
                    "links",
                    "links",
                    json!(current.links),
                    json!(links),
                    json!(serde_json::to_string(links).map_err(|e| StoreError::new(e.to_string()))?),
                );
            }
            if let Some(section) = &patch.section {
                // 빈 이름 섹션은 사고다 — 공백뿐인 입력은 해제로 본다.
                let title = section.as_deref().unwrap_or("").trim().to_string();
                if title.is_empty() {
                    apply(
                        "section_id",
                        "section",
                        json!(current.section_id),
                        Value::Null,
                        Value::Null,
                    );
                } else {
                    let section =
                        ensure_section_conn(&conn, &mut events, &current.board_id, &title, actor)?;
                    apply(
                        "section_id",
                        "section",
                        json!(current.section_id),
                        json!(section.id),
                        json!(section.id),
                    );
                }
            }
            if let Some(parent) = &patch.parent_id {
                match parent {
                    None => apply(
                        "parent_id",
                        "parentId",
                        json!(current.parent_id),
                        Value::Null,
                        Value::Null,
                    ),
                    Some(parent_ref) => {
                        let parent =
                            must_get_todo_conn(&conn, parent_ref, Some(&current.board_id))?;
                        if parent.board_id != current.board_id {
                            return Err(StoreError::new(format!(
                                "parent todo not in same board: {parent_ref}"
                            )));
                        }
                        if parent.id == current.id {
                            return Err(StoreError::new("todo cannot be its own parent"));
                        }
                        apply(
                            "parent_id",
                            "parentId",
                            json!(current.parent_id),
                            json!(parent.id),
                            json!(parent.id),
                        );
                    }
                }
            }

            if sets.is_empty() {
                current
            } else {
                sets.push("updated_at = ?".to_string());
                vals.push(json!(now_iso()));
                let sql = format!("UPDATE todos SET {} WHERE id = ?", sets.join(", "));
                let mut all: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
                for v in &vals {
                    all.push(json_to_sql(v));
                }
                all.push(Box::new(current.id.clone()));
                conn.execute(&sql, params_from_iter(all.iter().map(|b| b.as_ref())))?;
                record_history(
                    &conn,
                    &mut events,
                    HistoryEntity::Todo,
                    &current.id,
                    actor,
                    "update",
                    Some(&changes),
                    Some(&current.board_id),
                    true,
                )?;
                must_get_todo_conn(&conn, &current.id, None)?
            }
        };
        self.emit_all(events);
        Ok(todo)
    }

    pub fn set_todo_status(
        &self,
        todo_ref: &str,
        action: StatusAction,
        actor: &str,
        current_board_id: Option<&str>,
    ) -> StoreResult<Todo> {
        let mut events = Vec::new();
        let todo = {
            let conn = self.lock();
            let current = must_get_todo_conn(&conn, todo_ref, current_board_id)?;
            let now = now_iso();
            let mut changes = Changes::new();

            match action {
                StatusAction::Start => {
                    changes.insert("status".into(), json!([current.status, "doing"]));
                    // 착수를 핸드오프에 귀속시키고, 그 세션 id 를 doing 에 물려준다.
                    // 사람이 누른 start 면 None 이 와서 예전과 같은 모양이 된다.
                    let session_id = accept_handoff_for(&conn, &current.id, actor, &now)?;
                    conn.execute(
                        "UPDATE todos SET status = ?1, doing_by = ?2, doing_since = ?3, doing_session_id = ?4, updated_at = ?5 WHERE id = ?6",
                        params!["doing", actor, now, session_id, now, current.id],
                    )?;
                }
                StatusAction::Stop => {
                    changes.insert("status".into(), json!([current.status, "todo"]));
                    conn.execute(
                        "UPDATE todos SET status = ?1, doing_by = NULL, doing_since = NULL, doing_session_id = NULL, updated_at = ?2 WHERE id = ?3",
                        params!["todo", now, current.id],
                    )?;
                }
                StatusAction::Done => {
                    changes.insert("status".into(), json!([current.status, "done"]));
                    complete_handoff_for(&conn, &current.id, actor, &now)?;
                    conn.execute(
                        "UPDATE todos SET status = ?1, doing_by = NULL, doing_since = NULL, doing_session_id = NULL, completed_at = ?2, updated_at = ?3 WHERE id = ?4",
                        params!["done", now, now, current.id],
                    )?;
                }
                StatusAction::Reopen => {
                    changes.insert("status".into(), json!([current.status, "todo"]));
                    conn.execute(
                        "UPDATE todos SET status = ?1, completed_at = NULL, updated_at = ?2 WHERE id = ?3",
                        params!["todo", now, current.id],
                    )?;
                }
                StatusAction::Archive => {
                    changes.insert("archived".into(), json!([false, true]));
                    conn.execute(
                        "UPDATE todos SET archived_at = ?1, updated_at = ?2 WHERE id = ?3",
                        params![now, now, current.id],
                    )?;
                }
                StatusAction::Unarchive => {
                    changes.insert("archived".into(), json!([true, false]));
                    conn.execute(
                        "UPDATE todos SET archived_at = NULL, updated_at = ?1 WHERE id = ?2",
                        params![now, current.id],
                    )?;
                }
            }

            record_history(
                &conn,
                &mut events,
                HistoryEntity::Todo,
                &current.id,
                actor,
                action.as_str(),
                Some(&changes),
                Some(&current.board_id),
                true,
            )?;
            must_get_todo_conn(&conn, &current.id, None)?
        };
        self.emit_all(events);
        Ok(todo)
    }

    /// 보드에 등록한 수집함 — `board` 가 있으면 그 보드만, 없으면 전부(보관된 보드 제외). 만든 순서.
    pub fn list_board_inbox_sources(
        &self,
        board: Option<&str>,
    ) -> StoreResult<Vec<crate::inbox::BoardInboxSource>> {
        let conn = self.lock();
        let board_id = match board {
            Some(key) => match board_id_of_conn(&conn, key)? {
                Some(id) => Some(id),
                None => return Err(StoreError::new(format!("board not found: {key}"))),
            },
            None => None,
        };
        let mut stmt = conn.prepare(
            "SELECT s.id, b.key, s.name, s.adapter, s.params, s.created_at
               FROM board_inbox_sources s JOIN boards b ON b.id = s.board_id
              WHERE b.archived_at IS NULL AND (?1 IS NULL OR s.board_id = ?1)
              ORDER BY s.created_at, s.id",
        )?;
        let rows = stmt.query_map(params![board_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, board, name, adapter, params, created_at) = row?;
            out.push(crate::inbox::BoardInboxSource {
                id,
                board,
                name,
                adapter,
                params: serde_json::from_str(&params)
                    .map_err(|e| StoreError::new(format!("board_inbox_sources.params: {e}")))?,
                created_at,
            });
        }
        Ok(out)
    }

    /// 보드에 수집함을 등록한다. 값 검증(어댑터가 받는 칸인가)은 호출자(데몬)가 어댑터의 `--describe` 로
    /// 끝낸 뒤다 — 여기서는 이름 규칙과 중복만 본다. 보드 히스토리에 남기고 보드 이벤트를 낸다.
    pub fn create_board_inbox_source(
        &self,
        board: &str,
        name: &str,
        adapter: &str,
        params: &[crate::inbox::InboxParam],
        actor: &str,
    ) -> StoreResult<crate::inbox::BoardInboxSource> {
        if name.is_empty()
            || !name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return Err(StoreError::new(format!(
                "수집함 이름은 [a-z0-9-]+ 여야 한다: {name:?}"
            )));
        }
        let mut events = Vec::new();
        let created = {
            let conn = self.lock();
            let row = board_row_by_any_key(&conn, board)?
                .ok_or_else(|| StoreError::new(format!("board not found: {board}")))?;
            // 보관된 보드의 수집함은 목록에 안 나와 지울 길이 없다 — 이름만 전역으로 붙잡는다.
            if row.archived_at.is_some() {
                return Err(StoreError::new(format!(
                    "보관된 보드에는 수집함을 등록할 수 없다: {board}"
                )));
            }
            let board_id = row.id.clone();
            let taken: bool = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM board_inbox_sources WHERE name = ?1)",
                params![name],
                |r| r.get(0),
            )?;
            if taken {
                return Err(StoreError::new(format!("이미 있는 수집함 이름: {name}")));
            }
            let source = crate::inbox::BoardInboxSource {
                id: new_id(),
                // 별칭으로 등록해도 응답은 현재 key — 목록과 같은 값.
                board: row.key.clone(),
                name: name.to_string(),
                adapter: adapter.to_string(),
                params: params.to_vec(),
                created_at: now_iso(),
            };
            let params_json = serde_json::to_string(&source.params)
                .map_err(|e| StoreError::new(e.to_string()))?;
            conn.execute(
                "INSERT INTO board_inbox_sources (id, board_id, name, adapter, params, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![source.id, board_id, source.name, source.adapter, params_json, source.created_at],
            )?;
            let mut changes = Changes::new();
            changes.insert("inboxSource".into(), json!([null, name]));
            record_history(
                &conn,
                &mut events,
                HistoryEntity::Board,
                &board_id,
                actor,
                "inbox-source-add",
                Some(&changes),
                Some(&board_id),
                true,
            )?;
            source
        };
        self.emit_all(events);
        Ok(created)
    }

    /// 보드 수집함을 지운다(보관 없이 — 조건일 뿐 기록할 내용이 없다). 없으면 에러.
    pub fn delete_board_inbox_source(&self, id: &str, actor: &str) -> StoreResult<()> {
        let mut events = Vec::new();
        {
            let conn = self.lock();
            let (board_id, name): (String, String) = conn
                .query_row(
                    "SELECT board_id, name FROM board_inbox_sources WHERE id = ?1",
                    params![id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?
                .ok_or_else(|| StoreError::new(format!("inbox source not found: {id}")))?;
            conn.execute("DELETE FROM board_inbox_sources WHERE id = ?1", params![id])?;
            let mut changes = Changes::new();
            changes.insert("inboxSource".into(), json!([name, null]));
            record_history(
                &conn,
                &mut events,
                HistoryEntity::Board,
                &board_id,
                actor,
                "inbox-source-remove",
                Some(&changes),
                Some(&board_id),
                true,
            )?;
        }
        self.emit_all(events);
        Ok(())
    }

    /// 세션을 수집함 소스의 구독자로 — 이미 있으면 소켓·지문을 갱신하고 기준선을 다시 잡는다. `baseline` 은
    /// 구독 시점에 이미 있던 항목 id 로, **이 세션에게** "본 것" 이 된다(세션마다 따로 — 한 세션의 구독이
    /// 다른 세션이 아직 못 받은 항목을 삼키지 않게). `fingerprint` 는 소스의 실행 argv 지문(바뀌면 감시가
    /// 기준선을 다시 잡는다).
    pub fn subscribe_inbox(
        &self,
        source: &str,
        session_id: &str,
        socket: &str,
        fingerprint: &str,
        baseline: &[String],
    ) -> StoreResult<()> {
        let conn = self.lock();
        let at = now_iso();
        conn.execute_batch("BEGIN")?;
        let applied = (|| -> StoreResult<()> {
            conn.execute(
                "INSERT INTO inbox_subscriptions (source, session_id, socket, fingerprint, created_at) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(source, session_id) DO UPDATE SET socket = excluded.socket, fingerprint = excluded.fingerprint",
                params![source, session_id, socket, fingerprint, at],
            )?;
            conn.execute(
                "DELETE FROM inbox_seen WHERE source = ?1 AND session_id = ?2",
                params![source, session_id],
            )?;
            for id in baseline {
                conn.execute(
                    "INSERT OR IGNORE INTO inbox_seen (source, session_id, item_id, seen_at) VALUES (?1, ?2, ?3, ?4)",
                    params![source, session_id, id, at],
                )?;
            }
            conn.execute_batch("COMMIT")?;
            Ok(())
        })();
        if let Err(error) = applied {
            let _ = conn.execute_batch("ROLLBACK");
            return Err(error);
        }
        Ok(())
    }

    /// 구독 해지 — `source` 가 None 이면 그 세션의 구독 전부(세션이 끝났을 때). 그 구독의 본 항목도 지운다
    /// (다시 구독하면 기준선을 새로 잡으니 안전하다). 지운 구독 수.
    pub fn unsubscribe_inbox(&self, source: Option<&str>, session_id: &str) -> StoreResult<usize> {
        let conn = self.lock();
        conn.execute(
            "DELETE FROM inbox_seen WHERE session_id = ?1 AND (?2 IS NULL OR source = ?2)",
            params![session_id, source],
        )?;
        Ok(conn.execute(
            "DELETE FROM inbox_subscriptions WHERE session_id = ?1 AND (?2 IS NULL OR source = ?2)",
            params![session_id, source],
        )?)
    }

    /// 구독 전부 — 소스·만든 순.
    pub fn inbox_subscriptions(&self) -> StoreResult<Vec<crate::inbox::InboxSubscription>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT source, session_id, socket, fingerprint, created_at FROM inbox_subscriptions ORDER BY source, created_at",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(crate::inbox::InboxSubscription {
                source: r.get(0)?,
                session_id: r.get(1)?,
                socket: r.get(2)?,
                fingerprint: r.get(3)?,
                created_at: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// `ids` 중 이 세션이 아직 안 본 것 — 적지는 않는다(보내기에 성공한 뒤 `mark_inbox_seen`).
    pub fn unseen_inbox_ids(
        &self,
        source: &str,
        session_id: &str,
        ids: &[String],
    ) -> StoreResult<Vec<String>> {
        let conn = self.lock();
        let mut out = Vec::new();
        for id in ids {
            let seen: bool = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM inbox_seen WHERE source = ?1 AND session_id = ?2 AND item_id = ?3)",
                params![source, session_id, id],
                |r| r.get(0),
            )?;
            if !seen {
                out.push(id.clone());
            }
        }
        Ok(out)
    }

    /// 이 세션에게 "본 것" 으로 적는다.
    pub fn mark_inbox_seen(
        &self,
        source: &str,
        session_id: &str,
        ids: &[String],
    ) -> StoreResult<()> {
        let conn = self.lock();
        let at = now_iso();
        for id in ids {
            conn.execute(
                "INSERT OR IGNORE INTO inbox_seen (source, session_id, item_id, seen_at) VALUES (?1, ?2, ?3, ?4)",
                params![source, session_id, id, at],
            )?;
        }
        Ok(())
    }

    /// 세션 받은편지함 등록을 남긴다(같은 세션이면 덮어쓴다) — 데몬이 다시 떠도 첫 tick 의 알림이 갈 곳이 있게.
    pub fn save_session_inbox(
        &self,
        registration: &crate::peer_inbox::InboxRegistration,
    ) -> StoreResult<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO session_inboxes (session_id, socket, cwd, seen_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(session_id) DO UPDATE SET socket = excluded.socket, cwd = excluded.cwd, seen_at = excluded.seen_at",
            params![registration.session_id, registration.socket, registration.cwd, registration.seen_at],
        )?;
        Ok(())
    }

    /// `since`(유닉스 초) 이후에 갱신된 등록만 — 오래된 것은 지운다.
    pub fn load_session_inboxes(
        &self,
        since: i64,
    ) -> StoreResult<Vec<crate::peer_inbox::InboxRegistration>> {
        let conn = self.lock();
        conn.execute(
            "DELETE FROM session_inboxes WHERE seen_at < ?1",
            params![since],
        )?;
        let mut stmt =
            conn.prepare("SELECT session_id, socket, cwd, seen_at FROM session_inboxes")?;
        let rows = stmt
            .query_map([], |r| {
                Ok(crate::peer_inbox::InboxRegistration {
                    session_id: r.get(0)?,
                    socket: r.get(1)?,
                    cwd: r.get(2)?,
                    seen_at: r.get(3)?,
                    restored: true,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn delete_session_inbox(&self, session_id: &str) -> StoreResult<()> {
        let conn = self.lock();
        conn.execute(
            "DELETE FROM session_inboxes WHERE session_id = ?1",
            params![session_id],
        )?;
        Ok(())
    }

    /// 모든 todo(보관 포함, 전 보드)의 링크 url — 수집함 항목이 이미 올라갔는지의 판정 근거.
    /// 수집함은 전역이라 어느 보드에 올렸든, 끝내고 보관했든 "받아 두었다" 로 본다.
    pub fn linked_urls(&self) -> StoreResult<std::collections::HashSet<String>> {
        let conn = self.lock();
        let mut stmt = conn.prepare("SELECT links FROM todos WHERE links != '[]'")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        let mut urls = std::collections::HashSet::new();
        for raw in rows {
            let links: Vec<crate::types::TodoLink> =
                serde_json::from_str(&raw?).map_err(|e| StoreError::new(e.to_string()))?;
            urls.extend(links.into_iter().map(|l| l.url));
        }
        Ok(urls)
    }

    pub fn list_todos(&self, filter: &ListTodosFilter) -> StoreResult<Vec<Todo>> {
        let conn = self.lock();
        let mut wheres: Vec<&str> = Vec::new();
        let mut params_vec: Vec<String> = Vec::new();
        if !filter.include_archived {
            wheres.push("archived_at IS NULL");
        }
        if let Some(board) = &filter.board {
            let Some(board_id) = board_id_of_conn(&conn, board)? else {
                return Ok(Vec::new());
            };
            wheres.push("board_id = ?");
            params_vec.push(board_id);
        }
        if let Some(status) = filter.status {
            wheres.push("status = ?");
            params_vec.push(status.as_str().to_string());
        }
        let where_sql = if wheres.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", wheres.join(" AND "))
        };
        let mut stmt = conn.prepare(&format!(
            "SELECT * FROM todos {where_sql} ORDER BY position"
        ))?;
        let rows = stmt
            .query_map(params_from_iter(params_vec.iter()), todo_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(match &filter.label {
            Some(label) => rows
                .into_iter()
                .filter(|t| t.labels.iter().any(|l| l == label))
                .collect(),
            None => rows,
        })
    }

    /// todo 를 다른 보드로 옮긴다 — 번호는 대상 보드에서 새로 발급, 섹션은 같은 이름의
    /// 미보관 섹션이 있을 때만 잇고, 부모는 끊는다(하위 항목이 있으면 거부).
    pub fn move_todo_to_board(
        &self,
        todo_ref: &str,
        target_board_key: &str,
        actor: &str,
        current_board_id: Option<&str>,
    ) -> StoreResult<Todo> {
        let mut events = Vec::new();
        let todo = {
            let conn = self.lock();
            let todo = must_get_todo_conn(&conn, todo_ref, current_board_id)?;
            let target = ensure_board_conn(&conn, &mut events, target_board_key, None, actor)?;
            if target.id == todo.board_id {
                todo // 같은 보드 = 제자리
            } else {
                let child_count: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM todos WHERE parent_id = ?1",
                    params![todo.id],
                    |r| r.get(0),
                )?;
                if child_count > 0 {
                    return Err(StoreError::new(format!(
                        "cannot move todo with children: {todo_ref} — 하위 항목을 먼저 옮기거나 분리한다"
                    )));
                }
                let origin_key = board_key_of_conn(&conn, &todo.board_id)?;
                conn.execute_batch("BEGIN")?;
                let moved = (|| -> StoreResult<Todo> {
                    let number = next_number(&conn, "todos", Some(&target.id))?;
                    let position = next_position(&conn, "todos", Some(&target.id))?;
                    // 같은 이름의 미보관 섹션이 대상에 있을 때만 소속을 잇는다.
                    let mut section_id: Option<String> = None;
                    if let Some(current_section) = &todo.section_id {
                        let section_title: Option<String> = conn
                            .query_row(
                                "SELECT title FROM sections WHERE id = ?1",
                                params![current_section],
                                |r| r.get(0),
                            )
                            .optional()?;
                        if let Some(title) = section_title {
                            section_id = list_sections_conn(&conn, &target.id, false)?
                                .into_iter()
                                .find(|s| s.title == title)
                                .map(|s| s.id);
                        }
                    }
                    conn.execute(
                        "UPDATE todos SET board_id = ?1, number = ?2, position = ?3, section_id = ?4, parent_id = NULL, updated_at = ?5 WHERE id = ?6",
                        params![target.id, number, position, section_id, now_iso(), todo.id],
                    )?;
                    let mut changes = Changes::new();
                    changes.insert(
                        "board".into(),
                        json!([
                            origin_key.clone().unwrap_or_else(|| todo.board_id.clone()),
                            target.key
                        ]),
                    );
                    changes.insert("number".into(), json!([todo.number, number]));
                    record_history(
                        &conn,
                        &mut events,
                        HistoryEntity::Todo,
                        &todo.id,
                        actor,
                        "move-board",
                        Some(&changes),
                        Some(&target.id),
                        true,
                    )?;
                    must_get_todo_conn(&conn, &todo.id, None)
                })();
                match moved {
                    Ok(todo) => {
                        conn.execute_batch("COMMIT")?;
                        todo
                    }
                    Err(error) => {
                        let _ = conn.execute_batch("ROLLBACK");
                        return Err(error);
                    }
                }
            }
        };
        self.emit_all(events);
        Ok(todo)
    }

    /// 같은 보드 안에서 표시 순서를 옮긴다 — 보드 전체 행(보관 포함)을 1-based 로 재부여.
    pub fn move_todo(
        &self,
        todo_ref: &str,
        before_ref: Option<&str>,
        actor: &str,
        current_board_id: Option<&str>,
    ) -> StoreResult<Todo> {
        let mut events = Vec::new();
        let todo = {
            let conn = self.lock();
            let todo = must_get_todo_conn(&conn, todo_ref, current_board_id)?;
            let mut before_id: Option<String> = None;
            if let Some(before_ref) = before_ref {
                let before = must_get_todo_conn(&conn, before_ref, Some(&todo.board_id))?;
                if before.board_id != todo.board_id {
                    return Err(StoreError::new(format!(
                        "move target is in a different board: {before_ref}"
                    )));
                }
                if before.id == todo.id {
                    self.emit_all(events);
                    return Ok(todo); // 자기 앞으로 = 제자리
                }
                before_id = Some(before.id);
            }
            conn.execute_batch("BEGIN")?;
            let moved = (|| -> StoreResult<Todo> {
                let ordered: Vec<String> = {
                    let mut stmt =
                        conn.prepare("SELECT id FROM todos WHERE board_id = ?1 ORDER BY position")?;
                    let ids = stmt
                        .query_map(params![todo.board_id], |r| r.get::<_, String>(0))?
                        .collect::<rusqlite::Result<Vec<_>>>()?;
                    ids
                };
                let old_index = ordered
                    .iter()
                    .position(|id| *id == todo.id)
                    .map(|i| i as i64)
                    .unwrap_or(-1);
                let mut ids: Vec<String> =
                    ordered.into_iter().filter(|id| *id != todo.id).collect();
                let at = match &before_id {
                    None => ids.len(),
                    Some(bid) => ids.iter().position(|id| id == bid).unwrap_or(ids.len()),
                };
                ids.insert(at, todo.id.clone());
                {
                    let mut update =
                        conn.prepare("UPDATE todos SET position = ?1 WHERE id = ?2")?;
                    for (index, id) in ids.iter().enumerate() {
                        // 생성 경로(next_position = MAX+1)와 같은 1-based 재부여.
                        update.execute(params![(index as i64) + 1, id])?;
                    }
                }
                // 옮긴 행만 updated_at 을 올린다 — 나머지는 자리만 재부여됐다.
                conn.execute(
                    "UPDATE todos SET updated_at = ?1 WHERE id = ?2",
                    params![now_iso(), todo.id],
                )?;
                let mut changes = Changes::new();
                changes.insert("position".into(), json!([old_index + 1, (at as i64) + 1]));
                record_history(
                    &conn,
                    &mut events,
                    HistoryEntity::Todo,
                    &todo.id,
                    actor,
                    "reorder",
                    Some(&changes),
                    Some(&todo.board_id),
                    true,
                )?;
                must_get_todo_conn(&conn, &todo.id, None)
            })();
            match moved {
                Ok(todo) => {
                    conn.execute_batch("COMMIT")?;
                    todo
                }
                Err(error) => {
                    let _ = conn.execute_batch("ROLLBACK");
                    return Err(error);
                }
            }
        };
        self.emit_all(events);
        Ok(todo)
    }

    pub fn get_todo(
        &self,
        todo_ref: &str,
        current_board_id: Option<&str>,
    ) -> StoreResult<Option<Todo>> {
        let conn = self.lock();
        get_todo_conn(&conn, todo_ref, current_board_id)
    }
}

/// serde_json Value → SQL 파라미터. 이 스토어의 컬럼 값은 TEXT/INTEGER/NULL 뿐이다.
fn json_to_sql(value: &Value) -> Box<dyn rusqlite::ToSql> {
    match value {
        Value::Null => Box::new(None::<String>),
        Value::String(s) => Box::new(s.clone()),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Box::new(i)
            } else {
                Box::new(n.to_string())
            }
        }
        Value::Bool(b) => Box::new(*b),
        other => Box::new(other.to_string()),
    }
}

// ── notes ───────────────────────────────────────────────────────────────────

impl TodoStore {
    pub fn create_note(&self, input: &CreateNoteInput, actor: &str) -> StoreResult<Note> {
        let mut events = Vec::new();
        let note = {
            let conn = self.lock();
            let mut board_id: Option<String> = None;
            if let Some(board) = &input.board {
                board_id = Some(ensure_board_conn(&conn, &mut events, board, None, actor)?.id);
            }
            let now = now_iso();
            let note = Note {
                id: new_id(),
                number: next_number(&conn, "notes", board_id.as_deref())?,
                board_id: board_id.clone(),
                title: input.title.clone(),
                content: input.content.clone().unwrap_or_default(),
                position: next_position(&conn, "notes", board_id.as_deref())?,
                created_at: now.clone(),
                updated_at: now,
                archived_at: None,
                pinned_at: None,
            };
            conn.execute(
                "INSERT INTO notes (id, number, board_id, title, content, position, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    note.id,
                    note.number,
                    board_id,
                    note.title,
                    note.content,
                    note.position,
                    note.created_at,
                    note.updated_at,
                ],
            )?;
            record_history(
                &conn,
                &mut events,
                HistoryEntity::Note,
                &note.id,
                actor,
                "create",
                None,
                note.board_id.as_deref(),
                true,
            )?;
            note
        };
        self.emit_all(events);
        Ok(note)
    }

    pub fn update_note(
        &self,
        note_ref: &str,
        patch: &UpdateNotePatch,
        actor: &str,
        current_board_id: Option<&str>,
    ) -> StoreResult<Note> {
        let mut events = Vec::new();
        let mut doc_events = Vec::new();
        let note = {
            let conn = self.lock();
            let current = must_get_note_conn(&conn, note_ref, current_board_id)?;
            // state·content·history 는 한 트랜잭션 — 사이에서 죽으면 새 state 와 옛 본문이 같이
            // 남고, 다음 열기가 content 를 진실로 보고 편집을 되돌리는 삭제를 저장한다.
            conn.execute_batch("BEGIN")?;
            let result = (|| -> StoreResult<Note> {
                let mut changes = Changes::new();
                let mut sets: Vec<&str> = Vec::new();
                let mut vals: Vec<String> = Vec::new();

                if let Some(title) = &patch.title {
                    if *title != current.title {
                        changes.insert("title".into(), json!([current.title, title]));
                        sets.push("title = ?");
                        vals.push(title.clone());
                    }
                }
                if let Some(content) = &patch.content {
                    // 본문은 CRDT 문서를 거친다 — set 도 통째 교체가 아니라 최소 편집이라, 같은
                    // 순간 웹에서 고치던 글자가 살아남는다(설계 2026-09-28-note-crdt-design).
                    let doc = load_note_doc_conn(&conn, &current)?;
                    let sv_before = doc.state_vector();
                    match patch.mode {
                        NoteContentMode::Append => doc.append(content),
                        NoteContentMode::Set => doc.set_text(content),
                    }
                    let next = doc.text();
                    if next != current.content {
                        save_note_doc_conn(&conn, &current.id, &doc)?;
                        doc_events.push(NoteDocEvent {
                            note_id: current.id.clone(),
                            update: doc.diff_since(&sv_before).map_err(StoreError::new)?,
                            actor: actor.to_string(),
                            client: None,
                        });
                        changes.insert("content".into(), json!([current.content, next]));
                        sets.push("content = ?");
                        vals.push(next);
                    }
                }

                if sets.is_empty() {
                    return Ok(current.clone());
                }
                sets.push("updated_at = ?");
                vals.push(now_iso());
                vals.push(current.id.clone());
                let sql = format!("UPDATE notes SET {} WHERE id = ?", sets.join(", "));
                conn.execute(&sql, params_from_iter(vals.iter()))?;
                record_history(
                    &conn,
                    &mut events,
                    HistoryEntity::Note,
                    &current.id,
                    actor,
                    "update",
                    Some(&changes),
                    current.board_id.as_deref(),
                    true,
                )?;
                must_get_note_conn(&conn, &current.id, None)
            })();
            match result {
                Ok(note) => {
                    conn.execute_batch("COMMIT")?;
                    note
                }
                Err(error) => {
                    let _ = conn.execute_batch("ROLLBACK");
                    return Err(error);
                }
            }
        };
        self.emit_all(events);
        self.emit_docs(doc_events);
        Ok(note)
    }

    /// 노트 문서의 상태 — 처음 여는 클라이언트에게 주는 전체 update 와 서버의 state vector.
    /// `since` 가 있으면 그 이후의 차분만.
    pub fn note_doc_state(
        &self,
        note_ref: &str,
        since: Option<&[u8]>,
        current_board_id: Option<&str>,
    ) -> StoreResult<NoteDocState> {
        let conn = self.lock();
        let note = must_get_note_conn(&conn, note_ref, current_board_id)?;
        let doc = load_note_doc_conn(&conn, &note)?;
        let update = match since {
            Some(sv) => doc.diff_since(sv).map_err(StoreError::new)?,
            None => doc.state(),
        };
        Ok(NoteDocState {
            note_id: note.id,
            update,
            state_vector: doc.state_vector(),
        })
    }

    /// 클라이언트(웹 yjs)의 update 를 적용한다. state 가 앞으로 갔으면 저장하고(본문이 그대로여도
    /// — 의존 update 가 먼저 오거나 같은 글자를 지웠다 넣은 경우), 본문이 바뀌면 `notes.content`
    /// 도 따라간다. state·content·history 는 한 트랜잭션이다.
    ///
    /// 히스토리는 **묶는다** — 글자마다 한 줄이면 히스토리도 `/api/changes`(→ 세션 주입)도
    /// 잡음이 된다. 같은 actor 의 직전 `edit` 가 `NOTE_EDIT_COALESCE_SECS` 안이면 새 줄을
    /// 남기지 않는다. `client` 는 보낸 편집기의 id — 방송에 실려 자기 메아리를 버리게 한다.
    pub fn apply_note_update(
        &self,
        note_ref: &str,
        update: &[u8],
        actor: &str,
        client: Option<&str>,
        current_board_id: Option<&str>,
    ) -> StoreResult<NoteDocApplied> {
        let mut events = Vec::new();
        let mut doc_events = Vec::new();
        let applied = {
            let conn = self.lock();
            let current = must_get_note_conn(&conn, note_ref, current_board_id)?;
            conn.execute_batch("BEGIN")?;
            let result = (|| -> StoreResult<NoteDocApplied> {
                let doc = load_note_doc_conn(&conn, &current)?;
                let sv_before = doc.state_vector();
                let applied = doc.apply(update).map_err(StoreError::new)?;
                if !applied.state_changed {
                    return Ok(NoteDocApplied {
                        note: current.clone(),
                        changed: false,
                    });
                }
                save_note_doc_conn(&conn, &current.id, &doc)?;
                doc_events.push(NoteDocEvent {
                    note_id: current.id.clone(),
                    update: doc.diff_since(&sv_before).map_err(StoreError::new)?,
                    actor: actor.to_string(),
                    client: client.map(str::to_string),
                });
                if !applied.text_changed {
                    return Ok(NoteDocApplied {
                        note: current.clone(),
                        changed: false,
                    });
                }
                let now = now_iso();
                conn.execute(
                    "UPDATE notes SET content = ?1, updated_at = ?2 WHERE id = ?3",
                    params![doc.text(), now, current.id],
                )?;
                // 히스토리는 60초로 묶지만 **화면은 그보다 자주** 따라와야 한다 — 같은 노트를
                // 열어 둔 다른 브라우저·TUI 는 전역 이벤트로 refetch 하므로, 히스토리 줄이 없어도
                // 노트당 `NOTE_EDIT_EVENT_THROTTLE` 마다 한 번은 흘린다(글자마다는 아니다).
                let mut throttle = self.note_edit_events.lock().expect("note_edit_events");
                let due = throttle
                    .get(&current.id)
                    .is_none_or(|last| last.elapsed() >= NOTE_EDIT_EVENT_THROTTLE);
                if !recent_edit_exists_conn(&conn, &current.id, actor, &now)? {
                    record_history(
                        &conn,
                        &mut events,
                        HistoryEntity::Note,
                        &current.id,
                        actor,
                        "edit",
                        None,
                        current.board_id.as_deref(),
                        true,
                    )?;
                    throttle.insert(current.id.clone(), std::time::Instant::now());
                } else if due {
                    events.push(ChangeEvent {
                        entity: HistoryEntity::Note,
                        entity_id: current.id.clone(),
                        action: "edit".into(),
                        board_id: current.board_id.clone(),
                    });
                    throttle.insert(current.id.clone(), std::time::Instant::now());
                }
                Ok(NoteDocApplied {
                    note: must_get_note_conn(&conn, &current.id, None)?,
                    changed: true,
                })
            })();
            match result {
                Ok(applied) => {
                    conn.execute_batch("COMMIT")?;
                    applied
                }
                Err(error) => {
                    let _ = conn.execute_batch("ROLLBACK");
                    return Err(error);
                }
            }
        };
        self.emit_all(events);
        self.emit_docs(doc_events);
        Ok(applied)
    }

    pub fn archive_note(
        &self,
        note_ref: &str,
        actor: &str,
        current_board_id: Option<&str>,
    ) -> StoreResult<Note> {
        let mut events = Vec::new();
        let note = {
            let conn = self.lock();
            let current = must_get_note_conn(&conn, note_ref, current_board_id)?;
            conn.execute(
                "UPDATE notes SET archived_at = ?1, updated_at = ?2 WHERE id = ?3",
                params![now_iso(), now_iso(), current.id],
            )?;
            record_history(
                &conn,
                &mut events,
                HistoryEntity::Note,
                &current.id,
                actor,
                "archive",
                None,
                current.board_id.as_deref(),
                true,
            )?;
            must_get_note_conn(&conn, &current.id, None)?
        };
        self.emit_all(events);
        Ok(note)
    }

    pub fn unarchive_note(
        &self,
        note_ref: &str,
        actor: &str,
        current_board_id: Option<&str>,
    ) -> StoreResult<Note> {
        let mut events = Vec::new();
        let note = {
            let conn = self.lock();
            let current = must_get_note_conn(&conn, note_ref, current_board_id)?;
            conn.execute(
                "UPDATE notes SET archived_at = NULL, updated_at = ?1 WHERE id = ?2",
                params![now_iso(), current.id],
            )?;
            record_history(
                &conn,
                &mut events,
                HistoryEntity::Note,
                &current.id,
                actor,
                "unarchive",
                None,
                current.board_id.as_deref(),
                true,
            )?;
            must_get_note_conn(&conn, &current.id, None)?
        };
        self.emit_all(events);
        Ok(note)
    }

    /// 노트 고정/해제 — 이미 그 상태면 아무것도 안 바꾸고(히스토리도 없이) 그대로 돌려준다. 다시 고정해도
    /// 고정 시각이 밀리지 않아 순서가 흔들리지 않는다.
    pub fn set_note_pinned(
        &self,
        note_ref: &str,
        pinned: bool,
        actor: &str,
        current_board_id: Option<&str>,
    ) -> StoreResult<Note> {
        let mut events = Vec::new();
        let note = {
            let conn = self.lock();
            let current = must_get_note_conn(&conn, note_ref, current_board_id)?;
            if current.pinned_at.is_some() == pinned {
                return Ok(current);
            }
            let pinned_at = pinned.then(now_iso);
            conn.execute(
                "UPDATE notes SET pinned_at = ?1 WHERE id = ?2",
                params![pinned_at, current.id],
            )?;
            record_history(
                &conn,
                &mut events,
                HistoryEntity::Note,
                &current.id,
                actor,
                if pinned { "pin" } else { "unpin" },
                None,
                current.board_id.as_deref(),
                true,
            )?;
            must_get_note_conn(&conn, &current.id, None)?
        };
        self.emit_all(events);
        Ok(note)
    }

    pub fn list_notes(&self, filter: &ListNotesFilter) -> StoreResult<Vec<Note>> {
        let conn = self.lock();
        let mut wheres: Vec<&str> = Vec::new();
        let mut params_vec: Vec<String> = Vec::new();
        if !filter.include_archived {
            wheres.push("archived_at IS NULL");
        }
        if filter.global {
            wheres.push("board_id IS NULL");
        } else if let Some(board) = &filter.board {
            let Some(board_id) = board_id_of_conn(&conn, board)? else {
                return Ok(Vec::new());
            };
            wheres.push("board_id = ?");
            params_vec.push(board_id);
        }
        let where_sql = if wheres.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", wheres.join(" AND "))
        };
        let mut stmt = conn.prepare(&format!(
            "SELECT * FROM notes {where_sql} ORDER BY position"
        ))?;
        let rows = stmt
            .query_map(params_from_iter(params_vec.iter()), note_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn get_note(
        &self,
        note_ref: &str,
        current_board_id: Option<&str>,
    ) -> StoreResult<Option<Note>> {
        let conn = self.lock();
        get_note_conn(&conn, note_ref, current_board_id)
    }
}

// ── comments ────────────────────────────────────────────────────────────────

impl TodoStore {
    /// todo 에 댓글을 단다. 히스토리는 **부모 todo 의 것으로**(entity='todo') 기록한다 —
    /// 상세 조회·SSE·훅 주입 경로에 그대로 올라탄다.
    pub fn add_comment(
        &self,
        todo_ref: &str,
        body: &str,
        actor: &str,
        current_board_id: Option<&str>,
    ) -> StoreResult<Comment> {
        let trimmed = body.trim();
        if trimmed.is_empty() {
            return Err(StoreError::new("comment body is required"));
        }
        let mut events = Vec::new();
        let comment = {
            let conn = self.lock();
            let todo = must_get_todo_conn(&conn, todo_ref, current_board_id)?;
            let now = now_iso();
            let comment = Comment {
                id: new_id(),
                todo_id: todo.id.clone(),
                actor: actor.to_string(),
                body: trimmed.to_string(),
                created_at: now.clone(),
                updated_at: now,
                archived_at: None,
            };
            conn.execute(
                "INSERT INTO comments (id, todo_id, actor, body, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    comment.id,
                    comment.todo_id,
                    comment.actor,
                    comment.body,
                    comment.created_at,
                    comment.updated_at,
                ],
            )?;
            let mut changes = Changes::new();
            changes.insert("comment".into(), json!([null, trimmed]));
            record_history(
                &conn,
                &mut events,
                HistoryEntity::Todo,
                &todo.id,
                actor,
                "comment",
                Some(&changes),
                Some(&todo.board_id),
                true,
            )?;
            comment
        };
        self.emit_all(events);
        Ok(comment)
    }

    /// 한 todo 의 댓글 — 오래된 것부터(대화 순). 같은 밀리초 동률은 rowid 로 가른다.
    pub fn list_comments(
        &self,
        todo_id: &str,
        include_archived: bool,
    ) -> StoreResult<Vec<Comment>> {
        let conn = self.lock();
        let archived_filter = if include_archived {
            ""
        } else {
            " AND archived_at IS NULL"
        };
        let mut stmt = conn.prepare(&format!(
            "SELECT * FROM comments WHERE todo_id = ?1{archived_filter} ORDER BY created_at ASC, rowid ASC"
        ))?;
        let rows = stmt
            .query_map(params![todo_id], comment_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// 댓글 본문 수정 — 대상은 **댓글 id 로만** 지정한다(댓글은 번호 체계 밖).
    pub fn update_comment(&self, id: &str, body: &str, actor: &str) -> StoreResult<Comment> {
        let trimmed = body.trim();
        if trimmed.is_empty() {
            return Err(StoreError::new("comment body is required"));
        }
        let mut events = Vec::new();
        let comment = {
            let conn = self.lock();
            let current = must_get_comment_conn(&conn, id)?;
            if trimmed == current.body {
                current
            } else {
                let now = now_iso();
                conn.execute(
                    "UPDATE comments SET body = ?1, updated_at = ?2 WHERE id = ?3",
                    params![trimmed, now, id],
                )?;
                let mut changes = Changes::new();
                changes.insert("comment".into(), json!([current.body, trimmed]));
                let board_id = board_id_of_todo_conn(&conn, &current.todo_id)?;
                record_history(
                    &conn,
                    &mut events,
                    HistoryEntity::Todo,
                    &current.todo_id,
                    actor,
                    "comment-edit",
                    Some(&changes),
                    board_id.as_deref(),
                    true,
                )?;
                Comment {
                    body: trimmed.to_string(),
                    updated_at: now,
                    ..current
                }
            }
        };
        self.emit_all(events);
        Ok(comment)
    }

    /// 댓글 보관/복원 — 삭제는 없다(레포 전체 원칙).
    pub fn set_comment_archived(
        &self,
        id: &str,
        archived: bool,
        actor: &str,
    ) -> StoreResult<Comment> {
        let mut events = Vec::new();
        let comment = {
            let conn = self.lock();
            let current = must_get_comment_conn(&conn, id)?;
            let at = if archived { Some(now_iso()) } else { None };
            conn.execute(
                "UPDATE comments SET archived_at = ?1 WHERE id = ?2",
                params![at, id],
            )?;
            let board_id = board_id_of_todo_conn(&conn, &current.todo_id)?;
            record_history(
                &conn,
                &mut events,
                HistoryEntity::Todo,
                &current.todo_id,
                actor,
                if archived {
                    "comment-archive"
                } else {
                    "comment-unarchive"
                },
                None,
                board_id.as_deref(),
                true,
            )?;
            Comment {
                archived_at: at,
                ..current
            }
        };
        self.emit_all(events);
        Ok(comment)
    }

    /// 목록 배지용 집계 — 보관되지 않은 댓글 수와 마지막 작성 시각.
    pub fn comment_stats_of(&self, todo_id: &str) -> StoreResult<CommentStats> {
        let conn = self.lock();
        let (count, last_at): (i64, Option<String>) = conn.query_row(
            "SELECT COUNT(*), MAX(created_at) FROM comments WHERE todo_id = ?1 AND archived_at IS NULL",
            params![todo_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        Ok(CommentStats { count, last_at })
    }
}

fn must_get_comment_conn(conn: &Connection, id: &str) -> StoreResult<Comment> {
    conn.query_row(
        "SELECT * FROM comments WHERE id = ?1",
        params![id],
        comment_from_row,
    )
    .optional()?
    .ok_or_else(|| StoreError::new(format!("comment not found: {id}")))
}

/// 댓글이 속한 todo 의 boardId — change 이벤트용. 저장된 raw id 라 참조 해석이 불필요하다.
fn board_id_of_todo_conn(conn: &Connection, todo_id: &str) -> StoreResult<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT board_id FROM todos WHERE id = ?1",
            params![todo_id],
            |r| r.get(0),
        )
        .optional()?)
}

// ── handoffs ────────────────────────────────────────────────────────────────

/// `handoffs` 행 하나를 그대로 넣는다 — create_handoff / create_spawned_handoff 공용.
fn insert_handoff_row(conn: &Connection, handoff: &Handoff) -> StoreResult<()> {
    conn.execute(
        "INSERT INTO handoffs
           (id, todo_id, session_id, session_name, session_cwd, note, actor, status,
            created_at, delivered_at, delivered_via, accepted_at, completed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            handoff.id,
            handoff.todo_id,
            handoff.session_id,
            handoff.session_name,
            handoff.session_cwd,
            handoff.note,
            handoff.actor,
            handoff.status.as_str(),
            handoff.created_at,
            handoff.delivered_at,
            handoff.delivered_via.map(|v| v.as_str()),
            handoff.accepted_at,
            handoff.completed_at,
        ],
    )?;
    Ok(())
}

/// `start` 를 배달된 핸드오프에 귀속 — 가장 오래된 미수락 delivered 에 accepted_at 을
/// 찍고 그 session_id 를 돌려준다. **사람이 누른 start 는 귀속하지 않는다.**
fn accept_handoff_for(
    conn: &Connection,
    todo_id: &str,
    actor: &str,
    at: &str,
) -> StoreResult<Option<String>> {
    if !is_agent_actor(actor) {
        return Ok(None);
    }
    let row: Option<(String, String)> = conn
        .query_row(
            "SELECT id, session_id FROM handoffs
              WHERE todo_id = ?1 AND status = 'delivered' AND accepted_at IS NULL
              ORDER BY created_at ASC, rowid ASC LIMIT 1",
            params![todo_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((id, session_id)) = row else {
        return Ok(None);
    };
    conn.execute(
        "UPDATE handoffs SET accepted_at = ?1 WHERE id = ?2",
        params![at, id],
    )?;
    Ok(Some(session_id))
}

/// `done` 을 배달된 핸드오프에 귀속. 진행 중이던 건(accepted, 미완료)이 없으면 **start 를
/// 건너뛴 done** 으로 보고 미수락 건에 accepted_at 을 completed_at 과 같이 찍는다 —
/// "끝났는데 미착수" 모순 방지. `reopen` 은 이 기록을 되돌리지 않는다.
fn complete_handoff_for(
    conn: &Connection,
    todo_id: &str,
    actor: &str,
    at: &str,
) -> StoreResult<()> {
    if !is_agent_actor(actor) {
        return Ok(());
    }
    let in_flight: Option<String> = conn
        .query_row(
            "SELECT id FROM handoffs
              WHERE todo_id = ?1 AND status = 'delivered'
                AND accepted_at IS NOT NULL AND completed_at IS NULL
              ORDER BY accepted_at ASC, rowid ASC LIMIT 1",
            params![todo_id],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(id) = in_flight {
        conn.execute(
            "UPDATE handoffs SET completed_at = ?1 WHERE id = ?2",
            params![at, id],
        )?;
        return Ok(());
    }
    let skipped: Option<String> = conn
        .query_row(
            "SELECT id FROM handoffs
              WHERE todo_id = ?1 AND status = 'delivered' AND accepted_at IS NULL
              ORDER BY created_at ASC, rowid ASC LIMIT 1",
            params![todo_id],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(id) = skipped {
        conn.execute(
            "UPDATE handoffs SET accepted_at = ?1, completed_at = ?2 WHERE id = ?3",
            params![at, at, id],
        )?;
    }
    Ok(())
}

fn pending_handoff_of_conn(conn: &Connection, todo_id: &str) -> StoreResult<Option<Handoff>> {
    Ok(conn
        .query_row(
            "SELECT * FROM handoffs WHERE todo_id = ?1 AND status = 'pending' ORDER BY created_at ASC, rowid ASC LIMIT 1",
            params![todo_id],
            handoff_from_row,
        )
        .optional()?)
}

impl TodoStore {
    /// todo 를 실행 중인 세션 앞으로 넘긴다 — pending 으로 큐에 쌓고 훅이 당겨간다.
    pub fn create_handoff(&self, input: &CreateHandoffInput) -> StoreResult<Handoff> {
        let mut events = Vec::new();
        let handoff = {
            let conn = self.lock();
            let todo =
                must_get_todo_conn(&conn, &input.todo_ref, input.current_board_id.as_deref())?;
            if todo.archived_at.is_some() {
                return Err(StoreError::new(format!("todo is archived: {}", todo.id)));
            }
            if pending_handoff_of_conn(&conn, &todo.id)?.is_some() {
                return Err(StoreError::new(format!(
                    "handoff already pending for todo: {}",
                    todo.id
                )));
            }
            let handoff = Handoff {
                id: new_id(),
                todo_id: todo.id.clone(),
                session_id: input.session_id.clone(),
                session_name: input.session_name.clone(),
                session_cwd: input.session_cwd.clone(),
                note: input.note.as_deref().unwrap_or("").trim().to_string(),
                actor: input.actor.clone(),
                status: HandoffStatus::Pending,
                created_at: now_iso(),
                delivered_at: None,
                delivered_via: None,
                accepted_at: None,
                completed_at: None,
            };
            insert_handoff_row(&conn, &handoff)?;
            let mut changes = Changes::new();
            changes.insert(
                "handoff".into(),
                json!([
                    null,
                    handoff
                        .session_name
                        .clone()
                        .unwrap_or_else(|| handoff.session_id.clone())
                ]),
            );
            record_history(
                &conn,
                &mut events,
                HistoryEntity::Todo,
                &todo.id,
                &input.actor,
                "handoff",
                Some(&changes),
                Some(&todo.board_id),
                true,
            )?;
            handoff
        };
        self.emit_all(events);
        Ok(handoff)
    }

    /// 새로 띄운 백그라운드 세션 앞으로의 배달 기록 — 프롬프트로 이미 배달했으므로 생성
    /// 시점에 `delivered` 다(claim 대상이 아니다). **spawn 성공 뒤에만** 부른다.
    pub fn create_spawned_handoff(
        &self,
        input: &CreateSpawnedHandoffInput,
    ) -> StoreResult<Handoff> {
        let mut events = Vec::new();
        let handoff = {
            let conn = self.lock();
            let todo =
                must_get_todo_conn(&conn, &input.todo_ref, input.current_board_id.as_deref())?;
            if todo.archived_at.is_some() {
                return Err(StoreError::new(format!("todo is archived: {}", todo.id)));
            }
            let at = now_iso();
            let handoff = Handoff {
                id: new_id(),
                todo_id: todo.id.clone(),
                session_id: input.session_id.clone(),
                session_name: Some(input.session_name.clone()),
                session_cwd: Some(input.session_cwd.clone()),
                note: input.note.as_deref().unwrap_or("").trim().to_string(),
                actor: input.actor.clone(),
                status: HandoffStatus::Delivered,
                created_at: at.clone(),
                delivered_at: Some(at),
                delivered_via: Some(HandoffVia::Spawn),
                accepted_at: None,
                completed_at: None,
            };
            insert_handoff_row(&conn, &handoff)?;
            let mut changes = Changes::new();
            changes.insert("handoff".into(), json!([null, input.session_name]));
            record_history(
                &conn,
                &mut events,
                HistoryEntity::Todo,
                &todo.id,
                &input.actor,
                "handoff-spawn",
                Some(&changes),
                Some(&todo.board_id),
                true,
            )?;
            handoff
        };
        self.emit_all(events);
        Ok(handoff)
    }

    /// 이 todo 앞으로 아직 배달되지 않은 요청. 없으면 None.
    pub fn pending_handoff_of(&self, todo_id: &str) -> StoreResult<Option<Handoff>> {
        let conn = self.lock();
        pending_handoff_of_conn(&conn, todo_id)
    }

    /// 큐 조회 — 최신순. `open` 은 대기 중이거나 배달됐는데 완료되지 않은 것 — 단 todo 가
    /// **보관되면 그 핸드오프는 열린 것이 아니다**. 보관은 "이 일은 더 안 본다" 이므로 그 위의
    /// 미완료 배달·대기가 요약과 UI 에 영원히 남는 걸 막는다(해제하면 다시 열린다).
    pub fn list_handoffs(&self, filter: &ListHandoffsFilter) -> StoreResult<Vec<Handoff>> {
        let conn = self.lock();
        let mut clauses: Vec<&str> = Vec::new();
        let mut params_vec: Vec<String> = Vec::new();
        if filter.open {
            clauses.push(
                "(h.status = 'pending' OR (h.status = 'delivered' AND h.completed_at IS NULL))",
            );
            clauses.push("h.todo_id IN (SELECT id FROM todos WHERE archived_at IS NULL)");
        }
        if let Some(board_id) = &filter.board_id {
            clauses.push("h.todo_id IN (SELECT id FROM todos WHERE board_id = ?)");
            params_vec.push(board_id.clone());
        }
        if let Some(todo_id) = &filter.todo_id {
            clauses.push("h.todo_id = ?");
            params_vec.push(todo_id.clone());
        }
        if let Some(status) = filter.status {
            clauses.push("h.status = ?");
            params_vec.push(status.as_str().to_string());
        }
        let where_sql = if clauses.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", clauses.join(" AND "))
        };
        let mut stmt = conn.prepare(&format!(
            "SELECT h.* FROM handoffs h{where_sql} ORDER BY h.created_at DESC, h.rowid DESC"
        ))?;
        let rows = stmt
            .query_map(params_from_iter(params_vec.iter()), handoff_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// 이 세션 앞의 pending 중 **가장 오래된 한 건만** 배달 처리하고 돌려준다.
    /// 한 번에 하나 — 하나를 끝내면 Stop 훅이 다시 발동해 큐가 저절로 직렬 소화된다.
    pub fn claim_handoff(
        &self,
        session_id: &str,
        via: HandoffVia,
    ) -> StoreResult<Option<ClaimedHandoff>> {
        let mut events = Vec::new();
        let claimed = {
            let conn = self.lock();
            conn.execute_batch("BEGIN")?;
            let result = (|| -> StoreResult<Option<ClaimedHandoff>> {
                // 보관된 todo 의 pending 은 집지 않는다 — open 목록·요약에서 빠진 요청이 훅에서만
                // 튀어나와 사용자가 접은 일을 세션이 착수하게 되는 걸 막는다. 해제하면 다시 집힌다.
                let row = conn
                    .query_row(
                        "SELECT * FROM handoffs WHERE session_id = ?1 AND status = 'pending' AND todo_id IN (SELECT id FROM todos WHERE archived_at IS NULL) ORDER BY created_at ASC, rowid ASC LIMIT 1",
                        params![session_id],
                        handoff_from_row,
                    )
                    .optional()?;
                let Some(row) = row else {
                    return Ok(None);
                };
                let at = now_iso();
                conn.execute(
                    "UPDATE handoffs SET status = 'delivered', delivered_at = ?1, delivered_via = ?2 WHERE id = ?3 AND status = 'pending'",
                    params![at, via.as_str(), row.id],
                )?;
                let Some(todo) = get_todo_conn(&conn, &row.todo_id, None)? else {
                    // todo 가 사라진 요청은 배달할 수 없다 — delivered 로 닫고 넘어간다.
                    return Ok(None);
                };
                let remaining: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM handoffs WHERE session_id = ?1 AND status = 'pending' AND todo_id IN (SELECT id FROM todos WHERE archived_at IS NULL)",
                    params![session_id],
                    |r| r.get(0),
                )?;
                let todo_ref = ref_of_conn(&conn, Some(&todo.board_id), todo.number, &todo.id)?;
                Ok(Some(ClaimedHandoff {
                    handoff: Handoff {
                        status: HandoffStatus::Delivered,
                        delivered_at: Some(at),
                        delivered_via: Some(via),
                        ..row
                    },
                    todo_ref,
                    todo_title: todo.title,
                    remaining,
                }))
            })();
            match result {
                Ok(claimed) => {
                    conn.execute_batch("COMMIT")?;
                    if let Some(claimed) = &claimed {
                        record_history(
                            &conn,
                            &mut events,
                            HistoryEntity::Todo,
                            &claimed.handoff.todo_id,
                            claimed
                                .handoff
                                .session_name
                                .as_deref()
                                .unwrap_or(&claimed.handoff.session_id),
                            "handoff-delivered",
                            None,
                            None,
                            true,
                        )?;
                    }
                    claimed
                }
                Err(error) => {
                    let _ = conn.execute_batch("ROLLBACK");
                    return Err(error);
                }
            }
        };
        self.emit_all(events);
        Ok(claimed)
    }

    /// 대기 중인 요청을 취소한다.
    pub fn cancel_handoff(&self, id: &str, actor: &str) -> StoreResult<Handoff> {
        let mut events = Vec::new();
        let handoff = {
            let conn = self.lock();
            let row = conn
                .query_row(
                    "SELECT * FROM handoffs WHERE id = ?1",
                    params![id],
                    handoff_from_row,
                )
                .optional()?
                .ok_or_else(|| StoreError::new(format!("handoff not found: {id}")))?;
            if row.status != HandoffStatus::Pending {
                return Err(StoreError::new(format!("handoff is not pending: {id}")));
            }
            conn.execute(
                "UPDATE handoffs SET status = 'cancelled' WHERE id = ?1",
                params![id],
            )?;
            record_history(
                &conn,
                &mut events,
                HistoryEntity::Todo,
                &row.todo_id,
                actor,
                "handoff-cancel",
                None,
                None,
                true,
            )?;
            Handoff {
                status: HandoffStatus::Cancelled,
                ..row
            }
        };
        self.emit_all(events);
        Ok(handoff)
    }
}

// ── history ─────────────────────────────────────────────────────────────────

impl TodoStore {
    pub fn list_history(&self, filter: &ListHistoryFilter) -> StoreResult<Vec<HistoryEntry>> {
        let conn = self.lock();
        let mut wheres: Vec<String> = Vec::new();
        let mut params_vec: Vec<String> = Vec::new();
        if let Some(entity_id) = &filter.entity_id {
            wheres.push("entity_id = ?".into());
            params_vec.push(entity_id.clone());
        }
        if let Some(entity) = filter.entity {
            wheres.push("entity = ?".into());
            params_vec.push(entity.as_str().to_string());
        }
        if !filter.exclude_actions.is_empty() {
            let placeholders = filter
                .exclude_actions
                .iter()
                .map(|_| "?")
                .collect::<Vec<_>>()
                .join(", ");
            wheres.push(format!("action NOT IN ({placeholders})"));
            params_vec.extend(filter.exclude_actions.iter().cloned());
        }
        let where_sql = if wheres.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", wheres.join(" AND "))
        };
        // limit 은 반드시 마지막에 바인딩한다 — `LIMIT ?` 가 where 절 뒤에 온다.
        params_vec.push(filter.limit.unwrap_or(50).to_string());
        let mut stmt = conn.prepare(&format!(
            "SELECT * FROM history {where_sql} ORDER BY id DESC LIMIT ?"
        ))?;
        let rows = stmt
            .query_map(params_from_iter(params_vec.iter()), history_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// sinceId 이후의 변경 피드 — 히스토리에 엔티티 제목/보드 키를 붙여 반환한다(서사 순).
    /// handoff 계열은 뺀다 — 다른 세션의 프롬프트 주입에까지 실리면 노이즈다.
    pub fn list_changes_since(
        &self,
        since_id: i64,
        limit: Option<i64>,
    ) -> StoreResult<ChangesSince> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT * FROM history
              WHERE id > ?1
                AND action NOT IN ('handoff', 'handoff-delivered', 'handoff-cancel', 'handoff-spawn')
              ORDER BY id ASC LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(params![since_id, limit.unwrap_or(50)], history_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let max: Option<i64> = conn.query_row("SELECT MAX(id) FROM history", [], |r| r.get(0))?;
        let last_id = max.unwrap_or(since_id);

        let mut board_stmt = conn.prepare("SELECT id, key FROM boards")?;
        let board_key_by_id: HashMap<String, String> = board_stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<rusqlite::Result<HashMap<_, _>>>()?;

        let mut entries = Vec::with_capacity(rows.len());
        for history in rows {
            let (title, board_id): (String, Option<String>) = match history.entity {
                HistoryEntity::Todo => {
                    let found: Option<(String, String)> = conn
                        .query_row(
                            "SELECT title, board_id FROM todos WHERE id = ?1",
                            params![history.entity_id],
                            |r| Ok((r.get(0)?, r.get(1)?)),
                        )
                        .optional()?;
                    match found {
                        Some((title, board_id)) => (title, Some(board_id)),
                        None => (String::new(), None),
                    }
                }
                HistoryEntity::Note => {
                    let found: Option<(String, Option<String>)> = conn
                        .query_row(
                            "SELECT title, board_id FROM notes WHERE id = ?1",
                            params![history.entity_id],
                            |r| Ok((r.get(0)?, r.get(1)?)),
                        )
                        .optional()?;
                    match found {
                        Some((title, board_id)) => (title, board_id),
                        None => (String::new(), None),
                    }
                }
                HistoryEntity::Section => {
                    let found: Option<(String, String)> = conn
                        .query_row(
                            "SELECT title, board_id FROM sections WHERE id = ?1",
                            params![history.entity_id],
                            |r| Ok((r.get(0)?, r.get(1)?)),
                        )
                        .optional()?;
                    match found {
                        Some((title, board_id)) => (title, Some(board_id)),
                        None => (String::new(), None),
                    }
                }
                HistoryEntity::Board => (
                    board_key_by_id
                        .get(&history.entity_id)
                        .cloned()
                        .unwrap_or_default(),
                    Some(history.entity_id.clone()),
                ),
            };
            let board_key = board_id
                .as_ref()
                .and_then(|id| board_key_by_id.get(id).cloned());
            entries.push(ChangeFeedEntry {
                history,
                title,
                board_key,
            });
        }
        Ok(ChangesSince { last_id, entries })
    }
}
