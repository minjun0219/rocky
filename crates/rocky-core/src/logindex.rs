//! 로그 색인 — 작업로그·사용 로그(JSONL)를 SQLite 로 옮겨 보드에서 읽고 분석한다
//! (`docs/design/specs/2026-10-02-log-index-design.md`).
//!
//! **JSONL 이 진실이고 이 DB 는 파생 색인이다.** 쓰기 경로(Stop 훅·`worklog_append`·사용 로그 싱크)는 그대로
//! 파일에 덧붙이고, 데몬의 색인 스레드가 주기적으로 새로 붙은 줄만 옮긴다. DB 를 지우면 처음부터 다시 만든다.
//! 보드 DB(`todo.db`)와 파일을 나눠 잠금·장애가 섞이지 않는다.
//!
//! 증분: 파일마다 읽은 바이트 위치를 `log_files` 에 둔다. 파일이 그보다 작아졌으면(갈아엎음) 처음부터. 마지막 줄이
//! `\n` 없이 끝나면 아직 쓰는 중이라 다음 바퀴로 미룬다. 파싱 안 되는 줄은 건너뛴다(작업로그 `read` 와 같은 규칙).

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use crate::usage::{build_report, UsageEvent, UsageReport, UsageSource, KNOWN_SURFACES};
use crate::worklog::{WorklogEntry, WORKLOG_FILE};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS log_files (
  path TEXT PRIMARY KEY,
  offset INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS worklog_entries (
  id TEXT PRIMARY KEY,
  project_key TEXT NOT NULL,
  timestamp TEXT NOT NULL,
  kind TEXT NOT NULL,
  content TEXT NOT NULL,
  tags TEXT NOT NULL,
  todo_ref TEXT
);
CREATE INDEX IF NOT EXISTS worklog_by_project ON worklog_entries(project_key, timestamp);
CREATE INDEX IF NOT EXISTS worklog_by_todo ON worklog_entries(todo_ref, timestamp);
CREATE TABLE IF NOT EXISTS usage_events (
  file TEXT NOT NULL,
  offset INTEGER NOT NULL,
  ts TEXT NOT NULL,
  source TEXT NOT NULL,
  name TEXT NOT NULL,
  actor TEXT,
  client TEXT,
  ok INTEGER NOT NULL,
  ms INTEGER,
  PRIMARY KEY (file, offset)
);
CREATE INDEX IF NOT EXISTS usage_by_name ON usage_events(name, ts);
";

/// 작업로그 태그에서 할 일 참조를 뽑는다 — Stop 훅이 그 세션이 든 할 일을 `todo:<ref>` 로 붙인다.
pub fn todo_ref_from_tags(tags: &[String]) -> Option<String> {
    tags.iter()
        .find_map(|t| t.strip_prefix("todo:"))
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .map(str::to_string)
}

/// 트랜스크립트 한 바퀴 — 새 메시지가 들어간 세션(추천을 다시 셀 대상)과 파일별 실패.
#[derive(Debug, Default)]
pub struct TranscriptIngest {
    pub touched: std::collections::BTreeSet<String>,
    pub errors: Vec<String>,
}

/// 한 바퀴에 옮긴 것.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct IngestStats {
    pub files: usize,
    pub worklog: usize,
    pub usage: usize,
}

/// 작업로그 한 줄의 읽기 모양 — 응답에 그대로 싣는다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexedWorklog {
    pub id: String,
    pub project_key: String,
    pub timestamp: String,
    pub kind: String,
    pub content: String,
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub todo_ref: Option<String>,
}

/// 작업로그 조회 조건 — 비우면 전부. `before` 는 이 시각보다 이전(더 보기).
#[derive(Debug, Clone, Default)]
pub struct WorklogQuery {
    /// 이 프로젝트 키들만(보드의 레포). `None` 이면 전부.
    pub project_keys: Option<Vec<String>>,
    pub todo_ref: Option<String>,
    pub kind: Option<String>,
    /// 본문에 이 글자가 들어간 것(대소문자 무시).
    pub text: Option<String>,
    pub before: Option<String>,
    pub limit: usize,
}

pub struct LogIndex {
    conn: Connection,
}

