//! 세션 목록 조회기(stale-while-revalidate) — 오래된 값을 바로 주고 뒤에서 한 번만 새로 받는다.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rockyd::runner::{CmdOutput, Runner};
use rockyd::sessions_exec::swr_sessions;

/// 부를 때마다 수를 세고 `delay` 만큼 걸리는 가짜 `claude agents`.
fn counting_runner(calls: Arc<AtomicUsize>, delay: Duration) -> Runner {
    Arc::new(move |_, _, _| {
        let calls = calls.clone();
        Box::pin(async move {
            calls.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(delay).await;
            CmdOutput {
                code: 0,
                stdout: "[]".into(),
                stderr: String::new(),
            }
        })
    })
}

#[tokio::test]
async fn stale_values_are_served_immediately_while_one_refresh_runs_behind() {
    let calls = Arc::new(AtomicUsize::new(0));
    let provider = swr_sessions(
        counting_runner(calls.clone(), Duration::from_millis(150)),
        Duration::from_millis(100),
        Duration::from_secs(5),
    );
    // 처음: 받을 때까지 기다린다 — 동시에 몰린 셋은 한 번의 조회를 함께 기다린다
    let (a, b, c) = tokio::join!(provider(), provider(), provider());
    assert!(a.available && b.available && c.available);
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    // fresh 안: 캐시
    provider().await;
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    // fresh 지남, stale 안: 기다리지 않고 바로 — 뒤에서 한 번만 새로 받는다
    tokio::time::sleep(Duration::from_millis(120)).await;
    let started = Instant::now();
    provider().await;
    provider().await;
    assert!(
        started.elapsed() < Duration::from_millis(100),
        "오래된 값을 바로 준다: {:?}",
        started.elapsed()
    );
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert_eq!(calls.load(Ordering::SeqCst), 2, "뒤에서 한 번만");
}

#[tokio::test]
async fn too_old_values_wait_for_a_fresh_lookup() {
    let calls = Arc::new(AtomicUsize::new(0));
    let provider = swr_sessions(
        counting_runner(calls.clone(), Duration::from_millis(10)),
        Duration::from_millis(20),
        Duration::from_millis(40),
    );
    provider().await;
    tokio::time::sleep(Duration::from_millis(60)).await;
    provider().await;
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "stale 을 넘기면 기다려서 새로"
    );
}
