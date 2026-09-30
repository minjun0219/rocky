//! PR 감시의 순수 판정 — 데몬이 `gh api graphql` 로 받은 JSON 을 스냅숏으로 바꾸고, 직전
//! 스냅숏과 견줘 **사람이 움직여야 하는 전이**만 뽑는다. 설계는
//! `docs/design/specs/2026-09-28-pr-watch-design.md`. 세션 스크립트 `pr-threads.ts` 의
//! `readyVerdict`/`transitionsBetween` 과 같은 규칙의 Rust 판이다.
//!
//! DB·HTTP·프로세스를 모른다 — 스토어가 스냅숏을 기억하고, 데몬이 쿼리와 알림을 한다.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 열린 PR 한 건의 판정 재료 — CI 와 미해결 스레드. **상세 쿼리에서만** 쓴다.
///
/// **쿼리 비용은 실제 노드가 아니라 `first:` 로 요청한 노드 수로 매겨진다**(GraphQL 시간당
/// 5,000 포인트; 2026-09-28 실측 — 열린 PR 이 0개인 레포에서도 `first:100` × 스레드 100 ×
/// 리액션 30 이 263 포인트였다). 첫 판은 그걸 3분 × 레포 10개로 돌려 두 tick 만에 한도가
/// 바닥났고, 한도는 계정 단위라 **세션·터미널의 `gh` 까지 막혔다**. 그래서 (1) 목록은 상태
/// 조각만 싸게 받고, (2) 이 조각은 **실제로 열린 PR** 에만 별칭 배치로 묻고(비용이 현실의
/// 열린 PR 수에 비례), (3) 리액션은 노드 대신 `viewerHasReacted` 만 묻는다.
const PR_FIELDS: &str = r#"fragment prFields on PullRequest {
  ...prState
  commits(last:1) { nodes { commit { statusCheckRollup { state } } } }
  reviewThreads(first:50) { pageInfo { hasNextPage } nodes { id isResolved
    comments(first:1) { nodes {
      eyes: reactions(first:1, content:EYES) { viewerHasReacted }
      rocket: reactions(first:1, content:ROCKET) { viewerHasReacted } } } } }
}"#;

/// 상태 조각 — 목록 쿼리는 이것만 받는다(열린 것도). 닫힌 PR 은 이걸로 스냅숏이 완성된다
/// (`is_ready` 가 OPEN 을 요구하므로 스레드·CI 가 필요 없다).
const PR_STATE_FIELDS: &str = r#"fragment prState on PullRequest {
  number title url state isDraft headRefOid mergeStateStatus baseRefName updatedAt author { login }
}"#;

/// 응답마다 실어 오는 잔여 예산 — 데몬이 tick 을 쉴지 정하는 재료.
const RATE_LIMIT_FIELDS: &str = "rateLimit { cost remaining resetAt }";

/// 목록 쿼리 — 레포당 한 번, **상태만**. 열린 PR 은 최근 갱신 순 50건(창을 넘친 옛 열린 PR 은
/// 직전 스냅숏에서 알아 상세 쿼리에 끼운다), 닫힌 것은 최근 갱신 30건. 여기 두는 이유는
/// `parse_pr_list` 가 읽는 모양과 한 파일에서 맞추기 위해서다(변수: `owner`, `name`).
pub const PR_LIST_QUERY: &str = r#"query($owner:String!, $name:String!) {
  rateLimit { cost remaining resetAt }
  viewer { login }
  repository(owner:$owner, name:$name) {
    defaultBranchRef { name }
    open: pullRequests(first:50, states:[OPEN], orderBy:{field:UPDATED_AT, direction:DESC}) { pageInfo { hasNextPage } nodes { ...prState } }
    recent: pullRequests(last:30, states:[MERGED, CLOSED], orderBy:{field:UPDATED_AT, direction:ASC}) { nodes { ...prState } }
  }
}
"#;

/// 목록 쿼리 문자열 + 조각 — `gh api graphql -f query=` 에 그대로 준다.
pub fn list_query() -> String {
    debug_assert!(PR_LIST_QUERY.contains(RATE_LIMIT_FIELDS));
    format!(
        "{PR_LIST_QUERY}
{PR_STATE_FIELDS}"
    )
}