impl LogIndex {
    /// 색인 DB 를 연다(없으면 만든다). 색인 스레드와 조회가 각자 연다 — WAL 이라 쓰는 중에도 읽힌다.
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.busy_timeout(std::time::Duration::from_secs(2))?;
        conn.execute_batch(SCHEMA)?;
        conn.execute_batch(crate::tokens::SCHEMA)?;
        Ok(LogIndex { conn })
    }

    /// 조회용 연결 — `tokens` 의 질의 함수들이 받는다.
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// Claude Code 트랜스크립트 루트(`~/.claude/projects`)를 훑어 새 줄을 옮긴다 — 프로젝트 디렉터리의
    /// `<session>.jsonl` 과 `<session>/subagents/*.jsonl`. 파일 하나가 실패해도(읽는 사이 지워짐·권한) 나머지는 계속
    /// 옮기고 실패는 `errors` 로 돌려준다 — 한 파일이 뒤 파일들과 이미 옮긴 세션의 추천까지 막지 않게.
    pub fn ingest_transcripts(&mut self, root: &Path) -> TranscriptIngest {
        let mut files = Vec::new();
        collect_jsonl(root, 0, &mut files);
        let mut out = TranscriptIngest::default();
        for file in files {
            if let Err(e) = self.ingest_transcript(&file, &mut out.touched) {
                out.errors.push(e);
            }
        }
        out
    }

    fn ingest_transcript(
        &mut self,
        path: &Path,
        touched: &mut std::collections::BTreeSet<String>,
    ) -> Result<(), String> {
        use crate::tokens::{ingest_line, load_cursor, parse_line, save_cursor, TranscriptLine};
        let key = path.to_string_lossy().to_string();
        let err = |e: rusqlite::Error| format!("트랜스크립트 색인 실패({key}): {e}");
        let stored: u64 = self
            .conn
            .query_row(
                "SELECT offset FROM log_files WHERE path = ?1",
                params![key],
                |r| r.get::<_, i64>(0),
            )
            .optional()
            .map_err(err)?
            .map(|o| o.max(0) as u64)
            .unwrap_or(0);
        let Some(chunk) = read_complete_lines(path, stored)? else {
            return Ok(());
        };
        // 파일이 줄어 처음부터 다시 읽으면 열린 턴도 처음부터.
        let mut turn = if chunk.restarted {
            None
        } else {
            load_cursor(&self.conn, &key).map_err(err)?
        };
        let tx = self.conn.transaction().map_err(err)?;
        for (_, line) in &chunk.lines {
            let Some(parsed) = parse_line(line) else {
                continue;
            };
            if ingest_line(&tx, &parsed, &mut turn).map_err(err)? > 0 {
                if let TranscriptLine::Message(m) = &parsed {
                    if !m.meta.sidechain {
                        touched.insert(m.meta.session_id.clone());
                    }
                }
            }
        }
        save_cursor(&tx, &key, turn.as_deref()).map_err(err)?;
        tx.execute(
            "INSERT INTO log_files (path, offset) VALUES (?1, ?2) ON CONFLICT(path) DO UPDATE SET offset = excluded.offset",
            params![key, chunk.end as i64],
        )
        .map_err(err)?;
        tx.commit().map_err(err)
    }

    /// `<root>/<project-key>/worklog.jsonl` 을 전부 훑어 새 줄을 옮긴다. 디렉터리 이름이 프로젝트 키다.
    pub fn ingest_worklog_root(&mut self, root: &Path) -> Result<IngestStats, String> {
        let mut stats = IngestStats::default();
        let Ok(dirs) = std::fs::read_dir(root) else {
            return Ok(stats);
        };
        for dir in dirs.flatten() {
            let file = dir.path().join(WORKLOG_FILE);
            if !file.is_file() {
                continue;
            }
            let project_key = dir.file_name().to_string_lossy().to_string();
            stats.files += 1;
            stats.worklog += self.ingest_file(&file, |tx, _, line| {
                let Ok(entry) = serde_json::from_str::<WorklogEntry>(line) else {
                    return Ok(0);
                };
                let todo_ref = todo_ref_from_tags(&entry.tags);
                tx.execute(
                    "INSERT OR IGNORE INTO worklog_entries (id, project_key, timestamp, kind, content, tags, todo_ref) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        entry.id,
                        project_key,
                        entry.timestamp,
                        entry.kind,
                        entry.content,
                        serde_json::to_string(&entry.tags).unwrap_or_else(|_| "[]".into()),
                        todo_ref,
                    ],
                )
            })?;
        }
        Ok(stats)
    }

    /// `<dir>/YYYY-MM.jsonl` 사용 로그를 옮긴다. 사용 이벤트엔 id 가 없어 (파일, 줄 시작 위치)가 키다.
    pub fn ingest_usage_dir(&mut self, dir: &Path) -> Result<IngestStats, String> {
        let mut stats = IngestStats::default();
        let Ok(files) = std::fs::read_dir(dir) else {
            return Ok(stats);
        };
        for file in files.flatten() {
            let path = file.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let name = file.file_name().to_string_lossy().to_string();
            stats.files += 1;
            stats.usage += self.ingest_file(&path, |tx, offset, line| {
                let Ok(event) = serde_json::from_str::<UsageEvent>(line) else {
                    return Ok(0);
                };
                let source = serde_json::to_value(event.source)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_string))
                    .unwrap_or_default();
                tx.execute(
                    "INSERT OR IGNORE INTO usage_events (file, offset, ts, source, name, actor, client, ok, ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    params![
                        name,
                        offset as i64,
                        event.ts,
                        source,
                        event.name,
                        event.actor,
                        event.client,
                        event.ok,
                        event.ms.map(|m| m as i64),
                    ],
                )
            })?;
        }
        Ok(stats)
    }

    /// 파일 하나의 새 줄을 한 트랜잭션으로 옮기고 읽은 위치를 올린다. `insert` 는 (트랜잭션, 줄 시작 위치, 줄)을
    /// 받아 넣은 행 수를 돌려준다. 돌려주는 값은 새로 들어간 행 수다.
    fn ingest_file(
        &mut self,
        path: &Path,
        insert: impl Fn(&rusqlite::Transaction, u64, &str) -> rusqlite::Result<usize>,
    ) -> Result<usize, String> {
        let key = path.to_string_lossy().to_string();
        let err = |e: rusqlite::Error| format!("로그 색인 실패({key}): {e}");
        let stored: u64 = self
            .conn
            .query_row(
                "SELECT offset FROM log_files WHERE path = ?1",
                params![key],
                |r| r.get::<_, i64>(0),
            )
            .optional()
            .map_err(err)?
            .map(|o| o.max(0) as u64)
            .unwrap_or(0);
        let Some(chunk) = read_complete_lines(path, stored)? else {
            return Ok(0);
        };
        let tx = self.conn.transaction().map_err(err)?;
        let mut inserted = 0;
        for (offset, line) in &chunk.lines {
            inserted += insert(&tx, *offset, line).map_err(err)?;
        }
        tx.execute(
            "INSERT INTO log_files (path, offset) VALUES (?1, ?2) ON CONFLICT(path) DO UPDATE SET offset = excluded.offset",
            params![key, chunk.end as i64],
        )
        .map_err(err)?;
        tx.commit().map_err(err)?;
        Ok(inserted)
    }

    /// 작업로그를 최신순으로.
    pub fn worklog(&self, query: &WorklogQuery) -> rusqlite::Result<Vec<IndexedWorklog>> {
        let mut sql = String::from(
            "SELECT id, project_key, timestamp, kind, content, tags, todo_ref FROM worklog_entries WHERE 1=1",
        );
        let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(keys) = &query.project_keys {
            if keys.is_empty() {
                return Ok(Vec::new());
            }
            let marks = vec!["?"; keys.len()].join(",");
            sql.push_str(&format!(" AND project_key IN ({marks})"));
            for k in keys {
                args.push(Box::new(k.clone()));
            }
        }
        if let Some(r) = &query.todo_ref {
            sql.push_str(" AND todo_ref = ?");
            args.push(Box::new(r.clone()));
        }
        if let Some(k) = &query.kind {
            sql.push_str(" AND kind = ?");
            args.push(Box::new(k.clone()));
        }
        if let Some(t) = &query.text {
            sql.push_str(" AND content LIKE ? ESCAPE '\\'");
            let escaped = t
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_");
            args.push(Box::new(format!("%{escaped}%")));
        }
        if let Some(b) = &query.before {
            sql.push_str(" AND timestamp < ?");
            args.push(Box::new(b.clone()));
        }
        sql.push_str(" ORDER BY timestamp DESC, id DESC LIMIT ?");
        args.push(Box::new(query.limit.clamp(1, 500) as i64));
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(
            rusqlite::params_from_iter(args.iter().map(|a| a.as_ref())),
            |r| {
                let tags: String = r.get(5)?;
                Ok(IndexedWorklog {
                    id: r.get(0)?,
                    project_key: r.get(1)?,
                    timestamp: r.get(2)?,
                    kind: r.get(3)?,
                    content: r.get(4)?,
                    tags: serde_json::from_str(&tags).unwrap_or_default(),
                    todo_ref: r.get(6)?,
                })
            },
        )?;
        rows.collect()
    }

    /// 색인된 사용 이벤트 수 — 테스트·상태 확인용.
    pub fn usage_count(&self) -> rusqlite::Result<i64> {
        self.conn
            .query_row("SELECT COUNT(*) FROM usage_events", [], |r| r.get(0))
    }
}

