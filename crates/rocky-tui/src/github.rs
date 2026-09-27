//! 항목에 물린 GitHub 이슈·PR 의 상태 한 줄 — GraphQL 한 요청으로 링크 전부를 묶어 읽는다.
//!
//! 예전에는 링크마다 `gh pr view` 프로세스를 띄웠다(보드 rocky-21). 지금은 토큰만 `gh auth token`
//! 으로 한 번 받아 **메모리에만** 두고, `api.github.com/graphql` 에 직접 묻는다 — 링크가 N 개여도
//! 요청 하나. 토큰은 Authorization 헤더로만 나가고 로그·argv·에러 메시지에 절대 싣지 않는다.
//! 데몬은 관여하지 않는다(로컬 인증, 읽기). 순수 부분(URL 판별·쿼리 조립·응답 → 한 줄)은
//! 여기서 테스트하고, 실행은 별도 스레드에서 돌려 화면을 막지 않는다. 실패하면 줄만 비운다.

use std::process::Command;
use std::sync::mpsc::Sender;
use std::sync::OnceLock;
use std::time::Duration;

use serde_json::{json, Value};

use crate::events::Event;

/// 캐시 수명 — 상세 패널을 오갈 때마다 API 를 때리지 않게.
pub const GH_CACHE_TTL: Duration = Duration::from_secs(300);

const GRAPHQL: &str = "https://api.github.com/graphql";
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GhKind {
    Issue,
    Pull,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GhRef {
    pub owner: String,
    pub repo: String,
    pub kind: GhKind,
    pub number: u64,
}

/// GitHub 이름 규칙 — owner/repo 는 `[A-Za-z0-9._-]`. 그 밖의 문자는 URL 로 안 받는다
/// (GraphQL 문자열에 그대로 들어가므로 여기서 걸러야 인용부호 주입이 없다).
fn is_gh_name(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// `https://github.com/<owner>/<repo>/(issues|pull)/<n>` 만 받는다. 그 밖(코드 링크·비교 링크)은 None.
pub fn github_ref(url: &str) -> Option<GhRef> {
    let rest = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("http://github.com/"))?;
    let mut parts = rest.split('/');
    let owner = parts.next().filter(|s| is_gh_name(s))?;
    let repo = parts.next().filter(|s| is_gh_name(s))?;
    let kind = match parts.next()? {
        "issues" => GhKind::Issue,
        "pull" => GhKind::Pull,
        _ => return None,
    };
    let number = parts
        .next()?
        .split(['#', '?'])
        .next()?
        .parse::<u64>()
        .ok()?;
    Some(GhRef {
        owner: owner.to_string(),
        repo: repo.to_string(),
        kind,
        number,
    })
}

fn checks_summary(rollup: Option<&Value>) -> Option<&'static str> {
    let checks = rollup?.as_array()?;
    if checks.is_empty() {
        return None;
    }
    let mut pending = false;
    for check in checks {
        // check-run 은 status/conclusion, commit-status·rollup 은 state 로 온다.
        let status = check
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("COMPLETED");
        let conclusion = check
            .get("conclusion")
            .and_then(Value::as_str)
            .or_else(|| check.get("state").and_then(Value::as_str))
            .unwrap_or("");
        if status != "COMPLETED"
            || matches!(
                conclusion,
                "PENDING" | "IN_PROGRESS" | "EXPECTED" | "QUEUED"
            )
        {
            pending = true;
            continue;
        }
        if matches!(
            conclusion,
            "FAILURE" | "ERROR" | "TIMED_OUT" | "CANCELLED" | "ACTION_REQUIRED"
        ) {
            return Some("CI ✗");
        }
    }
    Some(if pending { "CI …" } else { "CI ✓" })
}

/// `{state, isDraft, reviewDecision, statusCheckRollup: [...]}` → 한 줄.
pub fn summarize_pr(number: u64, json: &Value) -> String {
    let state = json.get("state").and_then(Value::as_str).unwrap_or("?");
    let mut parts: Vec<String> = vec![format!("PR #{number}")];
    match state {
        "MERGED" => {
            parts.push("merged".into());
            return parts.join(" · ");
        }
        "CLOSED" => {
            parts.push("closed".into());
            return parts.join(" · ");
        }
        _ => parts.push("open".into()),
    }
    if json.get("isDraft").and_then(Value::as_bool) == Some(true) {
        parts.push("draft".into());
    }
    if let Some(ci) = checks_summary(json.get("statusCheckRollup")) {
        parts.push(ci.into());
    }
    let review = match json.get("reviewDecision").and_then(Value::as_str) {
        Some("APPROVED") => "승인",
        Some("CHANGES_REQUESTED") => "변경 요청",
        _ => "리뷰 대기",
    };
    parts.push(review.into());
    parts.join(" · ")
}

/// `{state}` → 한 줄.
pub fn summarize_issue(number: u64, json: &Value) -> String {
    let state = json
        .get("state")
        .and_then(Value::as_str)
        .map(str::to_lowercase)
        .unwrap_or_else(|| "?".into());
    format!("이슈 #{number} · {state}")
}

fn repos_of(refs: &[GhRef]) -> Vec<(&str, &str)> {
    let mut repos: Vec<(&str, &str)> = Vec::new();
    for r in refs {
        if !repos.iter().any(|(o, n)| *o == r.owner && *n == r.repo) {
            repos.push((&r.owner, &r.repo));
        }
    }
    repos
}

