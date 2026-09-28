//! PR 감시의 순수 판정 — 데몬이 `gh api graphql` 로 받은 JSON 을 스냅숏으로 바꾸고, 직전
//! 스냅숏과 견줘 **사람이 움직여야 하는 전이**만 뽑는다. 설계는
//! `docs/design/specs/2026-09-28-pr-watch-design.md`. 세션 스크립트 `pr-threads.ts` 의
//! `readyVerdict`/`transitionsBetween` 과 같은 규칙의 Rust 판이다.
//!
//! DB·HTTP·프로세스를 모른다 — 스토어가 스냅숏을 기억하고, 데몬이 쿼리와 알림을 한다.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// PR 한 건에서 읽는 필드 — 목록 쿼리와 낱개 쿼리가 같은 조각을 쓴다.
const PR_FIELDS: &str = r#"fragment prFields on PullRequest {
  number title url state isDraft headRefOid mergeStateStatus baseRefName updatedAt
  commits(last:1) { nodes { commit { statusCheckRollup { state } } } }
  reviewThreads(first:100) { nodes { isResolved
    comments(first:1) { nodes { reactions(first:30) { nodes { content user { login } } } } } } }
}"#;

/// 데몬이 보내는 GraphQL 쿼리 — 레포당 한 번. **열린 PR 은 전부**(최대 100), 닫힌 것은 최근
/// 갱신 30건만. 열린 것을 창으로 자르면 다른 PR 갱신에 밀린 열린 PR 이 영영 OPEN 으로 남는다.
/// 여기 두는 이유는 `parse_pull_requests` 가 읽는 모양과 한 파일에서 맞추기 위해서다
/// (변수: `owner`, `name`).
pub const PR_QUERY: &str = r#"query($owner:String!, $name:String!) {
  viewer { login }
  repository(owner:$owner, name:$name) {
    defaultBranchRef { name }
    open: pullRequests(first:100, states:[OPEN], orderBy:{field:UPDATED_AT, direction:ASC}) { nodes { ...prFields } }
    recent: pullRequests(last:30, states:[MERGED, CLOSED], orderBy:{field:UPDATED_AT, direction:ASC}) { nodes { ...prFields } }
  }
}
"#;

/// 낱개 쿼리 — 직전엔 OPEN 이었는데 목록에 없는 PR(닫힌 창 밖으로 밀린 것)의 지금 상태.
/// 변수: `owner`, `name`, `number`.
pub const PR_ONE_QUERY: &str = r#"query($owner:String!, $name:String!, $number:Int!) {
  viewer { login }
  repository(owner:$owner, name:$name) {
    defaultBranchRef { name }
    pullRequest(number:$number) { ...prFields }
  }
}
"#;

/// 쿼리 문자열 + 조각 — `gh api graphql -f query=` 에 그대로 준다.
pub fn list_query() -> String {
    format!("{PR_QUERY}\n{PR_FIELDS}")
}

pub fn one_query() -> String {
    format!("{PR_ONE_QUERY}\n{PR_FIELDS}")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CiState {
    Pass,
    Fail,
    Pending,
}

impl CiState {
    pub fn as_str(self) -> &'static str {
        match self {
            CiState::Pass => "pass",
            CiState::Fail => "fail",
            CiState::Pending => "pending",
        }
    }

    pub fn parse(s: &str) -> CiState {
        match s {
            "fail" => CiState::Fail,
            "pending" => CiState::Pending,
            _ => CiState::Pass,
        }
    }
}

/// PR 하나의 마지막으로 본 모습. `ready` 는 파생값이지만 저장한다 — 전이 판정이 직전 값을 본다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrSnapshot {
    pub repo: String,
    pub number: i64,
    pub title: String,
    pub url: String,
    /// OPEN / MERGED / CLOSED
    pub state: String,
    pub is_draft: bool,
    pub base: String,
    pub head: String,
    /// GitHub 의 `mergeStateStatus` — DIRTY 가 충돌.
    pub merge_state: String,
    pub ci: CiState,
    /// 미해결 스레드 중 👀 도 🚀 도 없는 것.
    pub unhandled: i64,
    /// 🚀(호출자 결정 필요).
    pub rocket: i64,
    pub ready: bool,
    pub updated_at: String,
}

/// "확인·머지해도 되는가" — 열려 있고, draft 가 아니고, 기본 브랜치를 향하고(스택의 맨 아래),
/// 충돌이 없고, CI 가 통과했고, 처리 안 된 스레드도 결정 필요한 스레드도 없다. 열린 스레드 수
/// 자체는 조건이 아니다(닫는 건 사람 몫). 사람 승인은 조건이 아니다(1인 레포).
#[allow(clippy::too_many_arguments)]
pub fn is_ready(
    state: &str,
    is_draft: bool,
    base: &str,
    default_branch: &str,
    merge_state: &str,
    ci: CiState,
    unhandled: i64,
    rocket: i64,
) -> bool {
    state == "OPEN"
        && !is_draft
        && base == default_branch
        && merge_state != "DIRTY"
        && ci == CiState::Pass
        && unhandled == 0
        && rocket == 0
}

