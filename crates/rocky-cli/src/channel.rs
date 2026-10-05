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
//! 전달 규칙은 훅 주입과 같다(`rocky_core::notify::pr_channel_events` — ready·conflict 만, **이 세션이 구독한
//! PR 만**, 세션 보드가 풀리면 그 보드 것만). 세션은 이 서버의 부모 프로세스(= Claude Code)의 pid 로 데몬
//! `/api/sessions` 에서 찾는다 — MCP 서버 env 의 `CLAUDE_CODE_SESSION_ID` 는 서버를 띄울 때의 id 라, `/clear`·resume
//! 으로 세션 id 가 바뀌어도 서버는 다시 뜨지 않고 옛 id 가 남는다(2.1.288 실측 — 떠 있던 세션 다섯 전부 달랐다). 시작
//! 시점의 watermark 이후 것만 보내고 과거는 재생하지 않는다. SSE 는 TUI 와 같은 방식(한 줄씩,
//! `data:` 가 오면 `/api/changes` 를 차분 조회, 끊기면 1·2·4·8초 백오프).

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::thread;
use std::time::Duration;

use rmcp::model::{CustomNotification, JsonObject, ServerNotification};
use rmcp::service::Peer;
use rmcp::RoleServer;
use rocky_core::notify::{page_cursor, pr_channel_events, pr_entries_for_session, PrChannelEvent};
use rocky_core::prwatch::PrSubscription;
use rocky_core::statusline::{board_key_for_cwd, BoardLocation};
use rocky_core::types::ChangesSince;

/// Claude Code 가 채널로 인식하는 capability 키 — `capabilities.experimental` 아래.
pub const CHANNEL_CAPABILITY: &str = "claude/channel";
/// 세션에 밀어 넣는 notification 의 method.
pub const CHANNEL_METHOD: &str = "notifications/claude/channel";

/// 서버가 붙을 때 Claude 에게 주는 안내 — 이벤트가 무엇이고 무엇을 하라는 것인지.
pub const INSTRUCTIONS: &str = "rocky 채널: 데몬의 PR 감시 전이가 <channel source=\"…\" kind=\"ready|conflict\" repo=\"owner/name\" number=\"N\" url=\"…\"> 로 온다. 단방향이다(회신 도구 없음). ready 는 \"머지 후보\"(기계 판정) — `/rocky:review-fix` 8단계대로 판단한 뒤 사람에게 알린다(PushNotification 이 있으면 그것으로, 한 줄). 이 세션이 구독한 PR 만 온다 — conflict 는 충돌을 푼다. 감시를 따로 돌리지 말고, 머지는 사람 몫이다.";

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

fn get_json<T: serde::de::DeserializeOwned>(agent: &ureq::Agent, url: &str) -> Option<T> {
    let mut response = agent.get(url).call().ok()?;
    if !(200..300).contains(&response.status().as_u16()) {
        return None;
    }
    response.body_mut().read_json().ok()
}

/// 이 서버를 띄운 세션 — `(session_id, 보드 key)`. 세션 목록·보드 목록을 못 읽으면 `Err`(다시 시도),
/// 목록에 이 pid 가 없으면 `Ok(None)`(보낼 세션이 없다).
fn this_session(
    agent: &ureq::Agent,
    base_url: &str,
    pid: i64,
) -> Result<Option<(String, Option<String>)>, ()> {
    let sessions: serde_json::Value =
        get_json(agent, &format!("{base_url}/api/sessions")).ok_or(())?;
    let Some(session) = sessions
        .get("sessions")
        .and_then(|s| s.as_array())
        .and_then(|list| {
            list.iter()
                .find(|s| s.get("pid").and_then(|p| p.as_i64()) == Some(pid))
        })
    else {
        return Ok(None);
    };
    let Some(session_id) = session.get("sessionId").and_then(|v| v.as_str()) else {
        return Ok(None);
    };
    let cwd = session.get("cwd").and_then(|v| v.as_str());
    let boards: Vec<serde_json::Value> =
        get_json(agent, &format!("{base_url}/api/boards")).ok_or(())?;
    let locations: Vec<BoardLocation> = boards
        .iter()
        .filter_map(|b| {
            Some(BoardLocation {
                key: b.get("key")?.as_str()?.to_string(),
                path: b.get("path").and_then(|p| p.as_str()).map(str::to_string),
            })
        })
        .collect();
    Ok(Some((
        session_id.to_string(),
        board_key_for_cwd(&locations, cwd),
    )))
}

/// `drain` 의 결과.
enum Drain {
    /// 여기까지 보냈다 — 다음 신호를 기다린다.
    Caught(i64),
    /// 조회가 실패해 이 cursor 에서 멈췄다 — SSE 는 다음 변경까지 아무것도 주지 않으므로(heartbeat 없음) 연결을
    /// 끊고 백오프 뒤 다시 붙어, 붙자마자 도는 따라잡기로 재시도한다.
    Retry(i64),
    /// 클라이언트가 떠났다.
    Gone,
}

