//! PR 감시 — 주기 잡. `repo` 가 설정된 보드의 레포마다 `gh api graphql` 한 번을 돌려
//! 스냅숏을 스토어에 넣고(전이는 거기서 히스토리로), 사람이 움직일 전이(ready·conflict)는
//! macOS 알림으로 쏜다. 판정은 전부 `rocky_core::prwatch`(순수), 여기는 배선이다.
//! 설계: `docs/design/specs/2026-09-28-pr-watch-design.md`.
//!
//! 읽기만 한다 — 쿼리뿐, GitHub 에 쓰는 것은 없다.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use rocky_core::prwatch::{
    default_branch_of, list_query, notification_text, one_query, osascript_args,
    parse_pull_request, parse_pull_requests, PrEvent, PrSnapshot,
};
use serde::Serialize;

use crate::runner::Runner;
use crate::server::ServerState;

/// 전이 히스토리의 actor.
pub const PR_WATCH_ACTOR: &str = "rocky";
/// `gh api graphql` 한 번의 상한.
const QUERY_TIMEOUT: Duration = Duration::from_secs(30);

/// (제목, 본문) → 사람에게. 기본은 osascript, 테스트는 붙잡는다.
pub type Notifier = Arc<dyn Fn(String, String) + Send + Sync>;

/// osascript 로 macOS 알림. 다른 OS 면 조용히 아무것도 안 한다.
pub fn osascript_notifier(runner: Runner) -> Notifier {
    Arc::new(move |title, body| {
        if !cfg!(target_os = "macos") {
            return;
        }
        let runner = runner.clone();
        tokio::spawn(async move {
            let _ = runner(
                osascript_args(&title, &body),
                String::new(),
                Duration::from_secs(10),
            )
            .await;
        });
    })
}

pub fn silent_notifier() -> Notifier {
    Arc::new(|_, _| {})
}

/// `/api/health` 의 `prWatch` — 핸드오프의 `sessions` 와 같은 모양(가능 여부 + 사유).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrWatchStatus {
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// 마지막으로 돈 시각(ISO). 아직이면 None.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_tick: Option<String>,
    /// 보고 있는 레포들.
    pub repos: Vec<String>,
}

/// 보드에 설정된 레포 목록 — 중복 없이, 정렬.
fn watched_repos(state: &ServerState) -> Vec<String> {
    state
        .store
        .list_boards(false)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|b| b.repo)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// `gh api graphql` 한 번 — `data` 를 돌려준다.
async fn graphql(
    runner: &Runner,
    query: String,
    fields: &[(String, String)],
) -> Result<serde_json::Value, String> {
    let mut cmd = vec![
        "gh".to_string(),
        "api".into(),
        "graphql".into(),
        "-f".into(),
        format!("query={query}"),
    ];
    for (k, v) in fields {
        cmd.push("-F".into());
        cmd.push(format!("{k}={v}"));
    }
    let out = runner(cmd, String::new(), QUERY_TIMEOUT).await;
    if !out.ok() {
        return Err(format!(
            "gh api graphql 실패(exit {}): {}",
            out.code,
            out.stderr.trim()
        ));
    }
    let body: serde_json::Value =
        serde_json::from_str(&out.stdout).map_err(|e| format!("응답이 JSON 이 아니다: {e}"))?;
    body.get("data")
        .cloned()
        .ok_or_else(|| "응답에 data 가 없다".to_string())
}

/// 한 레포 한 번 — 쿼리 → 파싱 → (창 밖으로 밀린 옛 열린 PR 은 낱개로) → 스토어 → 알림.
/// 실패는 사유 문자열.
async fn tick_repo(
    state: &Arc<ServerState>,
    runner: &Runner,
    notifier: &Notifier,
    notify: bool,
    repo: &str,
) -> Result<Vec<PrEvent>, String> {
    let Some((owner, name)) = repo.split_once('/') else {
        return Err(format!("repo 표기가 owner/name 이 아니다: {repo}"));
    };
    let repo_vars = vec![
        ("owner".to_string(), owner.to_string()),
        ("name".to_string(), name.to_string()),
    ];
    let data = graphql(runner, list_query(), &repo_vars).await?;
    let default_branch = default_branch_of(&data);
    let mut snapshots = parse_pull_requests(&data, repo, &default_branch)?;
    // 직전엔 OPEN 이었는데 이번 목록(열린 것 전부 + 최근 닫힌 30건)에 없는 PR — 닫힌 지 오래돼
    // 창 밖으로 밀린 것이다. 낱개로 물어 전이(merged/closed)를 잃지 않는다.
    let prev_open: Vec<PrSnapshot> = state
        .store
        .list_prs(Some(repo), true)
        .map_err(|e| e.to_string())?;
    let missing: Vec<i64> = prev_open
        .iter()
        .filter(|p| !snapshots.iter().any(|s| s.number == p.number))
        .map(|p| p.number)
        .collect();
    for number in missing {
        let mut vars = repo_vars.clone();
        vars.push(("number".to_string(), number.to_string()));
        let one = graphql(runner, one_query(), &vars).await?;
        if let Some(snap) = parse_pull_request(&one, repo, &default_branch)? {
            snapshots.push(snap);
        }
    }
    let events = state
        .store
        .apply_pr_snapshot(repo, &snapshots, PR_WATCH_ACTOR)
        .map_err(|e| e.to_string())?;
    if notify {
        for event in events.iter().filter(|e| e.kind.notifies()) {
            let (title, body) = notification_text(event);
            notifier(title, body);
        }
    }
    Ok(events)
}

/// 한 번 훑는다 — 레포마다 독립(하나가 실패해도 나머지는 돈다). 더는 보지 않는 레포의 스냅숏은
/// 걷는다. 결과는 health 에 반영.
pub async fn tick(
    state: &Arc<ServerState>,
    runner: &Runner,
    notifier: &Notifier,
    notify: bool,
) -> Vec<PrEvent> {
    let repos = watched_repos(state);
    let _ = state.store.retain_pr_repos(&repos);
    let mut events = Vec::new();
    let mut failures = Vec::new();
    for repo in &repos {
        match tick_repo(state, runner, notifier, notify, repo).await {
            Ok(mut ev) => events.append(&mut ev),
            Err(reason) => failures.push(format!("{repo}: {reason}")),
        }
    }
    state.set_pr_watch(PrWatchStatus {
        available: failures.is_empty(),
        reason: (!failures.is_empty()).then(|| failures.join("; ")),
        last_tick: Some(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
        repos,
    });
    events
}

/// 기동 뒤 `first_after` 지나 처음, 이후 `every` 마다. 레포가 없으면 조용히 돈다.
pub fn spawn_pr_watcher(
    state: Arc<ServerState>,
    runner: Runner,
    notifier: Notifier,
    notify: bool,
    first_after: Duration,
    every: Duration,
) {
    tokio::spawn(async move {
        tokio::time::sleep(first_after).await;
        loop {
            let events = tick(&state, &runner, &notifier, notify).await;
            for e in events.iter().filter(|e| e.kind.notifies()) {
                let (_, body) = notification_text(e);
                println!("rocky: PR — {body}");
            }
            tokio::time::sleep(every).await;
        }
    });
}
