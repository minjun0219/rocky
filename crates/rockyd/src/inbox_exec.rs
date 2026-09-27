//! 수집함 어댑터 실행 + 소스별 TTL 캐시 — `GET /api/inbox` 의 실행 절반.
//! 파싱·검증은 core(`rocky_core::inbox::parse_inbox_output`)가 한다.
//!
//! 소스는 **동시에** 돌린다 — 어댑터 하나가 느려도(외부 API) 나머지가 기다리지 않는다.
//! 실패는 그 소스만 `available:false` 로 돌아오고 다른 소스는 정상이다.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rocky_core::config::InboxSource;
use rocky_core::inbox::{
    parse_inbox_output, unavailable, InboxResponse, InboxSourceResult, DEFAULT_INBOX_TIMEOUT_MS,
};
use tokio::sync::Mutex;

use crate::runner::{BoxFut, Runner};

/// 조회기 — 서버 옵션 주입용. 인자는 `refresh`(캐시 우회).
pub type InboxProvider = Arc<dyn Fn(bool) -> BoxFut<InboxResponse> + Send + Sync>;

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// 소스 하나 실행 → 결과. 실행 실패·timeout·출력 불량 전부 `available:false` + 사유.
pub async fn fetch_source(runner: &Runner, source: &InboxSource) -> InboxSourceResult {
    let timeout = Duration::from_millis(source.timeout_ms.unwrap_or(DEFAULT_INBOX_TIMEOUT_MS));
    let output = runner(source.command.clone(), String::new(), timeout).await;
    let fetched_at = now_iso();
    if !output.ok() {
        // stderr 첫 줄이 사람이 읽을 사유다. 없으면 exit code 라도.
        let first_line = output
            .stderr
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .map(str::to_string)
            .unwrap_or_default();
        let reason = if first_line.is_empty() {
            format!("exit {}", output.code)
        } else {
            format!("exit {}: {first_line}", output.code)
        };
        return unavailable(&source.name, reason, fetched_at);
    }
    match parse_inbox_output(&output.stdout) {
        Ok(items) => InboxSourceResult {
            name: source.name.clone(),
            available: true,
            reason: None,
            fetched_at,
            items,
        },
        Err(reason) => unavailable(&source.name, reason, fetched_at),
    }
}

/// 소스별 TTL 캐시 조회기. `refresh=true` 는 전부 다시 실행한다.
pub fn cached_inbox(runner: Runner, sources: Vec<InboxSource>, ttl: Duration) -> InboxProvider {
    let cache: Arc<Mutex<HashMap<String, (Instant, InboxSourceResult)>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let sources = Arc::new(sources);
    Arc::new(move |refresh| {
        let runner = runner.clone();
        let cache = cache.clone();
        let sources = sources.clone();
        Box::pin(async move {
            // 캐시 히트는 잠금 안에서 가려내고, 실행은 잠금 밖에서 동시에.
            let mut results: Vec<Option<InboxSourceResult>> = Vec::with_capacity(sources.len());
            let mut pending: Vec<usize> = Vec::new();
            {
                let slot = cache.lock().await;
                for (index, source) in sources.iter().enumerate() {
                    let hit = (!refresh)
                        .then(|| slot.get(&source.name))
                        .flatten()
                        .filter(|(at, _)| at.elapsed() < ttl)
                        .map(|(_, cached)| cached.clone());
                    if hit.is_none() {
                        pending.push(index);
                    }
                    results.push(hit);
                }
            }
            if !pending.is_empty() {
                let mut set = tokio::task::JoinSet::new();
                for index in pending {
                    let runner = runner.clone();
                    let source = sources[index].clone();
                    set.spawn(async move { (index, fetch_source(&runner, &source).await) });
                }
                let mut fresh: Vec<(usize, InboxSourceResult)> = Vec::new();
                while let Some(joined) = set.join_next().await {
                    if let Ok(pair) = joined {
                        fresh.push(pair);
                    }
                }
                let mut slot = cache.lock().await;
                for (index, result) in fresh {
                    slot.insert(result.name.clone(), (Instant::now(), result.clone()));
                    results[index] = Some(result);
                }
            }
            // JoinSet 이 패닉으로 빠뜨린 자리는 실패로 채운다 — 순서는 설정 순서 그대로.
            let sources_out = results
                .into_iter()
                .enumerate()
                .map(|(index, r)| {
                    r.unwrap_or_else(|| {
                        unavailable(
                            &sources[index].name,
                            "어댑터 실행 태스크가 죽었다",
                            now_iso(),
                        )
                    })
                })
                .collect();
            InboxResponse {
                sources: sources_out,
            }
        })
    })
}

/// 고정 응답 조회기 — 테스트 주입용.
pub fn fixed_inbox(response: InboxResponse) -> InboxProvider {
    Arc::new(move |_| {
        let response = response.clone();
        Box::pin(async move { response })
    })
}
