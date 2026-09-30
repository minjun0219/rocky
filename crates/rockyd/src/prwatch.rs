//! PR 감시 — 주기 잡. `repo` 가 설정된 보드의 레포마다 `gh api graphql` 한 번을 돌려
//! 스냅숏을 스토어에 넣고(전이는 거기서 히스토리로), 사람이 움직일 전이(ready·conflict)는
//! macOS 알림으로 쏜다. 판정은 전부 `rocky_core::prwatch`(순수), 여기는 배선이다.
//! 설계: `docs/design/specs/2026-09-28-pr-watch-design.md`.
//!
//! 읽기만 한다 — 쿼리뿐, GitHub 에 쓰는 것은 없다.
//!
//! **예산을 지킨다.** GraphQL 한도(시간당 5,000 포인트)는 사용자 계정 하나에 걸리므로 데몬이
//! 다 쓰면 세션·터미널의 `gh` 까지 막힌다(2026-09-28 실측 — 20분 만에 바닥). 응답마다
//! `rateLimit` 을 읽어 잔여가 `RATE_LIMIT_FLOOR` 밑이면 그 tick 을 멈추고 리셋까지 쉬며, 한도
//! 에러를 받아도 같다. 쉬는 동안은 health 가 `pausedUntil` 로 말한다.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use rocky_core::prwatch::{
    bridge_payload, default_branch_of, detail_query, is_rate_limit_error, list_query,
    notification_text, osascript_args, parse_pr_details, parse_pr_list, pause_for, PrEvent,
    PrEventKind, PrSnapshot, RateLimit,
};
use serde::Serialize;

use crate::runner::Runner;
use crate::server::ServerState;

/// 전이 히스토리의 actor.
pub const PR_WATCH_ACTOR: &str = "rocky";
/// `gh api graphql` 한 번의 상한.
const QUERY_TIMEOUT: Duration = Duration::from_secs(30);

/// 전이 한 건 → 사람에게. 기본은 osascript, 브릿지는 명령, 테스트는 붙잡는다. 알림기가
/// 문구를 만든다(`notification_text` / `bridge_payload`) — tick 은 전이만 건넨다.
pub type Notifier = Arc<dyn Fn(&PrEvent) + Send + Sync>;

