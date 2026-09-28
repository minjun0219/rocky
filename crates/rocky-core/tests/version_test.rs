//! 버전 비교 — 매 턴 훅의 "구버전만 올린다" 판정.

use rocky_core::version::{is_older, parse_version};

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
