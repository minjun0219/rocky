//! 사용 로그 싱크 — 데몬이 REST·MCP 호출 한 건마다 `UsageEvent` 를 넘기면 파일에 한 줄씩
//! 쓴다. 요청 경로에서 디스크를 만지지 않으려고 채널 + 전용 스레드다. 실패는 조용히
//! 버린다(로그가 본업을 막지 않는다). 테스트는 `capture_sink` 로 이벤트를 붙잡는다.

use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};

use rocky_core::usage::{append_event, UsageEvent};

pub type UsageSink = Arc<dyn Fn(UsageEvent) + Send + Sync>;

/// 아무것도 안 쓴다 — 설정으로 끈 경우.
pub fn noop_sink() -> UsageSink {
    Arc::new(|_| {})
}

/// `<dir>/YYYY-MM.jsonl` 에 append. 스레드가 하나 뜨고 채널이 끊기면 끝난다.
pub fn file_sink(dir: PathBuf) -> UsageSink {
    let (tx, rx) = mpsc::channel::<UsageEvent>();
    std::thread::Builder::new()
        .name("rocky-usage".into())
        .spawn(move || {
            for event in rx {
                let _ = append_event(&dir, &event);
            }
        })
        .expect("usage sink thread");
    let tx: Mutex<Sender<UsageEvent>> = Mutex::new(tx);
    Arc::new(move |event| {
        if let Ok(tx) = tx.lock() {
            let _ = tx.send(event);
        }
    })
}

/// 테스트용 — 넘어온 이벤트를 모아 둔다.
pub fn capture_sink() -> (UsageSink, Arc<Mutex<Vec<UsageEvent>>>) {
    let store: Arc<Mutex<Vec<UsageEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = store.clone();
    (
        Arc::new(move |event| {
            if let Ok(mut v) = sink.lock() {
                v.push(event);
            }
        }),
        store,
    )
}