/// osascript 로 macOS 알림. 다른 OS 면 조용히 아무것도 안 한다.
pub fn osascript_notifier(runner: Runner) -> Notifier {
    Arc::new(move |event| {
        if !cfg!(target_os = "macos") || !event.kind.notifies() {
            return;
        }
        let (title, body) = notification_text(event);
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
    Arc::new(|_| {})
}

/// 브릿지 기본 상한 — 수집함 어댑터와 같다.
pub const DEFAULT_BRIDGE_TIMEOUT_MS: u64 = 10_000;

/// 알림 브릿지(`pr.notifiers[]`) — 명령을 argv 그대로 실행하고 stdin 에 `bridge_payload` JSON 을
/// 준다. exit ≠ 0 이면 stderr 첫 줄을 데몬 로그에 남긴다(이름과 함께). 토큰은 브릿지가
/// 스스로 읽는다 — 데몬은 어떤 서비스인지 모른다.
pub fn bridge_notifier(runner: Runner, bridge: rocky_core::config::CommandBridge) -> Notifier {
    Arc::new(move |event| {
        if !event.kind.notifies() {
            return;
        }
        let runner = runner.clone();
        let bridge = bridge.clone();
        let stdin = bridge_payload(event).to_string();
        tokio::spawn(async move {
            let timeout =
                Duration::from_millis(bridge.timeout_ms.unwrap_or(DEFAULT_BRIDGE_TIMEOUT_MS));
            let out = runner(bridge.command.clone(), stdin, timeout).await;
            if !out.ok() {
                let reason = out.stderr.lines().next().unwrap_or("").trim().to_string();
                eprintln!(
                    "rocky: 알림 브릿지 {} 실패(exit {}): {reason}",
                    bridge.name, out.code
                );
            }
        });
    })
}

/// 세션 알림 — ready·conflict·merged·ci-failed(와 autoResolve 가 켜진 보드의 리뷰 도착)를 그 레포 보드에서 일하는 Claude Code 세션의 받은편지함 소켓에 한 줄로
/// 쓴다(`rocky_core::peer_inbox`). 쉬던 세션은 그 자리에서 턴이 열린다. 가장 최근 세션부터 시도해
/// **처음 성공한 한 곳**에서 멈추고, 실패한 등록(끝난 세션)은 걷는다. 등록된 세션이 없으면 조용하다.
pub fn session_notifier(state: Arc<ServerState>) -> Notifier {
    Arc::new(move |event| {
        let boards = state.store.list_boards(false).unwrap_or_default();
        // 리뷰 도착은 그 레포를 둔 보드 중 하나라도 autoResolve 를 켰을 때만 세션에 처리를 시킨다
        // (`rocky board auto-resolve on` — 그 레포의 세션이 자기 보드를 켠다).
        let text = if event.kind == PrEventKind::Review {
            if !rocky_core::prwatch::auto_resolve_enabled(&boards, &event.repo) {
                return;
            }
            rocky_core::peer_inbox::review_session_message(event)
        } else {
            rocky_core::peer_inbox::pr_session_message(event)
        };
        let Some(text) = text else {
            return;
        };
        let locations: Vec<rocky_core::statusline::BoardLocation> = boards
            .iter()
            .map(|b| rocky_core::statusline::BoardLocation {
                key: b.key.clone(),
                path: b.path.clone(),
            })
            .collect();
        let registrations = state.inboxes();
        let now = chrono::Utc::now().timestamp();
        // 보드마다 후보를 최근 순으로 — 레포가 여러 보드에 걸려도 같은 세션에 두 번 보내지 않는다.
        let mut groups: Vec<Vec<rocky_core::peer_inbox::InboxRegistration>> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let viewer = state.gh_viewer();
        for board in boards
            .iter()
            .filter(|b| b.repo.as_deref() == Some(event.repo.as_str()))
            // 보드 속성 — 이 보드의 prAuthors 에 걸린 PR 이면 이 보드의 세션은 깨우지 않는다.
            .filter(|b| {
                rocky_core::prwatch::author_matches(
                    &b.pr_authors,
                    event.author.as_deref(),
                    viewer.as_deref(),
                )
            })
        {
            let group: Vec<_> = rocky_core::peer_inbox::session_candidates(
                &registrations,
                &locations,
                &board.key,
                now,
            )
            .into_iter()
            // "보내지 않기" 를 켠 세션은 건너뛴다 — 그 보드의 다음 세션이 받는다.
            .filter(|r| !state.is_muted(&r.session_id))
            .filter(|r| seen.insert(r.session_id.clone()))
            .cloned()
            .collect();
            if !group.is_empty() {
                groups.push(group);
            }
        }
        let line = rocky_core::peer_inbox::inbox_line(&text);
        let subject = format!("{}#{} {}", event.repo, event.number, event.title);
        for group in groups {
            let state = state.clone();
            let line = line.clone();
            let (kind, subject, url) = (
                event.kind.action().to_string(),
                subject.clone(),
                event.url.clone(),
            );
            // 가장 최근 세션부터 — 처음 성공한 한 곳에서 멈추고, 실패한 등록(끝난 세션)은 걷는다.
            tokio::task::spawn_blocking(move || {
                for target in group {
                    let ok = write_inbox(&target.socket, &line);
                    state.record_delivery(rocky_core::peer_inbox::Delivery {
                        at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                        kind: kind.clone(),
                        subject: subject.clone(),
                        url: Some(url.clone()),
                        session_id: target.session_id.clone(),
                        ok: ok.is_ok(),
                    });
                    match ok {
                        Ok(()) => return,
                        Err(e) => {
                            eprintln!(
                                "rocky: 세션 알림 — 받은편지함에 못 썼다({e}), 다음 세션으로"
                            );
                            state.forget_inbox(&target.session_id);
                        }
                    }
                }
            });
        }
    })
}

/// 받은편지함 소켓에 한 줄 — 연결·쓰기 모두 2초 안에. 유닉스가 아니면 조용히 실패한다.
pub(crate) fn write_inbox(socket: &str, line: &str) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        let mut stream = std::os::unix::net::UnixStream::connect(socket)?;
        stream.set_write_timeout(Some(Duration::from_secs(2)))?;
        stream.write_all(line.as_bytes())?;
        stream.shutdown(std::net::Shutdown::Write)?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = (socket, line);
        Err(std::io::Error::other("unix socket 이 없는 플랫폼"))
    }
}

/// 여러 알림기를 하나로 — 전부에게 같은 전이. 비어 있으면 조용하다.
pub fn compose_notifiers(notifiers: Vec<Notifier>) -> Notifier {
    Arc::new(move |event| {
        for n in &notifiers {
            n(event);
        }
    })
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
    /// 마지막 tick 의 예산 — `cost` 는 그 tick 의 누계, `remaining` 은 마지막 응답의 잔여.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<RateLimit>,
    /// 예산 때문에 쉬는 중이면 재개 시각(ISO).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paused_until: Option<String>,
}

