//! `claude agents --json` 실행 + TTL 캐시 — TS 원본 `src/sessions.ts` 의 실행 절반.
//! 파싱은 core(`rocky_core::sessions::parse_sessions`)가 한다.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rocky_core::sessions::{parse_sessions, SessionsResult};
use tokio::sync::Mutex;

use crate::runner::{CmdOutput, Runner};

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
        return SessionsResult::unavailable(failure_reason(&result, || {
            "claude CLI 를 실행할 수 없다".to_string()
        }));
    }
    parse_sessions(&result.stdout)
}

/// 실패한 명령의 이유 — stderr 와 stdout 을 줄을 나눠 잇는다. 둘 다 비었으면 `fallback`.
fn failure_reason(result: &CmdOutput, fallback: impl FnOnce() -> String) -> String {
    let parts: Vec<&str> = [result.stderr.trim(), result.stdout.trim()]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect();
    if parts.is_empty() {
        fallback()
    } else {
        parts.join("\n")
    }
}

/// `claude stop` 마감 — 살아 있는 세션에 2.5초가 걸렸다(2.1.289 실측).
pub const SESSION_STOP_TIMEOUT: Duration = Duration::from_secs(15);

/// background 세션 하나를 멈춘다(`claude stop <짧은 id>`) — 대화·워크트리는 남는다. 실패면 CLI 가 낸 이유.
/// id 는 [`rocky_core::sessions::stop_target`] 이 목록에서 고른 값이어야 한다.
pub async fn stop_session(runner: &Runner, id: &str) -> Result<(), String> {
    let result = runner(
        vec!["claude".into(), "stop".into(), id.into()],
        String::new(),
        SESSION_STOP_TIMEOUT,
    )
    .await;
    if result.ok() {
        return Ok(());
    }
    Err(failure_reason(&result, || {
        format!("claude stop {id} 가 코드 {} 로 끝났다", result.code)
    }))
}

/// 조회기 — 서버 옵션 주입용. 캐시 유무/수명은 만들 때 정해진다.
pub type SessionsProvider = Arc<dyn Fn() -> crate::runner::BoxFut<SessionsResult> + Send + Sync>;

/// 캐시를 비우는 손잡이 — 세션 목록을 바꾼 쪽(멈추기)이 부른다. 다음 조회는 새로 받을 때까지 기다린다.
pub type SessionsInvalidate = Arc<dyn Fn() + Send + Sync>;

/// 캐시 없는 조회기 — spawn 라우트 전용(가드가 spawn 이전 스냅샷을 보면 안 된다).
pub fn uncached_sessions(runner: Runner) -> SessionsProvider {
    Arc::new(move || {
        let runner = runner.clone();
        Box::pin(async move { list_sessions(&runner).await })
    })
}

/// 오래된 값을 바로 주고 뒤에서 새로 받는 조회기(stale-while-revalidate). `fresh` 안이면 캐시, `stale` 안이면
/// **기다리지 않고** 캐시를 주면서 뒤에서 한 번 새로 받는다, 그보다 오래됐거나 처음이면 받을 때까지 기다린다.
///
/// 할 일 목록이 진행 중 항목의 `doingState` 를 붙이려고 세션 목록을 묻는데, `claude agents` 가 수백 ms 라 TTL 이
/// 지날 때마다 요청 하나가 그만큼 기다렸다(`GET /api/todos` p95 161ms, p50 19ms). 대가: live/gone 판정이 한 번
/// 늦을 수 있다. 처음 받는 동안 몰린 호출은 한 번의 조회를 함께 기다린다.
pub fn swr_sessions(runner: Runner, fresh: Duration, stale: Duration) -> SessionsProvider {
    swr_sessions_with_invalidate(runner, fresh, stale).0
}

/// [`swr_sessions`] 에 캐시를 비우는 손잡이를 더한 것. 비우기 전에 시작한 조회는 끝나도 캐시에 쓰지 않는다(세대 번호) —
/// 쓰면 바뀌기 전의 목록이 "새 값" 으로 돌아온다.
pub fn swr_sessions_with_invalidate(
    runner: Runner,
    fresh: Duration,
    stale: Duration,
) -> (SessionsProvider, SessionsInvalidate) {
    let cache: Arc<std::sync::Mutex<Option<(Instant, SessionsResult)>>> =
        Arc::new(std::sync::Mutex::new(None));
    let refreshing = Arc::new(AtomicBool::new(false));
    let generation = Arc::new(AtomicU64::new(0));
    let gate = Arc::new(Mutex::new(()));
    let invalidate: SessionsInvalidate = {
        let cache = cache.clone();
        let generation = generation.clone();
        Arc::new(move || {
            generation.fetch_add(1, Ordering::SeqCst);
            *cache.lock().expect("sessions cache poisoned") = None;
        })
    };
    // 시작할 때의 세대가 그대로일 때만 캐시에 쓴다.
    let store = {
        let cache = cache.clone();
        let generation = generation.clone();
        move |started: u64, result: SessionsResult| {
            let mut slot = cache.lock().expect("sessions cache poisoned");
            if generation.load(Ordering::SeqCst) == started {
                *slot = Some((Instant::now(), result));
            }
        }
    };
    let provider: SessionsProvider = Arc::new(move || {
        let runner = runner.clone();
        let cache = cache.clone();
        let refreshing = refreshing.clone();
        let generation = generation.clone();
        let gate = gate.clone();
        let store = store.clone();
        Box::pin(async move {
            let cached = cache.lock().expect("sessions cache poisoned").clone();
            if let Some((at, value)) = &cached {
                let age = at.elapsed();
                if age < fresh {
                    return value.clone();
                }
                if age < stale {
                    if !refreshing.swap(true, Ordering::SeqCst) {
                        let started = generation.load(Ordering::SeqCst);
                        tokio::spawn(async move {
                            let result = list_sessions(&runner).await;
                            store(started, result);
                            refreshing.store(false, Ordering::SeqCst);
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
            let started = generation.load(Ordering::SeqCst);
            let result = list_sessions(&runner).await;
            store(started, result.clone());
            result
        })
    });
    (provider, invalidate)
}

/// 고정 결과 조회기 — 테스트 주입용.
pub fn fixed_sessions(result: SessionsResult) -> SessionsProvider {
    Arc::new(move || {
        let result = result.clone();
        Box::pin(async move { result })
    })
}
