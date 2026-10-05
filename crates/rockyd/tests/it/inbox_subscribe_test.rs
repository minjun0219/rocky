//! 수집함 구독 — 세션이 소스를 구독하면 그 뒤에 생긴 항목만 그 세션 받은편지함(유닉스 소켓)에 알린다.

use std::io::Read;
use std::os::unix::net::UnixListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use crate::common::*;
use rocky_core::config::InboxSource;
use rockyd::runner::{CmdOutput, Runner};
use serde_json::json;

/// 부를 때마다 항목이 하나씩 는다 — 첫 호출(구독 기준선)은 1·2, 다음은 1·2·3 …
fn growing(calls: Arc<AtomicUsize>) -> Runner {
    Arc::new(move |_cmd, _stdin, _timeout| {
        let n = calls.fetch_add(1, Ordering::SeqCst) + 2;
        let items: Vec<_> = (1..=n)
            .map(|i| json!({ "id": i.to_string(), "title": format!("버그 {i}"), "url": format!("https://x/{i}") }))
            .collect();
        let out = CmdOutput {
            code: 0,
            stdout: json!({ "items": items }).to_string(),
            stderr: String::new(),
        };
        Box::pin(async move { out })
    })
}

/// Claude Code 받은편지함 모양의 소켓(`…/cc-socks-*/<숫자>.sock`)을 연다.
fn inbox_socket(dir: &std::path::Path) -> (UnixListener, String) {
    let socks = dir.join("cc-socks-test");
    std::fs::create_dir_all(&socks).unwrap();
    let path = socks.join("42.sock");
    let listener = UnixListener::bind(&path).unwrap();
    listener.set_nonblocking(true).unwrap();
    (listener, path.to_string_lossy().to_string())
}

fn drain(listener: &UnixListener) -> Vec<String> {
    let mut out = Vec::new();
    while let Ok((mut stream, _)) = listener.accept() {
        stream.set_nonblocking(false).unwrap();
        let mut text = String::new();
        stream.read_to_string(&mut text).unwrap();
        out.push(text);
    }
    out
}

fn fx_sub(calls: Arc<AtomicUsize>) -> Fx {
    fx_with(|o| {
        o.gh_runner = Some(growing(calls));
        o.inbox_sources = vec![InboxSource {
            name: "gh-bugs".into(),
            command: vec!["adapter".into()],
            timeout_ms: None,
        }];
    })
}

