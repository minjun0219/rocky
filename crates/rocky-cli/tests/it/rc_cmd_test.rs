use rocky_cli::rc_cmd::{human_uptime, nightly_line, render_nightly, render_result, render_status};
use serde_json::json;

#[test]
fn uptime_is_one_unit() {
    assert_eq!(human_uptime(45), "45초");
    assert_eq!(human_uptime(600), "10분");
    assert_eq!(human_uptime(3 * 3600 + 5), "3시간");
    assert_eq!(human_uptime(2 * 86_400), "2일");
}

#[test]
fn status_lists_targets_strays_auth_and_agy() {
    let raw = json!({
        "configured": true,
        "servers": [
            {"label": "repo-a", "dir": "/w/repo-a", "pinned": true, "running": true, "pid": 1, "uptimeSecs": 600, "sessions": 2},
            {"label": "repo-b", "dir": "/w/repo-b", "pinned": false, "running": false, "sessions": 0}
        ],
        "strays": [{"label": "old", "dir": "/w/old", "pid": 9, "sessions": 0}],
        "auth": "out",
        "antigravity": {"state": "running", "pid": 7, "instance": "mac-1"}
    });
    assert_eq!(
        render_status(&raw),
        "● repo-a  고정  세션 2  10분\n○ repo-b\n대상 밖:\n  ● old  /w/old\n자격: ⚠ 로그아웃 — 새로 띄우는 서버가 로그인 안 된 채 뜬다\nantigravity: running (mac-1)"
    );
}

#[test]
fn unconfigured_points_at_the_config() {
    assert!(render_status(&json!({"configured": false})).contains("\"rc\""));
}

#[test]
fn probe_error_comes_first() {
    let raw = json!({"configured": true, "servers": [], "strays": [], "auth": "unknown", "probeError": "ps 실패: x"});
    assert!(render_status(&raw).starts_with("⚠ ps 실패: x — "));
}

#[test]
fn unconfigured_still_shows_agy() {
    // rc 블록이 없는 기기에서도 agy 줄은 보인다(설치 여부를 따른다).
    let out = render_status(&json!({
        "configured": false,
        "antigravity": {"state": "stopped", "instance": "mac-1"}
    }));
    assert!(out.ends_with("\nantigravity: stopped (mac-1)"), "{out}");
    assert_eq!(
        rocky_cli::rc_cmd::agy_line(&json!({"antigravity": null})),
        None
    );
}

#[test]
fn status_shows_action_and_failed_result() {
    let raw = json!({
        "configured": true,
        "servers": [
            {"label": "a", "dir": "/w/a", "pinned": false, "running": true, "sessions": 0, "action": "restarting"},
            {"label": "b", "dir": "/w/b", "pinned": false, "running": false, "sessions": 0,
             "lastResult": {"ok": false, "message": "뜨자마자 내려갔다 — x", "at": "t"}},
            {"label": "c", "dir": "/w/c", "pinned": false, "running": true, "sessions": 0,
             "lastResult": {"ok": true, "message": "떴다", "at": "t"}}
        ],
        "strays": [], "auth": "in", "antigravity": null
    });
    let out = render_status(&raw);
    assert!(out.contains("● a  재시작 중…"));
    assert!(out.contains("○ b  ✗ 뜨자마자 내려갔다 — x"));
    assert!(out.contains("● c\n") || out.contains("● c\r") || out.lines().any(|l| l == "● c"));
}

#[test]
fn result_line_marks_success_or_failure() {
    let ok =
        json!({"lastResult": {"ok": true, "message": "떴다 — 새 세션과 함께(pid 1)", "at": "t"}});
    assert_eq!(render_result("a", &ok), "✓ a: 떴다 — 새 세션과 함께(pid 1)");
    let bad = json!({"lastResult": {"ok": false, "message": "already served", "at": "t"}});
    assert_eq!(render_result("a", &bad), "✗ a: already served");
}

