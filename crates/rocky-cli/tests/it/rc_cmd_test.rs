use rocky_cli::rc_cmd::{human_uptime, render_status};
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
