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

/// 경로·이메일을 폴더 이름으로 — sha256 앞 12자리. 이메일이 디스크 경로에 그대로 남지 않게 한다.
fn short_hash(s: &str) -> String {
    let digest = ring::digest::digest(&ring::digest::SHA256, s.as_bytes());
    digest.as_ref()[..6]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
