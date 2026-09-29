//! 종료 유예 — 끝나지 않는 연결(SSE)이 있어도 종료 신호 뒤 한도 안에 돌아온다.

use std::time::Duration;

use rockyd::daemon::serve_with_grace;

/// 영원히 안 끝나는 서버(열린 SSE 를 기다리는 graceful shutdown 과 같다)도 신호 뒤 유예가 지나면 돌아온다.
#[tokio::test]
async fn a_server_stuck_on_open_streams_returns_after_the_grace() {
    let (tx, rx) = tokio::sync::watch::channel(false);
    let stuck = std::future::pending::<Result<(), ()>>();
    tx.send(true).unwrap();
    let out = tokio::time::timeout(
        Duration::from_secs(2),
        serve_with_grace(stuck, rx, Duration::from_millis(50)),
    )
    .await;
    assert_eq!(out, Ok(Ok(())), "유예 뒤에 돌아와야 한다");
}

/// 신호가 없으면 기다리지 않고 나가지도 않는다 — 평소에는 서버가 계속 돈다.
#[tokio::test]
async fn without_a_signal_it_keeps_serving() {
    let (_tx, rx) = tokio::sync::watch::channel(false);
    let stuck = std::future::pending::<Result<(), ()>>();
    let out = tokio::time::timeout(
        Duration::from_millis(200),
        serve_with_grace(stuck, rx, Duration::from_millis(10)),
    )
    .await;
    assert!(out.is_err(), "신호 전에는 돌아오면 안 된다");
}

/// 서버가 먼저 끝나면 그 결과를 그대로 돌려준다(에러 포함).
#[tokio::test]
async fn a_server_that_finishes_returns_its_own_result() {
    let (_tx, rx) = tokio::sync::watch::channel(false);
    let out = serve_with_grace(
        async { Err::<(), &str>("bind") },
        rx,
        Duration::from_secs(5),
    )
    .await;
    assert_eq!(out, Err("bind"));
}