#[test]
fn status_shows_supervise_and_auth_suspect() {
    let raw = json!({
        "configured": true,
        "servers": [
            {"label": "a", "dir": "/w/a", "pinned": true, "running": true, "sessions": 0, "authSuspect": true}
        ],
        "strays": [], "auth": "in", "antigravity": null,
        "supervise": {"lastTick": "2026-10-06T07:30:05+00:00", "loggedOut": false}
    });
    let out = render_status(&raw);
    assert!(out.contains("● a  고정  ⚠ 자격 의심"), "{out}");
    // 시각은 이 기기의 시간대로 바뀐다 — 줄 모양만 본다.
    let last = out.lines().last().unwrap();
    assert!(
        last.starts_with("감시: 켜짐 — 마지막 ") && last.ends_with(":30"),
        "{out}"
    );
}

#[test]
fn nightly_preview_lists_what_would_happen() {
    let raw = json!({
        "startedAt": "2026-10-07T04:30:00Z",
        "dryRun": true,
        "update": "건너뜀(리허설) · 설치 2.1.300",
        "version": "2.1.300",
        "items": [
            {"label": "repo-a", "outcome": "would-restart", "note": "2.1.283 → 2.1.300 · 새 세션과 함께"},
            {"label": "b", "outcome": "would-wait", "note": "작업 중 — 07:00 까지 5분마다 다시 본다"},
            {"label": "c", "outcome": "current", "note": "2.1.300"},
            {"label": "d", "outcome": "skipped", "note": "기동 버전 기록 없음 — 구버전인지 모른다"}
        ]
    });
    assert_eq!(
        render_nightly(&raw),
        "야간 리허설 — update: 건너뜀(리허설) · 설치 2.1.300\n\
         ↻ repo-a  2.1.283 → 2.1.300 · 새 세션과 함께\n\
         … b       작업 중 — 07:00 까지 5분마다 다시 본다\n\
         = c       2.1.300\n\
         – d       기동 버전 기록 없음 — 구버전인지 모른다"
    );
    let blocked = json!({"dryRun": true, "update": "x", "blocked": "logged-out", "items": []});
    assert_eq!(
        render_nightly(&blocked),
        "야간 리허설 — update: x\n전부 건너뜀 — logged-out"
    );
}

#[test]
fn nightly_line_summarizes_the_last_run() {
    assert_eq!(
        nightly_line(&json!({"at": "04:30", "running": false})),
        "야간: 04:30 · 아직 안 돌았다"
    );
    assert_eq!(
        nightly_line(&json!({"at": "04:30", "running": true})),
        "야간: 04:30 · 도는 중"
    );
    let last = json!({
        "at": "04:30",
        "running": false,
        "last": {"startedAt": "x", "update": "u", "items": [
            {"label": "a", "outcome": "restarted", "note": ""},
            {"label": "b", "outcome": "skipped", "note": ""},
            {"label": "c", "outcome": "down", "note": ""}
        ]}
    });
    assert_eq!(
        nightly_line(&last),
        "야간: 04:30 · 마지막 ? — 재시작 1 · 건너뜀 1 · ⚠ 못 띄움 1"
    );
    // 손으로만 돌렸으면 시각이 없다.
    let manual = json!({"running": false, "last": {"startedAt": "x", "update": "u", "blocked": "logged-out", "items": []}});
    assert_eq!(
        nightly_line(&manual),
        "야간: 마지막 ? — 전부 건너뜀(logged-out)"
    );
}

#[test]
fn stale_rows_say_so() {
    let raw = json!({
        "configured": true,
        "servers": [{"label": "a", "dir": "/w/a", "pinned": false, "running": true, "pid": 1, "sessions": 0, "stale": true}],
        "strays": [],
        "auth": "in"
    });
    assert!(render_status(&raw).starts_with("● a  구버전\n"));
}

#[test]
fn waiting_rows_say_the_turn_is_still_going() {
    let raw = json!({
        "configured": true,
        "servers": [{"label": "a", "dir": "/w/a", "pinned": false, "running": true, "pid": 1, "sessions": 1, "action": "waiting"}],
        "strays": [],
        "auth": "in"
    });
    assert!(render_status(&raw).starts_with("● a  세션 1  대화가 끝나길 기다리는 중…\n"));
}
