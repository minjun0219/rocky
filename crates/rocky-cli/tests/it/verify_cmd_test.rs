//! `rocky verify` 렌더링 — 통과·실패·도는 중·시작 못 함·배포.

use rocky_cli::verify_cmd::{render_rerun, render_verify};
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

#[test]
fn marks_records_that_needed_the_automatic_retry() {
    let raw = json!({ "targets": [
        { "board": "rocky", "branch": "main", "record": { "sha": "abcdef1234", "state": "passed", "attempt": 2, "subject": "s" } },
        { "board": "web", "branch": "main", "record": { "sha": "1234567abc", "state": "running", "attempt": 2 } }
    ]});
    let text = render_verify(&raw);
    assert!(
        text.contains("✓ rocky main abcdef1 통과(다시 돌려서)"),
        "{text}"
    );
    assert!(
        text.contains("… web main 1234567 검증 중(자동 재시도)"),
        "{text}"
    );
}

#[test]
fn shows_the_deploy_under_its_target() {
    let raw = json!({ "targets": [
        { "board": "rocky", "branch": "main", "record": { "sha": "abcdef1234", "state": "passed", "subject": "s" },
          "deploy": { "sha": "abcdef1234", "state": "succeeded", "finishedAt": "2026-10-18T01:10:00.000Z", "log": "/tmp/d.log" } },
        { "board": "next", "branch": "main", "record": { "sha": "1234567abc", "state": "passed", "subject": "s" },
          "deploy": { "sha": "1234567abc", "state": "running", "log": "/tmp/r.log" } },
        { "board": "web", "branch": "main", "record": { "sha": "9999999aaa", "state": "passed", "subject": "s" },
          "deploy": { "sha": "9999999aaa", "state": "failed", "reason": "update 종료 코드 1", "log": "/tmp/f.log" },
          "deployNote": "다른 main 빌드가 돌고 있다(main-sync.lock) — 끝나면 다음 바퀴에 다시 본다" }
    ]});
    let text = render_verify(&raw);
    let local = chrono::DateTime::parse_from_rfc3339("2026-10-18T01:10:00.000Z")
        .unwrap()
        .with_timezone(&chrono::Local)
        .format("%H:%M")
        .to_string();
    assert!(
        text.contains(&format!(
            "✓ rocky main abcdef1 통과  · s\n    ↑ 배포 abcdef1 {local}"
        )),
        "{text}"
    );
    assert!(text.contains("↑ 배포 중 1234567"), "{text}");
    assert!(
        text.contains("⚠ 배포 실패 9999999 — update 종료 코드 1\n    로그 /tmp/f.log"),
        "{text}"
    );
    assert!(
        text.contains("⚠ 배포: 다른 main 빌드가 돌고 있다(main-sync.lock)"),
        "{text}"
    );
}

#[test]
fn renders_rerun_queued_and_skipped_targets() {
    let raw = json!({
        "queued": [{ "board": "rocky", "branch": "main" }],
        "running": [{ "board": "web", "branch": "main" }]
    });
    let text = render_rerun(&raw);
    assert!(
        text.contains("↻ rocky main — 지금 커밋을 다시 돈다"),
        "{text}"
    );
    assert!(
        text.contains("… web main — 이미 도는 중이라 건너뛴다"),
        "{text}"
    );
}