fn is_bot_or_state_reaction(content: &str) -> bool {
    content == "EYES" || content == "ROCKET"
}

/// `statusCheckRollup.state` → CI 상태. check 가 없으면(null) 통과로 본다 — 막을 근거가 없다.
fn ci_of(rollup: Option<&str>) -> CiState {
    match rollup {
        Some("FAILURE") | Some("ERROR") => CiState::Fail,
        Some("PENDING") | Some("EXPECTED") => CiState::Pending,
        _ => CiState::Pass,
    }
}

/// `PR_QUERY` 의 응답(`data` 아래)을 스냅숏 목록으로 — 열린 것 전부 + 최근 닫힌 것. 모양이
/// 다르면 빈 목록이 아니라 에러 — 조용히 "PR 없음" 이 되면 전이가 엉뚱하게 난다.
pub fn parse_pull_requests(
    data: &Value,
    repo: &str,
    default_branch: &str,
) -> Result<Vec<PrSnapshot>, String> {
    let viewer = viewer_of(data)?;
    let open = data
        .pointer("/repository/open/nodes")
        .and_then(Value::as_array)
        .ok_or("repository.open.nodes 없음")?;
    let recent = data
        .pointer("/repository/recent/nodes")
        .and_then(Value::as_array)
        .ok_or("repository.recent.nodes 없음")?;
    let mut out = Vec::with_capacity(open.len() + recent.len());
    for node in open.iter().chain(recent.iter()) {
        out.push(parse_pr_node(node, viewer, repo, default_branch)?);
    }
    Ok(out)
}

/// `PR_ONE_QUERY` 의 응답 — PR 하나. 없으면(`null`) `Ok(None)`.
pub fn parse_pull_request(
    data: &Value,
    repo: &str,
    default_branch: &str,
) -> Result<Option<PrSnapshot>, String> {
    let viewer = viewer_of(data)?;
    match data.pointer("/repository/pullRequest") {
        None | Some(Value::Null) => Ok(None),
        Some(node) => parse_pr_node(node, viewer, repo, default_branch).map(Some),
    }
}

fn viewer_of(data: &Value) -> Result<&str, String> {
    data.pointer("/viewer/login")
        .and_then(Value::as_str)
        .ok_or_else(|| "viewer.login 없음".to_string())
}

fn parse_pr_node(
    node: &Value,
    viewer: &str,
    repo: &str,
    default_branch: &str,
) -> Result<PrSnapshot, String> {
    let str_of = |key: &str| {
        node.get(key)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    let number = node
        .get("number")
        .and_then(Value::as_i64)
        .ok_or("number 없음")?;
    let is_draft = node
        .get("isDraft")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let ci = ci_of(
        node.pointer("/commits/nodes/0/commit/statusCheckRollup/state")
            .and_then(Value::as_str),
    );
    let mut unhandled = 0;
    let mut rocket = 0;
    if let Some(threads) = node
        .pointer("/reviewThreads/nodes")
        .and_then(Value::as_array)
    {
        for thread in threads {
            if thread.get("isResolved").and_then(Value::as_bool) == Some(true) {
                continue;
            }
            let mine: Vec<&str> = thread
                .pointer("/comments/nodes/0/reactions/nodes")
                .and_then(Value::as_array)
                .map(|rs| {
                    rs.iter()
                        .filter(|r| {
                            r.pointer("/user/login").and_then(Value::as_str) == Some(viewer)
                        })
                        .filter_map(|r| r.get("content").and_then(Value::as_str))
                        .filter(|c| is_bot_or_state_reaction(c))
                        .collect()
                })
                .unwrap_or_default();
            if mine.contains(&"ROCKET") {
                rocket += 1;
            } else if !mine.contains(&"EYES") {
                unhandled += 1;
            }
        }
    }
    let state = str_of("state");
    let base = str_of("baseRefName");
    let merge_state = str_of("mergeStateStatus");
    let ready = is_ready(
        &state,
        is_draft,
        &base,
        default_branch,
        &merge_state,
        ci,
        unhandled,
        rocket,
    );
    Ok(PrSnapshot {
        repo: repo.to_string(),
        number,
        title: str_of("title"),
        url: str_of("url"),
        state,
        is_draft,
        base,
        head: str_of("headRefOid").chars().take(7).collect(),
        merge_state,
        ci,
        unhandled,
        rocket,
        ready,
        updated_at: str_of("updatedAt"),
    })
}

/// 사람이 움직여야 하는 전이. `Opened` 는 기록용(알리지 않는다), `Unready` 도 기록용.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PrEventKind {
    Opened,
    Ready,
    Unready,
    Conflict,
    Merged,
    Closed,
}

