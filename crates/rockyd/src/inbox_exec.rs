//! 수집함 어댑터 실행 + 소스별 TTL 캐시 — `GET /api/inbox` 의 실행 절반.
//! 파싱·검증은 core(`rocky_core::inbox::parse_inbox_output`)가 한다.
//!
//! 소스는 **동시에** 돌린다 — 어댑터 하나가 느려도(외부 API) 나머지가 기다리지 않는다.
//! 실패는 그 소스만 `available:false` 로 돌아오고 다른 소스는 정상이다.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rocky_core::config::InboxSource;
use rocky_core::inbox::{
    parse_inbox_output, unavailable, InboxResponse, InboxSourceResult, DEFAULT_INBOX_TIMEOUT_MS,
};
use tokio::sync::Mutex;

use crate::runner::{BoxFut, Runner};

/// 조회 모드.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InboxFetch {
    /// 캐시가 신선하면 캐시, 아니면 실행하고 기다린다.
    Normal,
    /// 캐시를 무시하고 전부 실행한다.
    Refresh,
    /// **절대 기다리지 않는다** — 캐시된 것만(만료돼도) 돌려주고, 없거나 만료된 소스는
    /// 백그라운드에서 갱신을 시작한다. statusline 처럼 초당 도는 소비자용.
    CachedOnly,
}

/// 조회기 — 서버 옵션 주입용.
pub type InboxProvider = Arc<dyn Fn(InboxFetch) -> BoxFut<InboxResponse> + Send + Sync>;

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
            board: None,
            available: true,
            reason: None,
            fetched_at,
            items,
        },
        Err(reason) => unavailable(&source.name, reason, fetched_at),
    }
}

/// 캐시 키 — 이름**과 실행 argv**. 보드 수집함은 지우고 같은 이름으로 다른 값을 다시 등록할 수 있어서,
/// 이름만 키로 쓰면 TTL 동안 옛 값으로 가져온 결과가 새 소스 이름으로 나온다.
fn cache_key(source: &InboxSource) -> String {
    let mut key = source.name.clone();
    for arg in &source.command {
        key.push('\0');
        key.push_str(arg);
    }
    key
}

/// 조회할 소스 목록을 그때그때 내는 함수 — 보드 설정 화면에서 등록한 소스가 재기동 없이 반영되게.
pub type SourcesFn = Arc<dyn Fn() -> Vec<InboxSource> + Send + Sync>;

/// 고정 소스의 TTL 캐시 조회기 — `cached_inbox_dynamic` 의 소스가 늘 같은 경우.
pub fn cached_inbox(runner: Runner, sources: Vec<InboxSource>, ttl: Duration) -> InboxProvider {
    let sources = Arc::new(sources);
    cached_inbox_dynamic(runner, Arc::new(move || sources.as_ref().clone()), ttl)
}

/// 소스별 TTL 캐시 조회기. `Refresh` 는 전부 다시, `CachedOnly` 는 기다리지 않는다. 소스 목록은
/// 호출마다 `sources()` 로 새로 받는다 — 캐시는 소스 이름이 키라, 지운 소스는 그냥 안 나온다.
pub fn cached_inbox_dynamic(runner: Runner, sources_fn: SourcesFn, ttl: Duration) -> InboxProvider {
    let cache: Arc<Mutex<HashMap<String, (Instant, InboxSourceResult)>>> =
        Arc::new(Mutex::new(HashMap::new()));
    // CachedOnly 가 띄운 백그라운드 갱신이 도는 소스 — 초당 호출이 갱신을 겹쳐 띄우지 않게.
    let refreshing: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));
    Arc::new(move |mode| {
        let runner = runner.clone();
        let cache = cache.clone();
        let refreshing = refreshing.clone();
        let sources = Arc::new(sources_fn());
        Box::pin(async move {
            if mode == InboxFetch::CachedOnly {
                return cached_only(&runner, &sources, &cache, &refreshing, ttl).await;
            }
            let refresh = mode == InboxFetch::Refresh;
            // 캐시 히트는 잠금 안에서 가려내고, 실행은 잠금 밖에서 동시에.
            let mut results: Vec<Option<InboxSourceResult>> = Vec::with_capacity(sources.len());
            let mut pending: Vec<usize> = Vec::new();
            {
                let slot = cache.lock().await;
                for (index, source) in sources.iter().enumerate() {
                    let hit = (!refresh)
                        .then(|| slot.get(&cache_key(source)))
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
                    slot.insert(cache_key(&sources[index]), (Instant::now(), result.clone()));
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

/// 기다리지 않는 조회 — 캐시된 소스만(만료돼도) 낸다. 없거나 만료된 소스는 백그라운드로
/// 갱신을 띄운다(소스당 하나만). 그래서 statusline 은 첫 호출에 비고, 다음 호출부터 채워지며,
/// 누군가 계속 보는 동안 TTL 주기로 새로워진다.
async fn cached_only(
    runner: &Runner,
    sources: &Arc<Vec<InboxSource>>,
    cache: &Arc<Mutex<HashMap<String, (Instant, InboxSourceResult)>>>,
    refreshing: &Arc<Mutex<HashSet<String>>>,
    ttl: Duration,
) -> InboxResponse {
    let mut out: Vec<InboxSourceResult> = Vec::new();
    let mut to_refresh: Vec<InboxSource> = Vec::new();
    {
        let slot = cache.lock().await;
        let mut busy = refreshing.lock().await;
        for source in sources.iter() {
            let key = cache_key(source);
            match slot.get(&key) {
                Some((at, cached)) => {
                    out.push(cached.clone());
                    if at.elapsed() >= ttl && busy.insert(key) {
                        to_refresh.push(source.clone());
                    }
                }
                None => {
                    if busy.insert(key) {
                        to_refresh.push(source.clone());
                    }
                }
            }
        }
    }
    for source in to_refresh {
        let runner = runner.clone();
        let cache = cache.clone();
        let refreshing = refreshing.clone();
        tokio::spawn(async move {
            let result = fetch_source(&runner, &source).await;
            cache
                .lock()
                .await
                .insert(cache_key(&source), (Instant::now(), result));
            refreshing.lock().await.remove(&cache_key(&source));
        });
    }
    InboxResponse { sources: out }
}

/// 고정 응답 조회기 — 테스트 주입용.
pub fn fixed_inbox(response: InboxResponse) -> InboxProvider {
    Arc::new(move |_| {
        let response = response.clone();
        Box::pin(async move { response })
    })
}
