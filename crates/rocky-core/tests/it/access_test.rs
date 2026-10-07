//! Cloudflare Access 토큰 검증. 서명은 테스트 전용 RSA 키(`fixtures/access-test-rsa.pk8`, 어디에도 등록되지 않은
//! 키)로 직접 만든다 — ring 은 RSA 키를 만들지 못해 파일로 둔다.

use base64::Engine;
use rocky_core::access::{parse_jwks, token_kid, verify_token, AccessConfig, AccessError, Jwk};
use serde_json::json;

const KEY: &[u8] = include_bytes!("fixtures/access-test-rsa.pk8");
const NOW: i64 = 1_800_000_000;

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn key_pair() -> ring::rsa::KeyPair {
    ring::rsa::KeyPair::from_pkcs8(KEY).expect("테스트 키")
}

fn jwk(kid: &str) -> Jwk {
    let components = ring::rsa::PublicKeyComponents::<Vec<u8>>::from(key_pair().public());
    Jwk {
        kid: kid.into(),
        n: components.n,
        e: components.e,
    }
}

fn config() -> AccessConfig {
    AccessConfig {
        team: "example-team".into(),
        aud: vec!["aud-tag".into()],
        emails: vec!["Owner@Example.com".into()],
        remote_control: true,
    }
}

fn claims() -> serde_json::Value {
    json!({
        "iss": "https://example-team.cloudflareaccess.com",
        "aud": ["aud-tag"],
        "email": "owner@example.com",
        "exp": NOW + 600,
        "nbf": NOW - 10,
    })
}

fn sign(header: serde_json::Value, claims: serde_json::Value) -> String {
    let input = format!(
        "{}.{}",
        b64(header.to_string().as_bytes()),
        b64(claims.to_string().as_bytes())
    );
    let pair = key_pair();
    let mut signature = vec![0u8; pair.public().modulus_len()];
    pair.sign(
        &ring::signature::RSA_PKCS1_SHA256,
        &ring::rand::SystemRandom::new(),
        input.as_bytes(),
        &mut signature,
    )
    .expect("서명");
    format!("{input}.{}", b64(&signature))
}

fn token(claims: serde_json::Value) -> String {
    sign(json!({ "alg": "RS256", "kid": "k1" }), claims)
}

#[test]
fn a_valid_token_from_an_allowed_email_passes() {
    let identity = verify_token(&token(claims()), &[jwk("k1")], &config(), NOW).expect("통과");
    assert_eq!(identity.email, "owner@example.com");
}

#[test]
fn a_single_string_audience_is_accepted() {
    let mut c = claims();
    c["aud"] = json!("aud-tag");
    assert!(verify_token(&token(c), &[jwk("k1")], &config(), NOW).is_ok());
}

#[test]
fn a_tampered_payload_fails_the_signature() {
    let good = token(claims());
    let parts: Vec<&str> = good.split('.').collect();
    let mut forged = claims();
    forged["email"] = json!("owner@example.com");
    forged["exp"] = json!(NOW + 999_999);
    let tampered = format!(
        "{}.{}.{}",
        parts[0],
        b64(forged.to_string().as_bytes()),
        parts[2]
    );
    assert_eq!(
        verify_token(&tampered, &[jwk("k1")], &config(), NOW),
        Err(AccessError::Signature)
    );
}

#[test]
fn an_unknown_key_id_is_refused() {
    assert_eq!(
        verify_token(&token(claims()), &[jwk("other")], &config(), NOW),
        Err(AccessError::UnknownKey("k1".into()))
    );
}

#[test]
fn only_rs256_is_accepted() {
    let none = sign(json!({ "alg": "none", "kid": "k1" }), claims());
    assert_eq!(
        verify_token(&none, &[jwk("k1")], &config(), NOW),
        Err(AccessError::Algorithm("none".into()))
    );
}

#[test]
fn issuer_audience_and_time_are_checked_after_the_signature() {
    let keys = [jwk("k1")];
    let mut c = claims();
    c["iss"] = json!("https://other.cloudflareaccess.com");
    assert!(matches!(
        verify_token(&token(c), &keys, &config(), NOW),
        Err(AccessError::Issuer(_))
    ));
    let mut c = claims();
    c["aud"] = json!(["another-app"]);
    assert_eq!(
        verify_token(&token(c), &keys, &config(), NOW),
        Err(AccessError::Audience)
    );
    // 만료는 60초 여유를 둔다.
    let mut c = claims();
    c["exp"] = json!(NOW - 61);
    assert_eq!(
        verify_token(&token(c), &keys, &config(), NOW),
        Err(AccessError::Expired)
    );
    let mut c = claims();
    c["exp"] = json!(NOW - 30);
    assert!(verify_token(&token(c), &keys, &config(), NOW).is_ok());
    let mut c = claims();
    c["nbf"] = json!(NOW + 120);
    assert_eq!(
        verify_token(&token(c), &keys, &config(), NOW),
        Err(AccessError::NotYetValid)
    );
}

#[test]
fn an_email_outside_the_allow_list_is_refused() {
    let mut c = claims();
    c["email"] = json!("someone@example.com");
    assert_eq!(
        verify_token(&token(c), &[jwk("k1")], &config(), NOW),
        Err(AccessError::NotAllowed("someone@example.com".into()))
    );
    let mut c = claims();
    c.as_object_mut().unwrap().remove("email");
    assert!(matches!(
        verify_token(&token(c), &[jwk("k1")], &config(), NOW),
        Err(AccessError::NotAllowed(_))
    ));
}

#[test]
fn garbage_is_malformed() {
    for bad in ["", "a.b", "a.b.c.d", "!!!.###.$$$"] {
        assert!(
            verify_token(bad, &[jwk("k1")], &config(), NOW).is_err(),
            "{bad}"
        );
    }
    assert_eq!(token_kid("nope"), None);
    assert_eq!(token_kid(&token(claims())).as_deref(), Some("k1"));
}

#[test]
fn jwks_reads_rsa_keys_and_skips_the_rest() {
    let key = jwk("k1");
    let body = json!({
        "keys": [
            { "kid": "k1", "kty": "RSA", "alg": "RS256", "n": b64(&key.n), "e": b64(&key.e) },
            { "kid": "ec", "kty": "EC", "x": "AA", "y": "AA" },
            { "kid": "broken", "kty": "RSA", "n": "***" , "e": "AQAB" },
        ],
        "public_cert": { "kid": "k1", "cert": "-" },
    })
    .to_string();
    assert_eq!(parse_jwks(&body), vec![key]);
    assert!(parse_jwks("not json").is_empty());
}

#[test]
fn config_names_the_team_urls() {
    assert_eq!(
        config().issuer(),
        "https://example-team.cloudflareaccess.com"
    );
    assert_eq!(
        config().certs_url(),
        "https://example-team.cloudflareaccess.com/cdn-cgi/access/certs"
    );
}
