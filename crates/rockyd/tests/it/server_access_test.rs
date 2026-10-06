//! Cloudflare Access 로 들어온 요청의 원격 제어 — 토큰이 팀 공개키로 검증되고 허용 이메일일 때만 원격 제어 탭(`/api/rc/*`)이
//! 열리고, 다른 로컬 전용 동작(이슈·세션 띄우기·agy)은 그대로 막힌다. 공개키는 가짜 받기 함수가 준다.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use base64::Engine;
use rocky_core::access::AccessConfig;
use rockyd::access::{AccessGate, CertsFetcher};
use serde_json::json;

use crate::common::*;

const KEY: &[u8] = include_bytes!("../../../rocky-core/tests/it/fixtures/access-test-rsa.pk8");

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn key_pair() -> ring::rsa::KeyPair {
    ring::rsa::KeyPair::from_pkcs8(KEY).unwrap()
}

fn jwks(kid: &str) -> String {
    let c = ring::rsa::PublicKeyComponents::<Vec<u8>>::from(key_pair().public());
    json!({ "keys": [{ "kid": kid, "kty": "RSA", "n": b64(&c.n), "e": b64(&c.e) }] }).to_string()
}

fn token(kid: &str, email: &str) -> String {
    let now = chrono::Utc::now().timestamp();
    let input = format!(
        "{}.{}",
        b64(json!({ "alg": "RS256", "kid": kid }).to_string().as_bytes()),
        b64(json!({
            "iss": "https://team.cloudflareaccess.com",
            "aud": ["aud-tag"],
            "email": email,
            "exp": now + 600,
        })
        .to_string()
        .as_bytes())
    );
    let pair = key_pair();
    let mut sig = vec![0u8; pair.public().modulus_len()];
    pair.sign(
        &ring::signature::RSA_PKCS1_SHA256,
        &ring::rand::SystemRandom::new(),
        input.as_bytes(),
        &mut sig,
    )
    .unwrap();
    format!("{input}.{}", b64(&sig))
}

/// 공개키를 몇 번 받았는지 세는 가짜 받기 함수.
fn fetcher(kid: &'static str, calls: Arc<AtomicUsize>) -> CertsFetcher {
    Arc::new(move |url: String| {
        calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(
            url,
            "https://team.cloudflareaccess.com/cdn-cgi/access/certs"
        );
        Box::pin(async move { Ok(jwks(kid)) })
    })
}

fn gate(remote_control: bool, calls: Arc<AtomicUsize>) -> Arc<AccessGate> {
    Arc::new(AccessGate::new(
        AccessConfig {
            team: "team".into(),
            aud: vec!["aud-tag".into()],
            emails: vec!["owner@example.com".into()],
            remote_control,
        },
        fetcher("k1", calls),
    ))
}

/// cloudflared 가 넘기는 모양 — 루프백 peer + Cloudflare 헤더.
fn via_access<'a>(token: &'a str, peer: &'a str) -> ReqOptions<'a> {
    ReqOptions {
        peer: Some(peer),
        headers: vec![
            ("cf-ray", "abc-ICN"),
            ("cf-connecting-ip", "203.0.113.7"),
            ("cf-access-authenticated-user-email", "owner@example.com"),
            ("cf-access-jwt-assertion", token),
        ],
        ..Default::default()
    }
}

async fn health(
    state: &Arc<rockyd::server::ServerState>,
    options: ReqOptions<'_>,
) -> serde_json::Value {
    let (status, body) = call(state, "GET", "/api/health", None, options).await;
    assert_eq!(status, 200);
    body
}

