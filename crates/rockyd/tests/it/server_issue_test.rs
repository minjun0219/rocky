//! TS `src/server.test.ts` 포팅 2/4 — github issue + 출처 게이트 + 보드 메타.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use crate::common::*;
use rockyd::runner::{CmdOutput, Runner};
use serde_json::json;

/// 성공하는 fake gh — 실제 `gh` 를 절대 부르지 않는다.
fn ok_gh(url: &str) -> Runner {
    let url = url.to_string();
    Arc::new(move |_cmd, _stdin, _timeout| {
        let url = url.clone();
        Box::pin(async move {
            CmdOutput {
                code: 0,
                stdout: format!("{url}\n"),
                stderr: String::new(),
            }
        })
    })
}

fn counting_gh(counter: Arc<AtomicUsize>) -> Runner {
    Arc::new(move |_cmd, _stdin, _timeout| {
        counter.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            CmdOutput {
                code: 0,
                stdout: "https://github.com/o/n/issues/5\n".into(),
                stderr: String::new(),
            }
        })
    })
}

async fn todo_with_repo(state: &Arc<rockyd::server::ServerState>) -> String {
    let (_, created) = post(state, "/api/todos", json!({"board":"rocky","title":"작업"})).await;
    patch(state, "/api/boards/rocky", json!({"repo":"o/n"})).await;
    created["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn patch_board_sets_repo() {
    let f = fx();
    post(&f.state, "/api/boards", json!({"key":"rocky"})).await;
    let (status, board) = patch(&f.state, "/api/boards/rocky", json!({"repo":"o/n"})).await;
    assert_eq!(status, 200);
    assert_eq!(board["repo"], "o/n");
}

#[tokio::test]
async fn patch_rejects_malformed_slug_and_unknown_board() {
    let f = fx();
    post(&f.state, "/api/boards", json!({"key":"rocky"})).await;
    let (status, _) = patch(&f.state, "/api/boards/rocky", json!({"repo":"not-a-slug"})).await;
    assert_eq!(status, 400);
    let (status, _) = patch(&f.state, "/api/boards/nosuch", json!({"repo":"o/n"})).await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn issue_400_without_repo_404_unknown_todo() {
    let f = fx();
    let (_, created) = post(
        &f.state,
        "/api/todos",
        json!({"board":"rocky","title":"작업"}),
    )
    .await;
    let id = created["id"].as_str().unwrap();
    let (status, _) = call(
        &f.state,
        "POST",
        &format!("/api/todos/{id}/issue"),
        None,
        ReqOptions::default(),
    )
    .await;
    assert_eq!(status, 400);
    let (status, _) = call(
        &f.state,
        "POST",
        "/api/todos/nosuchid/issue",
        None,
        ReqOptions::default(),
    )
    .await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn issue_409_when_link_already_exists() {
    let f = fx();
    let (_, created) = post(
        &f.state,
        "/api/todos",
        json!({"board":"rocky","title":"작업","links":[{"url":"https://github.com/o/n/issues/3"}]}),
    )
    .await;
    patch(&f.state, "/api/boards/rocky", json!({"repo":"o/n"})).await;
    let id = created["id"].as_str().unwrap();
    let (status, body) = call(
        &f.state,
        "POST",
        &format!("/api/todos/{id}/issue"),
        None,
        ReqOptions::default(),
    )
    .await;
    assert_eq!(status, 409);
    assert_eq!(body["url"], "https://github.com/o/n/issues/3");
}

#[tokio::test]
async fn issue_accepts_body_repo_and_persists_to_board() {
    let f = fx();
    let state = rebuild(&f, |o| {
        o.gh_runner = Some(ok_gh("https://github.com/o/n/issues/9"))
    });
    let (_, created) = post(
        &state,
        "/api/todos",
        json!({"board":"rocky","title":"작업"}),
    )
    .await;
    let id = created["id"].as_str().unwrap();

    let (status, body) = post(
        &state,
        &format!("/api/todos/{id}/issue"),
        json!({"repo":"o/n"}),
    )
    .await;
    assert_eq!(status, 201, "{body}");
    assert_eq!(body["url"], "https://github.com/o/n/issues/9");
    // 보드에 repo 가 영구 반영됐다
    let (_, boards) = get(&state, "/api/boards").await;
    assert_eq!(boards[0]["repo"], "o/n");
    // todo links 에도 붙었다
    assert_eq!(
        body["todo"]["links"][0]["url"],
        "https://github.com/o/n/issues/9"
    );
    assert_eq!(body["todo"]["links"][0]["title"], "#9");
}

#[tokio::test]
async fn issue_malformed_body_repo_is_400() {
    let f = fx();
    let (_, created) = post(
        &f.state,
        "/api/todos",
        json!({"board":"rocky","title":"작업"}),
    )
    .await;
    let id = created["id"].as_str().unwrap();
    let (status, _) = post(
        &f.state,
        &format!("/api/todos/{id}/issue"),
        json!({"repo":"not a slug"}),
    )
    .await;
    assert_eq!(status, 400);
}

#[tokio::test]
async fn gh_not_found_message_is_still_400() {
    let f = fx();
    let failing: Runner = Arc::new(|_cmd, _stdin, _timeout| {
        Box::pin(async {
            CmdOutput {
                code: 1,
                stdout: String::new(),
                stderr: "HTTP 404: Not Found (https://api.github.com/repos/o/n)".into(),
            }
        })
    });
    let state = rebuild(&f, |o| o.gh_runner = Some(failing));
    let id = todo_with_repo(&state).await;
    let (status, body) = call(
        &state,
        "POST",
        &format!("/api/todos/{id}/issue"),
        None,
        ReqOptions::default(),
    )
    .await;
    // `gh` 의 "Not Found" 가 404 로 새면 "todo not found" 계약이 깨진다.
    assert_eq!(status, 400, "{body}");
}

// ── 출처 게이트 ──

#[tokio::test]
async fn lan_peer_gets_403_and_no_issue_attempted() {
    let f = fx();
    let calls = Arc::new(AtomicUsize::new(0));
    let state = rebuild(&f, |o| o.gh_runner = Some(counting_gh(calls.clone())));
    let id = todo_with_repo(&state).await;

    let (status, _) = call(
        &state,
        "POST",
        &format!("/api/todos/{id}/issue"),
        Some(json!({"repo":"o/n"})),
        ReqOptions {
            peer: Some("192.168.1.20"),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(status, 403);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(f
        .store
        .get_todo(&id, None)
        .unwrap()
        .unwrap()
        .links
        .is_empty());
}

#[tokio::test]
async fn tailscale_proxied_request_is_403_despite_loopback_peer() {
    let f = fx();
    let state = rebuild(&f, |o| {
        o.gh_runner = Some(ok_gh("https://github.com/o/n/issues/5"))
    });
    let id = todo_with_repo(&state).await;
    let (status, _) = call(
        &state,
        "POST",
        &format!("/api/todos/{id}/issue"),
        None,
        ReqOptions {
            headers: vec![("x-forwarded-for", "100.101.102.103")],
            peer: Some("127.0.0.1"),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(status, 403);
    assert!(f
        .store
        .get_todo(&id, None)
        .unwrap()
        .unwrap()
        .links
        .is_empty());
}

#[tokio::test]
async fn gate_403_comes_before_todo_lookup() {
    let f = fx();
    let (status, _) = call(
        &f.state,
        "POST",
        "/api/todos/nosuchid/issue",
        None,
        ReqOptions {
            peer: Some("192.168.1.20"),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(status, 403); // 404 로 ref 존재를 흘리지 않는다
}

#[tokio::test]
async fn loopback_request_still_creates_issue() {
    let f = fx();
    let state = rebuild(&f, |o| {
        o.gh_runner = Some(ok_gh("https://github.com/o/n/issues/5"))
    });
    let id = todo_with_repo(&state).await;
    let (status, _) = call(
        &state,
        "POST",
        &format!("/api/todos/{id}/issue"),
        None,
        ReqOptions::default(),
    )
    .await;
    assert_eq!(status, 201);
    let links = f.store.get_todo(&id, None).unwrap().unwrap().links;
    assert_eq!(links[0].url, "https://github.com/o/n/issues/5");
}

#[tokio::test]
async fn only_issue_creation_is_gated_for_lan_peer() {
    let f = fx();
    let id = todo_with_repo(&f.state).await;
    let lan = ReqOptions {
        peer: Some("192.168.1.20"),
        ..Default::default()
    };
    let (status, _) = call(&f.state, "GET", "/api/todos", None, lan).await;
    assert_eq!(status, 200);
    let (status, _) = call(
        &f.state,
        "GET",
        &format!("/api/todos/{id}"),
        None,
        ReqOptions {
            peer: Some("192.168.1.20"),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(status, 200);
    let (status, _) = call(
        &f.state,
        "PATCH",
        &format!("/api/todos/{id}"),
        Some(json!({"title":"고침"})),
        ReqOptions {
            peer: Some("192.168.1.20"),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(status, 200);
}

#[tokio::test]
async fn link_appearing_during_body_read_is_still_409() {
    use axum::body::Body;
    use axum::http::Request;

    let f = fx();
    let state = rebuild(&f, |o| {
        o.gh_runner = Some(ok_gh("https://github.com/o/n/issues/5"))
    });
    let id = todo_with_repo(&state).await;

    // 본문 스트림이 소비되는 순간(= 사전 409 검사 **뒤**)에 링크를 붙인다 — 경쟁 재현.
    struct RacedBody {
        store: Arc<rocky_core::TodoStore>,
        id: String,
        yielded: bool,
    }
    impl tokio_stream::Stream for RacedBody {
        type Item = Result<Vec<u8>, std::convert::Infallible>;
        fn poll_next(
            mut self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Option<Self::Item>> {
            if self.yielded {
                return std::task::Poll::Ready(None);
            }
            self.yielded = true;
            self.store
                .update_todo(
                    &self.id,
                    &rocky_core::types::UpdateTodoPatch {
                        links: Some(vec![rocky_core::types::TodoLink {
                            url: "https://github.com/o/n/issues/3".into(),
                            title: None,
                        }]),
                        ..Default::default()
                    },
                    "other",
                    None,
                )
                .unwrap();
            std::task::Poll::Ready(Some(Ok(b"{}".to_vec())))
        }
    }
    let raced = RacedBody {
        store: f.store.clone(),
        id: id.clone(),
        yielded: false,
    };
    let request = Request::builder()
        .method("POST")
        .uri(format!("http://localhost/api/todos/{id}/issue"))
        .header("content-type", "application/json")
        .header("x-rocky-actor", "tester")
        .body(Body::from_stream(raced))
        .unwrap();
    let response = rockyd::server::handle_api(&state, request, Some("127.0.0.1".into())).await;
    assert_eq!(response.status(), 409);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["url"], "https://github.com/o/n/issues/3");
    assert_eq!(f.store.get_todo(&id, None).unwrap().unwrap().links.len(), 1);
}

#[tokio::test]
async fn health_reports_issue_create_allowed_per_origin() {
    let f = fx();
    let (_, local) = get(&f.state, "/api/health").await;
    assert_eq!(local["issueCreateAllowed"], true);
    let (_, remote) = call(
        &f.state,
        "GET",
        "/api/health",
        None,
        ReqOptions {
            peer: Some("192.168.1.20"),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(remote["issueCreateAllowed"], false);
}

/// Cloudflare Access 로 들어온 화면에만 로그인한 이메일을 알려 준다 — 웹이 ⋯ 메뉴에 로그아웃을 그린다.
#[tokio::test]
async fn health_reports_access_user_only_via_access() {
    let f = fx();
    let (_, local) = get(&f.state, "/api/health").await;
    assert!(local["accessUser"].is_null(), "로컬 화면에는 없다: {local}");
    let (_, via_access) = call(
        &f.state,
        "GET",
        "/api/health",
        None,
        ReqOptions {
            headers: vec![
                ("cf-connecting-ip", "203.0.113.7"),
                ("cf-access-authenticated-user-email", "me@example.com"),
            ],
            ..Default::default()
        },
    )
    .await;
    assert_eq!(via_access["accessUser"], "me@example.com");
    assert_eq!(
        via_access["issueCreateAllowed"], false,
        "터널 경유는 원격이다"
    );
}

// ── 보드 메타 관리 ──

#[tokio::test]
async fn patch_board_title_description_key_at_once() {
    let f = fx();
    post(&f.state, "/api/boards", json!({"key":"gotgan"})).await;
    let (status, board) = patch(
        &f.state,
        "/api/boards/gotgan",
        json!({"key":"tally","title":"Tally","description":"가계부"}),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(board["key"], "tally");
    assert_eq!(board["title"], "Tally");
    assert_eq!(board["description"], "가계부");
    assert_eq!(board["previousKeys"], json!(["gotgan"]));
}

#[tokio::test]
async fn renamed_board_still_reads_by_old_key() {
    let f = fx();
    post(
        &f.state,
        "/api/todos",
        json!({"board":"gotgan","title":"x"}),
    )
    .await;
    patch(&f.state, "/api/boards/gotgan", json!({"key":"tally"})).await;
    let (status, body) = get(&f.state, "/api/todos/gotgan-1").await;
    assert_eq!(status, 200);
    assert_eq!(body["todo"]["ref"], "tally-1");
}

#[tokio::test]
async fn rename_cannot_steal_used_key() {
    let f = fx();
    post(&f.state, "/api/boards", json!({"key":"gotgan"})).await;
    post(&f.state, "/api/boards", json!({"key":"tally"})).await;
    let (status, _) = patch(&f.state, "/api/boards/gotgan", json!({"key":"tally"})).await;
    assert_eq!(status, 400);
}

#[tokio::test]
async fn old_key_in_url_still_edits_board() {
    let f = fx();
    post(&f.state, "/api/boards", json!({"key":"gotgan"})).await;
    patch(&f.state, "/api/boards/gotgan", json!({"key":"tally"})).await;
    let (status, board) = patch(
        &f.state,
        "/api/boards/gotgan",
        json!({"description":"가계부"}),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(board["key"], "tally");
    assert_eq!(board["description"], "가계부");
}

#[tokio::test]
async fn blank_title_is_rejected() {
    let f = fx();
    post(&f.state, "/api/boards", json!({"key":"rocky"})).await;
    let (status, _) = patch(&f.state, "/api/boards/rocky", json!({"title":"   "})).await;
    assert_eq!(status, 400);
}

#[tokio::test]
async fn patch_board_accepts_path_and_repo_together() {
    let f = fx();
    post(&f.state, "/api/boards", json!({"key":"rocky"})).await;
    let (status, board) = patch(
        &f.state,
        "/api/boards/rocky",
        json!({"path":"/dev/rocky","repo":"o/n"}),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(board["path"], "/dev/rocky");
    assert_eq!(board["repo"], "o/n");
}

#[tokio::test]
async fn patch_board_empty_patch_says_so() {
    let f = fx();
    post(&f.state, "/api/boards", json!({"key":"rocky"})).await;
    let (status, body) = patch(&f.state, "/api/boards/rocky", json!({})).await;
    assert_eq!(status, 400);
    assert!(body["error"].as_str().unwrap().contains("required"));
}

#[tokio::test]
async fn patch_board_wrong_path_type_mentions_path_not_repo() {
    let f = fx();
    post(&f.state, "/api/boards", json!({"key":"rocky"})).await;
    let (status, body) = patch(&f.state, "/api/boards/rocky", json!({"path": 123})).await;
    assert_eq!(status, 400);
    assert!(body["error"].as_str().unwrap().contains("path"), "{body}");
}

#[tokio::test]
async fn patch_board_null_clears_path() {
    let f = fx();
    post(&f.state, "/api/boards", json!({"key":"rocky"})).await;
    patch(&f.state, "/api/boards/rocky", json!({"path":"/dev/rocky"})).await;
    let (status, board) = patch(&f.state, "/api/boards/rocky", json!({"path": null})).await;
    assert_eq!(status, 200);
    assert!(board.get("path").is_none() || board["path"].is_null());
}

/// 보드별 reviewFix — 켠 보드만 응답에 `reviewFix: true` 가 실리고, 끄면 다시 빠진다. 불리언이
/// 아니면 400, 원격 요청은 403(켜면 데몬이 세션에 일을 시킨다).
#[tokio::test]
async fn patch_board_toggles_review_fix_locally_only() {
    let f = fx();
    post(&f.state, "/api/boards", json!({"key":"rocky"})).await;
    let (status, board) = patch(&f.state, "/api/boards/rocky", json!({"reviewFix": true})).await;
    assert_eq!(status, 200);
    assert_eq!(board["reviewFix"], true);
    let (_, board) = patch(&f.state, "/api/boards/rocky", json!({"reviewFix": false})).await;
    assert!(
        board.get("reviewFix").is_none(),
        "끈 보드의 응답 모양은 그대로: {board}"
    );
    let (status, _) = patch(&f.state, "/api/boards/rocky", json!({"reviewFix": "yes"})).await;
    assert_eq!(status, 400);
    let (status, _) = call(
        &f.state,
        "PATCH",
        "/api/boards/rocky",
        Some(json!({"reviewFix": true})),
        ReqOptions {
            peer: Some("192.168.1.20"),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(status, 403);
}

/// 옛 이름 `autoResolve` 는 한 릴리스 동안 입력 별칭이다 — 받아서 `reviewFix` 로 고치고, 응답에는 새
/// 이름만 실린다. 둘 다 오면 `reviewFix` 가 이긴다. 원격 요청은 옛 이름으로도 403.
#[tokio::test]
async fn patch_board_accepts_legacy_auto_resolve_alias() {
    let f = fx();
    post(&f.state, "/api/boards", json!({"key":"rocky"})).await;
    let (status, board) = patch(&f.state, "/api/boards/rocky", json!({"autoResolve": true})).await;
    assert_eq!(status, 200);
    assert_eq!(board["reviewFix"], true);
    assert!(
        board.get("autoResolve").is_none(),
        "응답은 새 이름만: {board}"
    );
    let (_, listed) = get(&f.state, "/api/boards").await;
    let row = listed
        .as_array()
        .and_then(|a| a.iter().find(|b| b["key"] == "rocky"))
        .cloned()
        .expect("보드 목록에 rocky");
    assert_eq!(row["reviewFix"], true);
    assert!(row.get("autoResolve").is_none(), "목록도 새 이름만: {row}");
    let (status, board) = patch(
        &f.state,
        "/api/boards/rocky",
        json!({"autoResolve": true, "reviewFix": false}),
    )
    .await;
    assert_eq!(status, 200);
    assert!(
        board.get("reviewFix").is_none(),
        "reviewFix 가 이긴다: {board}"
    );
    let (status, _) = patch(&f.state, "/api/boards/rocky", json!({"autoResolve": 1})).await;
    assert_eq!(status, 400);
    let (status, _) = call(
        &f.state,
        "PATCH",
        "/api/boards/rocky",
        Some(json!({"autoResolve": true})),
        ReqOptions {
            peer: Some("192.168.1.20"),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(status, 403);
}