/// 변경 피드 한 페이지를 읽은 결과 — 보내기는 부르는 쪽(`drain`)이 한다.
#[derive(Debug, PartialEq, Eq)]
pub enum Page {
    /// 이 세션에 보낼 이벤트와 다음 cursor·이어 읽을지.
    Read {
        events: Vec<PrChannelEvent>,
        next: i64,
        more: bool,
    },
    /// 변경 피드·세션·보드·구독 조회가 실패했다 — cursor 를 넘기지 않는다.
    Retry,
}

/// cursor 이후 한 페이지를 읽어 **이 세션**(부모 pid `pid` 로 `/api/sessions` 에서 찾는다)에 보낼 PR 전이를 고른다.
/// 세션 목록에 그 pid 가 없으면 누구의 것인지 모르니 아무것도 보내지 않고 cursor 는 넘긴다.
pub fn read_page(agent: &ureq::Agent, base_url: &str, pid: i64, cursor: i64) -> Page {
    let Some(feed) = fetch_changes(agent, base_url, cursor, PAGE as i64) else {
        return Page::Retry;
    };
    let has_pr = feed
        .entries
        .iter()
        .any(|e| e.history.action.starts_with("pr-"));
    let mine = if has_pr {
        let Ok(session) = this_session(agent, base_url, pid) else {
            return Page::Retry;
        };
        match session {
            Some((session_id, board)) => {
                let Some(subscriptions) = get_json::<Vec<PrSubscription>>(
                    agent,
                    &format!("{base_url}/api/prs/subscriptions"),
                ) else {
                    return Page::Retry;
                };
                pr_entries_for_session(&feed.entries, board.as_deref(), &session_id, &subscriptions)
            }
            None => Vec::new(),
        }
    } else {
        Vec::new()
    };
    let (next, more) = page_cursor(&feed, PAGE);
    Page::Read {
        events: pr_channel_events(&mine),
        next,
        more,
    }
}

/// cursor 이후의 전이를 전부 밀어 넣는다 — 페이지가 꽉 찼으면 이어서 읽는다(전역 last_id 로 뛰면 그 사이를
/// 잃는다). 조회가 실패하면 그 페이지의 cursor 에서 멈춘다.
fn drain(
    peer: &Peer<RoleServer>,
    handle: &tokio::runtime::Handle,
    agent: &ureq::Agent,
    base_url: &str,
    pid: i64,
    mut cursor: i64,
) -> Drain {
    loop {
        let Page::Read { events, next, more } = read_page(agent, base_url, pid, cursor) else {
            return Drain::Retry(cursor);
        };
        for event in events {
            let notification = channel_notification(&event.content, &event.meta);
            if handle
                .block_on(peer.send_notification(notification))
                .is_err()
            {
                return Drain::Gone;
            }
        }
        cursor = next;
        if !more {
            return Drain::Caught(cursor);
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
    // 부모 = 이 stdio 서버를 띄운 Claude Code 프로세스.
    let pid = i64::from(std::os::unix::process::parent_id());
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
                // 붙자마자 한 번 따라잡는다 — 스트림은 재생하지 않으므로, 마지막 조회와 구독 성립
                // 사이(첫 구독의 레이스·재연결 공백)에 난 전이는 다음 `data:` 를 기다리면 못 본다.
                // 따라잡기가 끝나야 연결이 성한 것으로 보고 백오프를 되돌린다 — 조회가 계속 실패하면 늘어난다.
                let caught = match drain(
                    peer,
                    handle,
                    &fetch_agent,
                    base_url,
                    pid,
                    cursor.unwrap_or_default(),
                ) {
                    Drain::Caught(next) => {
                        cursor = Some(next);
                        attempt = 0;
                        true
                    }
                    Drain::Retry(next) => {
                        cursor = Some(next);
                        false
                    }
                    Drain::Gone => return,
                };
                let reader = BufReader::new(response.body_mut().as_reader());
                if caught {
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
                            pid,
                            cursor.unwrap_or_default(),
                        ) {
                            Drain::Caught(next) => cursor = Some(next),
                            // 끊고 다시 붙는다 — 다음 변경이 언제 올지 모르니 기다리지 않는다.
                            Drain::Retry(next) => {
                                cursor = Some(next);
                                break;
                            }
                            // 클라이언트가 떠났다 — 프로세스도 곧 끝난다.
                            Drain::Gone => return,
                        }
                    }
                }
            }
        }
        thread::sleep(backoff_for(attempt));
        attempt += 1;
    }
}
