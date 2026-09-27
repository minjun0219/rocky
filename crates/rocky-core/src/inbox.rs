//! 수집함 어댑터 규약 — 외부 투두 앱의 미완료 항목을 읽어 오는 명령의 출력 형식.
//!
//! 어댑터는 `rocky.json` 의 `todo.inbox[]` 에 등록된 **명령**이다. 데몬이 argv 그대로 실행하고
//! stdout 의 JSON 하나를 읽는다(`{ "items": [...] }`). 외부 앱과 동기화하지 않는다 — 읽어서
//! 보여주고, 사용자가 고른 것을 보드로 올리며 링크로 참조할 뿐이다. 인증은 어댑터 몫이고
//! 데몬은 모른다. 설계: `docs/design/specs/2026-09-27-bridges-and-tui-design.md`.
//!
//! 이 모듈은 순수 판정만 — 출력 파싱·검증. 실행·캐시는 `rockyd::inbox_exec`.

use serde::{Deserialize, Serialize};

/// 소스 하나의 기본 실행 시간 상한.
pub const DEFAULT_INBOX_TIMEOUT_MS: u64 = 10_000;

/// `GET /api/inbox` 의 소스별 캐시 수명(초). TUI 탭 진입은 캐시, `refresh=true` 는 우회.
pub const INBOX_CACHE_TTL_SECS: u64 = 60;

/// 어댑터가 내는 항목 하나. `id`·`title` 필수, 나머지 옵션.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InboxItem {
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// `YYYY-MM-DD`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due: Option<String>,
    /// RFC 3339.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
}

/// 소스 하나의 조회 결과 — 실패해도 이 모양으로 돌아온다(`available:false` + `reason`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InboxSourceResult {
    pub name: String,
    pub available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub fetched_at: String,
    pub items: Vec<InboxItem>,
}

/// `GET /api/inbox` 응답.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InboxResponse {
    pub sources: Vec<InboxSourceResult>,
}

#[derive(Deserialize)]
struct RawOutput {
    items: Vec<RawItem>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawItem {
    id: Option<serde_json::Value>,
    title: Option<String>,
    url: Option<String>,
    note: Option<String>,
    due: Option<String>,
    created_at: Option<String>,
}

/// 어댑터 stdout 을 항목 목록으로. 실패 사유는 사람이 읽을 한 줄 — 그대로 `reason` 이 된다.
///
/// 규칙: `items` 배열 필수, 항목마다 `id`(문자열 또는 숫자 → 문자열)와 비어 있지 않은 `title`
/// 필수. `due` 는 `YYYY-MM-DD`, `createdAt` 은 RFC 3339 — 형식이 틀리면 **항목 하나가 아니라
/// 출력 전체가 실패**다. 어댑터 버그를 조용히 반쯤 통과시키는 것보다 그 소스만 빨갛게
/// 표시되는 쪽이 고치기 쉽다.
pub fn parse_inbox_output(stdout: &str) -> Result<Vec<InboxItem>, String> {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Err("stdout 이 비어 있다 (JSON `{\"items\": []}` 를 기대)".into());
    }
    let raw: RawOutput =
        serde_json::from_str(trimmed).map_err(|e| format!("JSON 파싱 실패: {e}"))?;
    let mut items = Vec::with_capacity(raw.items.len());
    for (index, item) in raw.items.into_iter().enumerate() {
        let id = match item.id {
            Some(serde_json::Value::String(s)) if !s.trim().is_empty() => s,
            Some(serde_json::Value::Number(n)) => n.to_string(),
            _ => return Err(format!("items[{index}].id 가 없다")),
        };
        let title = match item.title.map(|t| t.trim().to_string()) {
            Some(t) if !t.is_empty() => t,
            _ => return Err(format!("items[{index}].title 이 없다 (id={id})")),
        };
        if let Some(due) = item.due.as_deref() {
            if chrono::NaiveDate::parse_from_str(due, "%Y-%m-%d").is_err() {
                return Err(format!(
                    "items[{index}].due 가 YYYY-MM-DD 가 아니다: {due:?} (id={id})"
                ));
            }
        }
        if let Some(created) = item.created_at.as_deref() {
            if chrono::DateTime::parse_from_rfc3339(created).is_err() {
                return Err(format!(
                    "items[{index}].createdAt 이 RFC 3339 가 아니다: {created:?} (id={id})"
                ));
            }
        }
        items.push(InboxItem {
            id,
            title,
            url: item.url.filter(|u| !u.trim().is_empty()),
            note: item.note,
            due: item.due,
            created_at: item.created_at,
        });
    }
    Ok(items)
}

/// 실패한 소스의 결과 — 실행기와 라우트가 같은 모양을 쓰게 한 곳에 둔다.
pub fn unavailable(name: &str, reason: impl Into<String>, fetched_at: String) -> InboxSourceResult {
    InboxSourceResult {
        name: name.to_string(),
        available: false,
        reason: Some(reason.into()),
        fetched_at,
        items: Vec::new(),
    }
}

/// 원격 호출자에게 낼 사유 — **exit code 만 남긴다.** stderr 첫 줄이나 출력 조각에는 어댑터가
/// 찍은 토큰·인증 URL 이 섞일 수 있어, `todo.expose` 로 노출된 데몬에서는 로컬 요청에만
/// 상세를 준다(이슈 생성·spawn 의 로컬 판정과 같은 `is_local_request`).
pub fn redact_reason(reason: &str) -> String {
    // "exit 1: token expired" → "exit 1", "exit 3" → 그대로. 파싱 실패류는 분류만.
    let code_prefix = reason
        .strip_prefix("exit ")
        .map(|rest| rest.split(':').next().unwrap_or("").trim())
        .filter(|code| !code.is_empty() && code.bytes().all(|b| b.is_ascii_digit()));
    match code_prefix {
        Some(code) => format!("exit {code}"),
        None => "어댑터 출력이 규약에 맞지 않는다 (상세는 로컬 요청에서만)".to_string(),
    }
}

impl InboxResponse {
    /// 원격 응답용 — 실패 소스의 `reason` 을 `redact_reason` 으로 바꾼 사본.
    pub fn redacted(mut self) -> Self {
        for source in &mut self.sources {
            if let Some(reason) = source.reason.as_deref() {
                source.reason = Some(redact_reason(reason));
            }
        }
        self
    }
}
