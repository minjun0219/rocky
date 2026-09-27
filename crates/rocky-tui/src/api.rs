//! 데몬 REST 클라이언트 — 루프백 `ureq`, 짧은 timeout. 실패는 사람이 읽을 한 줄로.

use std::time::Duration;

use rocky_core::refs::TodoView;
use rocky_core::types::{Board, Comment, HistoryEntry, Section};
use serde::Deserialize;

/// TUI 가 보내는 actor — 보드 히스토리에 이 이름으로 남는다.
pub const ACTOR: &str = "rocky-tui";

const TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub struct Api {
    pub base_url: String,
    agent: ureq::Agent,
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

#[derive(Debug, Deserialize)]
struct ErrorBody {
    error: Option<String>,
}

impl Api {
    pub fn new(base_url: impl Into<String>) -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(TIMEOUT))
            .http_status_as_error(false)
            .build()
            .into();
        Api {
            base_url: base_url.into(),
            agent,
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
        let response = self
            .agent
            .get(self.url(path))
            .header("x-rocky-actor", ACTOR)
            .call()
            .map_err(|e| format!("{path}: {e}"))?;
        Self::parse(path, response)
    }

    pub fn post<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<T, String> {
        let response = self
            .agent
            .post(self.url(path))
            .header("x-rocky-actor", ACTOR)
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
