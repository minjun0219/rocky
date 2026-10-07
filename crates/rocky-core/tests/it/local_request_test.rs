//! TS 원본 `src/local-request.test.ts` 포팅 — Request 대신 헤더 클로저.

use rocky_core::local_request::{
    access_user_email, is_cross_site_request, is_local_request, is_loopback_address,
};
use std::collections::HashMap;

const BASE: &str = "http://localhost/api/todos/abc/issue";

fn headers(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_lowercase(), v.to_string()))
        .collect()
}

fn local(map: &HashMap<String, String>, peer: Option<&str>) -> bool {
    is_local_request(peer, |name| map.contains_key(name))
}

fn cross(map: &HashMap<String, String>) -> bool {
    is_cross_site_request(|name| map.get(name).cloned(), BASE)
}

#[test]
fn loopback_ipv4_range() {
    assert!(is_loopback_address(Some("127.0.0.1")));
    assert!(is_loopback_address(Some("127.1.2.3")));
}

#[test]
fn loopback_ipv6_and_mapped() {
    assert!(is_loopback_address(Some("::1")));
    assert!(is_loopback_address(Some("0:0:0:0:0:0:0:1")));
    assert!(is_loopback_address(Some("::ffff:127.0.0.1")));
}

#[test]
fn loopback_strips_brackets_zone_case() {
    assert!(is_loopback_address(Some("[::1]")));
    assert!(is_loopback_address(Some("::1%lo0")));
    assert!(is_loopback_address(Some("::FFFF:127.0.0.1")));
}

#[test]
fn rejects_lan_tailnet_public() {
    assert!(!is_loopback_address(Some("192.168.1.20")));
    assert!(!is_loopback_address(Some("10.0.0.5")));
    assert!(!is_loopback_address(Some("100.101.102.103"))); // tailnet CGNAT
    assert!(!is_loopback_address(Some("93.184.216.34")));
    assert!(!is_loopback_address(Some("fe80::1")));
}

#[test]
fn lookalikes_are_not_loopback() {
    assert!(!is_loopback_address(Some("10.127.0.1")));
    assert!(!is_loopback_address(Some("1127.0.0.1")));
}

#[test]
fn unknown_address_is_not_loopback() {
    assert!(!is_loopback_address(None));
    assert!(!is_loopback_address(Some("")));
}

#[test]
fn bare_loopback_request_is_local() {
    assert!(local(&headers(&[]), Some("127.0.0.1")));
}

#[test]
fn lan_peer_is_not_local_even_without_headers() {
    assert!(!local(&headers(&[]), Some("192.168.1.20")));
}

#[test]
fn loopback_with_proxy_markers_is_not_local() {
    assert!(!local(
        &headers(&[("x-forwarded-for", "100.101.102.103")]),
        Some("127.0.0.1")
    ));
    assert!(!local(
        &headers(&[("tailscale-user-login", "someone@example.com")]),
        Some("127.0.0.1")
    ));
    assert!(!local(
        &headers(&[("forwarded", "for=100.101.102.103")]),
        Some("127.0.0.1")
    ));
    assert!(!local(
        &headers(&[("x-forwarded-proto", "https")]),
        Some("127.0.0.1")
    ));
}

#[test]
fn unknown_peer_is_not_local() {
    assert!(!local(&headers(&[]), None));
}

#[test]
fn forging_headers_only_loses_trust() {
    assert!(!local(&headers(&[]), Some("192.168.1.20")));
    assert!(!local(
        &headers(&[("x-forwarded-for", "127.0.0.1")]),
        Some("192.168.1.20")
    ));
}

// ── cross-site ──

#[test]
fn non_browser_clients_pass() {
    assert!(!cross(&headers(&[])));
}

#[test]
fn sec_fetch_site_is_first_priority() {
    assert!(cross(&headers(&[("sec-fetch-site", "cross-site")])));
    assert!(!cross(&headers(&[("sec-fetch-site", "same-origin")])));
    assert!(!cross(&headers(&[("sec-fetch-site", "same-site")]))); // 테일넷의 다른 기기
    assert!(!cross(&headers(&[("sec-fetch-site", "none")]))); // 주소창 입력·북마크
}

#[test]
fn sec_fetch_site_present_ignores_origin_mismatch() {
    // tailscale serve 경로 — Origin 문자열 비교는 정상 화면을 막는다.
    assert!(!cross(&headers(&[
        ("sec-fetch-site", "same-origin"),
        ("origin", "https://mac.tailnet.ts.net"),
    ])));
}

