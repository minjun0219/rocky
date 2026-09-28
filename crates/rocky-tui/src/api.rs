//! 데몬 REST 클라이언트 — 루프백 `ureq`, 짧은 timeout. 실패는 사람이 읽을 한 줄로.

use std::time::Duration;

use rocky_core::inbox::InboxResponse;
use rocky_core::refs::TodoView;
use rocky_core::types::{Board, Comment, HistoryEntry, Section};
use serde::Deserialize;

/// TUI 가 보내는 actor — 보드 히스토리에 이 이름으로 남는다.
pub const ACTOR: &str = "rocky-tui";

const TIMEOUT: Duration = Duration::from_secs(5);
/// 오래 걸릴 수 있는 라우트 — spawn 은 데몬이 `claude --bg` 를 최장 30초 기다리고, inbox 는 어댑터당
/// 기본 10초, issue 는 `gh` 호출. 전역 5초로 끊으면 정상 실행이 에러로 보이고 재시도가 409 를 만든다.
const SLOW_TIMEOUT: Duration = Duration::from_secs(45);

#[derive(Clone)]
pub struct Api {
    pub base_url: String,
    agent: ureq::Agent,
    slow_agent: ureq::Agent,
}

fn agent_with(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .http_status_as_error(false)
        .build()
        .into()
}

/// `GET /api/todos/:ref` 응답.
#[derive(Debug, Clone, Deserialize)]
pub struct TodoDetail {
    pub todo: TodoView,
    #[serde(default)]
    pub history: Vec<HistoryEntry>,
    #[serde(default)]
    pub comments: Vec<Comment>,
}

/// `GET /api/sessions` 의 세션 하나 — `AgentSession` + `matched`. core 타입은 Serialize 만이라
/// 여기서 읽기용으로 다시 정의한다(필요한 필드만).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionOut {
    pub session_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub cwd: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub matched: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SessionsOut {
    pub available: bool,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub sessions: Vec<SessionOut>,
}

#[derive(Debug, Deserialize)]
struct ErrorBody {
    error: Option<String>,
}

impl Api {
    pub fn new(base_url: impl Into<String>) -> Self {
        Api {
            base_url: base_url.into(),
            agent: agent_with(TIMEOUT),
            slow_agent: agent_with(SLOW_TIMEOUT),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }

    /// 2xx 가 아니면 본문의 `error` 필드(없으면 상태 코드)를 에러로.
    fn parse<T: serde::de::DeserializeOwned>(
        path: &str,
        mut response: ureq::http::Response<ureq::Body>,
    ) -> Result<T, String> {
        let status = response.status().as_u16();
        let text = response
            .body_mut()
            .read_to_string()
            .map_err(|e| format!("{path}: 본문 읽기 실패: {e}"))?;
        if !(200..300).contains(&status) {
            let message = serde_json::from_str::<ErrorBody>(&text)
                .ok()
                .and_then(|b| b.error)
                .unwrap_or_else(|| text.trim().to_string());
            return Err(format!("{path}: HTTP {status} {message}"));
        }
        serde_json::from_str(&text).map_err(|e| format!("{path}: JSON 파싱 실패: {e}"))
    }

    pub fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, String> {
        self.get_with(&self.agent, path)
    }

    fn get_with<T: serde::de::DeserializeOwned>(
        &self,
        agent: &ureq::Agent,
        path: &str,
    ) -> Result<T, String> {
        let response = agent
            .get(self.url(path))
            .header("x-rocky-actor", ACTOR)
            .header("x-rocky-client", "tui")
            .call()
            .map_err(|e| format!("{path}: {e}"))?;
        Self::parse(path, response)
    }

    pub fn post<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<T, String> {
        self.post_with(&self.agent, path, body)
    }

    fn post_with<T: serde::de::DeserializeOwned>(
        &self,
        agent: &ureq::Agent,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<T, String> {
        let response = agent
            .post(self.url(path))
            .header("x-rocky-actor", ACTOR)
            .header("x-rocky-client", "tui")
            .header("content-type", "application/json")
            .send(body.to_string().as_bytes())
            .map_err(|e| format!("{path}: {e}"))?;
        Self::parse(path, response)
    }

    pub fn health(&self) -> bool {
        self.get::<serde_json::Value>("/api/health")
            .map(|v| v.get("name").and_then(|n| n.as_str()) == Some("rocky"))
            .unwrap_or(false)
    }

    pub fn boards(&self) -> Result<Vec<Board>, String> {
        self.get("/api/boards")
    }

    pub fn sections(&self, board: &str) -> Result<Vec<Section>, String> {
        self.get(&format!("/api/sections?board={}", encode(board)))
    }

    pub fn todos(&self, board: &str) -> Result<Vec<TodoView>, String> {
        self.get(&format!("/api/todos?board={}", encode(board)))
    }

    pub fn detail(&self, todo_ref: &str) -> Result<TodoDetail, String> {
        self.get(&format!("/api/todos/{}", encode(todo_ref)))
    }

    /// `start` / `stop` / `done` / `reopen` / `archive` / `unarchive`.
    pub fn status(&self, todo_ref: &str, action: &str) -> Result<TodoView, String> {
        self.post(
            &format!("/api/todos/{}/status", encode(todo_ref)),
            &serde_json::json!({ "action": action }),
        )
    }

    pub fn inbox(&self, refresh: bool) -> Result<InboxResponse, String> {
        self.get_with(
            &self.slow_agent,
            if refresh {
                "/api/inbox?refresh=true"
            } else {
                "/api/inbox"
            },
        )
    }

    pub fn create_todo(&self, body: &serde_json::Value) -> Result<TodoView, String> {
        self.post("/api/todos", body)
    }

    pub fn sessions(&self, board: &str) -> Result<SessionsOut, String> {
        self.get(&format!("/api/sessions?board={}", encode(board)))
    }

    /// 열린 핸드오프(대기 + 미수락 배달) — 표시용이라 느슨하게 Value 로.
    pub fn open_handoffs(&self, board: &str) -> Result<Vec<serde_json::Value>, String> {
        self.get(&format!("/api/handoffs?open=true&board={}", encode(board)))
    }

    /// 대상 세션을 지정해 넘긴다. 후보 판정은 호출자(TUI)가 데몬과 같은 기준으로 먼저 한다.
    pub fn handoff(&self, todo_ref: &str, session_id: &str) -> Result<serde_json::Value, String> {
        self.post(
            &format!("/api/todos/{}/handoff", encode(todo_ref)),
            &serde_json::json!({ "sessionId": session_id }),
        )
    }

    /// 새 워크트리 세션 — 로컬 전용 라우트(TUI 는 루프백이라 통과). 409(60초 창)·400 은 메시지로.
    pub fn spawn(&self, todo_ref: &str) -> Result<serde_json::Value, String> {
        self.post_with(
            &self.slow_agent,
            &format!("/api/todos/{}/spawn", encode(todo_ref)),
            &serde_json::json!({}),
        )
    }

    /// GitHub 이슈 생성 — 로컬 전용. 중복이면 409 에 url 이 실려 온다(메시지에 포함).
    pub fn issue(&self, todo_ref: &str) -> Result<serde_json::Value, String> {
        self.post_with(
            &self.slow_agent,
            &format!("/api/todos/{}/issue", encode(todo_ref)),
            &serde_json::json!({}),
        )
    }
}

/// 경로 조각 인코딩 — 보드 key 와 ref 는 `[a-z0-9-]` 라 사실상 그대로지만 `#`·공백은 막는다.
fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
