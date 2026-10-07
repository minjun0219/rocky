//! notify-todo 훅과 rocky 채널의 **배선** — 순수 판정(`rocky_core::notify`)은 core 테스트가 고정하고, 여기서는 진짜
//! 소켓의 가짜 데몬으로 조회 순서·실패 처리·커서를 본다: 구독한 PR 만 싣는지, 구독·세션 조회가 실패하면 커서를
//! 넘기지 않는지, 세션이 이미 받아들인 받은편지함 메시지를 빼는지.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;

use rocky_cli::channel::{read_page, Page};
use rocky_cli::client::build_context;
use rocky_cli::hooks::{notify_agy_context, notify_todo_context};
use rocky_core::notify::read_cursor;
use serde_json::{json, Value};

/// 경로(쿼리 앞까지)별 응답 — 테스트 도중 바꿀 수 있다. 없는 경로는 404.
#[derive(Clone, Default)]
struct FakeDaemon {
    routes: Arc<Mutex<HashMap<String, (u16, String)>>>,
    port: u16,
}

impl FakeDaemon {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let fake = FakeDaemon {
            routes: Arc::default(),
            port: listener.local_addr().expect("addr").port(),
        };
        let routes = fake.routes.clone();
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut buf = [0u8; 16384];
                let read = stream.read(&mut buf).unwrap_or(0);
                let raw = String::from_utf8_lossy(&buf[..read]).to_string();
                let path = raw
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("")
                    .split('?')
                    .next()
                    .unwrap_or("")
                    .to_string();
                let (status, body) = routes
                    .lock()
                    .unwrap()
                    .get(&path)
                    .cloned()
                    .unwrap_or((404, r#"{"error":"not found"}"#.to_string()));
                let response = format!(
                    "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        fake
    }

    fn set(&self, path: &str, status: u16, body: Value) {
        self.routes
            .lock()
            .unwrap()
            .insert(path.to_string(), (status, body.to_string()));
    }

    fn base(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

/// 변경 피드의 PR 전이 한 건.
fn pr_entry(id: i64, action: &str, number: i64) -> Value {
    json!({
        "id": id, "entity": "board", "entityId": "b1", "actor": "rocky", "action": action,
        "changes": { "repo": "o/r", "number": number, "title": format!("PR {number}"),
                     "url": format!("https://github.com/o/r/pull/{number}") },
        "at": "2026-10-05T10:00:00.000Z", "title": "rocky", "boardKey": "rocky"
    })
}

fn feed(last_id: i64, entries: Vec<Value>) -> Value {
    json!({ "lastId": last_id, "entries": entries })
}

/// 이 세션이 #7 을, 다른 세션이 #8 을 구독했다. 보드는 없다(세션 cwd 가 어느 보드로도 안 풀린다 → 구독만으로 고른다).
fn daemon_with_subscriptions(session: &str) -> FakeDaemon {
    let fake = FakeDaemon::start();
    fake.set("/api/boards", 200, json!([]));
    fake.set("/api/handoffs/claim", 204, json!(null));
    fake.set(
        "/api/prs/subscriptions",
        200,
        json!([
            { "repo": "o/r", "number": 7, "sessionId": session, "createdAt": "2026-10-05T00:00:00Z" },
            { "repo": "o/r", "number": 8, "sessionId": "someone-else", "createdAt": "2026-10-05T00:00:00Z" }
        ]),
    );
    fake
}

/// 첫 프롬프트는 워터마크만 적고, 다음 프롬프트부터 이 세션이 구독한 PR 의 전이만 싣는다 — 남의 PR(#8)은 빠진다.
#[test]
fn the_hook_injects_only_transitions_of_prs_this_session_subscribed() {
    let fake = daemon_with_subscriptions("me");
    let dir = tempfile::tempdir().unwrap();
    let ctx = build_context(fake.port, dir.path(), "test");
    let cursors = dir.path().join("hook-cursors.json");
    let input = json!({ "session_id": "me", "cwd": "/nowhere" });

    fake.set("/api/changes", 200, feed(10, vec![]));
    assert_eq!(
        notify_todo_context(&ctx, &input),
        None,
        "첫 프롬프트는 과거를 싣지 않는다"
    );
    assert_eq!(read_cursor(&cursors, "me"), Some(10));

    fake.set(
        "/api/changes",
        200,
        feed(
            12,
            vec![pr_entry(11, "pr-ready", 7), pr_entry(12, "pr-conflict", 8)],
        ),
    );
    let text = notify_todo_context(&ctx, &input).expect("구독한 #7 은 실린다");
    assert!(text.contains("#7 머지 후보"), "{text}");
    assert!(!text.contains("#8"), "남이 구독한 PR 은 빠진다 — {text}");
    assert_eq!(read_cursor(&cursors, "me"), Some(12));
}

/// 구독 조회가 실패하면 이 세션 것인지 가를 수 없다 — 창을 통째로 미루고 커서를 넘기지 않아, 다음 턴에 다시 받는다.
#[test]
fn a_failed_subscription_lookup_holds_the_cursor_until_the_next_turn() {
    let fake = daemon_with_subscriptions("me");
    let dir = tempfile::tempdir().unwrap();
    let ctx = build_context(fake.port, dir.path(), "test");
    let cursors = dir.path().join("hook-cursors.json");
    let input = json!({ "session_id": "me", "cwd": "/nowhere" });
    fake.set("/api/changes", 200, feed(10, vec![]));
    notify_todo_context(&ctx, &input);

    fake.set(
        "/api/changes",
        200,
        feed(11, vec![pr_entry(11, "pr-ready", 7)]),
    );
    fake.set("/api/prs/subscriptions", 500, json!({ "error": "db" }));
    assert_eq!(notify_todo_context(&ctx, &input), None);
    assert_eq!(
        read_cursor(&cursors, "me"),
        Some(10),
        "커서를 넘기지 않는다"
    );

    fake.set(
        "/api/prs/subscriptions",
        200,
        json!([{ "repo": "o/r", "number": 7, "sessionId": "me", "createdAt": "2026-10-05T00:00:00Z" }]),
    );
    let text = notify_todo_context(&ctx, &input).expect("다음 턴에 같은 창을 다시 받는다");
    assert!(text.contains("#7 머지 후보"), "{text}");
    assert_eq!(read_cursor(&cursors, "me"), Some(11));
}

/// 받은편지함으로 와서 세션이 이미 받아들인 전이는 다시 넣지 않는다 — 트랜스크립트의 peer 기록이든, 이 턴을 연
/// 프롬프트 자체든. 받아들이지 않은 것(승인 창에서 거절 등 — 기록이 없다)은 넣는다.
#[test]
fn transitions_already_absorbed_from_the_inbox_are_not_injected_again() {
    let fake = daemon_with_subscriptions("me");
    fake.set(
        "/api/prs/subscriptions",
        200,
        json!([
            { "repo": "o/r", "number": 7, "sessionId": "me", "createdAt": "2026-10-05T00:00:00Z" },
            { "repo": "o/r", "number": 9, "sessionId": "me", "createdAt": "2026-10-05T00:00:00Z" }
        ]),
    );
    let dir = tempfile::tempdir().unwrap();
    let ctx = build_context(fake.port, dir.path(), "test");
    let transcript = dir.path().join("t.jsonl");
    let peer = json!({
        "type": "user", "isMeta": true, "origin": { "kind": "peer", "from": "unknown" },
        "timestamp": "2026-10-05T10:00:05.000Z",
        "message": { "role": "user", "content": "<cross-session-message from=\"unknown\">\nrocky: o/r #7 머지 후보 — PR 7\n</cross-session-message>" }
    });
    std::fs::write(&transcript, format!("{peer}\n")).unwrap();
    let input = |prompt: &str| {
        json!({ "session_id": "me", "cwd": "/nowhere",
                "transcript_path": transcript.to_string_lossy(), "prompt": prompt })
    };
    fake.set("/api/changes", 200, feed(10, vec![]));
    notify_todo_context(&ctx, &input("시작"));

    fake.set(
        "/api/changes",
        200,
        feed(
            12,
            vec![pr_entry(11, "pr-ready", 7), pr_entry(12, "pr-ready", 9)],
        ),
    );
    let text = notify_todo_context(&ctx, &input("일하자")).expect("#9 는 받아들인 적이 없다");
    assert!(
        !text.contains("#7"),
        "트랜스크립트에 받아들인 기록 — {text}"
    );
    assert!(text.contains("#9 머지 후보"), "{text}");

    // 이 턴을 연 프롬프트가 곧 받은편지함 메시지(쉬던 세션) — 아직 트랜스크립트에 없어도 뺀다.
    fake.set(
        "/api/changes",
        200,
        feed(13, vec![pr_entry(13, "pr-conflict", 9)]),
    );
    assert_eq!(
        notify_todo_context(&ctx, &input("rocky: o/r #9 충돌 — 풀어야 한다 — PR 9")),
        None
    );
}

/// 채널: 부모 pid 로 세션을 찾아 그 세션이 구독한 PR 만 보낸다. 세션 목록에 없으면 보내지 않고 cursor 는 넘긴다.
/// 세션·구독 조회가 실패하면 cursor 를 그대로 두고 다시 시도하게 한다(`Page::Retry`).
#[test]
fn the_channel_reads_a_page_for_the_session_found_by_its_parent_pid() {
    let fake = daemon_with_subscriptions("me");
    fake.set(
        "/api/changes",
        200,
        feed(
            12,
            vec![pr_entry(11, "pr-ready", 7), pr_entry(12, "pr-ready", 8)],
        ),
    );
    fake.set(
        "/api/sessions",
        200,
        json!({ "available": true, "sessions": [{ "pid": 4242, "sessionId": "me", "cwd": "/nowhere" }] }),
    );
    let agent: ureq::Agent = ureq::Agent::config_builder().build().into();
    let base = fake.base();

    let Page::Read { events, next, more } = read_page(&agent, &base, 4242, 10) else {
        panic!("읽혀야 한다");
    };
    assert_eq!((next, more), (12, false));
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0].meta["number"], "7");

    let Page::Read { events, next, .. } = read_page(&agent, &base, 9999, 10) else {
        panic!("모르는 세션도 읽히기는 한다");
    };
    assert!(events.is_empty(), "누구의 것인지 모르면 보내지 않는다");
    assert_eq!(next, 12);

    fake.set("/api/prs/subscriptions", 500, json!({ "error": "db" }));
    assert_eq!(read_page(&agent, &base, 4242, 10), Page::Retry);
    fake.set("/api/sessions", 500, json!({ "error": "agents" }));
    assert_eq!(read_page(&agent, &base, 4242, 10), Page::Retry);
}

/// 사람이 바꾼 보드 항목 한 건.
fn human_entry(id: i64) -> Value {
    json!({
        "id": id, "entity": "todo", "entityId": format!("t{id}"), "actor": "logan", "action": "update",
        "at": "2026-10-05T10:00:00.000Z", "title": format!("할 일 {id}"), "boardKey": "rocky"
    })
}

/// 오래 조용했던 세션은 변경이 100건 넘게 밀린다 — 꽉 찬 페이지면 응답의 `lastId`(전역 MAX)로 뛰지 않고 받은
/// 마지막 id 까지만 커서를 옮기고, 남았다는 한 줄을 붙여 다음 턴에 이어 싣는다(예전엔 그 사이를 영영 건너뛰었다).
#[test]
fn a_full_page_advances_only_to_the_last_received_entry() {
    let fake = daemon_with_subscriptions("me");
    let dir = tempfile::tempdir().unwrap();
    let ctx = build_context(fake.port, dir.path(), "test");
    let cursors = dir.path().join("hook-cursors.json");
    let input = json!({ "session_id": "me", "cwd": "/nowhere" });
    fake.set("/api/changes", 200, feed(10, vec![]));
    notify_todo_context(&ctx, &input);

    fake.set(
        "/api/changes",
        200,
        feed(250, (11..=110).map(human_entry).collect()),
    );
    let text = notify_todo_context(&ctx, &input).expect("첫 페이지");
    assert!(text.contains("할 일 110"), "{text}");
    assert!(text.contains("밀린 변경이 더 있다"), "{text}");
    assert_eq!(
        read_cursor(&cursors, "me"),
        Some(110),
        "전역 MAX(250)로 뛰지 않는다"
    );

    fake.set(
        "/api/changes",
        200,
        feed(250, (111..=120).map(human_entry).collect()),
    );
    let text = notify_todo_context(&ctx, &input).expect("다음 턴에 이어서");
    assert!(
        text.contains("할 일 111") && !text.contains("밀린 변경이 더 있다"),
        "{text}"
    );
    assert_eq!(
        read_cursor(&cursors, "me"),
        Some(250),
        "다 받았으면 lastId 로 맞춘다"
    );
}

/// 보드 항목에 actor 가 한 일 한 건.
fn entry_by(id: i64, actor: &str, action: &str) -> Value {
    json!({
        "id": id, "entity": "todo", "entityId": format!("t{id}"), "actor": actor, "action": action,
        "at": "2026-10-05T10:00:00.000Z", "title": format!("할 일 {id}"), "boardKey": "rocky"
    })
}

/// agy 대화도 첫 호출은 워터마크만 적고, 다음 호출부터 사람의 변경만 싣는다 — agy 자신(`antigravity`)과 다른 에이전트의
/// 변경은 빠진다. 커서는 Claude Code 세션의 파일과 따로 둔다.
#[test]
fn agy_gets_only_human_changes_keyed_by_conversation() {
    let fake = FakeDaemon::start();
    let dir = tempfile::tempdir().unwrap();
    let ctx = build_context(fake.port, dir.path(), "test");
    let cursors = dir.path().join("hook-cursors-agy.json");
    let input = json!({ "conversationId": "conv-1", "invocationNum": 0, "workspacePaths": ["/w"] });

    fake.set("/api/changes", 200, feed(10, vec![]));
    assert_eq!(
        notify_agy_context(&ctx, &input),
        None,
        "첫 호출은 과거를 싣지 않는다"
    );
    assert_eq!(read_cursor(&cursors, "conv-1"), Some(10));
    assert!(
        !dir.path().join("hook-cursors.json").exists(),
        "Claude Code 세션의 커서 파일을 건드리지 않는다"
    );

    fake.set(
        "/api/changes",
        200,
        feed(
            13,
            vec![
                entry_by(11, "antigravity", "start"),
                entry_by(12, "logan", "comment"),
                entry_by(13, "claude-code", "update"),
            ],
        ),
    );
    let text = notify_agy_context(&ctx, &input).expect("사람의 댓글은 실린다");
    assert!(
        text.contains("logan") && text.contains("할 일 12"),
        "{text}"
    );
    assert!(
        !text.contains("할 일 11") && !text.contains("할 일 13"),
        "agy 자신과 에이전트의 변경은 빠진다 — {text}"
    );
    assert_eq!(read_cursor(&cursors, "conv-1"), Some(13));

    // 커서 13 뒤로는 새 변경이 없다(가짜 데몬은 sinceId 를 보지 않으니 빈 피드를 직접 준다).
    fake.set("/api/changes", 200, feed(13, vec![]));
    assert_eq!(
        notify_agy_context(&ctx, &input),
        None,
        "같은 변경을 다음 모델 호출에 다시 싣지 않는다"
    );
    assert_eq!(
        notify_agy_context(&ctx, &json!({ "session_id": "conv-1" })),
        None,
        "conversationId 가 없으면 아무것도 하지 않는다"
    );
}

/// 바이너리 입구 — `rocky hook notify-todo agy` 는 agy 가 읽는 모양(`injectSteps[].ephemeralMessage`)을 내고, 실을 게
/// 없으면 `{}` 를 낸다.
#[test]
fn the_agy_entry_prints_inject_steps_json() {
    use std::process::{Command, Stdio};
    let fake = FakeDaemon::start();
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("rocky.json");
    let todo_dir = dir.path().join("todo");
    std::fs::write(
        &config,
        json!({ "todo": { "port": fake.port, "dir": todo_dir, "expose": "off" } }).to_string(),
    )
    .unwrap();
    let run = || -> Value {
        let mut child = Command::new(env!("CARGO_BIN_EXE_rocky"))
            .args(["hook", "notify-todo", "agy"])
            .env("HOME", dir.path())
            .env("ROCKY_USAGE_DIR", dir.path().join("usage"))
            .env("ROCKY_CONFIG", &config)
            .env_remove("ROCKY_TODO_WATCH")
            .env_remove("ROCKY_TODO_PORT")
            .env_remove("ROCKY_TODO_DIR")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(
                json!({ "conversationId": "conv-9", "invocationNum": 0 })
                    .to_string()
                    .as_bytes(),
            )
            .unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success());
        serde_json::from_slice(&out.stdout).expect("stdout 은 JSON 하나다")
    };

    fake.set("/api/changes", 200, feed(10, vec![]));
    assert_eq!(run(), json!({}));
    fake.set(
        "/api/changes",
        200,
        feed(11, vec![entry_by(11, "logan", "comment")]),
    );
    let out = run();
    let message = out["injectSteps"][0]["ephemeralMessage"]
        .as_str()
        .unwrap_or_else(|| panic!("{out}"));
    assert!(message.contains("호출자의 보드 변경"), "{message}");
}
