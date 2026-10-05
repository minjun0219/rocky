//! Claude Code 토큰 사용 색인 — 세션 트랜스크립트(`~/.claude/projects/**/*.jsonl`)에서 모델·effort·토큰을 뽑아
//! `logs.db` 에 쌓고, 모델×effort 통계와 규칙 기반 추천을 낸다.
//!
//! **트랜스크립트가 진실이고 이 표들은 파생 색인이다**(`logindex` 와 같은 규칙). 훅은 토큰을 받지 못하고(훅 입력에
//! usage 가 없다) 모델도 `SessionStart` 에만 실리므로, 수집은 데몬의 색인 스레드가 트랜스크립트를 바이트 위치로
//! 증분해서 읽는 것 하나뿐이다 — Claude Code 쪽에 붙는 훅이 없어 세션을 늦출 일이 없다.
//!
//! 트랜스크립트 모양(실측, Claude Code 2.x):
//! - `type: "assistant"` 줄마다 `message.{id, model, usage, stop_reason}`, `effort`(·`perTurnEffort`), `sessionId`,
//!   `cwd`, `gitBranch`, `timestamp`, `isSidechain`. **content 블록마다 한 줄**이라 같은 `message.id` 가 여러 줄(실측
//!   최대 7줄)에 같은 usage 로 반복된다 — 메시지 id 로 한 번만 센다. 도구 호출은 `tool_use` 블록의 id 로 센다.
//! - 턴 경계는 사람이 쓴 `user` 줄(`transcript::is_real_user_prompt`) — `isMeta`(스킬 본문 주입)·`isCompactSummary`
//!   (압축 요약)는 경계가 아니다.
//! - 서브에이전트는 `<session>/subagents/agent-*.jsonl` 에 `isSidechain: true` 로 따로 쌓인다. 토큰 합계엔 넣고
//!   턴 수·추천에서는 뺀다.

use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::Serialize;
use serde_json::Value;

use crate::transcript::is_real_user_prompt;

pub const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS cc_sessions (
  session_id TEXT PRIMARY KEY,
  cwd TEXT,
  git_branch TEXT,
  started_at TEXT NOT NULL,
  ended_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS cc_sessions_by_cwd ON cc_sessions(cwd, ended_at);
CREATE TABLE IF NOT EXISTS cc_messages (
  message_id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL,
  turn_id TEXT,
  ts TEXT NOT NULL,
  model TEXT NOT NULL,
  effort TEXT,
  git_branch TEXT,
  input_tokens INTEGER NOT NULL,
  output_tokens INTEGER NOT NULL,
  cache_read_tokens INTEGER NOT NULL,
  cache_write_tokens INTEGER NOT NULL,
  stop_reason TEXT,
  sidechain INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS cc_messages_by_session ON cc_messages(session_id, ts);
CREATE INDEX IF NOT EXISTS cc_messages_by_ts ON cc_messages(ts);
CREATE TABLE IF NOT EXISTS cc_tool_uses (
  tool_use_id TEXT PRIMARY KEY,
  message_id TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS cc_tool_uses_by_message ON cc_tool_uses(message_id);
CREATE TABLE IF NOT EXISTS cc_cursors (
  path TEXT PRIMARY KEY,
  turn_id TEXT
);
";

/// 트랜스크립트 한 줄에서 색인할 것.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranscriptLine {
    /// 사람이 쓴 프롬프트 — 턴 경계. `uuid` 가 턴 id 가 된다.
    Prompt(LineMeta),
    Message(MessageLine),
}

/// 줄마다 붙는 세션 맥락.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LineMeta {
    pub uuid: String,
    pub session_id: String,
    pub ts: String,
    pub cwd: Option<String>,
    pub git_branch: Option<String>,
    pub sidechain: bool,
}

/// 어시스턴트 응답 한 줄(=API 응답 하나의 content 블록 하나).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MessageLine {
    pub meta: LineMeta,
    pub message_id: String,
    pub model: String,
    pub effort: Option<String>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub stop_reason: Option<String>,
    pub tool_use_ids: Vec<String>,
}

