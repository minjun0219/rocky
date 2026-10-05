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