/// 상세 쿼리 — 번호마다 `p<번호>: pullRequest(number:<번호>) { ...prFields }` 별칭으로 한 요청에
/// 묶는다(변수: `owner`, `name`). 번호가 없으면 부르지 않는다(`None`). 번호는 쿼리 문자열에
/// 박히지만 정수라 주입이 없다.
pub fn detail_query(numbers: &[i64]) -> Option<String> {
    if numbers.is_empty() {
        return None;
    }
    let fields: Vec<String> = numbers
        .iter()
        .map(|n| format!("    p{n}: pullRequest(number:{n}) {{ ...prFields }}"))
        .collect();
    Some(format!(
        "query($owner:String!, $name:String!) {{\n  {RATE_LIMIT_FIELDS}\n  repository(owner:$owner, name:$name) {{\n    defaultBranchRef {{ name }}\n{}\n  }}\n}}\n{PR_FIELDS}\n{PR_STATE_FIELDS}",
        fields.join("\n")
    ))
}

/// 이 밑으로 남으면 데몬은 다음 리셋까지 쉰다 — 한도는 사용자 계정 하나에 걸리므로, 데몬이
/// 끝까지 쓰면 세션·터미널의 `gh` 가 죽는다. 5,000 의 1/5 을 남긴다.
pub const RATE_LIMIT_FLOOR: i64 = 1_000;

/// 리셋 시각을 모를 때 쉬는 길이 — GraphQL 한도 창(1시간)의 1/4.
pub const RATE_LIMIT_BLIND_PAUSE_SECS: u64 = 15 * 60;

/// `rateLimit { cost remaining resetAt }` — 응답 한 건의 예산 정보.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimit {
    /// 이 응답(또는 tick 누계)의 포인트.
    pub cost: i64,
    /// 응답 시점의 잔여 포인트.
    pub remaining: i64,
    /// 한도 창이 리셋되는 시각(ISO).
    pub reset_at: String,
}

impl RateLimit {
    /// 응답의 `rateLimit` 블록. 없으면 None — 판정은 못 하지만 스냅숏은 정상이다.
    pub fn of(data: &Value) -> Option<RateLimit> {
        let rl = data.get("rateLimit")?;
        Some(RateLimit {
            cost: rl.get("cost").and_then(Value::as_i64)?,
            remaining: rl.get("remaining").and_then(Value::as_i64)?,
            reset_at: rl.get("resetAt").and_then(Value::as_str)?.to_string(),
        })
    }

    /// 예산 바닥 — 이 tick 을 여기서 멈추고 리셋까지 쉰다.
    pub fn exhausted(&self) -> bool {
        self.remaining < RATE_LIMIT_FLOOR
    }

    /// `now` 부터 리셋까지 남은 시간. 리셋이 이미 지났거나 시각을 못 읽으면 None.
    pub fn until_reset(&self, now: chrono::DateTime<chrono::Utc>) -> Option<Duration> {
        let reset = chrono::DateTime::parse_from_rfc3339(&self.reset_at).ok()?;
        (reset.with_timezone(&chrono::Utc) - now).to_std().ok()
    }
}

/// `gh api graphql` 의 실패 출력이 레이트 리밋인가 — 본문의 `errors[].type` 이 `RATE_LIMIT`
/// 또는 `RATE_LIMITED`(GitHub 이 둘 다 낸다 — 2026-09-28 실측; gh 는 실패해도 응답 JSON 을
/// stdout 에 그대로 낸다), 없으면 stderr 문구로.
pub fn is_rate_limit_error(stdout: &str, stderr: &str) -> bool {
    if let Ok(body) = serde_json::from_str::<Value>(stdout) {
        if let Some(errors) = body.get("errors").and_then(Value::as_array) {
            if errors.iter().any(|e| {
                e.get("type")
                    .and_then(Value::as_str)
                    .is_some_and(|t| t.starts_with("RATE_LIMIT"))
            }) {
                return true;
            }
        }
    }
    let s = stderr.to_ascii_lowercase();
    s.contains("rate limit") || s.contains("ratelimit")
}