fn str_at<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// effort 는 트랜스크립트에선 문자열(`"high"`), 훅 입력에선 `{ "level": "high" }` — 둘 다 받는다.
fn effort_of(v: &Value) -> Option<String> {
    ["effort", "perTurnEffort"].iter().find_map(|key| {
        let raw = v.get(*key)?;
        raw.as_str()
            .or_else(|| raw.get("level").and_then(Value::as_str))
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    })
}

/// 한 줄을 읽는다. 색인할 게 아니면 `None` — 손상 줄, 다른 타입, 합성 메시지(`<synthetic>`), 하네스 주입.
pub fn parse_line(line: &str) -> Option<TranscriptLine> {
    // 대부분의 줄(첨부·도구 결과·시스템)은 큰데 쓸모가 없다 — 파싱 전에 거른다.
    if !line.contains("\"assistant\"") && !line.contains("\"user\"") {
        return None;
    }
    let v: Value = serde_json::from_str(line).ok()?;
    let meta = LineMeta {
        uuid: str_at(&v, "uuid").unwrap_or_default().to_string(),
        session_id: str_at(&v, "sessionId")?.to_string(),
        ts: str_at(&v, "timestamp")?.to_string(),
        cwd: str_at(&v, "cwd").map(str::to_string),
        git_branch: str_at(&v, "gitBranch").map(str::to_string),
        sidechain: v.get("isSidechain").and_then(Value::as_bool) == Some(true),
    };
    match v.get("type").and_then(Value::as_str)? {
        "user" => {
            let injected = v.get("isMeta").and_then(Value::as_bool) == Some(true)
                || v.get("isCompactSummary").and_then(Value::as_bool) == Some(true);
            if injected || meta.uuid.is_empty() || !is_real_user_prompt(v.get("message")?) {
                return None;
            }
            Some(TranscriptLine::Prompt(meta))
        }
        "assistant" => {
            let msg = v.get("message")?;
            let model = str_at(msg, "model")?;
            if model.starts_with('<') {
                return None; // `<synthetic>` — API 를 부르지 않은 메시지
            }
            let usage = msg.get("usage")?;
            let n = |k: &str| usage.get(k).and_then(Value::as_u64).unwrap_or(0);
            let tool_use_ids = msg
                .get("content")
                .and_then(Value::as_array)
                .map(|blocks| {
                    blocks
                        .iter()
                        .filter(|b| b.get("type").and_then(Value::as_str) == Some("tool_use"))
                        .filter_map(|b| str_at(b, "id").map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            Some(TranscriptLine::Message(MessageLine {
                message_id: str_at(msg, "id")?.to_string(),
                model: model.to_string(),
                effort: effort_of(&v),
                input_tokens: n("input_tokens"),
                output_tokens: n("output_tokens"),
                cache_read_tokens: n("cache_read_input_tokens"),
                cache_write_tokens: n("cache_creation_input_tokens"),
                stop_reason: str_at(msg, "stop_reason").map(str::to_string),
                tool_use_ids,
                meta,
            }))
        }
        _ => None,
    }
}

/// 파일 하나를 읽는 동안의 상태 — 지금 열린 턴. 다음 바퀴로 이어지게 `cc_cursors` 에 둔다.
pub fn load_cursor(conn: &Connection, path: &str) -> rusqlite::Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT turn_id FROM cc_cursors WHERE path = ?1",
            params![path],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten())
}

pub fn save_cursor(tx: &Transaction, path: &str, turn_id: Option<&str>) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT INTO cc_cursors (path, turn_id) VALUES (?1, ?2) ON CONFLICT(path) DO UPDATE SET turn_id = excluded.turn_id",
        params![path, turn_id],
    )?;
    Ok(())
}

