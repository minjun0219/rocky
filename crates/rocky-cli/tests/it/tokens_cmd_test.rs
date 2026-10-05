//! `rocky tokens` 렌더링 — 숫자 줄이기, 합계 표, 현재 세션과 추천.

use rocky_cli::tokens_cmd::{compact, render_current, render_summary};
use serde_json::json;

#[test]
fn compact_shortens_big_numbers() {
    assert_eq!(compact(999), "999");
    assert_eq!(compact(1_234), "1.2k");
    assert_eq!(compact(43_000), "43k");
    assert_eq!(compact(3_718_277), "3.7M");
    assert_eq!(compact(1_740_448_341), "1.7B");
}

#[test]
fn summary_table_shows_keys_and_per_turn_output() {
    let raw = json!({ "groupBy": "model,effort", "rows": [
        { "model": "claude-opus-5-5", "effort": "medium", "sessions": 3, "turns": 10, "requests": 40,
          "inputTokens": 100, "outputTokens": 25_000, "mainOutputTokens": 25_000, "cacheReadTokens": 2_000_000, "cacheWriteTokens": 5_000, "toolCalls": 12 }
    ]});
    let text = render_summary(&raw, "7d");
    assert!(text.contains("claude-opus-5-5 · medium"), "{text}");
    assert!(text.contains("2.5k"), "턴당 출력: {text}");
    assert!(text.contains("2.0M"), "{text}");
    assert!(render_summary(&json!({ "rows": [] }), "7d").contains("없다"));
}

#[test]
fn current_session_shows_turns_changes_and_recommendation() {
    let raw = json!({
        "session": { "sessionId": "abcdef123456", "cwd": "/repo", "gitBranch": "main" },
        "turns": [{ "startedAt": "2026-10-05T01:02:03Z", "model": "claude-opus-5-5", "effort": "max", "outputTokens": 800, "toolCalls": 0 }],
        "effortChanges": [{ "from": "high", "to": "max", "ts": "2026-10-05T01:00:00Z" }],
        "recommendation": { "suggestions": [{ "rule": "lower-effort", "message": "medium 으로 낮추는 것을 고려" }] }
    });
    let text = render_current(&raw);
    assert!(text.starts_with("세션 abcdef12 · /repo · main"), "{text}");
    assert!(text.contains("effort high → max"), "{text}");
    assert!(text.contains("→ medium 으로 낮추는 것을 고려"), "{text}");

    let held =
        json!({ "session": {}, "recommendation": { "suggestions": [], "held": "턴이 2개뿐이다" } });
    assert!(render_current(&held).contains("추천 없음 — 턴이 2개뿐이다"));
}
