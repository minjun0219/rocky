//! `claude agents --json` 실행 + TTL 캐시 — TS 원본 `src/sessions.ts` 의 실행 절반.
//! 파싱은 core(`rocky_core::sessions::parse_sessions`)가 한다.

use std::sync::Arc;
use std::time::{Duration, Instant};

use rocky_core::sessions::{parse_sessions, SessionsResult};
use tokio::sync::Mutex;

use crate::runner::Runner;

pub const SESSIONS_TIMEOUT: Duration = Duration::from_secs(5);

/// 세션 목록 한 번 — 실행 실패는 available:false + reason.
pub async fn list_sessions(runner: &Runner) -> SessionsResult {
    let result = runner(
        vec!["claude".into(), "agents".into(), "--json".into()],
        String::new(),
        SESSIONS_TIMEOUT,
    )
    .await;
    if !result.ok() {
        let reason = format!("{}{}", result.stderr, result.stdout)
            .trim()
            .to_string();
        let reason = if reason.is_empty() {
            "claude CLI 를 실행할 수 없다".to_string()
        } else {
            reason
        };
        return SessionsResult::unavailable(reason);
    }
    parse_sessions(&result.stdout)
}

/// 조회기 — 서버 옵션 주입용. 캐시 유무/수명은 만들 때 정해진다.
pub type SessionsProvider = Arc<dyn Fn() -> crate::runner::BoxFut<SessionsResult> + Send + Sync>;

/// 캐시 없는 조회기 — spawn 라우트 전용(가드가 spawn 이전 스냅샷을 보면 안 된다).
pub fn uncached_sessions(runner: Runner) -> SessionsProvider {
    Arc::new(move || {
        let runner = runner.clone();
        Box::pin(async move { list_sessions(&runner).await })
    })
}

/// TTL 메모이즈 조회기 — 기본 3초, statusline 라우트는 15초.
pub fn cached_sessions(runner: Runner, ttl: Duration) -> SessionsProvider {
    let cache: Arc<Mutex<Option<(Instant, SessionsResult)>>> = Arc::new(Mutex::new(None));
    Arc::new(move || {
        let runner = runner.clone();
        let cache = cache.clone();
        Box::pin(async move {
            let mut slot = cache.lock().await;
            if let Some((at, cached)) = slot.as_ref() {
                if at.elapsed() < ttl {
                    return cached.clone();
                }
            }
            let fresh = list_sessions(&runner).await;
            *slot = Some((Instant::now(), fresh.clone()));
            fresh
        })
    })
}

/// 오래된 값을 바로 주고 뒤에서 새로 받는 조회기(stale-while-revalidate). `fresh` 안이면 캐시, `stale` 안이면
/// **기다리지 않고** 캐시를 주면서 뒤에서 한 번 새로 받는다, 그보다 오래됐거나 처음이면 받을 때까지 기다린다.
///
/// 할 일 목록이 진행 중 항목의 `doingState` 를 붙이려고 세션 목록을 묻는데, `claude agents` 가 수백 ms 라 TTL 이
/// 지날 때마다 요청 하나가 그만큼 기다렸다(`GET /api/todos` p95 161ms, p50 19ms). 대가: live/gone 판정이 한 번
/// 늦을 수 있다. 처음 받는 동안 몰린 호출은 한 번의 조회를 함께 기다린다.
pub fn swr_sessions(runner: Runner, fresh: Duration, stale: Duration) -> SessionsProvider {
    let cache: Arc<std::sync::Mutex<Option<(Instant, SessionsResult)>>> =
        Arc::new(std::sync::Mutex::new(None));
    let refreshing = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let gate = Arc::new(Mutex::new(()));
    Arc::new(move || {
        let runner = runner.clone();
        let cache = cache.clone();
        let refreshing = refreshing.clone();
        let gate = gate.clone();
        Box::pin(async move {
            let cached = cache.lock().expect("sessions cache poisoned").clone();
            if let Some((at, value)) = &cached {
                let age = at.elapsed();
                if age < fresh {
                    return value.clone();
                }
                if age < stale {
                    if !refreshing.swap(true, std::sync::atomic::Ordering::SeqCst) {
                        let cache = cache.clone();
                        let refreshing = refreshing.clone();
                        tokio::spawn(async move {
                            let result = list_sessions(&runner).await;
                            *cache.lock().expect("sessions cache poisoned") =
                                Some((Instant::now(), result));
                            refreshing.store(false, std::sync::atomic::Ordering::SeqCst);
                        });
                    }
                    return value.clone();
                }
            }
            let _turn = gate.lock().await;
            // 기다리는 사이 앞 사람이 받아 왔으면 그걸 쓴다.
            if let Some((at, value)) = cache.lock().expect("sessions cache poisoned").as_ref() {
                if at.elapsed() < fresh {
                    return value.clone();
                }
            }
            let result = list_sessions(&runner).await;
            *cache.lock().expect("sessions cache poisoned") =
                Some((Instant::now(), result.clone()));
            result
        })
    })
}

/// 고정 결과 조회기 — 테스트 주입용.
pub fn fixed_sessions(result: SessionsResult) -> SessionsProvider {
    Arc::new(move || {
        let result = result.clone();
        Box::pin(async move { result })
    })
}
