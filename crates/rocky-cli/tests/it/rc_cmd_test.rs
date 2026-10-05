use rocky_cli::rc_cmd::{human_uptime, render_result, render_status};
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
