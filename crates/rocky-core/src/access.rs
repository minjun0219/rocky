//! Cloudflare Access 토큰 검증 — 순수 함수. 데몬이 공개키(JWKS)를 받아 넘기고, 여기서는 서명·클레임만 본다.
//!
//! Access 를 거쳐 온 요청에는 `Cf-Access-Jwt-Assertion`(RS256 JWT)이 붙는다. 헤더 이름만 보고 믿으면 안 된다 —
//! tailscale serve·내부망으로 온 사람도 같은 헤더를 써 넣을 수 있다. 그래서 팀의 공개키로 서명을 확인하고,
//! `aud`(Access 애플리케이션)·`iss`(팀 도메인)·만료·허용 이메일을 모두 맞춰 본 것만 사람으로 인정한다.

use base64::Engine;
use serde_json::Value;

/// `rocky.json` 의 `access` 블록 — 사용자 설정에만 둔다(데몬은 사용자 설정만 읽는다).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessConfig {
    /// 팀 이름 — `<team>.cloudflareaccess.com`. 발급자(`iss`)와 공개키 주소가 여기서 나온다.
    pub team: String,
    /// Access 애플리케이션의 AUD 태그. 하나라도 토큰의 `aud` 에 있어야 한다.
    pub aud: Vec<String>,
    /// 원격 제어를 허용할 이메일(대소문자 무시). 비면 아무도 허용하지 않는다.
    pub emails: Vec<String>,
    /// 원격 제어 탭(rc 서버 띄우기·재시작·닫기·야간)을 Access 로 들어온 허용 이메일에게 연다.
    pub remote_control: bool,
}

impl AccessConfig {
    /// 토큰의 `iss` 가 되어야 하는 값.
    pub fn issuer(&self) -> String {
        format!("https://{}.cloudflareaccess.com", self.team)
    }

    /// 공개키 목록 주소.
    pub fn certs_url(&self) -> String {
        format!("{}/cdn-cgi/access/certs", self.issuer())
    }
}

/// 공개키 하나 — JWKS 의 RSA 항목에서 쓰는 칸만.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Jwk {
    pub kid: String,
    /// 모듈러스(big-endian 바이트).
    pub n: Vec<u8>,
    /// 공개 지수(big-endian 바이트).
    pub e: Vec<u8>,
}

/// 검증을 통과한 사람.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessIdentity {
    pub email: String,
}

/// 검증 실패 이유 — 로그에 남길 수 있게 토큰 내용은 담지 않는다.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AccessError {
    #[error("토큰 모양이 JWT 가 아니다")]
    Malformed,
    #[error("서명 방식 {0} 은 받지 않는다(RS256 만)")]
    Algorithm(String),
    #[error("공개키 {0} 를 모른다")]
    UnknownKey(String),
    #[error("서명이 맞지 않는다")]
    Signature,
    #[error("발급자가 다르다: {0}")]
    Issuer(String),
    #[error("Access 애플리케이션(aud)이 다르다")]
    Audience,
    #[error("만료된 토큰이다")]
    Expired,
    #[error("아직 유효하지 않은 토큰이다")]
    NotYetValid,
    #[error("허용 목록에 없는 사람이다: {0}")]
    NotAllowed(String),
}

/// 시계가 조금 어긋나도 받아 주는 폭(초).
const CLOCK_SKEW_SECS: i64 = 60;

fn b64url(part: &str) -> Result<Vec<u8>, AccessError> {
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(part.trim_end_matches('='))
        .map_err(|_| AccessError::Malformed)
}

/// JWKS 문서(`{"keys":[{kid,kty,n,e,…}]}`)에서 RSA 키만 읽는다. 모양이 틀린 항목은 건너뛴다.
pub fn parse_jwks(body: &str) -> Vec<Jwk> {
    let Ok(doc) = serde_json::from_str::<Value>(body) else {
        return Vec::new();
    };
    doc.get("keys")
        .and_then(Value::as_array)
        .map(|keys| {
            keys.iter()
                .filter(|k| k.get("kty").and_then(Value::as_str) == Some("RSA"))
                .filter_map(|k| {
                    let text = |name: &str| k.get(name).and_then(Value::as_str);
                    Some(Jwk {
                        kid: text("kid")?.to_string(),
                        n: b64url(text("n")?).ok()?,
                        e: b64url(text("e")?).ok()?,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 토큰 머리의 `kid` — 공개키를 고르거나, 모르는 키면 목록을 다시 받을지 정한다.
pub fn token_kid(token: &str) -> Option<String> {
    let header = token.split('.').next()?;
    let parsed: Value = serde_json::from_slice(&b64url(header).ok()?).ok()?;
    parsed.get("kid")?.as_str().map(str::to_string)
}

/// 토큰을 검증하고 사람을 돌려준다. `now` 는 유닉스 초.
pub fn verify_token(
    token: &str,
    keys: &[Jwk],
    config: &AccessConfig,
    now: i64,
) -> Result<AccessIdentity, AccessError> {
    let mut parts = token.trim().split('.');
    let (Some(header_b64), Some(payload_b64), Some(signature_b64), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(AccessError::Malformed);
    };
    let header: Value =
        serde_json::from_slice(&b64url(header_b64)?).map_err(|_| AccessError::Malformed)?;
    let alg = header.get("alg").and_then(Value::as_str).unwrap_or("");
    if alg != "RS256" {
        return Err(AccessError::Algorithm(alg.to_string()));
    }
    let kid = header
        .get("kid")
        .and_then(Value::as_str)
        .ok_or(AccessError::Malformed)?;
    let key = keys
        .iter()
        .find(|k| k.kid == kid)
        .ok_or_else(|| AccessError::UnknownKey(kid.to_string()))?;

    // 서명 먼저 — 클레임은 서명이 맞은 뒤에야 믿을 수 있다.
    let signing_input = &token.trim()[..header_b64.len() + 1 + payload_b64.len()];
    let signature = b64url(signature_b64)?;
    ring::signature::RsaPublicKeyComponents {
        n: &key.n,
        e: &key.e,
    }
    .verify(
        &ring::signature::RSA_PKCS1_2048_8192_SHA256,
        signing_input.as_bytes(),
        &signature,
    )
    .map_err(|_| AccessError::Signature)?;

    let claims: Value =
        serde_json::from_slice(&b64url(payload_b64)?).map_err(|_| AccessError::Malformed)?;
    let issuer = claims.get("iss").and_then(Value::as_str).unwrap_or("");
    if issuer != config.issuer() {
        return Err(AccessError::Issuer(issuer.to_string()));
    }
    let audiences: Vec<&str> = match claims.get("aud") {
        Some(Value::String(one)) => vec![one.as_str()],
        Some(Value::Array(many)) => many.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    };
    if !audiences
        .iter()
        .any(|a| config.aud.iter().any(|want| want == a))
    {
        return Err(AccessError::Audience);
    }
    let exp = claims
        .get("exp")
        .and_then(Value::as_i64)
        .ok_or(AccessError::Malformed)?;
    if now > exp + CLOCK_SKEW_SECS {
        return Err(AccessError::Expired);
    }
    if let Some(nbf) = claims.get("nbf").and_then(Value::as_i64) {
        if now + CLOCK_SKEW_SECS < nbf {
            return Err(AccessError::NotYetValid);
        }
    }
    let email = claims
        .get("email")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    if email.is_empty()
        || !config
            .emails
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(email))
    {
        return Err(AccessError::NotAllowed(email.to_string()));
    }
    Ok(AccessIdentity {
        email: email.to_string(),
    })
}
