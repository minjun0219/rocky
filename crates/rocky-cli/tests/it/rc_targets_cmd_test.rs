use rocky_cli::rc_targets_cmd::{
    edit_line, reload_failure, reload_lines, render_list, stray_pid, tilde, write_config,
};
use rocky_core::rc_targets::{Change, Edit, TargetLine, TargetList};
use serde_json::{json, Value};

const PATH: &str = "/home/u/.config/rocky/rocky.json";

fn edit(change: Change, pinned: bool) -> Edit {
    Edit {
        change,
        label: "ogpeek".into(),
        dir: "/home/u/dev/workspaces/ogpeek".into(),
        entry: "ogpeek".into(),
        pinned,
        disabled: false,
    }
}

/// 데몬의 다시 읽기 응답 — 대상 목록이 바뀌었고 재시작할 칸은 없다.
fn reloaded(extra: Value) -> Value {
    let mut raw = json!({
        "active": true, "supervise": false, "applied": true, "added": [], "removed": [],
        "pinned": [], "unpinned": [], "restartNeeded": [], "path": PATH,
    });
    for (k, v) in extra.as_object().unwrap() {
        raw[k] = v.clone();
    }
    raw
}

#[test]
fn tilde_shortens_only_paths_under_home() {
    assert_eq!(tilde("/home/u", "/home/u"), "~");
    assert_eq!(tilde("/home/u/dev/x", "/home/u/"), "~/dev/x");
    assert_eq!(tilde("/home/ux", "/home/u"), "/home/ux");
    assert_eq!(tilde("/srv/x", ""), "/srv/x");
}

#[test]
fn edit_line_names_what_changed() {
    let cases = [
        (Change::Added, false, "✓ 대상을 더했다"),
        (Change::Added, true, "✓ 고정 대상을 더했다"),
        (Change::Pinned, true, "✓ 고정 대상으로 바꿨다"),
        (Change::Unpinned, false, "✓ 고정을 풀었다"),
        (Change::Unchanged, false, "= 이미 대상이다"),
        (Change::Unchanged, true, "= 이미 고정 대상이다"),
        (Change::Removed, true, "✓ 대상을 뺐다"),
    ];
    for (change, pinned, head) in cases {
        assert_eq!(
            edit_line(&edit(change, pinned), "/home/u"),
            format!("{head}: ogpeek (~/dev/workspaces/ogpeek)")
        );
    }
}

#[test]
fn reload_after_an_edit_says_what_to_do_next() {
    let raw = reloaded(json!({ "added": ["ogpeek"] }));
    assert_eq!(
        reload_lines(&raw, PATH, Some(&edit(Change::Added, false))),
        ["  데몬에 반영했다. 띄우려면: rocky rc start ogpeek"]
    );
    // 이미 있던 대상이어도(손으로 고치고 다시 읽히지 않은 목록) 데몬은 이참에 맞췄다.
    assert_eq!(
        reload_lines(
            &reloaded(json!({ "applied": false })),
            PATH,
            Some(&edit(Change::Unchanged, false))
        ),
        ["  데몬에 반영했다. 띄우려면: rocky rc start ogpeek"]
    );
    // 고정 대상은 감시가 켜져 있을 때만 감시에 맡긴다.
    assert_eq!(
        reload_lines(&raw, PATH, Some(&edit(Change::Pinned, true))),
        ["  데몬에 반영했다. 띄우려면: rocky rc start ogpeek"]
    );
    let watched = reloaded(json!({ "supervise": true }));
    assert_eq!(
        reload_lines(&watched, PATH, Some(&edit(Change::Added, true))),
        ["  데몬에 반영했다. 꺼져 있으면 감시가 띄운다(2분마다 잰다)"]
    );
    assert_eq!(
        reload_lines(&raw, PATH, Some(&edit(Change::Removed, false))),
        ["  데몬에 반영했다"]
    );
    // 고정을 풀면 띄울 것이 없다 — 감시가 켜져 있으면 더는 되살리지 않는다고만 알린다.
    assert_eq!(
        reload_lines(&raw, PATH, Some(&edit(Change::Unpinned, false))),
        ["  데몬에 반영했다"]
    );
    assert_eq!(
        reload_lines(&watched, PATH, Some(&edit(Change::Unpinned, false))),
        ["  데몬에 반영했다. 꺼져도 감시가 고정으로 되살리지 않는다"]
    );
}

