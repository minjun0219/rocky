//! SSE 구독 스레드 — `GET /api/events` 를 한 줄씩 읽어 채널로 넘긴다.
//!
//! 계약: 구독자는 payload 를 보지 않고 **refetch 만** 한다. 그래서 여기서는 `data:` 줄이
//! 왔다는 사실만 보내고 본문은 버린다. 끊기면 1·2·4·8초 백오프로 다시 붙고, 붙을 때마다
//! `Connected` 를 보내 전체 refetch 를 유도한다(끊긴 사이 놓친 변경을 그렇게 따라잡는다).

use std::io::{BufRead, BufReader};
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// 스트림이 열렸다(재연결 포함) — 전체 refetch.
    Connected,
    /// store change 이벤트 하나 — refetch.
    Changed,
    /// 스트림이 끊겼다 — 화면에 경고, 재연결은 이 스레드가 알아서.
    Disconnected,
    /// GitHub 링크 조회 결과 — (url, 한 줄 요약. None 은 실패).
    Gh(String, Option<String>),
}

/// 백오프 단계(초). 마지막 값에서 머문다.
pub const BACKOFF_SECS: [u64; 4] = [1, 2, 4, 8];

pub fn backoff_for(attempt: usize) -> Duration {
    Duration::from_secs(BACKOFF_SECS[attempt.min(BACKOFF_SECS.len() - 1)])
}

/// 구독 스레드를 띄운다. 수신 쪽이 사라지면 스레드는 조용히 끝난다.
pub fn spawn(base_url: String, tx: Sender<Event>) {
    thread::Builder::new()
        .name("rocky-tui-sse".into())
        .spawn(move || run(&base_url, &tx))
        .expect("SSE 스레드 생성");
}

fn run(base_url: &str, tx: &Sender<Event>) {
    // SSE 는 열어 둔 채 기다리는 연결이라 전역 timeout 을 두지 않는다 — 연결 단계만 제한.
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(3)))
        .timeout_global(None)
        .build()
        .into();
    let mut attempt = 0usize;
    loop {
        match agent.get(format!("{base_url}/api/events")).call() {
            Ok(mut response) if response.status().is_success() => {
                attempt = 0;
                if tx.send(Event::Connected).is_err() {
                    return;
                }
                let reader = BufReader::new(response.body_mut().as_reader());
                for line in reader.lines() {
                    let Ok(line) = line else { break };
                    if line.starts_with("data:") && tx.send(Event::Changed).is_err() {
                        return;
                    }
                }
                if tx.send(Event::Disconnected).is_err() {
                    return;
                }
            }
            _ => {
                if tx.send(Event::Disconnected).is_err() {
                    return;
                }
            }
        }
        thread::sleep(backoff_for(attempt));
        attempt += 1;
    }
}