fn touch_session(tx: &Transaction, meta: &LineMeta) -> rusqlite::Result<()> {
    // 시작은 가장 이른 줄, 끝·cwd·브랜치는 가장 늦은 줄 — 파일 순서와 무관하게(서브에이전트 파일이 늦게 읽혀도).
    tx.execute(
        "INSERT INTO cc_sessions (session_id, cwd, git_branch, started_at, ended_at) VALUES (?1, ?2, ?3, ?4, ?4)
         ON CONFLICT(session_id) DO UPDATE SET
           started_at = MIN(started_at, excluded.started_at),
           cwd = CASE WHEN excluded.ended_at >= ended_at THEN COALESCE(excluded.cwd, cwd) ELSE cwd END,
           git_branch = CASE WHEN excluded.ended_at >= ended_at THEN COALESCE(excluded.git_branch, git_branch) ELSE git_branch END,
           ended_at = MAX(ended_at, excluded.ended_at)",
        params![meta.session_id, meta.cwd, meta.git_branch, meta.ts],
    )?;
    Ok(())
}

/// 한 줄을 넣는다. `turn` 은 이 파일에서 지금 열린 턴(프롬프트면 갱신된다). 넣은 새 메시지 수를 돌려준다 —
/// 같은 메시지의 둘째 줄부터는 0(도구 호출 id 만 더한다).
pub fn ingest_line(
    tx: &Transaction,
    line: &TranscriptLine,
    turn: &mut Option<String>,
) -> rusqlite::Result<usize> {
    match line {
        TranscriptLine::Prompt(meta) => {
            *turn = Some(meta.uuid.clone());
            touch_session(tx, meta)?;
            Ok(0)
        }
        TranscriptLine::Message(m) => {
            touch_session(tx, &m.meta)?;
            let inserted = tx.execute(
                "INSERT INTO cc_messages (message_id, session_id, turn_id, ts, model, effort, git_branch, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, stop_reason, sidechain)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
                 ON CONFLICT(message_id) DO NOTHING",
                params![
                    m.message_id,
                    m.meta.session_id,
                    turn.as_deref(),
                    m.meta.ts,
                    m.model,
                    m.effort,
                    m.meta.git_branch,
                    m.input_tokens as i64,
                    m.output_tokens as i64,
                    m.cache_read_tokens as i64,
                    m.cache_write_tokens as i64,
                    m.stop_reason,
                    m.meta.sidechain,
                ],
            )?;
            if inserted == 0 && m.stop_reason.is_some() {
                // 같은 메시지의 뒤 줄에만 멈춘 이유가 붙어 있을 수 있다.
                tx.execute(
                    "UPDATE cc_messages SET stop_reason = ?2 WHERE message_id = ?1 AND stop_reason IS NULL",
                    params![m.message_id, m.stop_reason],
                )?;
            }
            for id in &m.tool_use_ids {
                tx.execute(
                    "INSERT OR IGNORE INTO cc_tool_uses (tool_use_id, message_id) VALUES (?1, ?2)",
                    params![id, m.message_id],
                )?;
            }
            Ok(inserted)
        }
    }
}

// ── 조회 ─────────────────────────────────────────────────────────────────────

/// 조회 구간 `[from, to)` — 주지 않은 끝은 `days`(기본 30, 1~365)일 전 / 끝없음. 값은 ISO 문자열(날짜만도 된다).
pub fn range(
    from: Option<&str>,
    to: Option<&str>,
    days: Option<i64>,
    now: chrono::DateTime<chrono::Utc>,
) -> (String, String) {
    let pick = |v: Option<&str>| {
        v.map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string)
    };
    let from = pick(from).unwrap_or_else(|| {
        let days = days.unwrap_or(30).clamp(1, 365);
        (now - chrono::Duration::days(days)).to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
    });
    (from, pick(to).unwrap_or_else(|| "9999".to_string()))
}

/// 요약을 무엇으로 묶나.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupBy {
    ModelEffort,
    Model,
    Effort,
    Session,
    Branch,
}

impl GroupBy {
    /// `model,effort` · `model` · `effort` · `session` · `branch`. 모르는 값은 `None`.
    pub fn parse(raw: &str) -> Option<Self> {
        let parts: Vec<String> = raw
            .split(',')
            .map(|p| p.trim().to_ascii_lowercase())
            .filter(|p| !p.is_empty())
            .collect();
        let parts: Vec<&str> = parts.iter().map(String::as_str).collect();
        match parts.as_slice() {
            ["model", "effort"] | ["effort", "model"] => Some(Self::ModelEffort),
            ["model"] => Some(Self::Model),
            ["effort"] => Some(Self::Effort),
            ["session"] => Some(Self::Session),
            ["branch"] => Some(Self::Branch),
            _ => None,
        }
    }

