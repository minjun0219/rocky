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
    /// 이미 보드에 올라갔나 — 데몬이 **모든 보드**(보관 포함)의 링크로 판정해 채운다. 어댑터가 준 값은
    /// 덮어쓴다(`mark_promoted`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub promoted: bool,
}

/// 소스 하나의 조회 결과 — 실패해도 이 모양으로 돌아온다(`available:false` + `reason`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InboxSourceResult {
    pub name: String,
    /// 보드에 등록한 소스면 그 보드 key — 설정 파일의 소스(모든 보드 공통)는 없다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub board: Option<String>,
    pub available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub fetched_at: String,
    pub items: Vec<InboxItem>,
}

/// 수집함 항목에 "이미 올라감" 을 표시한다 — url 이 `promoted` 에 있으면. url 없는 항목은 판정할 수
/// 없어 늘 미올림이다. 판정은 이 함수 한 곳이고 요약·TUI·CLI 가 같은 값을 본다.
pub fn mark_promoted(inbox: &mut InboxResponse, promoted: &std::collections::HashSet<String>) {
    for item in inbox.sources.iter_mut().flat_map(|s| s.items.iter_mut()) {
        item.promoted = item.url.as_deref().is_some_and(|u| promoted.contains(u));
    }
}

/// `rocky inbox` 의 사람용 출력 — 소스마다 머리줄(이름 · 미올림/전체, 실패면 사유 한 줄) 아래 항목.
/// 항목은 `✓`(올라감)/`·`(미올림) + 제목, 다음 줄에 url. 소스가 없으면 설정 안내 한 줄.
pub fn render_inbox(inbox: &InboxResponse) -> String {
    if inbox.sources.is_empty() {
        return "수집함 소스 없음 — rocky.json 의 todo.inbox[] 에 어댑터를 등록한다".to_string();
    }
    let mut lines: Vec<String> = Vec::new();
    for source in &inbox.sources {
        if !source.available {
            let reason = source.reason.as_deref().unwrap_or("사유 없음");
            let reason = reason.lines().next().unwrap_or_default();
            lines.push(format!("{} — 실패: {reason}", source.name));
            continue;
        }
        let open = source.items.iter().filter(|i| !i.promoted).count();
        lines.push(format!(
            "{} — 미올림 {open} / {}",
            source.name,
            source.items.len()
        ));
        for item in &source.items {
            let mark = if item.promoted { "✓" } else { "·" };
            lines.push(format!(
                "  {mark} {}",
                crate::summary::one_line(&item.title, 80)
            ));
            if let Some(url) = &item.url {
                lines.push(format!("    {url}"));
            }
        }
    }
    lines.join("\n")
}

/// 어댑터 입력 칸 하나 — `--describe` 출력의 `params[]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DescribeParam {
    pub flag: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
    #[serde(default)]
    pub required: bool,
}

/// 어댑터가 `--describe` 로 내는 입력 칸 목록 — 보드 설정 화면이 폼을 그린다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdapterDescribe {
    pub title: String,
    pub params: Vec<DescribeParam>,
}

/// `--describe` 출력 → 칸 목록. 플래그는 `--` 로 시작하는 이름이어야 한다(argv 로 그대로 나간다).
pub fn parse_describe(stdout: &str) -> Result<AdapterDescribe, String> {
    let describe: AdapterDescribe = serde_json::from_str(stdout.trim())
        .map_err(|e| format!("--describe 출력이 JSON 이 아니다: {e}"))?;
    for p in &describe.params {
        let name = p.flag.strip_prefix("--").unwrap_or_default();
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            return Err(format!(
                "--describe 의 flag 가 --이름 모양이 아니다: {:?}",
                p.flag
            ));
        }
    }
    Ok(describe)
}

/// 보드에 등록된 입력 값 하나 — 플래그와 값.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InboxParam {
    pub flag: String,
    pub value: String,
}

/// 보드마다 등록한 수집함 — 설정 파일의 어댑터(`todo.inboxAdapters[]`, **무엇을 실행할지**) + 화면에서 채운
/// 값(**무엇을 거를지**). 실행 명령은 늘 설정 파일에서 오고 화면은 어댑터가 알려 준 칸만 채운다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardInboxSource {
    pub id: String,
    /// 보드 key.
    pub board: String,
    /// 소스 이름 — `[a-z0-9-]+`, 설정의 소스와 겹치지 않는다.
    pub name: String,
    /// `todo.inboxAdapters[].name`.
    pub adapter: String,
    pub params: Vec<InboxParam>,
    pub created_at: String,
}

/// 화면 값 한 칸의 상한 — 필터 문자열이 들어가는 자리라 넉넉하되 끝은 있다.
pub const INBOX_PARAM_MAX: usize = 500;

/// 화면에서 온 값을 어댑터의 칸 목록으로 검증한다 — 목록에 없는 플래그(`--from` 같은 테스트 인자)는
/// 거부, 필수 칸은 비면 거부, 제어문자·`-` 로 시작하는 값(어댑터가 플래그로 읽을 수 있다)·너무 긴 값도
/// 거부. 순서는 칸 목록 순서로 맞춰 돌려준다.
pub fn validate_params(
    describe: &AdapterDescribe,
    values: &std::collections::BTreeMap<String, String>,
) -> Result<Vec<InboxParam>, String> {
    for flag in values.keys() {
        if !describe.params.iter().any(|p| &p.flag == flag) {
            return Err(format!("이 어댑터가 받지 않는 칸: {flag}"));
        }
    }
    let mut out = Vec::new();
    for p in &describe.params {
        let value = values.get(&p.flag).map(|v| v.trim()).unwrap_or_default();
        if value.is_empty() {
            if p.required {
                return Err(format!("{} 칸이 비었다", p.label));
            }
            continue;
        }
        if value.chars().any(char::is_control) {
            return Err(format!("{} 칸에 줄바꿈·제어문자가 있다", p.label));
        }
        if value.starts_with('-') {
            return Err(format!("{} 칸은 - 로 시작할 수 없다", p.label));
        }
        if value.chars().count() > INBOX_PARAM_MAX {
            return Err(format!("{} 칸이 {INBOX_PARAM_MAX}자를 넘는다", p.label));
        }
        out.push(InboxParam {
            flag: p.flag.clone(),
            value: value.to_string(),
        });
    }
    Ok(out)
}

/// 어댑터 명령 + 보드 값 → 실행할 argv. 셸을 거치지 않으므로 값은 인자 하나로 그대로 간다.
pub fn source_argv(command: &[String], params: &[InboxParam]) -> Vec<String> {
    let mut argv = command.to_vec();
    for p in params {
        argv.push(p.flag.clone());
        argv.push(p.value.clone());
    }
    argv
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
            // 어댑터는 모른다 — 데몬이 `mark_promoted` 로 채운다.
            promoted: false,
        });
    }
    Ok(items)
}

/// 실패한 소스의 결과 — 실행기와 라우트가 같은 모양을 쓰게 한 곳에 둔다.
pub fn unavailable(name: &str, reason: impl Into<String>, fetched_at: String) -> InboxSourceResult {
    InboxSourceResult {
        name: name.to_string(),
        board: None,
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
