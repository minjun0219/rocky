//! 버전 비교 — 매 턴 훅의 "구버전만 올린다" 판정.

use rocky_core::version::{build_label, is_older, label, parse_version, same_build, GIT_COMMIT};

#[test]
fn parses_release_and_prerelease_forms() {
    let v = parse_version("v0.27.0").unwrap();
    assert_eq!((v.major, v.minor, v.patch), (0, 27, 0));
    assert!(!v.is_prerelease());
    assert!(parse_version("0.15.0-next.1").unwrap().is_prerelease());
    for bad in ["", "1.2", "1.2.3.4", "a.b.c", "1.2.3-", "latest"] {
        assert!(parse_version(bad).is_none(), "{bad}");
    }
}

#[test]
fn older_is_strictly_less_and_unknown_is_never_older() {
    assert!(is_older("0.26.1", "0.27.0"));
    assert!(is_older("0.9.9", "0.10.0")); // 문자열 비교였다면 틀린다
    assert!(!is_older("0.27.0", "0.27.0"));
    assert!(!is_older("0.28.0", "0.27.0"));
    // 프리릴리스는 같은 숫자의 정식보다 앞, 프리릴리스끼리는 번호 순.
    assert!(is_older("0.27.0-next.1", "0.27.0"));
    assert!(!is_older("0.27.0", "0.27.0-next.1"));
    assert!(is_older("0.27.0-next.1", "0.27.0-next.2"));
    // 못 읽는 쪽이 있으면 건드리지 않는다.
    assert!(!is_older("dev", "0.27.0"));
    assert!(!is_older("0.27.0", "dev"));
}

#[test]
fn label_puts_the_commit_in_parentheses_only_when_known() {
    assert_eq!(label("0.49.0", Some("abc1234")), "0.49.0 (abc1234)");
    assert_eq!(label("0.49.0", None), "0.49.0");
    assert_eq!(label("0.49.0", Some("")), "0.49.0");
}

#[test]
fn build_label_starts_with_the_crate_version() {
    // 커밋은 빌드한 자리(git 안·밖)에 따라 있거나 없다 — 버전이 앞에 오는 것만 고정한다.
    let own = build_label();
    assert!(own.starts_with(env!("CARGO_PKG_VERSION")), "{own}");
    match GIT_COMMIT {
        Some(c) => assert!(own.ends_with(&format!("({c})")), "{own}"),
        None => assert_eq!(own, env!("CARGO_PKG_VERSION")),
    }
}

#[test]
fn same_build_compares_commits_only_when_both_are_known() {
    assert!(same_build(
        Some("0.49.0"),
        Some("abc1234"),
        "0.49.0",
        Some("abc1234")
    ));
    // main 빌드끼리 — 버전은 같고 커밋이 다르면 다른 빌드다
    assert!(!same_build(
        Some("0.49.0"),
        Some("abc1234"),
        "0.49.0",
        Some("def5678")
    ));
    // 어느 한쪽이 커밋을 모르면(dev 빌드·구버전 데몬) 버전만 본다
    assert!(same_build(Some("0.49.0"), None, "0.49.0", Some("def5678")));
    assert!(same_build(Some("0.49.0"), Some("abc1234"), "0.49.0", None));
    assert!(!same_build(
        Some("0.48.0"),
        Some("abc1234"),
        "0.49.0",
        Some("abc1234")
    ));
    assert!(!same_build(None, None, "0.49.0", None));
}