    fn columns(self) -> &'static [&'static str] {
        match self {
            Self::ModelEffort => &["m.model", "m.effort"],
            Self::Model => &["m.model"],
            Self::Effort => &["m.effort"],
            Self::Session => &["m.session_id"],
            Self::Branch => &["m.git_branch"],
        }
    }
}

/// 토큰 합계 한 줄. 묶지 않은 칸은 비운다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SummaryRow {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
    pub sessions: u64,
    /// 사람이 쓴 프롬프트 수(서브에이전트 제외).
    pub turns: u64,
    /// API 응답 수(서브에이전트 포함).
    pub requests: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub tool_calls: u64,
}

const TOOL_COUNTS: &str =
    "LEFT JOIN (SELECT message_id, COUNT(*) AS n FROM cc_tool_uses GROUP BY message_id) t ON t.message_id = m.message_id";

/// `[from, to)` 구간(ISO)의 합계. 큰 출력 순.
pub fn summary(
    conn: &Connection,
    from: &str,
    to: &str,
    group_by: GroupBy,
) -> rusqlite::Result<Vec<SummaryRow>> {
    let cols = group_by.columns();
    let keys = cols.join(", ");
    let sql = format!(
        "SELECT {keys},
                COUNT(DISTINCT m.session_id),
                COUNT(DISTINCT CASE WHEN m.sidechain = 0 THEN m.turn_id END),
                COUNT(*),
                SUM(m.input_tokens), SUM(m.output_tokens), SUM(m.cache_read_tokens), SUM(m.cache_write_tokens),
                COALESCE(SUM(t.n), 0)
         FROM cc_messages m {TOOL_COUNTS}
         WHERE m.ts >= ?1 AND m.ts < ?2
         GROUP BY {keys}
         ORDER BY SUM(m.output_tokens) DESC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![from, to], |r| {
        let n = cols.len();
        let num = |i: usize| r.get::<_, i64>(n + i).map(|v| v.max(0) as u64);
        let mut row = SummaryRow {
            sessions: num(0)?,
            turns: num(1)?,
            requests: num(2)?,
            input_tokens: num(3)?,
            output_tokens: num(4)?,
            cache_read_tokens: num(5)?,
            cache_write_tokens: num(6)?,
            tool_calls: num(7)?,
            ..Default::default()
        };
        for (i, col) in cols.iter().enumerate() {
            let value: Option<String> = r.get(i)?;
            match *col {
                "m.model" => row.model = value,
                "m.effort" => row.effort = value,
                "m.session_id" => row.session_id = value,
                _ => row.git_branch = value,
            }
        }
        Ok(row)
    })?;
    rows.collect()
}

/// 세션 머리.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub session_id: String,
    pub cwd: Option<String>,
    pub git_branch: Option<String>,
    pub started_at: String,
    pub ended_at: String,
}

/// 사람의 프롬프트 하나부터 다음 프롬프트 전까지(서브에이전트 제외).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TurnStat {
    pub turn_id: String,
    pub started_at: String,
    pub ended_at: String,
    /// 턴의 마지막 응답의 모델·effort — 턴 중간에 바꿨으면 바꾼 뒤 값.
    pub model: String,
    pub effort: Option<String>,
    pub requests: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub tool_calls: u64,
    pub stop_reason: Option<String>,
}

/// effort 가 바뀐 지점 — 앞 턴과 다음 턴의 effort 가 다르면 하나.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EffortChange {
    pub ts: String,
    pub from: Option<String>,
    pub to: Option<String>,
    /// 어디서 알았나 — 지금은 늘 `transcript`(턴별 effort 기록).
    pub source: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDetail {
    pub session: SessionInfo,
    /// 오래된 순.
    pub turns: Vec<TurnStat>,
    pub effort_changes: Vec<EffortChange>,
}

