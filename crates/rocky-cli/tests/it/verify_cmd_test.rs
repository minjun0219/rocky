//! `rocky verify` 렌더링 — 통과·실패·도는 중·시작 못 함.

use rocky_cli::verify_cmd::render_verify;
use serde_json::json;

#[test]
fn renders_each_target_state() {
    let raw = json!({ "targets": [
        { "board": "rocky", "branch": "main", "record": { "sha": "abcdef1234", "state": "passed", "subject": "feat: x", "finishedAt": "2026-10-05T12:03:00.000Z" } },
        { "board": "web", "branch": "main", "record": { "sha": "1234567abc", "state": "failed", "failedStep": "cargo-test", "reason": "종료 코드 101", "subject": "fix: y", "log": "/tmp/v.log" } },
        { "board": "api", "branch": "dev", "record": { "sha": "9999999aaa", "state": "running" } },
        { "board": "bare", "branch": "main", "error": "보드 bare 에 path 가 없다" }
    ]});
    let text = render_verify(&raw);
    let local = chrono::DateTime::parse_from_rfc3339("2026-10-05T12:03:00.000Z")
        .unwrap()
        .with_timezone(&chrono::Local)
        .format("%H:%M")
        .to_string();
    assert!(
        text.contains(&format!("✓ rocky main abcdef1 통과 {local} · feat: x")),
        "{text}"
    );
    assert!(
        text.contains("✗ web main 1234567 cargo-test 실패 — 종료 코드 101"),
        "{text}"
    );
    assert!(text.contains("로그 /tmp/v.log"), "{text}");
    assert!(text.contains("… api dev 9999999 검증 중"), "{text}");
    assert!(
        text.contains("· bare main — 아직 돌지 않았다\n    ⚠ 보드 bare 에 path 가 없다"),
        "{text}"
    );
    assert!(render_verify(&json!({ "targets": [] })).contains("검증 대상이 없다"));
}
