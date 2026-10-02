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

/// 업그레이드 때 새 데몬은 옛 데몬이 끝난 뒤에만 DB 를 연다 — pid 파일의 rockyd 가 살아 있으면 기다린다. 남의
/// 프로세스가 같은 번호를 받았거나(이름이 rockyd 가 아님) 내 pid 면 기다리지 않는다.
#[test]
fn previous_daemon_alive_only_for_another_live_rockyd() {
    use rockyd::daemon::previous_daemon_alive;
    let dir = tempfile::tempdir().unwrap();
    let pid_file = dir.path().join("daemon.pid");
    assert_eq!(
        previous_daemon_alive(&pid_file, 10, |_| true),
        None,
        "파일 없음"
    );
    std::fs::write(&pid_file, "42\n").unwrap();
    assert_eq!(previous_daemon_alive(&pid_file, 10, |p| p == 42), Some(42));
    assert_eq!(
        previous_daemon_alive(&pid_file, 10, |_| false),
        None,
        "rockyd 가 아니다"
    );
    assert_eq!(
        previous_daemon_alive(&pid_file, 42, |_| true),
        None,
        "내 pid"
    );
    std::fs::write(&pid_file, "garbage").unwrap();
    assert_eq!(previous_daemon_alive(&pid_file, 10, |_| true), None);
}