pub fn session_info(conn: &Connection, session_id: &str) -> rusqlite::Result<Option<SessionInfo>> {
    conn.query_row(
        "SELECT session_id, cwd, git_branch, started_at, ended_at FROM cc_sessions WHERE session_id = ?1",
        params![session_id],
        |r| {
            Ok(SessionInfo {
                session_id: r.get(0)?,
                cwd: r.get(1)?,
                git_branch: r.get(2)?,
                started_at: r.get(3)?,
                ended_at: r.get(4)?,
            })
        },
    )
    .optional()
}

/// 세션의 최근 턴 `limit` 개를 오래된 순으로. 프롬프트 전 응답(턴 id 없음)은 뺀다.
pub fn recent_turns(
    conn: &Connection,
    session_id: &str,
    limit: usize,
) -> rusqlite::Result<Vec<TurnStat>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT m.turn_id, MIN(m.ts), MAX(m.ts), COUNT(*),
                SUM(m.input_tokens), SUM(m.output_tokens), SUM(m.cache_read_tokens), SUM(m.cache_write_tokens),
                COALESCE(SUM(t.n), 0)
         FROM cc_messages m {TOOL_COUNTS}
         WHERE m.session_id = ?1 AND m.sidechain = 0 AND m.turn_id IS NOT NULL
         GROUP BY m.turn_id
         ORDER BY MIN(m.ts) DESC
         LIMIT ?2"
    ))?;
    let mut turns = stmt
        .query_map(params![session_id, limit.max(1) as i64], |r| {
            let num = |i: usize| r.get::<_, i64>(i).map(|v| v.max(0) as u64);
            Ok(TurnStat {
                turn_id: r.get(0)?,
                started_at: r.get(1)?,
                ended_at: r.get(2)?,
                requests: num(3)?,
                input_tokens: num(4)?,
                output_tokens: num(5)?,
                cache_read_tokens: num(6)?,
                cache_write_tokens: num(7)?,
                tool_calls: num(8)?,
                ..Default::default()
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    // 턴의 마지막 응답 — 모델·effort·멈춘 이유.
    let mut last = conn.prepare(
        "SELECT model, effort, stop_reason FROM cc_messages WHERE turn_id = ?1 AND sidechain = 0 ORDER BY ts DESC LIMIT 1",
    )?;
    for turn in &mut turns {
        let (model, effort, stop) = last.query_row(params![turn.turn_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })?;
        turn.model = model;
        turn.effort = effort;
        turn.stop_reason = stop;
    }
    turns.reverse();
    Ok(turns)
}

/// 턴 목록에서 effort 가 바뀐 지점.
pub fn effort_changes(turns: &[TurnStat]) -> Vec<EffortChange> {
    turns
        .windows(2)
        .filter(|w| w[0].effort != w[1].effort)
        .map(|w| EffortChange {
            ts: w[1].started_at.clone(),
            from: w[0].effort.clone(),
            to: w[1].effort.clone(),
            source: "transcript",
        })
        .collect()
}

/// 세션 상세 — 최근 `limit` 턴.
pub fn session_detail(
    conn: &Connection,
    session_id: &str,
    limit: usize,
) -> rusqlite::Result<Option<SessionDetail>> {
    let Some(session) = session_info(conn, session_id)? else {
        return Ok(None);
    };
    let turns = recent_turns(conn, session_id, limit)?;
    Ok(Some(SessionDetail {
        effort_changes: effort_changes(&turns),
        session,
        turns,
    }))
}

/// 이 디렉터리(또는 그 아래)에서 가장 최근에 움직인 세션 — "지금 세션".
pub fn latest_session_for_cwd(conn: &Connection, cwd: &str) -> rusqlite::Result<Option<String>> {
    let cwd = cwd.trim_end_matches('/');
    let escaped = cwd
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    conn.query_row(
        "SELECT session_id FROM cc_sessions WHERE cwd = ?1 OR cwd LIKE ?2 ESCAPE '\\' ORDER BY ended_at DESC LIMIT 1",
        params![cwd, format!("{escaped}/%")],
        |r| r.get(0),
    )
    .optional()
}

/// 최근에 움직인 세션들 — 추천 이벤트를 다시 계산할 대상.
pub fn sessions_active_since(conn: &Connection, since: &str) -> rusqlite::Result<Vec<String>> {
    let mut stmt =
        conn.prepare("SELECT session_id FROM cc_sessions WHERE ended_at >= ?1 ORDER BY ended_at")?;
    let rows = stmt.query_map(params![since], |r| r.get(0))?;
    rows.collect()
}

// ── 추천(규칙 v1) ────────────────────────────────────────────────────────────

/// 추천 SSE 이벤트 이름(`GET /api/tokens/events`).
pub const RECOMMENDATION_EVENT: &str = "tokens.recommendation";

/// 추천 규칙의 설정 — `rocky.json` 의 `tokens.recommend`(`config::load_tokens_block`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecommendConfig {
    /// 최근 몇 턴을 보나.
    pub window: usize,
    /// 이보다 턴이 적으면 판단하지 않는다.
    pub min_turns: usize,
    /// 턴 평균 출력이 이 이하면 "짧다".
    pub low_output_tokens: u64,
    pub lower_effort: bool,
    pub hold_after_raise: bool,
    pub switch_to_sonnet: bool,
}

impl Default for RecommendConfig {
    fn default() -> Self {
        RecommendConfig {
            window: 15,
            min_turns: 5,
            low_output_tokens: 3_000,
            lower_effort: true,
            hold_after_raise: true,
            switch_to_sonnet: true,
        }
    }
}

/// 추천 하나.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    /// `lower-effort` · `switch-to-sonnet`.
    pub rule: &'static str,
    pub message: String,
}

/// 근거 수치 — 추천이 없어도 싣는다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Evidence {
    pub turns: usize,
    pub avg_output_tokens: u64,
    pub tool_calls: u64,
    pub model: Option<String>,
    pub effort: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Recommendation {
    pub session_id: String,
    pub suggestions: Vec<Suggestion>,
    /// 추천을 내지 않은 이유(턴 부족·이미 조정함). 추천이 있으면 비운다.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub held: Option<String>,
    pub evidence: Evidence,
}

/// effort 높낮이. 모르는 값은 `None`.
pub fn effort_rank(effort: Option<&str>) -> Option<u8> {
    match effort? {
        "low" => Some(1),
        "medium" => Some(2),
        "high" => Some(3),
        "xhigh" => Some(4),
        "max" => Some(5),
        _ => None,
    }
}

fn avg_output(turns: &[TurnStat]) -> u64 {
    if turns.is_empty() {
        return 0;
    }
    turns.iter().map(|t| t.output_tokens).sum::<u64>() / turns.len() as u64
}

fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// 세션 상세 + 추천 — "지금 세션" 응답(REST `/api/tokens/current`, MCP `token_current_session`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentSession {
    #[serde(flatten)]
    pub detail: SessionDetail,
    pub recommendation: Recommendation,
}