impl PrEventKind {
    /// 히스토리 action 이름 — `notify-todo` 훅과 웹이 이 접두사(`pr-`)로 골라낸다.
    pub fn action(self) -> &'static str {
        match self {
            PrEventKind::Opened => "pr-opened",
            PrEventKind::Ready => "pr-ready",
            PrEventKind::Unready => "pr-unready",
            PrEventKind::Conflict => "pr-conflict",
            PrEventKind::Merged => "pr-merged",
            PrEventKind::Closed => "pr-closed",
        }
    }

    /// 사람에게 알릴 것 — 확인이 필요하거나(ready) 손이 필요한(conflict) 것만. 머지는 대개
    /// 오너 자신이 한 일이다.
    pub fn notifies(self) -> bool {
        matches!(self, PrEventKind::Ready | PrEventKind::Conflict)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrEvent {
    pub kind: PrEventKind,
    pub repo: String,
    pub number: i64,
    pub title: String,
    pub url: String,
}

/// 직전 스냅숏(레포 단위)과 새 스냅숏의 차이. 직전에 없던 PR 은 `Opened`(열려 있을 때만) 과,
/// 이미 ready 면 `Ready` 도 함께. 새 목록에서 빠진 옛 PR(창 밖으로 밀림)은 건드리지 않는다.
pub fn diff(prev: &[PrSnapshot], cur: &[PrSnapshot]) -> Vec<PrEvent> {
    let mut out = Vec::new();
    for p in cur {
        let ev = |kind| PrEvent {
            kind,
            repo: p.repo.clone(),
            number: p.number,
            title: p.title.clone(),
            url: p.url.clone(),
        };
        let was = prev
            .iter()
            .find(|q| q.repo == p.repo && q.number == p.number);
        let Some(was) = was else {
            if p.state == "OPEN" {
                out.push(ev(PrEventKind::Opened));
                if p.ready {
                    out.push(ev(PrEventKind::Ready));
                }
            }
            continue;
        };
        if p.state != was.state {
            match p.state.as_str() {
                "MERGED" => out.push(ev(PrEventKind::Merged)),
                "CLOSED" => out.push(ev(PrEventKind::Closed)),
                _ => {}
            }
            continue;
        }
        if p.state != "OPEN" {
            continue;
        }
        if p.ready && !was.ready {
            out.push(ev(PrEventKind::Ready));
        } else if !p.ready && was.ready {
            out.push(ev(PrEventKind::Unready));
        }
        if p.merge_state == "DIRTY" && was.merge_state != "DIRTY" {
            out.push(ev(PrEventKind::Conflict));
        }
    }
    out
}

/// macOS 알림의 (제목, 본문).
pub fn notification_text(event: &PrEvent) -> (String, String) {
    let title = format!("rocky · {}", event.repo);
    let body = match event.kind {
        PrEventKind::Ready => format!("#{} 확인·머지해도 된다 — {}", event.number, event.title),
        PrEventKind::Conflict => format!("#{} 충돌 — {}", event.number, event.title),
        PrEventKind::Merged => format!("#{} 머지됨 — {}", event.number, event.title),
        PrEventKind::Closed => format!("#{} 닫힘 — {}", event.number, event.title),
        PrEventKind::Opened => format!("#{} 열림 — {}", event.number, event.title),
        PrEventKind::Unready => format!("#{} 다시 대기 — {}", event.number, event.title),
    };
    (title, body)
}

/// 히스토리 `changes` 에 싣는 모양 — 훅·웹이 읽는다.
pub fn event_changes(event: &PrEvent) -> serde_json::Map<String, Value> {
    let mut m = serde_json::Map::new();
    m.insert("number".into(), Value::from(event.number));
    m.insert("title".into(), Value::from(event.title.clone()));
    m.insert("url".into(), Value::from(event.url.clone()));
    m.insert("repo".into(), Value::from(event.repo.clone()));
    m
}

/// `osascript` 인자 — 따옴표를 이스케이프해 AppleScript 문자열 안에 넣는다.
pub fn osascript_args(title: &str, body: &str) -> Vec<String> {
    let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    vec![
        "osascript".into(),
        "-e".into(),
        format!(
            "display notification \"{}\" with title \"{}\"",
            esc(body),
            esc(title)
        ),
    ]
}