/// 한 tick 의 결과 — 전이와, 예산 때문에 다음 tick 을 미뤄야 하면 그 길이.
#[derive(Debug, Default)]
pub struct TickOutcome {
    pub events: Vec<PrEvent>,
    pub pause: Option<Duration>,
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

/// 쿼리 실패 — 한도는 따로 가른다(그 뒤의 레포는 물어봐야 똑같이 실패한다).
enum QueryError {
    RateLimited,
    Other(String),
}

/// `gh api graphql` 한 번 — `data` 와 그 응답의 예산.
async fn graphql(
    runner: &Runner,
    query: String,
    fields: &[(String, String)],
) -> Result<(serde_json::Value, Option<RateLimit>), QueryError> {
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
        if is_rate_limit_error(&out.stdout, &out.stderr) {
            return Err(QueryError::RateLimited);
        }
        return Err(QueryError::Other(format!(
            "gh api graphql 실패(exit {}): {}",
            out.code,
            out.stderr.trim()
        )));
    }
    let body: serde_json::Value = serde_json::from_str(&out.stdout)
        .map_err(|e| QueryError::Other(format!("응답이 JSON 이 아니다: {e}")))?;
    let data = body
        .get("data")
        .cloned()
        .ok_or_else(|| QueryError::Other("응답에 data 가 없다".to_string()))?;
    let limit = RateLimit::of(&data);
    Ok((data, limit))
}

/// 한 레포에서 본 것 — 전이와 이 레포에 쓴 예산(마지막 응답 기준 잔여, 비용은 누계).
struct RepoOutcome {
    events: Vec<PrEvent>,
    limit: Option<RateLimit>,
}

fn accumulate(total: &mut Option<RateLimit>, seen: Option<RateLimit>) {
    let Some(seen) = seen else { return };
    match total {
        Some(t) => {
            t.cost += seen.cost;
            t.remaining = seen.remaining;
            t.reset_at = seen.reset_at;
        }
        None => *total = Some(seen),
    }
}

/// 한 레포 한 번 — 목록(상태만) → 열린 것 + 직전엔 열려 있었는데 목록에 없는 것을 상세로 →
/// 스토어 → 알림. 상세는 별칭 배치 한 요청이라 레포당 호출은 최대 둘이다.
async fn tick_repo(
    state: &Arc<ServerState>,
    runner: &Runner,
    notifier: &Notifier,
    notify: bool,
    repo: &str,
) -> Result<RepoOutcome, QueryError> {
    let Some((owner, name)) = repo.split_once('/') else {
        return Err(QueryError::Other(format!(
            "repo 표기가 owner/name 이 아니다: {repo}"
        )));
    };
    let repo_vars = vec![
        ("owner".to_string(), owner.to_string()),
        ("name".to_string(), name.to_string()),
    ];
    let (data, mut limit) = graphql(runner, list_query(), &repo_vars).await?;
    let default_branch = default_branch_of(&data);
    let list = parse_pr_list(&data, repo, &default_branch).map_err(QueryError::Other)?;
    // 직전엔 OPEN 이었는데 이번 목록(열린 50 + 최근 닫힌 30)에 없는 PR — 열린 창을 넘쳤거나
    // 닫힌 지 오래돼 창 밖으로 밀린 것이다. 상세에 끼워 전이(merged/closed)를 잃지 않는다.
    let prev_open: Vec<PrSnapshot> = state
        .store
        .list_prs(Some(repo), true)
        .map_err(|e| QueryError::Other(e.to_string()))?;
    let mut numbers = list.open.clone();
    for p in &prev_open {
        if !numbers.contains(&p.number) && !list.closed.iter().any(|c| c.number == p.number) {
            numbers.push(p.number);
        }
    }
    let mut snapshots = list.closed;
    if let Some(query) = detail_query(&numbers) {
        let (detail, seen) = graphql(runner, query, &repo_vars).await?;
        accumulate(&mut limit, seen);
        snapshots
            .extend(parse_pr_details(&detail, repo, &default_branch).map_err(QueryError::Other)?);
    }
    // 알릴지는 보드의 PR 작성자 필터(`prAuthors`)가 정한다 — `@me` 는 gh 로그인 계정(목록 쿼리의 viewer).
    // 걸린 전이도 히스토리·스냅숏에는 남는다(보기는 넓게, 깨우기는 좁게).
    let viewer = data
        .pointer("/viewer/login")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    let boards = state.store.list_boards(false).unwrap_or_default();
    // `@me` 를 풀 계정이 응답에 없으면 그 보드는 아무것도 알리지 못한다 — 조용히 끊기지 않게 남긴다.
    if viewer.is_none()
        && boards
            .iter()
            .any(|b| b.repo.as_deref() == Some(repo) && b.pr_authors.iter().any(|a| a == "@me"))
    {
        eprintln!(
            "rocky: PR 감시 {repo} — 응답에 viewer 가 없어 @me 판정 불가(그 보드는 알림이 가지 않는다)"
        );
    }
    state.set_gh_viewer(viewer.clone());
    // board_id 가 있으면 그 보드의 필터(세션 훅 주입), 없으면 레포 단위(배너·브릿지).
    let allow = |e: &rocky_core::prwatch::PrEvent, board_id: Option<&str>| {
        rocky_core::prwatch::author_allowed(
            &boards,
            &e.repo,
            board_id,
            e.author.as_deref(),
            viewer.as_deref(),
        )
    };
    let events = state
        .store
        .apply_pr_snapshot(repo, &snapshots, PR_WATCH_ACTOR, &allow)
        .map_err(|e| QueryError::Other(e.to_string()))?;
    if notify {
        // 사람에게 알릴 것(ready·conflict)과 세션이 처리할 것(review·merged). 배너·브릿지는 앞의 둘만 쓴다.
        for event in events
            .iter()
            .filter(|e| e.kind.reaches_session() && !e.quiet)
        {
            notifier(event);
        }
    }
    Ok(RepoOutcome { events, limit })
}