/// 이 디렉터리(또는 그 아래)의 최근 세션 상세와 추천. 세션이 없으면 `None`.
pub fn current_session(
    conn: &Connection,
    cwd: &str,
    limit: usize,
    cfg: &RecommendConfig,
) -> rusqlite::Result<Option<CurrentSession>> {
    let Some(id) = latest_session_for_cwd(conn, cwd)? else {
        return Ok(None);
    };
    let Some(detail) = session_detail(conn, &id, limit)? else {
        return Ok(None);
    };
    Ok(Some(CurrentSession {
        recommendation: recommendation_for(conn, &id, cfg)?,
        detail,
    }))
}

/// 세션의 최근 턴을 읽어 추천을 낸다.
pub fn recommendation_for(
    conn: &Connection,
    session_id: &str,
    cfg: &RecommendConfig,
) -> rusqlite::Result<Recommendation> {
    let turns = recent_turns(conn, session_id, cfg.window.max(1))?;
    Ok(recommend(session_id, &turns, cfg))
}

impl Recommendation {
    /// 알릴 만큼 바뀌었나를 가르는 열쇠 — 낸 규칙들. 근거 수치는 턴마다 바뀌므로 넣지 않는다.
    pub fn rules_key(&self) -> String {
        self.suggestions
            .iter()
            .map(|s| s.rule)
            .collect::<Vec<_>>()
            .join(",")
    }
}