#[tokio::test]
async fn only_items_after_subscribing_reach_the_session_once() {
    let f = fx_sub(Arc::default());
    let dir = tempfile::tempdir().unwrap();
    let (listener, socket) = inbox_socket(dir.path());
    let (status, res) = post(
        &f.state,
        "/api/inbox/subscriptions",
        json!({ "source": "gh-bugs", "sessionId": "s1", "socket": socket }),
    )
    .await;
    assert_eq!(status, 200, "{res}");
    assert_eq!(res["baseline"], 2, "구독 시점의 항목은 기준선");

    assert_eq!(rockyd::inbox_watch::tick(&f.state).await, 1);
    let got = drain(&listener);
    assert_eq!(got.len(), 1);
    assert!(
        got[0].contains("버그 3") && got[0].contains("https://x/3"),
        "{}",
        got[0]
    );
    assert!(got[0].contains("알리기만"), "착수는 사람이 — {}", got[0]);
    assert!(!got[0].contains("버그 1 "));

    // 해지하면 새 항목이 생겨도 조용하다.
    let (status, _) = call(
        &f.state,
        "DELETE",
        "/api/inbox/subscriptions?sessionId=s1&source=gh-bugs",
        None,
        ReqOptions::default(),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(rockyd::inbox_watch::tick(&f.state).await, 0);
}

#[tokio::test]
async fn a_session_whose_inbox_is_gone_is_unsubscribed() {
    let f = fx_sub(Arc::default());
    let dir = tempfile::tempdir().unwrap();
    let (listener, socket) = inbox_socket(dir.path());
    post(
        &f.state,
        "/api/inbox/subscriptions",
        json!({ "source": "gh-bugs", "sessionId": "s1", "socket": socket }),
    )
    .await;
    drop(listener); // 세션이 끝났다
    assert_eq!(rockyd::inbox_watch::tick(&f.state).await, 0);
    let (_, list) = get(&f.state, "/api/inbox/subscriptions").await;
    assert_eq!(list, json!([]), "못 쓴 세션의 구독은 걷는다");
}

#[tokio::test]
async fn subscribing_is_local_only_and_checks_socket_and_source() {
    let f = fx_sub(Arc::default());
    let dir = tempfile::tempdir().unwrap();
    let (_listener, socket) = inbox_socket(dir.path());
    let remote = ReqOptions {
        headers: vec![("x-forwarded-for", "10.0.0.2")],
        ..ReqOptions::default()
    };
    let body = json!({ "source": "gh-bugs", "sessionId": "s1", "socket": socket });
    let (status, _) = call(
        &f.state,
        "POST",
        "/api/inbox/subscriptions",
        Some(body),
        remote,
    )
    .await;
    assert_eq!(status, 403);
    let (status, _) = post(
        &f.state,
        "/api/inbox/subscriptions",
        json!({ "source": "gh-bugs", "sessionId": "s1", "socket": "/var/run/docker.sock" }),
    )
    .await;
    assert_eq!(status, 400, "받은편지함 모양이 아닌 소켓에는 쓰지 않는다");
    let (status, _) = post(
        &f.state,
        "/api/inbox/subscriptions",
        json!({ "source": "nope", "sessionId": "s1", "socket": socket }),
    )
    .await;
    assert_eq!(status, 404);
}

/// 두 번째 세션의 구독이 첫 세션이 아직 못 받은 항목을 삼키면 안 된다 — 본 항목은 세션마다 따로다.
/// (negative control: 기준선을 소스 공용으로 되돌리면 A 가 "버그 3" 을 못 받아 실패한다 — 작성 때 확인.)
#[tokio::test]
async fn a_second_subscriber_does_not_swallow_the_first_ones_new_items() {
    let f = fx_sub(Arc::default());
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();
    let (a, socket_a) = inbox_socket(dir_a.path());
    let (b, socket_b) = inbox_socket(dir_b.path());
    // A 구독(항목 1·2) → 3 이 생긴 뒤 B 구독(기준선 1·2·3) → tick(1~4)
    post(
        &f.state,
        "/api/inbox/subscriptions",
        json!({ "source": "gh-bugs", "sessionId": "A", "socket": socket_a }),
    )
    .await;
    post(
        &f.state,
        "/api/inbox/subscriptions",
        json!({ "source": "gh-bugs", "sessionId": "B", "socket": socket_b }),
    )
    .await;
    assert_eq!(rockyd::inbox_watch::tick(&f.state).await, 2);
    let got_a = drain(&a).join("");
    let got_b = drain(&b).join("");
    assert!(
        got_a.contains("버그 3") && got_a.contains("버그 4"),
        "A: {got_a}"
    );
    assert!(
        got_a.contains("새 항목 2건"),
        "한 번에 메시지 하나 — {got_a}"
    );
    assert!(
        got_b.contains("버그 4") && !got_b.contains("버그 3"),
        "B: {got_b}"
    );
}

/// 같은 이름의 소스가 다른 명령으로 바뀌면(지우고 다시 등록 등) 쏟아내지 않고 기준선을 다시 잡는다.
#[tokio::test]
async fn a_source_whose_command_changed_is_rebaselined_not_flooded() {
    let calls: Arc<AtomicUsize> = Arc::default();
    let f = fx_sub(calls.clone());
    let dir = tempfile::tempdir().unwrap();
    let (listener, socket) = inbox_socket(dir.path());
    post(
        &f.state,
        "/api/inbox/subscriptions",
        json!({ "source": "gh-bugs", "sessionId": "s1", "socket": socket }),
    )
    .await;
    // 설정이 바뀐 것과 같게 — 같은 이름, 다른 argv 로 서버를 다시 만든다(스토어는 그대로).
    let state = rebuild(&f, |o| {
        o.gh_runner = Some(growing(calls.clone()));
        o.inbox_sources = vec![InboxSource {
            name: "gh-bugs".into(),
            command: vec!["adapter".into(), "--other".into()],
            timeout_ms: None,
        }];
    });
    // 재시작한 데몬은 세션 등록이 비어 있다 — 다음 턴의 훅처럼 다시 등록한다.
    post(
        &state,
        "/api/sessions/inbox",
        json!({ "sessionId": "s1", "socket": socket, "cwd": "/w/x" }),
    )
    .await;
    assert_eq!(
        rockyd::inbox_watch::tick(&state).await,
        0,
        "지문이 바뀐 첫 tick 은 기준선만"
    );
    assert!(drain(&listener).is_empty());
    assert_eq!(
        rockyd::inbox_watch::tick(&state).await,
        1,
        "그 뒤에 생긴 것만"
    );
}

/// 한 번에 많이 들어와도 메시지는 하나 — 앞의 몇 건과 "외 N건".
#[test]
fn many_new_items_fold_into_one_message() {
    let items: Vec<rocky_core::inbox::InboxItem> = (1..=8)
        .map(|i| rocky_core::inbox::InboxItem {
            id: i.to_string(),
            title: format!("버그 {i}"),
            url: None,
            note: None,
            due: None,
            created_at: None,
            promoted: false,
        })
        .collect();
    let refs: Vec<&rocky_core::inbox::InboxItem> = items.iter().collect();
    let msg = rocky_core::peer_inbox::inbox_item_message("gh-bugs", &refs);
    assert!(
        msg.contains("새 항목 8건") && msg.contains("외 3건"),
        "{msg}"
    );
    assert!(msg.contains("버그 5") && !msg.contains("버그 6"));
}

/// 세션이 이어 열려 소켓이 바뀌면(훅이 새 소켓을 등록) 그쪽으로 보낸다 — 옛 소켓에 실패해 구독이 사라지지 않게.
#[tokio::test]
async fn a_resumed_session_is_reached_on_its_new_socket() {
    let f = fx_sub(Arc::default());
    let dir_old = tempfile::tempdir().unwrap();
    let dir_new = tempfile::tempdir().unwrap();
    let (old, socket_old) = inbox_socket(dir_old.path());
    post(
        &f.state,
        "/api/inbox/subscriptions",
        json!({ "source": "gh-bugs", "sessionId": "s1", "socket": socket_old }),
    )
    .await;
    drop(old);
    let (new, socket_new) = inbox_socket(dir_new.path());
    let (status, _) = post(
        &f.state,
        "/api/sessions/inbox",
        json!({ "sessionId": "s1", "socket": socket_new, "cwd": "/w/x" }),
    )
    .await;
    assert_eq!(status, 204);
    assert_eq!(rockyd::inbox_watch::tick(&f.state).await, 1);
    assert_eq!(drain(&new).len(), 1);
    let (_, list) = get(&f.state, "/api/inbox/subscriptions").await;
    assert_eq!(list.as_array().unwrap().len(), 1, "구독은 그대로");
}

/// 살아 있는 등록이 없으면 구독 때 적은 소켓으로 보내지 않는다 — 끝난 세션의 숫자 소켓 경로를 다른
/// 세션이 다시 쓰고 있으면 엉뚱한 세션에 간다(Codex 지적). 본 것으로 적지 않고 미뤘다가 등록이 돌아오면 보낸다.
#[tokio::test]
async fn without_a_live_registration_nothing_is_sent_to_the_stored_socket() {
    let f = fx_sub(Arc::default());
    let dir = tempfile::tempdir().unwrap();
    let (listener, socket) = inbox_socket(dir.path());
    post(
        &f.state,
        "/api/inbox/subscriptions",
        json!({ "source": "gh-bugs", "sessionId": "s1", "socket": socket }),
    )
    .await;
    f.state.forget_inbox("s1"); // 등록이 사라졌다(TTL·끝난 세션)
    assert_eq!(rockyd::inbox_watch::tick(&f.state).await, 0);
    assert!(
        drain(&listener).is_empty(),
        "저장된 소켓으로 폴백하지 않는다"
    );
    // 등록이 돌아오면(다음 턴의 훅) 미뤘던 항목까지 보낸다.
    post(
        &f.state,
        "/api/sessions/inbox",
        json!({ "sessionId": "s1", "socket": socket, "cwd": "/w/x" }),
    )
    .await;
    assert_eq!(rockyd::inbox_watch::tick(&f.state).await, 1);
    let got = drain(&listener).join("");
    assert!(got.contains("버그 3") && got.contains("버그 4"), "{got}");
}

/// `/clear` — 같은 프로세스(같은 소켓)에서 세션 id 만 바뀌어 새 id 로 등록하면, 데몬은 옛 세션을 `/clear` 된 것으로
/// 적고 깨우지 않는다(맥락 없는 새 세션이 옛 PR·수집함 알림을 받지 않게). 구독은 그대로 남아 웹 "세션 전달" 에 뜨고,
/// 사람이 "새 세션으로 넘기기" 를 누르면 그때 새 세션이 받는다.
#[tokio::test]
async fn a_cleared_session_waits_for_the_web_to_decide() {
    let f = fx_sub(Arc::default());
    let dir = tempfile::tempdir().unwrap();
    let (listener, socket) = inbox_socket(dir.path());
    let register = |id: &'static str| {
        post(
            &f.state,
            "/api/sessions/inbox",
            json!({ "sessionId": id, "socket": socket, "cwd": "/w/x" }),
        )
    };
    assert_eq!(register("before-clear").await.0, 204);
    post(
        &f.state,
        "/api/inbox/subscriptions",
        json!({ "source": "gh-bugs", "sessionId": "before-clear", "socket": socket }),
    )
    .await;
    let (status, res) = post(
        &f.state,
        "/api/prs/subscriptions",
        json!({ "repo": "o/r", "number": 7, "sessionId": "before-clear" }),
    )
    .await;
    assert_eq!(status, 201, "{res}");

    assert_eq!(register("after-clear").await.0, 204);

    // 기다리는 동안 — 구독은 옛 id 에 남고(PR 감시는 이어진다) 아무도 깨우지 않는다.
    let (_, prs) = get(&f.state, "/api/prs/subscriptions").await;
    assert_eq!(prs[0]["sessionId"], "before-clear", "{prs}");
    let (_, status) = get(&f.state, "/api/deliveries").await;
    let cleared = &status["cleared"][0];
    assert_eq!(cleared["sessionId"], "before-clear", "{status}");
    assert_eq!(cleared["successorId"], "after-clear");
    assert_eq!(cleared["prs"][0], "o/r#7");
    assert_eq!(cleared["inbox"][0], "gh-bugs");
    assert_eq!(rockyd::inbox_watch::tick(&f.state).await, 0);
    assert!(
        drain(&listener).is_empty(),
        "맥락 없는 새 세션을 깨우지 않는다"
    );
    assert_eq!(rockyd::inbox_watch::tick(&f.state).await, 0);
    let (_, inbox) = get(&f.state, "/api/inbox/subscriptions").await;
    assert_eq!(
        inbox.as_array().unwrap().len(),
        1,
        "정할 때까지 걷지 않는다"
    );

    // 사람이 넘기기를 누른다.
    let (status, res) = post(
        &f.state,
        "/api/sessions/cleared",
        json!({ "sessionId": "before-clear", "action": "handover" }),
    )
    .await;
    assert_eq!(status, 200, "{res}");
    let (_, prs) = get(&f.state, "/api/prs/subscriptions").await;
    assert_eq!(prs[0]["sessionId"], "after-clear", "{prs}");
    let (_, status) = get(&f.state, "/api/deliveries").await;
    assert!(status["cleared"].as_array().unwrap().is_empty());
    assert_eq!(rockyd::inbox_watch::tick(&f.state).await, 1);
    assert_eq!(drain(&listener).len(), 1);

    // 두 번 정할 수는 없다.
    let (status, _) = post(
        &f.state,
        "/api/sessions/cleared",
        json!({ "sessionId": "before-clear", "action": "watch" }),
    )
    .await;
    assert_eq!(status, 404);
    let (status, _) = post(
        &f.state,
        "/api/sessions/cleared",
        json!({ "sessionId": "x", "action": "nope" }),
    )
    .await;
    assert_eq!(status, 400);
}