/// 한 번 훑는다 — 레포마다 독립(하나가 실패해도 나머지는 돈다). 더는 보지 않는 레포의 스냅숏은
/// 걷는다. 예산이 바닥이면(한도 에러, 또는 잔여 < `RATE_LIMIT_FLOOR`) 남은 레포는 건너뛰고
/// 리셋까지 쉬라고 돌려준다. 결과는 health 에 반영.
pub async fn tick(
    state: &Arc<ServerState>,
    runner: &Runner,
    notifier: &Notifier,
    notify: bool,
) -> TickOutcome {
    let repos = watched_repos(state);
    let _ = state.store.retain_pr_repos(&repos);
    let mut events = Vec::new();
    let mut failures = Vec::new();
    let mut limit: Option<RateLimit> = None;
    let mut pause = None;
    let now = chrono::Utc::now();
    for repo in &repos {
        match tick_repo(state, runner, notifier, notify, repo).await {
            Ok(mut out) => {
                events.append(&mut out.events);
                accumulate(&mut limit, out.limit);
                if limit.as_ref().is_some_and(RateLimit::exhausted) {
                    let d = pause_for(limit.as_ref(), now);
                    failures.push(format!(
                        "GraphQL 잔여 {} < {} — 세션의 gh 를 남기려고 {} 까지 쉼",
                        limit.as_ref().map(|l| l.remaining).unwrap_or_default(),
                        rocky_core::prwatch::RATE_LIMIT_FLOOR,
                        local_clock(now + chrono::Duration::from_std(d).unwrap_or_default())
                    ));
                    pause = Some(d);
                    break;
                }
            }
            Err(QueryError::RateLimited) => {
                let d = pause_for(limit.as_ref(), now);
                failures.push(format!(
                    "{repo}: GitHub GraphQL 레이트 리밋 — {} 까지 쉼",
                    local_clock(now + chrono::Duration::from_std(d).unwrap_or_default())
                ));
                pause = Some(d);
                break;
            }
            Err(QueryError::Other(reason)) => failures.push(format!("{repo}: {reason}")),
        }
    }
    state.set_pr_watch(PrWatchStatus {
        available: failures.is_empty(),
        reason: (!failures.is_empty()).then(|| failures.join("; ")),
        last_tick: Some(iso(now)),
        repos,
        rate_limit: limit,
        paused_until: pause.map(|d| iso(now + chrono::Duration::from_std(d).unwrap_or_default())),
    });
    TickOutcome { events, pause }
}

fn iso(t: chrono::DateTime<chrono::Utc>) -> String {
    t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// 사람이 읽는 시각 — 데몬 머신의 로컬 시계로 `HH:MM`.
fn local_clock(t: chrono::DateTime<chrono::Utc>) -> String {
    t.with_timezone(&chrono::Local).format("%H:%M").to_string()
}

/// 기동 뒤 `first_after` 지나 처음, 이후 `every` 마다 — 예산 때문에 쉬어야 하면 그만큼 더.
/// 레포가 없으면 조용히 돈다.
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
            let outcome = tick(&state, &runner, &notifier, notify).await;
            for e in outcome
                .events
                .iter()
                .filter(|e| e.kind.notifies() && !e.quiet)
            {
                let (_, body) = notification_text(e);
                println!("rocky: PR — {body}");
            }
            if let Some(reason) = state.pr_watch().reason {
                if outcome.pause.is_some() {
                    println!("rocky: PR 감시 쉼 — {reason}");
                }
            }
            tokio::time::sleep(outcome.pause.map_or(every, |p| p.max(every))).await;
        }
    });
}