#[test]
fn reload_on_its_own_lists_the_changes() {
    let raw = reloaded(json!({ "added": ["a", "b"], "removed": ["c"], "unpinned": ["d"] }));
    assert_eq!(
        reload_lines(&raw, PATH, None),
        [
            "✓ 데몬이 대상 목록을 다시 읽었다",
            "  더함: a, b · 뺌: c · 고정 풂: d"
        ]
    );
    let same = reloaded(json!({ "applied": false, "restartNeeded": ["supervise", "nightly"] }));
    assert_eq!(
        reload_lines(&same, PATH, None),
        [
            "= 데몬의 대상 목록이 이미 파일과 같다",
            "  데몬을 다시 띄워야 반영되는 설정이 바뀌었다(supervise · nightly): rocky daemon restart",
        ]
    );
}

#[test]
fn reload_names_worktree_switches_and_how_to_apply_them() {
    // 띄우는 방식이라 떠 있는 서버는 그대로다 — 다시 띄우는 법까지 알린다.
    let one = reloaded(json!({ "worktreeOn": ["rocky"] }));
    assert_eq!(
        reload_lines(&one, PATH, None),
        [
            "✓ 데몬이 대상 목록을 다시 읽었다",
            "  워크트리 켬: rocky",
            "  떠 있는 서버는 다시 띄워야 바뀐 모드로 뜬다: rocky rc restart rocky",
        ]
    );
    let two = reloaded(json!({ "worktreeOn": ["a"], "worktreeOff": ["b"] }));
    assert_eq!(
        reload_lines(&two, PATH, None),
        [
            "✓ 데몬이 대상 목록을 다시 읽었다",
            "  워크트리 켬: a · 워크트리 끔: b",
            "  떠 있는 서버는 다시 띄워야 바뀐 모드로 뜬다(a · b): rocky rc restart <이름>",
        ]
    );
}

#[test]
fn reload_points_at_a_restart_when_rc_turns_on_or_off() {
    let off_at_start = json!({
        "active": false, "applied": false, "restartNeeded": ["enabled"], "path": PATH
    });
    let want = ["  데몬이 rc 없이 떠 있다. 다시 띄워야 대상이 반영된다: rocky daemon restart"];
    assert_eq!(reload_lines(&off_at_start, PATH, None), want);
    assert_eq!(
        reload_lines(&off_at_start, PATH, Some(&edit(Change::Added, false))),
        want,
        "띄우라는 안내 대신 재시작"
    );
    let turned_off = reloaded(json!({ "applied": false, "restartNeeded": ["enabled"] }));
    assert_eq!(
        reload_lines(&turned_off, PATH, None),
        ["  rc 를 끄는 설정은 데몬을 다시 띄워야 반영된다: rocky daemon restart"]
    );
    let unused = json!({ "active": false, "applied": false, "restartNeeded": [], "path": PATH });
    assert_eq!(
        reload_lines(&unused, PATH, None),
        ["= 데몬이 rc 를 쓰지 않는다. rocky.json 에 rc 블록이 없거나 rc.enabled 가 false 다"]
    );
    assert!(
        reload_lines(&unused, PATH, Some(&edit(Change::Added, false))).is_empty(),
        "꺼진 rc 는 고친 줄의 메모가 이미 말한다"
    );
}

#[test]
fn reload_warns_when_the_daemon_reads_another_file() {
    let raw = reloaded(json!({ "path": "/other/rocky.json", "applied": false }));
    let warning =
        format!("  ⚠ 데몬은 다른 설정 파일을 읽는다: /other/rocky.json. 이 셸의 설정 파일: {PATH}");
    assert_eq!(reload_lines(&raw, PATH, None)[0], warning);

    // 고친 파일은 데몬이 읽지 않는다 — 반영했다 · 띄우라는 안내도, 데몬 파일의 재시작 안내도 붙이지 않는다.
    let other = reloaded(json!({ "path": "/other/rocky.json", "restartNeeded": ["supervise"] }));
    for change in [Change::Added, Change::Removed] {
        assert_eq!(
            reload_lines(&other, PATH, Some(&edit(change, false))),
            [
                warning.as_str(),
                "  이 셸의 파일을 고쳐도 데몬에는 반영되지 않는다. 데몬이 읽는 파일을 고치려면 ROCKY_CONFIG 를 그 경로로 설정하고 다시 한다",
            ],
            "{change:?}"
        );
    }
}