/// 작업로그 집계 — 회고용(어느 레포·요일·할 일에 턴이 몰렸나).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorklogStats {
    pub turns: u64,
    /// 프로젝트 키 → 턴 수, 많은 순.
    pub by_project: Vec<(String, u64)>,
    /// 현지 시각 요일(일=0 … 토=6) → 턴 수.
    pub by_weekday: Vec<u64>,
    /// 할 일 참조 → 붙은 턴 수, 많은 순(상위 10).
    pub by_todo: Vec<(String, u64)>,
}

/// 통계 한 벌 — 회고(작업로그)와 rocky 개선(사용 로그, `rocky usage` 와 같은 집계).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogStats {
    pub since: String,
    pub worklog: WorklogStats,
    pub usage: UsageReport,
}

impl LogIndex {
    /// `since`(ISO) 이후의 통계. 사용 로그 쪽은 `rocky usage` 와 같은 `build_report`.
    pub fn stats(&self, since: &str, until: &str) -> rusqlite::Result<LogStats> {
        use chrono::{DateTime, Datelike, Local};
        let mut turns = 0u64;
        let mut by_project: std::collections::HashMap<String, u64> = Default::default();
        let mut by_weekday = vec![0u64; 7];
        let mut by_todo: std::collections::HashMap<String, u64> = Default::default();
        let mut stmt = self.conn.prepare(
            "SELECT project_key, timestamp, todo_ref FROM worklog_entries WHERE kind = 'turn' AND timestamp >= ?1",
        )?;
        let rows = stmt.query_map(params![since], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })?;
        for row in rows {
            let (project, ts, todo) = row?;
            turns += 1;
            *by_project.entry(project).or_default() += 1;
            if let Ok(t) = DateTime::parse_from_rfc3339(&ts) {
                by_weekday[t.with_timezone(&Local).weekday().num_days_from_sunday() as usize] += 1;
            }
            if let Some(todo) = todo {
                *by_todo.entry(todo).or_default() += 1;
            }
        }
        let ranked = |map: std::collections::HashMap<String, u64>, top: usize| {
            let mut v: Vec<(String, u64)> = map.into_iter().collect();
            v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
            v.truncate(top);
            v
        };

