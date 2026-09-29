//! rocky 채널 — 데몬의 PR 전이(ready·conflict)를 **이 세션에 밀어 넣는다**.
//!
//! Claude Code 의 channels(리서치 프리뷰): MCP 서버가 `claude/channel` capability 를 선언하고
//! `notifications/claude/channel` 을 보내면 세션이 그 자리에서 깨어나 `<channel source=… kind=…
//! repo=… number=…>본문</channel>` 을 받는다. 데몬은 세션을 찌를 수 없고 훅 주입은 사람이
//! 타이핑해야 열리는데, 이 경로는 **턴을 연다** — 폰 알림(`PushNotification`)이나 리뷰 대응이
//! 전이 직후에 시작된다.
//!
//! 이 서버(`rocky mcp worklog`)는 세션마다 하나씩 뜨므로 채널도 세션마다 선언되지만, Claude Code
//! 는 `--channels`/`--dangerously-load-development-channels` 로 띄운 세션에만 배달한다(나머지는
//! 조용히 버린다). 그래서 "알림 담당" 세션 하나만 그 플래그로 띄우면 된다. 우리 마켓플레이스는
//! 허용 목록에 없어서 프리뷰 동안은 개발 플래그다:
//! `claude --dangerously-load-development-channels plugin:rocky@rocky-marketplace`.
//!
//! 전달 규칙은 훅 주입과 같다(`rocky_core::notify::pr_channel_events` — ready·conflict 만). 시작
//! 시점의 watermark 이후 것만 보내고 과거는 재생하지 않는다. SSE 는 TUI 와 같은 방식(한 줄씩,
//! `data:` 가 오면 `/api/changes` 를 차분 조회, 끊기면 1·2·4·8초 백오프).

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::thread;
use std::time::Duration;

use rmcp::model::{CustomNotification, JsonObject, ServerNotification};
use rmcp::service::Peer;
use rmcp::RoleServer;
use rocky_core::notify::{page_cursor, pr_channel_events};
use rocky_core::types::ChangesSince;

/// Claude Code 가 채널로 인식하는 capability 키 — `capabilities.experimental` 아래.
pub const CHANNEL_CAPABILITY: &str = "claude/channel";
/// 세션에 밀어 넣는 notification 의 method.
pub const CHANNEL_METHOD: &str = "notifications/claude/channel";

/// 서버가 붙을 때 Claude 에게 주는 안내 — 이벤트가 무엇이고 무엇을 하라는 것인지.
pub const INSTRUCTIONS: &str = "rocky 채널: 데몬의 PR 감시 전이가 <channel source=\"…\" kind=\"ready|conflict\" repo=\"owner/name\" number=\"N\" url=\"…\"> 로 온다. 단방향이다(회신 도구 없음). ready 는 \"머지 후보\"(기계 판정) — `/rocky:review-fix` 8단계대로 판단한 뒤 사람에게 알린다(PushNotification 이 있으면 그것으로, 한 줄). conflict 는 그 PR 이 이 세션의 것이면 충돌을 풀고, 아니면 알린다. 감시를 따로 돌리지 말고, 머지는 사람 몫이다.";

/// 백오프 단계(초). 마지막 값에서 머문다.
const BACKOFF_SECS: [u64; 4] = [1, 2, 4, 8];
/// 데몬이 없을 때 다시 두드리는 간격.
const NO_DAEMON_RETRY: Duration = Duration::from_secs(30);
/// `/api/changes` 한 페이지.
const PAGE: usize = 100;

pub fn channel_capabilities() -> BTreeMap<String, JsonObject> {
    BTreeMap::from([(CHANNEL_CAPABILITY.to_string(), JsonObject::new())])
}

/// 채널 notification 본문 — `content` 가 태그 본문, `meta` 가 태그 속성.
pub fn channel_notification(content: &str, meta: &BTreeMap<String, String>) -> ServerNotification {
    ServerNotification::CustomNotification(CustomNotification::new(
        CHANNEL_METHOD,
        Some(serde_json::json!({ "content": content, "meta": meta })),
    ))
}

