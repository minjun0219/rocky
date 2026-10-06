//! 지금 statusline 을 띄운 세션이 **어느 Claude Code 계정**인가 — cc-usage `internal/config` · `internal/account` 의 이식.
//! 파일을 읽지 않는 순수 규칙만 둔다(읽기는 CLI).
//!
//! 기준은 세션의 환경이다. `CLAUDE_CONFIG_DIR` 이 설정값을 이긴다 — Claude Code 는 이 변수를 절대적으로 따르고, 값이
//! 다르면 로그인 상태부터 갈린다(자격 증명을 공유하지 않는다). 설정값을 우선하면 다른 계정의 숫자를 그럴듯하게 그린다.
//! rocky 는 이 판단으로 캐시도 나눈다: `<cache>/rocky/statusline/<config_dir 해시>/<이메일 해시>/`.

use std::path::{Path, PathBuf};

use serde_json::Value;

/// `~` 로 시작하는 경로를 홈 기준으로 편다.
pub fn expand(path: &str, home: &Path) -> PathBuf {
    match path.strip_prefix('~') {
        Some("") => home.to_path_buf(),
        Some(rest) if rest.starts_with('/') => home.join(&rest[1..]),
        _ => PathBuf::from(path),
    }
}

/// Claude Code 의 기본 설정 폴더(`~/.claude`).
pub fn default_config_dir(home: &Path) -> PathBuf {
    home.join(".claude")
}

/// 이 세션의 설정 폴더 — `CLAUDE_CONFIG_DIR` > 설정의 `configDir` > `~/.claude`.
pub fn config_dir(env_dir: Option<&str>, configured: Option<&str>, home: &Path) -> PathBuf {
    let chosen = env_dir
        .filter(|d| !d.is_empty())
        .or(configured.filter(|d| !d.is_empty()));
    chosen.map_or_else(|| default_config_dir(home), |d| expand(d, home))
}

/// 계정 파일(`.claude.json`) 후보 — 앞의 것일수록 구체적이다. 첫 후보가 곧 "이 세션의 계정 자리" 다.
///
/// `CLAUDE_CONFIG_DIR` 이 있으면 **그 폴더의 파일 하나뿐**이다 — 거기서 못 읽었다고 다른 후보를 보면 다른 계정의
/// 이메일을 집어 온다. 없으면 `<config_dir>/.claude.json`, 그리고 **기본 설치일 때만** `~/.claude.json`.
pub fn account_paths(env_dir: Option<&str>, config_dir: &Path, home: &Path) -> Vec<PathBuf> {
    if let Some(dir) = env_dir.filter(|d| !d.is_empty()) {
        return vec![expand(dir, home).join(".claude.json")];
    }
    let mut out = vec![config_dir.join(".claude.json")];
    if clean(config_dir) == default_config_dir(home) {
        out.push(home.join(".claude.json"));
    }
    out
}

/// 경로를 문자열 단위로 정리한다(`a//b/` → `a/b`) — Go 의 `filepath.Clean` 처럼 파일 시스템을 보지 않는다.
fn clean(p: &Path) -> PathBuf {
    p.components().collect()
}

/// 계정 파일 내용 → 로그인된 이메일. `None` 은 **읽지 못했다**(JSON 이 아니거나 필드 타입이 틀림),
/// `Some("")` 는 읽었는데 이메일이 없다(API 키 인증 등) — 둘을 가른다. 섞으면 원자적 재작성 중의 일시적 실패가
/// "이메일 없음 = 기본 계정" 이라는 틀린 신호로 굳는다.
pub fn email_from_account_file(raw: &str) -> Option<String> {
    let v: Value = serde_json::from_str(raw).ok()?;
    let Some(account) = v.get("oauthAccount") else {
        return Some(String::new());
    };
    match account {
        Value::Null => return Some(String::new()),
        Value::Object(_) => {}
        _ => return None,
    }
    // Go 디코더처럼, 선언한 필드의 타입이 틀리면 파일 전체를 못 읽은 것으로 본다.
    if account
        .get("hasExtraUsageEnabled")
        .is_some_and(|f| !f.is_boolean() && !f.is_null())
    {
        return None;
    }
    match account.get("emailAddress") {
        None | Some(Value::Null) => Some(String::new()),
        Some(Value::String(email)) => Some(email.clone()),
        Some(_) => None,
    }
}

/// 캐시 루트 — `$XDG_CACHE_HOME`, 없으면 `~/.cache`.
pub fn cache_root(xdg_cache_home: Option<&str>, home: &Path) -> PathBuf {
    xdg_cache_home
        .filter(|d| !d.is_empty())
        .map_or_else(|| home.join(".cache"), PathBuf::from)
}