/// 한도에 걸렸거나 바닥이 보일 때 얼마나 쉴지 — 리셋을 알면 그때까지(+1분 여유), 모르면
/// 고정 길이. 상한은 한도 창(1시간) + 여유 — 시계가 어긋나도 영영 쉬지 않는다.
pub fn pause_for(limit: Option<&RateLimit>, now: chrono::DateTime<chrono::Utc>) -> Duration {
    let grace = Duration::from_secs(60);
    let cap = Duration::from_secs(60 * 60) + grace;
    match limit.and_then(|l| l.until_reset(now)) {
        Some(d) => (d + grace).min(cap),
        None => Duration::from_secs(RATE_LIMIT_BLIND_PAUSE_SECS),
    }
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
    /// 미해결 스레드 중 🚀 도 👀 도 없는 것.
    pub unhandled: i64,
    /// 그 스레드들의 id — 새 리뷰는 "수가 늘었다" 가 아니라 "처음 보는 id 가 있다" 로 가린다
    /// (하나를 🚀 로 처리하는 사이 새 스레드가 붙으면 수는 그대로다). 이 필드가 생기기 전의
    /// 스냅숏은 비어 있다(`serde(default)`).
    #[serde(default)]
    pub unhandled_ids: Vec<String>,
    /// 👀(오너 결정 필요) 가 달린 스레드 수. 옛 스냅숏의 이름은 `rocket` 이었다(의미가 뒤집히기 전).
    #[serde(alias = "rocket")]
    pub decision: i64,
    pub ready: bool,
    pub updated_at: String,
    /// PR 작성자 login — 보드의 `prAuthors` 필터가 본다. 이 필드 전의 스냅숏·삭제된 계정은 None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
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
    decision: i64,
) -> bool {
    state == "OPEN"
        && !is_draft
        && base == default_branch
        && merge_state != "DIRTY"
        && ci == CiState::Pass
        && unhandled == 0
        && decision == 0
}

/// `statusCheckRollup.state` → CI 상태. check 가 없으면(null) 통과로 본다 — 막을 근거가 없다.
fn ci_of(rollup: Option<&str>) -> CiState {
    match rollup {
        Some("FAILURE") | Some("ERROR") => CiState::Fail,
        Some("PENDING") | Some("EXPECTED") => CiState::Pending,
        _ => CiState::Pass,
    }
}

/// 응답의 기본 브랜치 이름 — 없으면 `main`.
pub fn default_branch_of(data: &Value) -> String {
    data.pointer("/repository/defaultBranchRef/name")
        .and_then(Value::as_str)
        .unwrap_or("main")
        .to_string()
}

/// 목록 쿼리의 응답(`data` 아래) — 열린 PR 의 번호와 닫힌 PR 의 완성된 스냅숏. 모양이 다르면
/// 빈 목록이 아니라 에러 — 조용히 "PR 없음" 이 되면 전이가 엉뚱하게 난다.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PrList {
    /// 열린 PR 번호(최근 갱신 순). 상세 쿼리의 대상.
    pub open: Vec<i64>,
    /// 열린 것이 창(50)을 넘쳤다 — 나머지는 직전 스냅숏의 열린 것에서 보충한다.
    pub open_truncated: bool,
    /// 최근 닫힌 PR — 상태만으로 스냅숏이 완성된다.
    pub closed: Vec<PrSnapshot>,
}

pub fn parse_pr_list(data: &Value, repo: &str, default_branch: &str) -> Result<PrList, String> {
    let open = data
        .pointer("/repository/open/nodes")
        .and_then(Value::as_array)
        .ok_or("repository.open.nodes 없음")?;
    let recent = data
        .pointer("/repository/recent/nodes")
        .and_then(Value::as_array)
        .ok_or("repository.recent.nodes 없음")?;
    let mut list = PrList {
        open_truncated: data
            .pointer("/repository/open/pageInfo/hasNextPage")
            .and_then(Value::as_bool)
            == Some(true),
        ..PrList::default()
    };
    for node in open {
        list.open.push(
            node.get("number")
                .and_then(Value::as_i64)
                .ok_or("number 없음")?,
        );
    }
    for node in recent {
        list.closed.push(parse_pr_node(node, repo, default_branch)?);
    }
    Ok(list)
}