#[test]
fn origin_fallback_when_no_fetch_metadata() {
    assert!(!cross(&headers(&[("origin", "http://localhost")])));
    assert!(cross(&headers(&[("origin", "https://evil.example")])));
    assert!(!cross(&headers(&[
        ("origin", "https://mac.tailnet.ts.net"),
        ("x-forwarded-host", "mac.tailnet.ts.net"),
    ])));
    assert!(!cross(&headers(&[
        ("origin", "https://mac.tailnet.ts.net"),
        ("x-forwarded-host", "mac.tailnet.ts.net:443"),
    ])));
}

#[test]
fn origin_fallback_ignores_port() {
    assert!(!cross(&headers(&[("origin", "http://localhost:3000")])));
}

#[test]
fn opaque_and_unparsable_origins_are_rejected() {
    assert!(cross(&headers(&[("origin", "null")])));
    assert!(cross(&headers(&[("origin", "not a url")])));
}

#[test]
fn cloudflare_tunnel_and_access_headers_mark_the_request_as_relayed() {
    // cloudflared 는 루프백으로 프록시한다 — peer 가 127.0.0.1 이어도 cf-* 가 있으면 원격이다.
    assert!(!local(
        &headers(&[("cf-connecting-ip", "203.0.113.9")]),
        Some("127.0.0.1")
    ));
    assert!(!local(
        &headers(&[("cf-ray", "8a1b2c3d4e5f-ICN")]),
        Some("::1")
    ));
    assert!(!local(
        &headers(&[("cf-access-authenticated-user-email", "me@example.com")]),
        Some("127.0.0.1")
    ));
    assert!(!local(
        &headers(&[("cf-access-jwt-assertion", "eyJ...")]),
        Some("127.0.0.1")
    ));
}

/// Access 를 거친 요청의 이메일은 로그아웃 링크용 힌트 — 없거나 비면 None.
#[test]
fn access_user_email_reads_the_access_header() {
    let via_access = headers(&[("Cf-Access-Authenticated-User-Email", " me@example.com ")]);
    assert_eq!(
        access_user_email(|name| via_access.get(name).cloned()).as_deref(),
        Some("me@example.com")
    );
    let blank = headers(&[("cf-access-authenticated-user-email", "  ")]);
    assert_eq!(access_user_email(|name| blank.get(name).cloned()), None);
    let tunnel_only = headers(&[("cf-ray", "abc"), ("cf-connecting-ip", "1.2.3.4")]);
    assert_eq!(
        access_user_email(|name| tunnel_only.get(name).cloned()),
        None
    );
}

#[test]
fn same_origin_is_narrower_than_not_cross_site() {
    use rocky_core::local_request::is_same_origin_request;
    let headers = |pairs: Vec<(&'static str, &'static str)>| {
        move |name: &str| {
            pairs
                .iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| v.to_string())
        }
    };
    let host = "rocky.example.com";
    assert!(is_same_origin_request(
        headers(vec![("sec-fetch-site", "same-origin")]),
        host
    ));
    assert!(!is_same_origin_request(
        headers(vec![("sec-fetch-site", "same-site")]),
        host
    ));
    assert!(!is_same_origin_request(
        headers(vec![("sec-fetch-site", "cross-site")]),
        host
    ));
    assert!(!is_same_origin_request(
        headers(vec![("sec-fetch-site", "none")]),
        host
    ));
    // Sec-Fetch-Site 가 없으면 Origin 의 authority 를 Host 와 맞댄다.
    assert!(is_same_origin_request(
        headers(vec![("origin", "https://rocky.example.com")]),
        host
    ));
    assert!(!is_same_origin_request(
        headers(vec![("origin", "https://evil.example.com")]),
        host
    ));
    assert!(!is_same_origin_request(
        headers(vec![("origin", "https://rocky.example.com:8443")]),
        host
    ));
    assert!(!is_same_origin_request(
        headers(vec![("origin", "null")]),
        host
    ));
    assert!(is_same_origin_request(
        headers(vec![
            ("origin", "https://rocky.example.com"),
            ("x-forwarded-host", "rocky.example.com")
        ]),
        "127.0.0.1:8636"
    ));
    // 둘 다 없으면 비브라우저 클라이언트.
    assert!(is_same_origin_request(headers(vec![]), host));
}