/// 이 설정 폴더의 캐시 자리 — 계정 캐시(`account.json`)가 여기 있다.
pub fn cache_slot(cache_root: &Path, config_dir: &Path) -> PathBuf {
    cache_root
        .join("rocky")
        .join("statusline")
        .join(short_hash(&clean(config_dir).to_string_lossy()))
}

/// 한 계정의 캐시 — `state.json` · `usage.json` · `refresh.lock`. 이메일을 모르면(빈 값 포함) `_`.
pub fn cache_bucket(slot: &Path, email: Option<&str>) -> PathBuf {
    match email.filter(|e| !e.is_empty()) {
        Some(email) => slot.join(short_hash(email)),
        None => slot.join("_"),
    }
}

/// 경로·이메일을 폴더 이름으로 — sha256 앞 12자리. 폴더 이름에 쓸 수 없는 글자를 피하고 길이를 고정한다(이메일 원문은
/// 0600 `account.json` 에만 있다).
fn short_hash(s: &str) -> String {
    let digest = ring::digest::digest(&ring::digest::SHA256, s.as_bytes());
    digest.as_ref()[..6]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// 기본 설정 폴더의 keychain 항목 이름 — 접미사가 없어 폴더와 무관하게 같은 값이다.
pub const DEFAULT_KEYCHAIN_SERVICE: &str = "Claude Code-credentials";

/// 토큰을 읽을 keychain 항목. 설정값(`keychainService`)은 **그 설정이 가리키는 폴더**(`settings_dir` — 설정의
/// `configDir`, 없으면 `~/.claude`)의 세션에만 쓰고, 그 밖에서는 기본 규칙 — **기본 설정 폴더일 때만** 기본 이름.
///
/// rocky 는 사용자 `rocky.json` 하나를 모든 세션이 같이 쓴다. 설정값을 폴더와 상관없이 쓰면 다른 계정 세션이 그 토큰을
/// 집어 남의 숫자를 그린다. 비기본 폴더에서 기본 이름을 읽어도 같은 일이 난다(cc-usage 가 실측한 함정) — 그래서 그
/// 경우는 keychain 을 건너뛰고 `<config_dir>/.credentials.json` 을 본다. 못 찾으면 숫자가 안 나오지만, 틀린 계정의
/// 숫자보다 낫다.
pub fn keychain_service(
    configured: Option<&str>,
    settings_dir: &Path,
    config_dir: &Path,
    home: &Path,
) -> Option<String> {
    if let Some(name) = configured.filter(|n| !n.is_empty()) {
        if clean(config_dir) == clean(settings_dir) {
            return Some(name.to_string());
        }
    }
    (clean(config_dir) == default_config_dir(home)).then(|| DEFAULT_KEYCHAIN_SERVICE.to_string())
}

/// 토큰 파일 — 설정값(`credentialsFile`)은 그 설정이 가리키는 폴더의 세션에만, 그 밖에서는 `<config_dir>/.credentials.json`.
pub fn credentials_file(
    configured: Option<&str>,
    settings_dir: &Path,
    config_dir: &Path,
    home: &Path,
) -> PathBuf {
    match configured.filter(|p| !p.is_empty()) {
        Some(p) if clean(config_dir) == clean(settings_dir) => expand(p, home),
        _ => config_dir.join(".credentials.json"),
    }
}

/// Claude Code 의 OAuth 토큰. 읽기만 한다 — 만료돼도 갱신하지 않는다(Claude Code 가 다음 요청에서 갱신한다).
#[derive(Debug, Clone, PartialEq)]
pub struct OauthToken {
    pub access_token: String,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// 만료된 토큰의 에러 문구.
pub const TOKEN_EXPIRED: &str = "oauth token expired (Claude Code를 한 번 사용하면 갱신됩니다)";

/// keychain 값이나 `.credentials.json` 내용 → 토큰. `{` 로 시작하지 않으면 토큰 그 자체로 본다.
pub fn parse_token(raw: &str) -> Result<OauthToken, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("empty".into());
    }
    if !raw.starts_with('{') {
        return Ok(OauthToken {
            access_token: raw.to_string(),
            expires_at: None,
        });
    }
    let v: Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    let oauth = v.get("claudeAiOauth");
    let access = oauth
        .and_then(|o| o.get("accessToken"))
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty())
        .ok_or("claudeAiOauth.accessToken missing")?;
    let expires_at = oauth
        .and_then(|o| o.get("expiresAt"))
        .and_then(Value::as_i64)
        .filter(|ms| *ms > 0)
        .and_then(chrono::DateTime::from_timestamp_millis);
    Ok(OauthToken {
        access_token: access.to_string(),
        expires_at,
    })
}

/// 만료됐으면 에러.
pub fn check_token(
    token: OauthToken,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<OauthToken, String> {
    if token.expires_at.is_some_and(|at| now > at) {
        return Err(TOKEN_EXPIRED.into());
    }
    Ok(token)
}