/// 상세 쿼리의 응답 — `repository` 아래 `p<번호>` 별칭마다 스냅숏 하나. `null`(사라진 PR)은
/// 건너뛴다. `repository` 자체가 없으면 에러.
pub fn parse_pr_details(
    data: &Value,
    repo: &str,
    default_branch: &str,
) -> Result<Vec<PrSnapshot>, String> {
    let repository = data
        .get("repository")
        .and_then(Value::as_object)
        .ok_or("repository 없음")?;
    let mut out = Vec::new();
    for (key, node) in repository {
        if !key.starts_with('p') || key[1..].parse::<i64>().is_err() {
            continue;
        }
        if node.is_null() {
            continue;
        }
        out.push(parse_pr_node(node, repo, default_branch)?);
    }
    Ok(out)
}

/// 스레드 첫 코멘트에 내가 그 리액션을 달았는가 — `<alias>: reactions(content:…) { viewerHasReacted }`.
fn viewer_reacted(thread: &Value, alias: &str) -> bool {
    thread
        .pointer(&format!("/comments/nodes/0/{alias}/viewerHasReacted"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn parse_pr_node(node: &Value, repo: &str, default_branch: &str) -> Result<PrSnapshot, String> {
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
    let mut unhandled_ids = Vec::new();
    let mut decision = 0;
    // 스레드가 첫 페이지(50)를 넘치면 못 본 것이 있다 — 보수적으로 "처리 안 됨" 하나로 친다.
    // (이 레포에서 50개를 넘는 PR 은 없다; 넘치면 ready 알림이 안 나가는 쪽이 안전하다.)
    if node
        .pointer("/reviewThreads/pageInfo/hasNextPage")
        .and_then(Value::as_bool)
        == Some(true)
    {
        unhandled += 1;
    }
    if let Some(threads) = node
        .pointer("/reviewThreads/nodes")
        .and_then(Value::as_array)
    {
        for thread in threads {
            if thread.get("isResolved").and_then(Value::as_bool) == Some(true) {
                continue;
            }
            // 규약: 🚀 = 처리 완료(오너가 resolve 해도 된다), 👀 = 오너 결정 필요. 👍/👎 는 Codex 에
            // 주는 피드백이라 상태가 아니다.
            if viewer_reacted(thread, "eyes") {
                decision += 1;
            } else if !viewer_reacted(thread, "rocket") {
                unhandled += 1;
                if let Some(id) = thread.get("id").and_then(Value::as_str) {
                    unhandled_ids.push(id.to_string());
                }
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
        decision,
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
        unhandled_ids,
        decision,
        ready,
        updated_at: str_of("updatedAt"),
        author: node
            .pointer("/author/login")
            .and_then(Value::as_str)
            .map(str::to_string),
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
    /// 처리 안 된 리뷰 스레드가 늘었다 — 봇·사람 리뷰가 새로 붙었다. 사람에게는 알리지 않고(배너
    /// 없음), 그 레포 보드가 켰으면 세션에 리뷰 처리를 시킨다(보드의 `autoResolve`).
    Review,
    /// CI 가 실패로 바뀌었다(통과·진행 중 → 실패, 또는 새 head 가 실패). 같은 head 에서 실패가 이어지는
    /// 동안은 다시 내지 않는다 — 재실행이 다시 실패하면(진행 중을 거치므로) 또 낸다. 사람에게는 알리지
    /// 않고 세션에만 보낸다: 인프라 문제면 한 번 재실행, 코드 문제면 고쳐 푸시하는 건 세션 몫이다.
    CiFailed,
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
            PrEventKind::Review => "pr-review",
            PrEventKind::CiFailed => "pr-ci-failed",
        }
    }

    /// 사람에게 알릴 것 — 확인이 필요하거나(ready) 손이 필요한(conflict) 것만. 머지는 대개
    /// 오너 자신이 한 일이다.
    pub fn notifies(self) -> bool {
        matches!(self, PrEventKind::Ready | PrEventKind::Conflict)
    }

    /// 세션에 넘길 것 — 사람에게 알릴 것(ready·conflict)에 더해, 세션이 처리할 리뷰 도착과 머지 뒤
    /// 정리할 머지. 배너·브릿지는 `notifies` 만 쓴다(머지는 대개 오너가 한 일이라 다시 알리지 않는다).
    pub fn reaches_session(self) -> bool {
        self.notifies()
            || matches!(
                self,
                PrEventKind::Review | PrEventKind::Merged | PrEventKind::CiFailed
            )
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
    /// PR 작성자 login(모르면 None).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// 알리지 않는 전이 — 보드의 `prAuthors` 필터에 걸렸다. 히스토리·`rocky pr` 에는 남고 세션·배너·
    /// 브릿지·훅 주입은 건너뛴다("보기는 넓게, 깨우기는 좁게").
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub quiet: bool,
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
            author: p.author.clone(),
            quiet: false,
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
                // 처음 볼 때 이미 처리 안 된 스레드가 있으면 그것도 도착한 리뷰다.
                if p.unhandled > 0 {
                    out.push(ev(PrEventKind::Review));
                }
                if p.ci == CiState::Fail {
                    out.push(ev(PrEventKind::CiFailed));
                }
            }
            continue;
        };
        if p.state != was.state {
            match p.state.as_str() {
                "MERGED" => out.push(ev(PrEventKind::Merged)),
                "CLOSED" => out.push(ev(PrEventKind::Closed)),
                // 다시 열림 — 처음 보는 열린 PR 과 같이 대한다(ready 면 그것도, 충돌이면 그것도).
                "OPEN" => {
                    out.push(ev(PrEventKind::Opened));
                    if p.ready {
                        out.push(ev(PrEventKind::Ready));
                    }
                    if p.merge_state == "DIRTY" {
                        out.push(ev(PrEventKind::Conflict));
                    }
                    if p.unhandled > 0 {
                        out.push(ev(PrEventKind::Review));
                    }
                    if p.ci == CiState::Fail {
                        out.push(ev(PrEventKind::CiFailed));
                    }
                }
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
        if has_new_review(was, p) {
            out.push(ev(PrEventKind::Review));
        }
        if p.ci == CiState::Fail && (was.ci != CiState::Fail || was.head != p.head) {
            out.push(ev(PrEventKind::CiFailed));
        }
    }
    out
}

/// 리뷰가 새로 붙었나 — 처리 안 된 스레드 중 **직전에 없던 id** 가 있으면. 🚀 를 달면 목록에서
/// 빠지므로 처리 중인 스레드로는 안 난다. 직전 스냅숏에 id 가 없으면(필드 도입 전 저장분) 수로
/// 비교한다 — 업그레이드 첫 tick 에 이미 알던 스레드를 전부 "새 리뷰" 로 보지 않게.
fn has_new_review(was: &PrSnapshot, cur: &PrSnapshot) -> bool {
    if was.unhandled_ids.is_empty() && was.unhandled > 0 {
        return cur.unhandled > was.unhandled;
    }
    // 페이지를 넘친 몫(`hasNextPage`)은 id 없이 수에만 잡힌다 — 그 몫이 새로 생긴 것도 새 리뷰다.
    let overflow = |s: &PrSnapshot| s.unhandled - s.unhandled_ids.len() as i64;
    cur.unhandled_ids
        .iter()
        .any(|id| !was.unhandled_ids.contains(id))
        || overflow(cur) > overflow(was)
}

/// 이 레포의 리뷰를 세션에 처리시킬지 — 그 레포를 둔 보드 중 하나라도 `auto_resolve` 가 켜졌으면.
/// 레포 이름은 대소문자를 가리지 않는다(GitHub 과 같다).
pub fn auto_resolve_enabled(boards: &[crate::types::Board], repo: &str) -> bool {
    boards.iter().any(|b| {
        b.auto_resolve
            && b.repo
                .as_deref()
                .is_some_and(|r| r.eq_ignore_ascii_case(repo))
    })
}

/// macOS 알림의 (제목, 본문).
pub fn notification_text(event: &PrEvent) -> (String, String) {
    let title = format!("rocky · {}", event.repo);
    let body = match event.kind {
        PrEventKind::Ready => format!("#{} 머지 후보 — {}", event.number, event.title),
        PrEventKind::Conflict => format!("#{} 충돌 — {}", event.number, event.title),
        PrEventKind::Merged => format!("#{} 머지됨 — {}", event.number, event.title),
        PrEventKind::Closed => format!("#{} 닫힘 — {}", event.number, event.title),
        PrEventKind::Opened => format!("#{} 열림 — {}", event.number, event.title),
        PrEventKind::Unready => format!("#{} 다시 대기 — {}", event.number, event.title),
        PrEventKind::Review => format!("#{} 리뷰 도착 — {}", event.number, event.title),
        PrEventKind::CiFailed => format!("#{} CI 실패 — {}", event.number, event.title),
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
    if let Some(author) = &event.author {
        m.insert("author".into(), Value::from(author.clone()));
    }
    // 알리지 않는 전이 — 훅 주입·채널이 이 표시로 건너뛴다(기록은 남는다).
    if event.quiet {
        m.insert("quiet".into(), Value::from(true));
    }
    m
}

/// PR 작성자 필터 — `filter` 가 비면 전부 통과(하위 호환). `@me` 는 `viewer`(gh 로그인 계정)로 풀고,
/// 이름은 대소문자를 가리지 않는다(GitHub 과 같다). 작성자를 모르면(삭제된 계정 등) 필터가 있을 때 통과하지 않는다.
pub fn author_matches(filter: &[String], author: Option<&str>, viewer: Option<&str>) -> bool {
    if filter.is_empty() {
        return true;
    }
    let Some(author) = author else {
        return false;
    };
    filter.iter().any(|f| {
        let want = if f == "@me" {
            viewer
        } else {
            Some(f.trim_start_matches('@'))
        };
        want.is_some_and(|w| w.eq_ignore_ascii_case(author))
    })
}

/// 이 전이를 알릴지. `board_id` 가 있으면 그 보드의 필터만(세션 훅 주입·받은편지함 — 보드 속성), 없으면 그
/// 레포를 둔 보드 중 하나라도 통과시키면 알린다(배너·브릿지 — 보드와 무관한 경로). 레포 이름은 대소문자를
/// 가리지 않는다.
pub fn author_allowed(
    boards: &[crate::types::Board],
    repo: &str,
    board_id: Option<&str>,
    author: Option<&str>,
    viewer: Option<&str>,
) -> bool {
    if let Some(id) = board_id {
        return boards
            .iter()
            .find(|b| b.id == id)
            .is_none_or(|b| author_matches(&b.pr_authors, author, viewer));
    }
    let mut any = false;
    for b in boards.iter().filter(|b| {
        b.repo
            .as_deref()
            .is_some_and(|r| r.eq_ignore_ascii_case(repo))
    }) {
        any = true;
        if author_matches(&b.pr_authors, author, viewer) {
            return true;
        }
    }
    !any
}

/// 보드 `prAuthors` 값 하나가 쓸 수 있는 모양인가 — `@me` 또는 GitHub login(영숫자·`-`, 39자까지).
pub fn is_pr_author(value: &str) -> bool {
    let v = value.trim_start_matches('@');
    value == "@me"
        || (!v.is_empty()
            && v.len() <= 39
            && v.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'))
}

/// 알림 브릿지(`pr.notifiers[]`)의 stdin 에 주는 JSON — 전이 한 건. `heading`·`text` 는 macOS
/// 배너와 같은 문구라 브릿지가 그대로 보내도 되고, 나머지 필드로 직접 조립해도 된다.
pub fn bridge_payload(event: &PrEvent) -> Value {
    let (heading, text) = notification_text(event);
    serde_json::json!({
        "kind": event.kind.action().trim_start_matches("pr-"),
        "repo": event.repo,
        "number": event.number,
        "title": event.title,
        "url": event.url,
        "heading": heading,
        "text": text,
    })
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