        let mut stmt = self.conn.prepare(
            "SELECT ts, source, name, actor, client, ok, ms FROM usage_events WHERE ts >= ?1",
        )?;
        let events = stmt
            .query_map(params![since], |r| {
                let source: String = r.get(1)?;
                Ok(UsageEvent {
                    ts: r.get(0)?,
                    source: serde_json::from_value(serde_json::Value::String(source))
                        .unwrap_or(UsageSource::Rest),
                    name: r.get(2)?,
                    actor: r.get(3)?,
                    client: r.get(4)?,
                    ok: r.get(5)?,
                    ms: r.get::<_, Option<i64>>(6)?.map(|m| m.max(0) as u64),
                    meta: None,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(LogStats {
            since: since.to_string(),
            worklog: WorklogStats {
                turns,
                by_project: ranked(by_project, usize::MAX),
                by_weekday,
                by_todo: ranked(by_todo, 10),
            },
            usage: build_report(&events, since, until, KNOWN_SURFACES),
        })
    }
}

/// 위치 `from` 이후의 **완결된 줄**(끝에 `\n`)만. 파일이 `from` 보다 작아졌으면 처음부터. 새 줄이 없으면 `None`.
struct Chunk {
    lines: Vec<(u64, String)>,
    end: u64,
    /// 파일이 저장된 위치보다 작아져 처음부터 읽었다.
    restarted: bool,
}

/// `.jsonl` 파일을 모은다 — 트랜스크립트 루트 아래 `<project>/<session>.jsonl`(깊이 1)과
/// `<project>/<session>/subagents/*.jsonl`(깊이 3). 그보다 깊이는 내려가지 않는다.
fn collect_jsonl(dir: &Path, depth: usize, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() && depth < 3 {
            collect_jsonl(&path, depth + 1, out);
        } else if kind.is_file() && path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            out.push(path);
        }
    }
}

fn read_complete_lines(path: &Path, from: u64) -> Result<Option<Chunk>, String> {
    let err = |e: std::io::Error| format!("로그 파일을 읽지 못했다({}): {e}", path.display());
    let mut file = std::fs::File::open(path).map_err(err)?;
    let len = file.metadata().map_err(err)?.len();
    let start = if len < from { 0 } else { from };
    if len == start {
        return Ok(None);
    }
    file.seek(SeekFrom::Start(start)).map_err(err)?;
    let mut buf = Vec::with_capacity((len - start) as usize);
    file.read_to_end(&mut buf).map_err(err)?;
    let Some(last_newline) = buf.iter().rposition(|b| *b == b'\n') else {
        return Ok(None); // 아직 한 줄도 끝나지 않았다
    };
    let complete = &buf[..=last_newline];
    let mut lines = Vec::new();
    let mut pos = 0usize;
    for raw in complete.split_inclusive(|b| *b == b'\n') {
        let text = String::from_utf8_lossy(raw);
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            lines.push((start + pos as u64, trimmed.to_string()));
        }
        pos += raw.len();
    }
    Ok(Some(Chunk {
        lines,
        end: start + complete.len() as u64,
        restarted: start == 0 && from > 0,
    }))
}