fn backoff_for(attempt: usize) -> Duration {
    Duration::from_secs(BACKOFF_SECS[attempt.min(BACKOFF_SECS.len() - 1)])
}

/// 전달 스레드를 띄운다. 프로세스(stdio 서버)가 끝나면 같이 끝난다.
pub fn spawn_forwarder(peer: Peer<RoleServer>, base_url: String, handle: tokio::runtime::Handle) {
    let _ = thread::Builder::new()
        .name("rocky-channel".into())
        .spawn(move || run(&peer, &base_url, &handle));
}

fn fetch_changes(
    agent: &ureq::Agent,
    base_url: &str,
    since_id: i64,
    limit: i64,
) -> Option<ChangesSince> {
    let mut response = agent
        .get(format!(
            "{base_url}/api/changes?sinceId={since_id}&limit={limit}"
        ))
        .call()
        .ok()?;
    if !(200..300).contains(&response.status().as_u16()) {
        return None;
    }
    response.body_mut().read_json().ok()
}

/// cursor 이후의 전이를 전부 밀어 넣고 새 cursor 를 돌려준다 — 페이지가 꽉 찼으면 이어서 읽는다
/// (전역 last_id 로 뛰면 그 사이를 잃는다). 조회 실패면 cursor 그대로(다음 신호에 다시).
/// 클라이언트가 떠났으면 `None`.
fn drain(
    peer: &Peer<RoleServer>,
    handle: &tokio::runtime::Handle,
    agent: &ureq::Agent,
    base_url: &str,
    mut cursor: i64,
) -> Option<i64> {
    loop {
        let Some(feed) = fetch_changes(agent, base_url, cursor, PAGE as i64) else {
            return Some(cursor);
        };
        for event in pr_channel_events(&feed.entries) {
            let notification = channel_notification(&event.content, &event.meta);
            if handle
                .block_on(peer.send_notification(notification))
                .is_err()
            {
                return None;
            }
        }
        let (next, more) = page_cursor(&feed, PAGE);
        cursor = next;
        if !more {
            return Some(cursor);
        }
    }
}

fn run(peer: &Peer<RoleServer>, base_url: &str, handle: &tokio::runtime::Handle) {
    // SSE 는 열어 둔 채 기다리는 연결이라 전역 timeout 을 두지 않는다 — 연결 단계만 제한.
    let stream_agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(3)))
        .timeout_global(None)
        .build()
        .into();
    let fetch_agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(5)))
        .build()
        .into();
    let mut cursor: Option<i64> = None;
    let mut attempt = 0usize;
    loop {
        // watermark — 시작 이후의 전이만. 데몬이 없으면 30초 뒤 다시.
        if cursor.is_none() {
            cursor = fetch_changes(&fetch_agent, base_url, 0, 1).map(|f| f.last_id);
            if cursor.is_none() {
                thread::sleep(NO_DAEMON_RETRY);
                continue;
            }
        }
        if let Ok(mut response) = stream_agent.get(format!("{base_url}/api/events")).call() {
            if response.status().is_success() {
                attempt = 0;
                // 붙자마자 한 번 따라잡는다 — 스트림은 재생하지 않으므로, 마지막 조회와 구독 성립
                // 사이(첫 구독의 레이스·재연결 공백)에 난 전이는 다음 `data:` 를 기다리면 못 본다.
                match drain(
                    peer,
                    handle,
                    &fetch_agent,
                    base_url,
                    cursor.unwrap_or_default(),
                ) {
                    Some(next) => cursor = Some(next),
                    None => return,
                }
                let reader = BufReader::new(response.body_mut().as_reader());
                for line in reader.lines() {
                    let Ok(line) = line else { break };
                    if !line.starts_with("data:") {
                        continue;
                    }
                    match drain(
                        peer,
                        handle,
                        &fetch_agent,
                        base_url,
                        cursor.unwrap_or_default(),
                    ) {
                        Some(next) => cursor = Some(next),
                        // 클라이언트가 떠났다 — 프로세스도 곧 끝난다.
                        None => return,
                    }
                }
            }
        }
        thread::sleep(backoff_for(attempt));
        attempt += 1;
    }
}