#[test]
fn reload_failure_tells_an_old_daemon_apart() {
    assert_eq!(
        reload_failure("not found: POST /api/rc/reload"),
        ["  데몬이 다시 읽기를 모르는 옛 버전이다. 반영하려면: rocky daemon restart"]
    );
    assert_eq!(
        reload_failure("설정 파일이 JSON이 아니다(/x): y"),
        [
            "  데몬에 반영하지 못했다: 설정 파일이 JSON이 아니다(/x): y",
            "  다시 해 보려면: rocky rc reload",
        ]
    );
}

#[test]
fn stray_pid_finds_the_server_left_in_a_removed_folder() {
    let raw = json!({ "strays": [{ "label": "x", "dir": "/w/x", "pid": 42 }] });
    assert_eq!(stray_pid(&raw, "/w/x"), Some(42));
    assert_eq!(stray_pid(&raw, "/w/y"), None);
    assert_eq!(stray_pid(&json!({}), "/w/x"), None);
}

fn line(label: &str, dir: &str, pinned: bool, exists: bool, ambiguous: bool) -> TargetLine {
    TargetLine {
        label: label.into(),
        dir: dir.into(),
        entry: label.into(),
        pinned,
        worktree: false,
        exists,
        ambiguous,
    }
}

#[test]
fn list_is_a_table_with_memos_and_a_footer() {
    let list = TargetList {
        configured: true,
        enabled: true,
        root: "/home/u/dev/workspaces".into(),
        targets: vec![
            line("알파", "/home/u/dev/workspaces/알파", true, true, false),
            line("x", "/srv/x", false, false, true),
            TargetLine {
                worktree: true,
                ..line("rocky", "/home/u/dev/workspaces/rocky", false, true, false)
            },
        ],
        unmatched_worktree: vec!["rokcy".into()],
    };
    assert_eq!(
        render_list(&list, "~/.config/rocky/rocky.json", "/home/u"),
        "\
이름   고정  폴더
알파   고정  ~/dev/workspaces/알파
x            /srv/x  ⚠ 폴더가 없다 · ⚠ 이름이 겹쳐 이름으로 부를 수 없다
rocky        ~/dev/workspaces/rocky  워크트리
⚠ 대상이 아닌 워크트리 이름(무시된다): rokcy

설정 파일: ~/.config/rocky/rocky.json · 맨 이름은 root 아래: ~/dev/workspaces"
    );
    let off = TargetList {
        configured: true,
        enabled: false,
        root: "/w".into(),
        targets: vec![],
        unmatched_worktree: vec![],
    };
    assert_eq!(
        render_list(&off, "/c/rocky.json", "/home/u"),
        "\
⚠ 이 기기에서는 rc 가 꺼져 있다(rc.enabled: false). 목록만 남아 있다
rc 대상이 없다. 더하려면: rocky rc targets add <이름|경로>

설정 파일: /c/rocky.json · 맨 이름은 root 아래: /w"
    );
}

#[cfg(unix)]
#[test]
fn write_config_keeps_links_and_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();

    // 없던 파일 — 폴더까지 만든다.
    let fresh = dir.path().join("new/rocky.json");
    write_config(&fresh, "{}\n").unwrap();
    assert_eq!(std::fs::read_to_string(&fresh).unwrap(), "{}\n");

    // dotfiles 링크 — 링크는 그대로, 가리키는 파일만 바뀐다. 권한도 그대로.
    let real = dir.path().join("dotfiles-rocky.json");
    std::fs::write(&real, "{\"a\":1}").unwrap();
    std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o600)).unwrap();
    let link = dir.path().join("rocky.json");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    write_config(&link, "{\"a\":2}\n").unwrap();
    assert!(std::fs::symlink_metadata(&link)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(std::fs::read_to_string(&real).unwrap(), "{\"a\":2}\n");
    assert_eq!(
        std::fs::metadata(&real).unwrap().permissions().mode() & 0o777,
        0o600
    );
    // 임시 파일이 남지 않는다.
    let names: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(names.len(), 3, "{names:?}");
}
