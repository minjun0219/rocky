//! 활성 Claude Code 세션 목록 — TS 원본 `src/sessions.ts` 의 **순수 부분**.
//!
//! 실제 `claude agents --json` 실행(RunCommand)과 TTL 캐시는 데몬(rockyd) 몫이다 —
//! 여기는 출력 파싱과 보드 매칭만 둔다(테스트가 프로세스 없이 계약을 검증한다).

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSession {
    /// 프로세스 pid. 프로세스가 없는 background 세션(사람 답을 기다리며 잠든 `blocked` 등)에는 없다 —
    /// Claude Code 2.1.289 의 `claude agents --json` 이 그 행에 `pid`·`status` 를 싣지 않는다.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<i64>,
    pub cwd: String,
    /// 'interactive' | 'background' — CLI 가 주는 값을 그대로 둔다.
    pub kind: String,
    /// 짧은 id(8자) — `claude attach/logs/stop/rm` 이 받는 값. background 세션에만 붙는다.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub session_id: String,
    /// 사람이 읽는 세션 이름 (예: `eelpout-a3`).
    pub name: String,
    /// 'idle' | 'busy' — CLI 가 주는 값을 그대로 둔다.
    pub status: String,
    /// background 세션의 수명 상태 — 'working' | 'blocked' | 'done'. 없음은 "죽지 않았다".
    /// 'blocked' 는 사람의 답을 기다리는 중이다(살아 있다).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    pub started_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SessionsResult {
    /// 세션 목록을 얻을 수 있었는가. false 면 이 기능 전체가 비활성이다.
    pub available: bool,
    pub sessions: Vec<AgentSession>,
    /// available 이 false 인 이유 — 사용자에게 그대로 보여준다.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl SessionsResult {
    pub fn unavailable(reason: impl Into<String>) -> Self {
        SessionsResult {
            available: false,
            sessions: Vec::new(),
            reason: Some(reason.into()),
        }
    }
}

fn to_session(value: &serde_json::Value) -> Option<AgentSession> {
    let row = value.as_object()?;
    let pid = row.get("pid").and_then(|v| v.as_i64());
    let cwd = row.get("cwd")?.as_str()?;
    let session_id = row.get("sessionId")?.as_str()?;
    let name = row.get("name")?.as_str()?;
    Some(AgentSession {
        pid,
        cwd: cwd.to_string(),
        kind: row
            .get("kind")
            .and_then(|v| v.as_str())
            .unwrap_or("interactive")
            .to_string(),
        id: row.get("id").and_then(|v| v.as_str()).map(str::to_string),
        session_id: session_id.to_string(),
        name: name.to_string(),
        status: row
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("idle")
            .to_string(),
        state: row
            .get("state")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        started_at: row.get("startedAt").and_then(|v| v.as_i64()).unwrap_or(0),
    })
}

/// `claude agents --json` 의 stdout 을 세션 목록으로 파싱한다 — TS `listSessions` 의
/// 파싱 절반. 실행 실패는 호출자가 `SessionsResult::unavailable` 로 만든다.
pub fn parse_sessions(stdout: &str) -> SessionsResult {
    let parsed: serde_json::Value = match serde_json::from_str(stdout) {
        Ok(v) => v,
        Err(_) => return SessionsResult::unavailable("claude agents --json 출력을 읽을 수 없다"),
    };
    let Some(items) = parsed.as_array() else {
        return SessionsResult::unavailable("claude agents --json 출력이 배열이 아니다");
    };
    let sessions = items.iter().filter_map(to_session).collect();
    SessionsResult {
        available: true,
        sessions,
        reason: None,
    }
}

/// 핸드오프를 받을 수 있는 세션인가 — 사람 답을 기다리며 잠든(`blocked`) 세션과 끝난(`done`) background 세션은
/// 아니다. 목록에는 남아 doing 생존 판정에는 쓰이지만(잠든 것은 사라진 게 아니다), 일을 새로 넘길 곳은 아니다.
pub fn takes_handoff(session: &AgentSession) -> bool {
    !matches!(session.state.as_deref(), Some("blocked" | "done"))
}

/// 보드 key 로 후보 세션을 고른다 — **cwd 의 경로 세그먼트 중 하나가 key 와 정확히
/// 일치**하면 후보다. basename 만 보면 워크트리를 놓친다.
pub fn match_board<'a>(sessions: &'a [AgentSession], board_key: &str) -> Vec<&'a AgentSession> {
    if board_key.is_empty() {
        return Vec::new();
    }
    sessions
        .iter()
        .filter(|s| s.cwd.split('/').any(|seg| seg == board_key))
        .collect()
}

/// background 세션의 작업 요약 — Claude Code 가 `<설정 폴더>/jobs/<짧은 id>/state.json` 에 남기는 것 중
/// 화면에 쓸 것만 고른다(`claude agents` 화면이 보여 주는 그 요약). 문서화되지 않은 내부 파일이라 형식이
/// 바뀌면 [`parse_job_state`] 가 `None` 을 내고, 화면은 그 줄만 비운다.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobSummary {
    /// 지금 하는 일 또는 멈춘 자리 한 줄.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// 사람에게 필요한 것 — `blocked` 일 때 무엇을 기다리는지.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub needs: Option<String>,
    /// Claude 가 제안한 답장.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_reply: Option<String>,
    /// 지금까지 쓴 토큰.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens: Option<i64>,
    /// 요약을 마지막으로 고친 시각(ISO).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// `state.json` 을 [`JobSummary`] 로 읽는다. 못 읽거나 쓸 필드가 하나도 없으면 `None`.
pub fn parse_job_state(raw: &str) -> Option<JobSummary> {
    let value: serde_json::Value = serde_json::from_str(raw).ok()?;
    let row = value.as_object()?;
    let text = |key: &str| {
        row.get(key)
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    let summary = JobSummary {
        detail: text("detail"),
        needs: text("needs"),
        suggested_reply: text("suggestedReply"),
        tokens: row.get("tokens").and_then(|v| v.as_i64()),
        updated_at: text("updatedAt"),
    };
    (summary != JobSummary::default()).then_some(summary)
}

/// 짧은 id 의 `state.json` 경로. id 는 CLI 출력에서 온 값이라 경로 조각으로 쓰기 전에 영숫자만 받는다 —
/// `..` 나 `/` 가 섞이면 `None`.
pub fn job_state_path(jobs_dir: &std::path::Path, id: &str) -> Option<std::path::PathBuf> {
    let safe = !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric());
    safe.then(|| jobs_dir.join(id).join("state.json"))
}

/// background 작업 폴더 — `$CLAUDE_CONFIG_DIR/jobs`, 없으면 `~/.claude/jobs`.
pub fn claude_jobs_dir(env: &crate::config::EnvMap) -> std::path::PathBuf {
    env.get("CLAUDE_CONFIG_DIR")
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(crate::config::expand_tilde)
        .unwrap_or_else(|| crate::config::expand_tilde("~/.claude"))
        .join("jobs")
}
