//! `rocky_core::claude_account` — 이 세션이 어느 계정인가의 규칙(cc-usage `internal/config` · `internal/account`).

use std::path::{Path, PathBuf};

use rocky_core::claude_account::*;

const HOME: &str = "/home/me";

fn home() -> &'static Path {
    Path::new(HOME)
}

#[test]
fn config_dir_prefers_the_session_env_then_config_then_default() {
    assert_eq!(
        config_dir(Some("/w"), Some("~/x"), home()),
        PathBuf::from("/w")
    );
    assert_eq!(
        config_dir(None, Some("~/x"), home()),
        PathBuf::from("/home/me/x")
    );
    assert_eq!(
        config_dir(Some(""), None, home()),
        PathBuf::from("/home/me/.claude")
    );
    assert_eq!(
        config_dir(Some("~"), None, home()),
        PathBuf::from("/home/me")
    );
}

#[test]
fn account_paths_never_leak_into_another_accounts_file() {
    // CLAUDE_CONFIG_DIR 가 있으면 그 폴더의 파일 하나뿐.
    assert_eq!(
        account_paths(Some("/w"), Path::new("/w"), home()),
        [PathBuf::from("/w/.claude.json")]
    );
    // 기본 설치만 홈 루트 폴백.
    assert_eq!(
        account_paths(None, Path::new("/home/me/.claude/"), home()),
        [
            PathBuf::from("/home/me/.claude/.claude.json"),
            PathBuf::from("/home/me/.claude.json")
        ]
    );
    assert_eq!(
        account_paths(None, Path::new("/home/me/other"), home()),
        [PathBuf::from("/home/me/other/.claude.json")]
    );
}

#[test]
fn email_parse_tells_unreadable_from_no_email() {
    assert_eq!(
        email_from_account_file(
            r#"{"oauthAccount":{"emailAddress":"a@x","hasExtraUsageEnabled":true}}"#
        ),
        Some("a@x".into())
    );
    for no_email in [
        r#"{}"#,
        r#"{"oauthAccount":null}"#,
        r#"{"oauthAccount":{}}"#,
    ] {
        assert_eq!(
            email_from_account_file(no_email),
            Some(String::new()),
            "{no_email}"
        );
    }
    for unreadable in [
        "{ not json",
        r#"{"oauthAccount":"x"}"#,
        r#"{"oauthAccount":{"emailAddress":3}}"#,
        r#"{"oauthAccount":{"emailAddress":"a@x","hasExtraUsageEnabled":"yes"}}"#,
    ] {
        assert_eq!(email_from_account_file(unreadable), None, "{unreadable}");
    }
}

#[test]
fn cache_dirs_split_by_config_dir_and_account() {
    let root = cache_root(None, home());
    assert_eq!(root, PathBuf::from("/home/me/.cache"));
    assert_eq!(cache_root(Some("/c"), home()), PathBuf::from("/c"));
    let a = cache_slot(&root, Path::new("/home/me/.claude"));
    let b = cache_slot(&root, Path::new("/home/me/work"));
    assert_ne!(a, b);
    assert!(a.starts_with("/home/me/.cache/rocky/statusline"));
    // 같은 폴더면 끝의 `/` 와 무관하게 같은 자리.
    assert_eq!(a, cache_slot(&root, Path::new("/home/me/.claude/")));
    assert_ne!(cache_bucket(&a, Some("a@x")), cache_bucket(&a, Some("b@x")));
    assert_eq!(cache_bucket(&a, None), a.join("_"));
    assert_eq!(cache_bucket(&a, Some("")), a.join("_"));
    // 이메일이 경로에 그대로 남지 않는다.
    assert!(!cache_bucket(&a, Some("a@x"))
        .to_string_lossy()
        .contains("a@x"));
}