/// 소켓 이름은 pid 라 프로세스가 끝나고 pid 가 재사용되면 같은 경로가 남의 세션 것이 된다 — 지금 프로세스(소켓 파일)가
/// 생기기 전의 등록이면 구독을 넘기지 않는다.
#[tokio::test]
async fn a_recycled_socket_path_does_not_inherit_subscriptions() {
    let f = fx_sub(Arc::default());
    let dir = tempfile::tempdir().unwrap();
    let (_listener, socket) = inbox_socket(dir.path());
    let an_hour_ago = chrono::Utc::now().timestamp() - 3600;
    f.state
        .register_inbox(rocky_core::peer_inbox::InboxRegistration {
            session_id: "dead".into(),
            socket: socket.clone(),
            cwd: "/w/x".into(),
            seen_at: an_hour_ago,
            restored: false,
        });
    post(
        &f.state,
        "/api/prs/subscriptions",
        json!({ "repo": "o/r", "number": 7, "sessionId": "dead" }),
    )
    .await;

    post(
        &f.state,
        "/api/sessions/inbox",
        json!({ "sessionId": "stranger", "socket": socket, "cwd": "/w/x" }),
    )
    .await;

    let (_, prs) = get(&f.state, "/api/prs/subscriptions").await;
    assert_eq!(prs[0]["sessionId"], "dead", "{prs}");
}
