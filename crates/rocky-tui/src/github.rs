//! 항목에 물린 GitHub 이슈·PR 의 상태 한 줄 — `gh` CLI 를 읽기 전용으로 부른다.
//!
//! 데몬은 관여하지 않는다(로컬 인증, 읽기). 순수 부분(URL 판별·JSON → 한 줄)은 여기서
//! 테스트하고, 실행은 별도 스레드에서 돌려 화면을 막지 않는다. `gh` 가 없거나 실패하면
//! 그 줄만 비운다.

use std::process::Command;
use std::sync::mpsc::Sender;
use std::time::Duration;

use serde_json::Value;

use crate::events::Event;

/// 캐시 수명 — 상세 패널을 오갈 때마다 `gh` 를 때리지 않게.
pub const GH_CACHE_TTL: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GhKind {
    Issue,
    Pull,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GhRef {
    pub kind: GhKind,
    pub number: u64,
}

/// `https://github.com/<owner>/<repo>/(issues|pull)/<n>` 만 받는다. 그 밖(코드 링크·비교 링크)은 None.
pub fn github_ref(url: &str) -> Option<GhRef> {
    let rest = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("http://github.com/"))?;
    let mut parts = rest.split('/');
    let _owner = parts.next().filter(|s| !s.is_empty())?;
    let _repo = parts.next().filter(|s| !s.is_empty())?;
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
    Some(GhRef { kind, number })
}

fn checks_summary(rollup: Option<&Value>) -> Option<&'static str> {
    let checks = rollup?.as_array()?;
    if checks.is_empty() {
        return None;
    }
    let mut pending = false;
    for check in checks {
        // check-run 은 status/conclusion, commit-status 는 state 로 온다.
        let status = check
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("COMPLETED");
        let conclusion = check
            .get("conclusion")
            .and_then(Value::as_str)
            .or_else(|| check.get("state").and_then(Value::as_str))
            .unwrap_or("");
        if status != "COMPLETED" || conclusion == "PENDING" || conclusion == "IN_PROGRESS" {
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

/// `gh pr view --json state,isDraft,mergedAt,reviewDecision,statusCheckRollup` → 한 줄.
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

/// `gh issue view --json state` → 한 줄.
pub fn summarize_issue(number: u64, json: &Value) -> String {
    let state = json
        .get("state")
        .and_then(Value::as_str)
        .map(str::to_lowercase)
        .unwrap_or_else(|| "?".into());
    format!("이슈 #{number} · {state}")
}

/// `gh` 를 실제로 부른다 — 호출자는 스레드에서 돌린다. 실패는 None(줄을 비운다).
pub fn fetch(url: &str) -> Option<String> {
    let gh_ref = github_ref(url)?;
    let (sub, fields) = match gh_ref.kind {
        GhKind::Pull => (
            "pr",
            "state,isDraft,mergedAt,reviewDecision,statusCheckRollup",
        ),
        GhKind::Issue => ("issue", "state"),
    };
    let output = Command::new("gh")
        .args([sub, "view", url, "--json", fields])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let json: Value = serde_json::from_slice(&output.stdout).ok()?;
    Some(match gh_ref.kind {
        GhKind::Pull => summarize_pr(gh_ref.number, &json),
        GhKind::Issue => summarize_issue(gh_ref.number, &json),
    })
}

/// 스레드에서 `fetch` 를 돌리고 결과를 이벤트 채널로 보낸다. 수신 쪽이 사라지면 조용히 끝난다.
pub fn spawn_fetch(url: String, tx: Sender<Event>) {
    std::thread::Builder::new()
        .name("rocky-tui-gh".into())
        .spawn(move || {
            let summary = fetch(&url);
            let _ = tx.send(Event::Gh(url, summary));
        })
        .expect("gh 조회 스레드 생성");
}