#[test]
fn keychain_name_follows_claude_codes_rule_and_the_setting_stays_in_its_dir() {
    let default = Path::new("/home/me/.claude");
    let work = Path::new("/home/me/work");
    // 환경 변수 없이 띄운 기본 폴더 세션 — 접미사 없음.
    assert_eq!(
        keychain_service(None, default, default, None, home()),
        DEFAULT_KEYCHAIN_SERVICE
    );
    // CLAUDE_CONFIG_DIR 로 띄운 세션 — sha256(그 값) 앞 8자리(실측: /Users/minjun/.claude → 8f9c6e44).
    assert_eq!(
        keychain_service(
            None,
            Path::new("/Users/minjun/.claude"),
            Path::new("/Users/minjun/.claude"),
            Some("/Users/minjun/.claude"),
            Path::new("/Users/minjun")
        ),
        "Claude Code-credentials-8f9c6e44"
    );
    let work_name = keychain_service(None, default, work, Some("/home/me/work"), home());
    assert!(
        work_name.starts_with("Claude Code-credentials-") && work_name.len() == 32,
        "{work_name}"
    );
    // 환경 변수 없이 설정의 configDir 이 비기본 폴더면 그 경로로 같은 규칙.
    assert_eq!(keychain_service(None, work, work, None, home()), work_name);
    // 설정값은 그것이 가리키는 폴더의 세션에만 — 다른 폴더 세션이 그 토큰을 집으면 남의 숫자다.
    assert_eq!(
        keychain_service(Some("Mine"), work, work, Some("/home/me/work"), home()),
        "Mine"
    );
    assert_eq!(
        keychain_service(Some("Mine"), work, default, None, home()),
        DEFAULT_KEYCHAIN_SERVICE
    );
    assert_eq!(
        keychain_service(Some("Mine"), default, work, Some("/home/me/work"), home()),
        work_name
    );

    assert_eq!(
        credentials_file(None, default, work, home()),
        PathBuf::from("/home/me/work/.credentials.json")
    );
    assert_eq!(
        credentials_file(Some("~/c.json"), work, work, home()),
        PathBuf::from("/home/me/c.json")
    );
    assert_eq!(
        credentials_file(Some("~/c.json"), default, work, home()),
        PathBuf::from("/home/me/work/.credentials.json")
    );
}

#[test]
fn token_parse_accepts_bare_and_oauth_json_and_checks_expiry() {
    let bare = parse_token("  tok\n").unwrap();
    assert_eq!((bare.access_token.as_str(), bare.expires_at), ("tok", None));
    let json =
        parse_token(r#"{"claudeAiOauth":{"accessToken":"a","expiresAt":1757997600000}}"#).unwrap();
    assert_eq!(json.access_token, "a");
    let at = json.expires_at.unwrap();
    assert!(check_token(json.clone(), at - chrono::TimeDelta::seconds(1)).is_ok());
    assert_eq!(
        check_token(json, at + chrono::TimeDelta::seconds(1)).unwrap_err(),
        TOKEN_EXPIRED
    );
    for bad in [
        "",
        "{}",
        r#"{"claudeAiOauth":{"accessToken":""}}"#,
        "{ broken",
    ] {
        assert!(parse_token(bad).is_err(), "{bad:?}");
    }
}

/// 크레딧 힌트 — "필드 없음"(모름)과 `false` 를 가른다. 타입이 틀리면 파일 전체를 못 읽은 것이다.
#[test]
fn account_file_carries_the_credit_hint() {
    let hint = |raw: &str| parse_account_file(raw).map(|a| a.extra_usage_enabled);
    assert_eq!(
        hint(r#"{"oauthAccount":{"emailAddress":"a@x","hasExtraUsageEnabled":true}}"#),
        Some(Some(true))
    );
    assert_eq!(
        hint(r#"{"oauthAccount":{"hasExtraUsageEnabled":false}}"#),
        Some(Some(false))
    );
    assert_eq!(
        hint(r#"{"oauthAccount":{"emailAddress":"a@x"}}"#),
        Some(None)
    );
    assert_eq!(hint(r#"{}"#), Some(None));
    assert_eq!(
        hint(r#"{"oauthAccount":{"hasExtraUsageEnabled":"yes"}}"#),
        None
    );
    assert_eq!(hint("not json"), None);
    assert_eq!(
        allow_file(Path::new("/c")),
        PathBuf::from("/c/rocky/statusline/allow.json")
    );
}

/// keychain 토큰이 만료됐으면 유효한 파일 토큰이 이긴다 — 만료 시각을 모르는 파일 토큰도 유효로 본다.
#[test]
fn a_fresh_file_token_beats_an_expired_keychain_token() {
    let now = chrono::DateTime::from_timestamp(2_000_000, 0).unwrap();
    let token = |at: Option<i64>| OauthToken {
        access_token: "t".into(),
        expires_at: at.and_then(|s| chrono::DateTime::from_timestamp(s, 0)),
    };
    assert!(file_beats_expired_keychain(&token(Some(2_000_100)), now));
    assert!(file_beats_expired_keychain(&token(None), now));
    assert!(!file_beats_expired_keychain(&token(Some(1_999_000)), now));
}