#[tokio::test]
async fn a_verified_owner_gets_remote_control_but_nothing_else_local() {
    let calls = Arc::new(AtomicUsize::new(0));
    let f = fx_with(|o| o.access = Some(gate(true, calls.clone())));
    let t = token("k1", "owner@example.com");
    let body = health(&f.state, via_access(&t, "127.0.0.1")).await;
    assert_eq!(body["rcControlAllowed"], true);
    assert_eq!(body["spawnAllowed"], false);
    assert_eq!(body["issueCreateAllowed"], false);

    // 원격 제어 라우트는 가드를 지난다 — rc 가 꺼진 픽스처라 404(로컬 가드라면 403).
    let (status, _) = call(
        &f.state,
        "POST",
        "/api/rc/nightly",
        None,
        via_access(&t, "127.0.0.1"),
    )
    .await;
    assert_eq!(status, 404);
    let (status, _) = call(
        &f.state,
        "POST",
        "/api/rc/servers/x/start",
        None,
        via_access(&t, "127.0.0.1"),
    )
    .await;
    assert_eq!(status, 404);
    // Antigravity 켜기·끄기는 이 범위가 아니다.
    let (status, _) = call(
        &f.state,
        "POST",
        "/api/rc/antigravity/stop",
        None,
        via_access(&t, "127.0.0.1"),
    )
    .await;
    assert_eq!(status, 403);
    // 부를 때마다 프로세스를 띄우는 읽기(리허설 · 최근 활동)도 이 범위가 아니다.
    let (status, _) = call(
        &f.state,
        "GET",
        "/api/rc/nightly/preview",
        None,
        via_access(&t, "127.0.0.1"),
    )
    .await;
    assert_eq!(status, 403);
    let (status, _) = call(
        &f.state,
        "GET",
        "/api/rc/servers?activity=1",
        None,
        via_access(&t, "127.0.0.1"),
    )
    .await;
    assert_eq!(status, 403);
    // 공개키는 한 번만 받았다(캐시).
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn header_names_alone_or_a_wrong_person_get_nothing() {
    let calls = Arc::new(AtomicUsize::new(0));
    let f = fx_with(|o| o.access = Some(gate(true, calls.clone())));
    // 서명이 틀린 토큰(마지막 조각을 바꿈).
    let good = token("k1", "owner@example.com");
    let forged = format!("{}AAAA", &good[..good.len() - 4]);
    assert_eq!(
        health(&f.state, via_access(&forged, "127.0.0.1")).await["rcControlAllowed"],
        false
    );
    // 허용 목록 밖의 사람.
    let other = token("k1", "someone@example.com");
    assert_eq!(
        health(&f.state, via_access(&other, "127.0.0.1")).await["rcControlAllowed"],
        false
    );
    // 맞는 토큰이라도 루프백이 아닌 peer(데몬을 내부망에 노출한 경우)는 받지 않는다.
    assert_eq!(
        health(&f.state, via_access(&good, "192.168.0.9")).await["rcControlAllowed"],
        false
    );
    // 토큰 없이 이메일 헤더만.
    let only_email = ReqOptions {
        headers: vec![("cf-access-authenticated-user-email", "owner@example.com")],
        ..Default::default()
    };
    assert_eq!(
        health(&f.state, only_email).await["rcControlAllowed"],
        false
    );
    let (status, _) = call(
        &f.state,
        "POST",
        "/api/rc/nightly",
        None,
        via_access(&forged, "127.0.0.1"),
    )
    .await;
    assert_eq!(status, 403);
}

#[tokio::test]
async fn remote_control_off_or_no_access_block_keeps_the_old_rule() {
    let calls = Arc::new(AtomicUsize::new(0));
    let off = fx_with(|o| o.access = Some(gate(false, calls.clone())));
    let t = token("k1", "owner@example.com");
    assert_eq!(
        health(&off.state, via_access(&t, "127.0.0.1")).await["rcControlAllowed"],
        false
    );
    // 꺼져 있으면 공개키를 받으러 가지도 않는다.
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    let none = fx();
    assert_eq!(
        health(&none.state, via_access(&t, "127.0.0.1")).await["rcControlAllowed"],
        false
    );
    // 로컬 화면은 지금처럼 다 된다.
    let local = health(&none.state, ReqOptions::default()).await;
    assert_eq!(local["rcControlAllowed"], true);
    assert_eq!(local["spawnAllowed"], true);
}

#[tokio::test]
async fn an_unknown_key_refetches_at_most_once_a_minute() {
    let calls = Arc::new(AtomicUsize::new(0));
    let f = fx_with(|o| o.access = Some(gate(true, calls.clone())));
    let good = token("k1", "owner@example.com");
    assert_eq!(
        health(&f.state, via_access(&good, "127.0.0.1")).await["rcControlAllowed"],
        true
    );
    // 지어낸 kid 를 계속 보내도 1분 안에는 다시 받지 않는다.
    for _ in 0..3 {
        let made_up = token("k-unknown", "owner@example.com");
        assert_eq!(
            health(&f.state, via_access(&made_up, "127.0.0.1")).await["rcControlAllowed"],
            false
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn changes_from_a_sibling_subdomain_are_refused_even_with_a_valid_token() {
    let calls = Arc::new(AtomicUsize::new(0));
    let f = fx_with(|o| o.access = Some(gate(true, calls.clone())));
    let t = token("k1", "owner@example.com");
    let with = |extra: Vec<(&'static str, &'static str)>| {
        let mut options = via_access(&t, "127.0.0.1");
        options.headers.extend(extra);
        options
    };
    // 같은 등록 도메인의 다른 서브도메인 페이지 — Access 쿠키가 실려 토큰도 붙는다.
    let (status, _) = call(
        &f.state,
        "POST",
        "/api/rc/nightly",
        None,
        with(vec![("sec-fetch-site", "same-site")]),
    )
    .await;
    assert_eq!(status, 403);
    let (status, _) = call(
        &f.state,
        "POST",
        "/api/rc/strays/x/stop",
        None,
        with(vec![("origin", "https://evil.example.com")]),
    )
    .await;
    assert_eq!(status, 403);
    // 같은 출처면 가드를 지난다(rc 꺼진 픽스처라 404).
    let (status, _) = call(
        &f.state,
        "POST",
        "/api/rc/nightly",
        None,
        with(vec![("sec-fetch-site", "same-origin")]),
    )
    .await;
    assert_eq!(status, 404);
    let (status, _) = call(
        &f.state,
        "POST",
        "/api/rc/nightly",
        None,
        with(vec![("origin", "http://localhost")]),
    )
    .await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn a_valid_token_does_not_open_spawn_or_issue_routes() {
    let calls = Arc::new(AtomicUsize::new(0));
    let f = fx_with(|o| o.access = Some(gate(true, calls.clone())));
    let todo = f
        .store
        .create_todo(
            &rocky_core::types::CreateTodoInput {
                board: "b".into(),
                title: "t".into(),
                description: None,
                section: None,
                parent_id: None,
                priority: None,
                due: None,
                labels: None,
                links: None,
            },
            "tester",
        )
        .unwrap();
    let t = token("k1", "owner@example.com");
    for path in [
        format!("/api/todos/{}/spawn", todo.id),
        format!("/api/todos/{}/issue", todo.id),
    ] {
        let (status, _) = call(
            &f.state,
            "POST",
            &path,
            Some(json!({})),
            via_access(&t, "127.0.0.1"),
        )
        .await;
        assert_eq!(status, 403, "{path}");
    }
}