/// 최근 턴(오래된 순)으로 추천을 낸다 — 순수 함수.
///
/// 1. `lower-effort`: 평균 출력 ≤ 임계이고 지금 effort 가 xhigh/max → medium 고려.
/// 2. `hold-after-raise`: 창 안에서 사람이 effort 를 올렸고 그 뒤 턴들의 평균 출력이 앞보다 길어졌으면 → 추천 억제.
/// 3. `switch-to-sonnet`: 지금 모델이 Opus 이고 창의 턴이 전부 도구 호출 0 + 평균 출력 ≤ 임계 → Sonnet medium 고려.
pub fn recommend(session_id: &str, turns: &[TurnStat], cfg: &RecommendConfig) -> Recommendation {
    let window = &turns[turns.len().saturating_sub(cfg.window.max(1))..];
    let latest = window.last();
    let avg = avg_output(window);
    let evidence = Evidence {
        turns: window.len(),
        avg_output_tokens: avg,
        tool_calls: window.iter().map(|t| t.tool_calls).sum(),
        model: latest.map(|t| t.model.clone()),
        effort: latest.and_then(|t| t.effort.clone()),
    };
    let mut out = Recommendation {
        session_id: session_id.to_string(),
        suggestions: Vec::new(),
        held: None,
        evidence,
    };
    if window.len() < cfg.min_turns.max(1) {
        out.held = Some(format!(
            "턴이 {}개뿐이다(최소 {}개)",
            window.len(),
            cfg.min_turns
        ));
        return out;
    }
    if cfg.hold_after_raise {
        let raised_at = window.windows(2).rposition(|w| {
            match (
                effort_rank(w[0].effort.as_deref()),
                effort_rank(w[1].effort.as_deref()),
            ) {
                (Some(a), Some(b)) => b > a,
                _ => false,
            }
        });
        if let Some(i) = raised_at {
            let (before, after) = window.split_at(i + 1);
            let (b, a) = (avg_output(before), avg_output(after));
            if a > b {
                out.held = Some(format!(
                    "effort 를 {} → {} 로 올린 뒤 턴 평균 출력이 {} → {} 토큰으로 길어졌다 — 이미 조정했다",
                    before
                        .last()
                        .and_then(|t| t.effort.as_deref())
                        .unwrap_or("?"),
                    after[0].effort.as_deref().unwrap_or("?"),
                    thousands(b),
                    thousands(a),
                ));
                return out;
            }
        }
    }
    let Some(latest) = latest else {
        return out;
    };
    let n = window.len();
    if cfg.lower_effort
        && avg <= cfg.low_output_tokens
        && matches!(latest.effort.as_deref(), Some("xhigh" | "max"))
    {
        out.suggestions.push(Suggestion {
            rule: "lower-effort",
            message: format!(
                "최근 {n}턴 평균 출력 {} 토큰(기준 {} 이하)인데 effort 가 {} — medium 으로 낮추는 것을 고려",
                thousands(avg),
                thousands(cfg.low_output_tokens),
                latest.effort.as_deref().unwrap_or_default(),
            ),
        });
    }
    if cfg.switch_to_sonnet
        && latest.model.to_ascii_lowercase().contains("opus")
        && avg <= cfg.low_output_tokens
        && window.iter().all(|t| t.tool_calls == 0)
    {
        out.suggestions.push(Suggestion {
            rule: "switch-to-sonnet",
            message: format!(
                "최근 {n}턴 동안 도구 호출 0, 평균 출력 {} 토큰 — {} 대신 Sonnet medium 으로 전환 고려",
                thousands(avg),
                latest.model,
            ),
        });
    }
    out
}
