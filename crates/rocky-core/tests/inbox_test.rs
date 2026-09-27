//! 수집함 어댑터 규약 — stdout JSON 파싱·검증.

use rocky_core::inbox::{parse_inbox_output, unavailable, InboxItem};

#[test]
fn parses_full_and_minimal_items() {
    let out = r#"{"items":[
      {"id":"a1","title":"보드 TUI 설계","url":"https://tasks.example/a1","note":"평문","due":"2026-10-01","createdAt":"2026-09-27T01:02:03Z"},
      {"id":7,"title":"  숫자 id  "}
    ]}"#;
    let items = parse_inbox_output(out).unwrap();
    assert_eq!(
        items[0],
        InboxItem {
            id: "a1".into(),
            title: "보드 TUI 설계".into(),
            url: Some("https://tasks.example/a1".into()),
            note: Some("평문".into()),
            due: Some("2026-10-01".into()),
            created_at: Some("2026-09-27T01:02:03Z".into()),
        }
    );
    // 숫자 id 는 문자열로, title 은 trim.
    assert_eq!(items[1].id, "7");
    assert_eq!(items[1].title, "숫자 id");
    assert_eq!(items[1].url, None);
}

#[test]
fn empty_items_is_ok_and_blank_url_is_dropped() {
    assert!(parse_inbox_output(r#"{"items":[]}"#).unwrap().is_empty());
    let items = parse_inbox_output(r#"{"items":[{"id":"x","title":"t","url":"  "}]}"#).unwrap();
    assert_eq!(items[0].url, None);
}

#[test]
fn missing_required_fields_fail_the_whole_output() {
    let err = parse_inbox_output(r#"{"items":[{"title":"no id"}]}"#).unwrap_err();
    assert!(err.contains("items[0].id"), "{err}");
    let err = parse_inbox_output(r#"{"items":[{"id":"a","title":"ok"},{"id":"b","title":"  "}]}"#)
        .unwrap_err();
    assert!(
        err.contains("items[1].title") && err.contains("id=b"),
        "{err}"
    );
}

#[test]
fn bad_dates_are_rejected_with_context() {
    let err =
        parse_inbox_output(r#"{"items":[{"id":"a","title":"t","due":"2026/10/01"}]}"#).unwrap_err();
    assert!(err.contains("due") && err.contains("2026/10/01"), "{err}");
    let err = parse_inbox_output(r#"{"items":[{"id":"a","title":"t","createdAt":"yesterday"}]}"#)
        .unwrap_err();
    assert!(
        err.contains("createdAt") && err.contains("yesterday"),
        "{err}"
    );
}

#[test]
fn non_json_and_empty_stdout_are_errors() {
    assert!(parse_inbox_output("").unwrap_err().contains("비어"));
    assert!(parse_inbox_output("not json").unwrap_err().contains("JSON"));
    assert!(parse_inbox_output(r#"{"todos":[]}"#)
        .unwrap_err()
        .contains("JSON"));
}

#[test]
fn unavailable_shape() {
    let r = unavailable("gtasks", "exit 1: boom", "2026-09-27T00:00:00.000Z".into());
    let json = serde_json::to_value(&r).unwrap();
    assert_eq!(json["name"], "gtasks");
    assert_eq!(json["available"], false);
    assert_eq!(json["reason"], "exit 1: boom");
    assert_eq!(json["fetchedAt"], "2026-09-27T00:00:00.000Z");
    assert_eq!(json["items"], serde_json::json!([]));
}

#[test]
fn redact_keeps_only_exit_code() {
    use rocky_core::inbox::{redact_reason, InboxResponse, InboxSourceResult};
    let generic = "어댑터 출력이 규약에 맞지 않는다 (상세는 로컬 요청에서만)";
    assert_eq!(
        redact_reason("exit 1: https://api.example/?token=SECRET"),
        "exit 1"
    );
    assert_eq!(redact_reason("exit 3"), "exit 3");
    assert_eq!(redact_reason("exit 200ms 안에 끝나지 않았다"), generic);
    assert_eq!(
        redact_reason("JSON 파싱 실패: expected value at line 1"),
        generic
    );

    let ok = InboxSourceResult {
        name: "b".into(),
        available: true,
        reason: None,
        fetched_at: "t".into(),
        items: vec![InboxItem {
            id: "x".into(),
            title: "t".into(),
            url: None,
            note: None,
            due: None,
            created_at: None,
        }],
    };
    let response = InboxResponse {
        sources: vec![unavailable("a", "exit 1: token=SECRET", "t".into()), ok],
    }
    .redacted();
    assert_eq!(response.sources[0].reason.as_deref(), Some("exit 1"));
    assert_eq!(response.sources[1].reason, None);
    assert_eq!(response.sources[1].items.len(), 1);
}