/// 링크 여러 개를 GraphQL 한 요청으로. 레포마다 `r<i>` 별칭, 항목마다 `n<번호>` 별칭.
/// `issueOrPullRequest` 라 이슈/PR 을 미리 가르지 않아도 된다(URL 의 kind 는 표시용).
pub fn build_query(refs: &[GhRef]) -> String {
    let mut q = String::from("query {");
    for (i, (owner, repo)) in repos_of(refs).iter().enumerate() {
        q.push_str(&format!(
            " r{i}: repository(owner: \"{owner}\", name: \"{repo}\") {{"
        ));
        let mut numbers: Vec<u64> = refs
            .iter()
            .filter(|r| r.owner == *owner && r.repo == *repo)
            .map(|r| r.number)
            .collect();
        numbers.sort_unstable();
        numbers.dedup();
        for n in numbers {
            q.push_str(&format!(
                " n{n}: issueOrPullRequest(number: {n}) {{ __typename \
                 ... on PullRequest {{ state isDraft reviewDecision \
                 commits(last: 1) {{ nodes {{ commit {{ statusCheckRollup {{ state }} }} }} }} }} \
                 ... on Issue {{ state }} }}"
            ));
        }
        q.push_str(" }");
    }
    q.push_str(" }");
    q
}

/// 응답 `data` 를 링크별 한 줄로. 못 찾은 항목(권한 없음·삭제)은 None — 줄을 비운다.
pub fn parse_batch(refs: &[GhRef], data: &Value) -> Vec<(GhRef, Option<String>)> {
    let repos = repos_of(refs);
    refs.iter()
        .map(|r| {
            let ri = repos
                .iter()
                .position(|(o, n)| *o == r.owner && *n == r.repo)
                .unwrap_or(0);
            let node = data
                .get(format!("r{ri}"))
                .and_then(|repo| repo.get(format!("n{}", r.number)));
            let summary = node.filter(|n| !n.is_null()).map(|node| {
                let typename = node.get("__typename").and_then(Value::as_str);
                if typename == Some("PullRequest") {
                    // rollup 은 커밋 하나의 state 하나 — checks_summary 가 읽는 모양으로 감싼다.
                    let rollup = node
                        .pointer("/commits/nodes/0/commit/statusCheckRollup/state")
                        .and_then(Value::as_str)
                        .map(|s| json!([{ "state": s }]))
                        .unwrap_or_else(|| json!([]));
                    summarize_pr(
                        r.number,
                        &json!({
                            "state": node.get("state"),
                            "isDraft": node.get("isDraft"),
                            "reviewDecision": node.get("reviewDecision"),
                            "statusCheckRollup": rollup,
                        }),
                    )
                } else {
                    summarize_issue(r.number, &json!({ "state": node.get("state") }))
                }
            });
            (r.clone(), summary)
        })
        .collect()
}

/// `gh auth token` 한 번 — 프로세스 수명 동안 메모리에만. 없으면 None(상태 줄을 비운다).
/// 호스트를 github.com 으로 고정한다 — 요청이 `api.github.com` 으로만 가므로, `GH_HOST` 가
/// 다른 호스트(GHES)를 가리켜도 그쪽 토큰이 github.com 에 실려 나가지 않게.
fn token() -> Option<&'static str> {
    static TOKEN: OnceLock<Option<String>> = OnceLock::new();
    TOKEN
        .get_or_init(|| {
            let out = Command::new("gh")
                .args(["auth", "token", "--hostname", "github.com"])
                .output()
                .ok()?;
            if !out.status.success() {
                return None;
            }
            let t = String::from_utf8_lossy(&out.stdout).trim().to_string();
            (!t.is_empty()).then_some(t)
        })
        .as_deref()
}

/// 링크들을 한 요청으로 조회한다 — 호출자는 스레드에서 돌린다. 요청 자체가 실패하면 전부 None.
pub fn fetch_batch(urls: &[String]) -> Vec<(String, Option<String>)> {
    let pairs: Vec<(String, GhRef)> = urls
        .iter()
        .filter_map(|u| github_ref(u).map(|r| (u.clone(), r)))
        .collect();
    if pairs.is_empty() {
        return Vec::new();
    }
    let refs: Vec<GhRef> = pairs.iter().map(|(_, r)| r.clone()).collect();
    let none = || pairs.iter().map(|(u, _)| (u.clone(), None)).collect();
    let Some(token) = token() else {
        return none();
    };
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(HTTP_TIMEOUT))
        .http_status_as_error(false)
        .build()
        .into();
    let body = json!({ "query": build_query(&refs) }).to_string();
    let Ok(mut response) = agent
        .post(GRAPHQL)
        .header("Authorization", &format!("Bearer {token}"))
        .header("User-Agent", "rocky-tui")
        .header("Content-Type", "application/json")
        .send(body.as_bytes())
    else {
        return none();
    };
    if !response.status().is_success() {
        return none();
    }
    let Ok(text) = response.body_mut().read_to_string() else {
        return none();
    };
    let Ok(parsed) = serde_json::from_str::<Value>(&text) else {
        return none();
    };
    let Some(data) = parsed.get("data") else {
        return none();
    };
    let summaries = parse_batch(&refs, data);
    pairs
        .into_iter()
        .zip(summaries)
        .map(|((u, _), (_, s))| (u, s))
        .collect()
}

/// 스레드에서 `fetch_batch` 를 돌리고 링크마다 결과를 이벤트 채널로 보낸다. 수신 쪽이 사라지면 조용히 끝난다.
pub fn spawn_fetch_batch(urls: Vec<String>, tx: Sender<Event>) {
    std::thread::Builder::new()
        .name("rocky-tui-gh".into())
        .spawn(move || {
            for (url, summary) in fetch_batch(&urls) {
                if tx.send(Event::Gh(url, summary)).is_err() {
                    return;
                }
            }
        })
        .expect("gh 조회 스레드 생성");
}
