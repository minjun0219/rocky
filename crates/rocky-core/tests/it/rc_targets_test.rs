use rocky_core::config::{NightlyConfig, RcConfig};
use rocky_core::rc_targets::*;
use serde_json::{json, Value};

const HOME: &str = "/home/u";
const ROOT: &str = "/home/u/dev/workspaces";

fn any_dir(_: &str) -> bool {
    true
}

fn no_dir(_: &str) -> bool {
    false
}

fn add(doc: &mut Value, input: &str, pin: bool) -> Result<Edit, String> {
    add_target(
        doc,
        input,
        pin,
        "/home/u/dev/workspaces/beta",
        HOME,
        &any_dir,
    )
}

fn rm(doc: &mut Value, key: &str) -> Result<Edit, String> {
    remove_target(doc, key, "/home/u/dev/workspaces/beta", HOME)
}

fn unpin(doc: &mut Value, key: &str) -> Result<Edit, String> {
    unpin_target(doc, key, "/home/u/dev/workspaces/beta", HOME)
}

fn edit(change: Change, label: &str, dir: &str, entry: &str, pinned: bool) -> Edit {
    Edit {
        change,
        label: label.into(),
        dir: dir.into(),
        entry: entry.into(),
        pinned,
        disabled: false,
    }
}

#[test]
fn entry_is_written_in_the_shortest_form() {
    let cases = [
        ("맨 이름", "ogpeek", ROOT, "ogpeek"),
        ("끝 슬래시", "ogpeek/", ROOT, "ogpeek"),
        ("겹친 조각", "a//b/./c/../d", ROOT, "a/b/d"),
        (
            "root 아래 절대 경로(셸이 ~ 를 푼 꼴)",
            "/home/u/dev/workspaces/ogpeek",
            ROOT,
            "ogpeek",
        ),
        ("root 아래 ~ 꼴", "~/dev/workspaces/ogpeek", ROOT, "ogpeek"),
        ("홈 아래", "/home/u/notes", ROOT, "~/notes"),
        ("홈", "~", ROOT, "~"),
        ("root 자신", "~/dev/workspaces", ROOT, "~/dev/workspaces"),
        ("홈 밖", "/srv/x/../y", ROOT, "/srv/y"),
        ("지금 폴더", ".", "/home/u/dev/workspaces/beta", "beta"),
        ("윗 폴더", "..", "/home/u/dev/workspaces/beta/sub", "beta"),
        (
            "옆 폴더",
            "../alpha",
            "/home/u/dev/workspaces/beta",
            "alpha",
        ),
        ("홈 아래 지금 폴더", ".", "/home/u/other/", "~/other"),
        ("홈 밖 지금 폴더", "./x", "/srv", "/srv/x"),
    ];
    for (what, input, cwd, want) in cases {
        assert_eq!(
            entry_for(input, cwd, HOME, ROOT).as_deref(),
            Ok(want),
            "{what}: {input}"
        );
    }
    assert_eq!(
        entry_for("  ", ROOT, HOME, ROOT),
        Err("대상 이름이 비었다".into())
    );
}

#[test]
fn add_creates_the_block_after_existing_keys() {
    let mut doc = json!({ "$schema": "s", "todo": { "port": 1 } });
    assert_eq!(
        add(&mut doc, "ogpeek", false),
        Ok(edit(
            Change::Added,
            "ogpeek",
            "/home/u/dev/workspaces/ogpeek",
            "ogpeek",
            false
        ))
    );
    assert_eq!(
        add(&mut doc, ".", true).map(|e| (e.change, e.entry, e.pinned)),
        Ok((Change::Added, "beta".into(), true))
    );
    assert_eq!(
        serde_json::to_string(&doc).unwrap(),
        r#"{"$schema":"s","todo":{"port":1},"rc":{"targets":["ogpeek"],"pinned":["beta"]}}"#
    );
}

#[test]
fn add_keeps_existing_entries_and_only_promotes_to_pinned() {
    let mut doc = json!({ "rc": { "pinned": ["alpha"], "targets": ["~/dev/workspaces/beta"] } });
    let before = doc.clone();
    // 꼴이 달라도 같은 폴더면 그대로 — 적힌 이름을 돌려준다.
    assert_eq!(
        add(&mut doc, "beta", false),
        Ok(edit(
            Change::Unchanged,
            "beta",
            "/home/u/dev/workspaces/beta",
            "~/dev/workspaces/beta",
            false
        ))
    );
    assert_eq!(
        add(&mut doc, "/home/u/dev/workspaces/alpha", true).map(|e| (e.change, e.pinned)),
        Ok((Change::Unchanged, true))
    );
    // --pin 없이 다시 더해도 고정을 풀지 않는다.
    assert_eq!(
        add(&mut doc, "alpha", false).map(|e| (e.change, e.pinned)),
        Ok((Change::Unchanged, true))
    );
    assert_eq!(doc, before);
    assert_eq!(
        add(&mut doc, "beta", true),
        Ok(edit(
            Change::Pinned,
            "beta",
            "/home/u/dev/workspaces/beta",
            "~/dev/workspaces/beta",
            true
        ))
    );
    assert_eq!(
        doc,
        json!({ "rc": { "pinned": ["alpha", "~/dev/workspaces/beta"], "targets": [] } })
    );
}

#[test]
fn add_refuses_a_label_clash_and_a_missing_folder() {
    let mut doc = json!({ "rc": { "targets": ["/srv/ogpeek"] } });
    let before = doc.clone();
    assert_eq!(
        add(&mut doc, "ogpeek", false),
        Err("이름이 같은 대상이 이미 있다: ogpeek(/srv/ogpeek). rc 는 폴더 이름으로 부르므로 둘을 함께 둘 수 없다".into())
    );
    let cwd = "/home/u/dev/workspaces/beta";
    assert_eq!(
        add_target(&mut doc, "nope", false, cwd, HOME, &no_dir),
        Err("폴더가 없다: /home/u/dev/workspaces/nope. 맨 이름은 root 아래로 푼다: /home/u/dev/workspaces".into())
    );
    assert_eq!(
        add_target(&mut doc, "/srv/nope", false, cwd, HOME, &no_dir),
        Err("폴더가 없다: /srv/nope".into())
    );
    // 고정으로 옮길 때도 폴더를 본다.
    assert_eq!(
        add_target(&mut doc, "/srv/ogpeek", true, cwd, HOME, &no_dir),
        Err("폴더가 없다: /srv/ogpeek".into())
    );
    assert_eq!(
        add(&mut doc, "/", false),
        Err("rc 대상으로 쓸 수 없는 폴더다: /".into())
    );
    assert_eq!(doc, before);
}

#[test]
fn add_follows_root_and_reports_a_disabled_block() {
    let mut doc = json!({ "rc": { "root": "~/src", "enabled": false } });
    let got = add(&mut doc, "x", false).unwrap();
    assert_eq!((got.dir.as_str(), got.disabled), ("/home/u/src/x", true));
    assert_eq!(doc["rc"]["targets"], json!(["x"]));
}

#[test]
fn add_refuses_hand_broken_shapes() {
    let cases = [
        (json!([]), "rocky.json의 맨 위가 객체가 아니다"),
        (
            json!({ "rc": "x" }),
            "rocky.json의 rc가 객체가 아니다. 손으로 고친 뒤 다시 한다",
        ),
        (
            json!({ "rc": { "targets": "x" } }),
            "rocky.json의 rc.targets가 배열이 아니다. 손으로 고친 뒤 다시 한다",
        ),
    ];
    for (mut doc, want) in cases {
        assert_eq!(add(&mut doc, "a", false), Err(want.to_string()));
    }
}

#[test]
fn remove_matches_a_label_or_a_path_and_clears_both_lists() {
    // 손으로 고치다 두 목록에 같은 폴더가 들어간 경우 — 둘 다 지운다.
    let mut doc = json!({ "rc": {
        "pinned": ["alpha", "~/dev/workspaces/beta"],
        "targets": ["beta/", "/srv/gamma"]
    } });
    assert_eq!(
        rm(&mut doc, "beta"),
        Ok(edit(
            Change::Removed,
            "beta",
            "/home/u/dev/workspaces/beta",
            "~/dev/workspaces/beta",
            true
        ))
    );
    assert_eq!(
        rm(&mut doc, "/srv/gamma").map(|e| (e.label, e.pinned)),
        Ok(("gamma".into(), false))
    );
    assert_eq!(
        remove_target(&mut doc, ".", "/home/u/dev/workspaces/alpha", HOME).map(|e| e.label),
        Ok("alpha".into())
    );
    assert_eq!(doc, json!({ "rc": { "pinned": [], "targets": [] } }));
}

#[test]
fn list_marks_worktree_targets_and_names_what_matches_none() {
    // 대상과 같은 폴더로 풀리면 어느 꼴로 적어도 맞는다 — 오타 · 대상 밖 폴더만 남긴다(한 번씩).
    let doc = json!({ "rc": {
        "pinned": ["alpha"],
        "targets": ["beta"],
        "worktree": ["~/dev/workspaces/alpha", "rokcy", " ", "/srv/other", "rokcy"]
    } });
    let list = list_targets(&doc, HOME, &any_dir);
    let marks: Vec<(&str, bool)> = list
        .targets
        .iter()
        .map(|t| (t.label.as_str(), t.worktree))
        .collect();
    assert_eq!(marks, vec![("alpha", true), ("beta", false)]);
    assert_eq!(list.unmatched_worktree, vec!["rokcy", "/srv/other"]);
}

#[test]
fn remove_also_drops_the_folder_from_the_worktree_list() {
    // 남겨 두면 같은 폴더를 다시 더할 때 말없이 워크트리 모드로 뜬다.
    let mut doc = json!({ "rc": {
        "pinned": ["alpha"],
        "targets": ["beta"],
        "worktree": ["alpha", "~/dev/workspaces/beta"]
    } });
    assert_eq!(rm(&mut doc, "beta").map(|e| e.label), Ok("beta".into()));
    assert_eq!(
        doc,
        json!({ "rc": { "pinned": ["alpha"], "targets": [], "worktree": ["alpha"] } })
    );
    // 고정 풀기는 목록만 옮긴다 — 워크트리 모드는 그대로다.
    assert_eq!(
        unpin(&mut doc, "alpha").map(|e| e.label),
        Ok("alpha".into())
    );
    assert_eq!(
        doc,
        json!({ "rc": { "pinned": [], "targets": ["alpha"], "worktree": ["alpha"] } })
    );
}

#[test]
fn remove_explains_what_it_could_not_match() {
    assert_eq!(
        rm(&mut json!({}), "x"),
        Err("rc 대상이 아니다: x. rocky.json에 rc 블록이 없다".into())
    );
    assert_eq!(
        rm(&mut json!({ "rc": {} }), "x"),
        Err("rc 대상이 아니다: x. 대상이 하나도 없다".into())
    );
    assert_eq!(
        rm(
            &mut json!({ "rc": { "pinned": ["a"], "targets": ["b"] } }),
            "c"
        ),
        Err("rc 대상이 아니다: c. 지금 대상: a, b".into())
    );
    let mut doc = json!({ "rc": { "targets": ["/srv/x", "/opt/x"] } });
    assert_eq!(
        rm(&mut doc, "x"),
        Err("이름이 같은 대상이 둘 이상이다: x. 경로로 고른다: /srv/x, /opt/x".into())
    );
    // 경로가 든 열쇠는 라벨로 맞대지 않는다 — 그 경로만 뺀다.
    assert_eq!(rm(&mut doc, "/opt/x").map(|e| e.dir), Ok("/opt/x".into()));
    assert_eq!(doc, json!({ "rc": { "targets": ["/srv/x"] } }));
}

#[test]
fn unpin_moves_the_written_name_to_targets() {
    let mut doc = json!({ "rc": {
        "pinned": ["alpha", "~/dev/workspaces/beta"],
        "targets": ["/srv/gamma"]
    } });
    // 라벨로 골라도 적힌 이름을 그대로 옮긴다 — 라벨을 적으면 root 아래의 다른 폴더로 풀린다.
    assert_eq!(
        unpin(&mut doc, "beta"),
        Ok(edit(
            Change::Unpinned,
            "beta",
            "/home/u/dev/workspaces/beta",
            "~/dev/workspaces/beta",
            false
        ))
    );
    assert_eq!(
        doc,
        json!({ "rc": {
            "pinned": ["alpha"],
            "targets": ["/srv/gamma", "~/dev/workspaces/beta"]
        } })
    );
    // 경로로도 고른다(`.` 은 지금 폴더). 일반 목록이 없으면 만든다.
    let mut doc = json!({ "rc": { "enabled": false, "pinned": ["alpha"] } });
    assert_eq!(
        unpin_target(&mut doc, ".", "/home/u/dev/workspaces/alpha", HOME)
            .map(|e| (e.change, e.entry, e.disabled)),
        Ok((Change::Unpinned, "alpha".into(), true))
    );
    assert_eq!(
        doc,
        json!({ "rc": { "enabled": false, "pinned": [], "targets": ["alpha"] } })
    );
}

#[test]
fn unpin_does_not_list_a_folder_twice() {
    // 손으로 고치다 두 목록에 같은 폴더가 들어간 경우 — 고정만 지우고 일반 목록에 또 적지 않는다.
    let mut doc = json!({ "rc": { "pinned": ["beta"], "targets": ["beta/"] } });
    assert_eq!(
        unpin(&mut doc, "beta").map(|e| e.change),
        Ok(Change::Unpinned)
    );
    assert_eq!(doc, json!({ "rc": { "pinned": [], "targets": ["beta/"] } }));
}

#[test]
fn unpin_refuses_what_is_not_pinned() {
    let mut doc = json!({ "rc": { "pinned": ["alpha"], "targets": ["beta"] } });
    let before = doc.clone();
    assert_eq!(
        unpin(&mut doc, "beta"),
        Err("고정 대상이 아니다: beta. 고정 대상: alpha".into())
    );
    assert_eq!(
        unpin(&mut json!({ "rc": { "targets": ["beta"] } }), "beta"),
        Err("고정 대상이 아니다: beta. 고정 대상이 하나도 없다".into())
    );
    // 대상이 아니면 rm 과 같은 말로 거절한다.
    assert_eq!(
        unpin(&mut doc, "c"),
        Err("rc 대상이 아니다: c. 지금 대상: alpha, beta".into())
    );
    assert_eq!(
        unpin(&mut json!({}), "x"),
        Err("rc 대상이 아니다: x. rocky.json에 rc 블록이 없다".into())
    );
    assert_eq!(doc, before);
}

#[test]
fn list_follows_the_daemon_order_and_flags_problems() {
    let doc = json!({ "rc": { "pinned": ["a"], "targets": ["/srv/x", "a", "/opt/x"] } });
    let list = list_targets(&doc, HOME, &|d: &str| d != "/opt/x");
    assert_eq!((list.configured, list.enabled), (true, true));
    assert_eq!(list.root, ROOT);
    let rows: Vec<(&str, &str, bool, bool, bool)> = list
        .targets
        .iter()
        .map(|t| {
            (
                t.label.as_str(),
                t.entry.as_str(),
                t.pinned,
                t.exists,
                t.ambiguous,
            )
        })
        .collect();
    assert_eq!(
        rows,
        vec![
            ("a", "a", true, true, false),
            ("x", "/srv/x", false, true, true),
            ("x", "/opt/x", false, false, true),
        ]
    );
    assert!(list.unmatched_worktree.is_empty());
    let none = list_targets(&json!({}), HOME, &any_dir);
    assert_eq!(
        (none.configured, none.enabled, none.targets.len()),
        (false, false, 0)
    );
    let off = list_targets(
        &json!({ "rc": { "enabled": false, "targets": ["a"] } }),
        HOME,
        &any_dir,
    );
    assert_eq!(
        (off.configured, off.enabled, off.targets.len()),
        (true, false, 1)
    );
}

#[test]
fn render_keeps_the_indent_unit_and_key_order() {
    let original = "{\n    \"z\": 1,\n    \"rc\": { \"targets\": [\"a\"] }\n}";
    let doc: Value = serde_json::from_str(original).unwrap();
    assert_eq!(
        render_config(&doc, Some(original)),
        "{\n    \"z\": 1,\n    \"rc\": {\n        \"targets\": [\n            \"a\"\n        ]\n    }\n}\n"
    );
    let tabs = "{\n\t\"a\": 1\n}\n";
    assert_eq!(
        render_config(&serde_json::from_str(tabs).unwrap(), Some(tabs)),
        tabs
    );
    // 들여쓰기를 못 찾으면(한 줄 파일 · 새 파일) 두 칸.
    assert_eq!(
        render_config(&json!({ "a": 1 }), Some(r#"{"a":1}"#)),
        "{\n  \"a\": 1\n}\n"
    );
    assert_eq!(render_config(&json!({}), None), "{}\n");
}

#[test]
fn new_config_is_the_init_template() {
    let doc = new_config();
    assert_eq!(
        doc,
        serde_json::from_str::<Value>(&rocky_core::setup::default_config_json()).unwrap()
    );
    assert!(doc.get("rc").is_none());
}

fn rc(pinned: &[&str], targets: &[&str]) -> RcConfig {
    RcConfig {
        pinned: pinned.iter().map(|s| s.to_string()).collect(),
        targets: targets.iter().map(|s| s.to_string()).collect(),
        ..RcConfig::default()
    }
}

#[test]
fn reload_applies_the_lists_and_keeps_the_switches() {
    let current = RcConfig {
        supervise: true,
        ..rc(&["a"], &["b", "c"])
    };
    let file = RcConfig {
        supervise: true,
        nightly: Some(NightlyConfig::default()),
        root: Some("~/dev/workspaces/".into()),
        ..rc(&["b"], &["a", "d"])
    };
    let (next, report) = plan_reload(Some(&current), Some(&file), HOME);
    assert_eq!(
        next,
        Some(RcConfig {
            root: Some("~/dev/workspaces/".into()),
            pinned: vec!["b".into()],
            targets: vec!["a".into(), "d".into()],
            worktree: vec![],
            supervise: true,
            nightly: None,
        })
    );
    assert_eq!(
        report,
        Reload {
            active: true,
            supervise: true,
            applied: true,
            added: vec!["d".into()],
            removed: vec!["c".into()],
            pinned: vec!["b".into()],
            unpinned: vec!["a".into()],
            worktree_on: vec![],
            worktree_off: vec![],
            restart_needed: vec!["nightly"],
        }
    );
}

#[test]
fn reload_applies_the_worktree_list_and_names_what_switched() {
    let current = RcConfig {
        worktree: vec!["a".into()],
        ..rc(&["a"], &["b", "c"])
    };
    // a 는 끄고 b 는 켜고(고정도 같이), 대상이 아닌 이름(z)은 아무것도 바꾸지 않는다.
    let file = RcConfig {
        worktree: vec!["b".into(), "z".into()],
        ..rc(&["a", "b"], &["c"])
    };
    let (next, report) = plan_reload(Some(&current), Some(&file), HOME);
    assert_eq!(
        next.map(|c| c.worktree),
        Some(vec!["b".to_string(), "z".to_string()])
    );
    assert_eq!(
        (
            report.pinned.clone(),
            report.worktree_on.clone(),
            report.worktree_off.clone()
        ),
        (vec!["b".into()], vec!["b".into()], vec!["a".into()])
    );
    assert_eq!(
        report.changes(),
        vec!["고정: b", "워크트리 켬: b", "워크트리 끔: a"]
    );
    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(json["worktreeOn"], serde_json::json!(["b"]));
    assert_eq!(json["worktreeOff"], serde_json::json!(["a"]));
    // 대상 밖 이름만 바뀌었다 — 띄우는 방식은 그대로지만 파일과 맞춘다.
    let (next, report) = plan_reload(
        Some(&current),
        Some(&RcConfig {
            worktree: vec!["a".into(), "z".into()],
            ..current.clone()
        }),
        HOME,
    );
    assert!(next.is_some() && report.changes().is_empty());
}

#[test]
fn reload_without_list_changes_applies_nothing() {
    let current = rc(&["a"], &["b"]);
    let same = plan_reload(Some(&current), Some(&current.clone()), HOME);
    assert_eq!(
        same,
        (
            None,
            Reload {
                active: true,
                ..Reload::default()
            }
        )
    );
    // 감시만 바뀌었다 — 대상 목록은 그대로, 감시는 재시작 몫.
    let file = RcConfig {
        supervise: true,
        ..current.clone()
    };
    let (next, report) = plan_reload(Some(&current), Some(&file), HOME);
    assert_eq!(next, None);
    assert_eq!(
        (report.applied, report.supervise, report.restart_needed),
        (false, false, vec!["supervise"])
    );
    // root 가 바뀌면 맨 이름은 다른 폴더다 — 뺀 것과 더한 것으로 보인다.
    let moved = RcConfig {
        root: Some("~/src".into()),
        ..current.clone()
    };
    let (next, report) = plan_reload(Some(&current), Some(&moved), HOME);
    assert_eq!(next.and_then(|c| c.root), Some("~/src".into()));
    assert_eq!(
        (report.added, report.removed),
        (vec!["a".into(), "b".into()], vec!["a".into(), "b".into()])
    );
}

#[test]
fn reload_defers_turning_rc_on_or_off() {
    let config = RcConfig {
        supervise: true,
        ..rc(&["a"], &[])
    };
    assert_eq!(
        plan_reload(None, Some(&config), HOME),
        (
            None,
            Reload {
                restart_needed: vec!["enabled"],
                ..Reload::default()
            }
        )
    );
    assert_eq!(
        plan_reload(Some(&config), None, HOME),
        (
            None,
            Reload {
                active: true,
                supervise: true,
                restart_needed: vec!["enabled"],
                ..Reload::default()
            }
        )
    );
    assert_eq!(plan_reload(None, None, HOME), (None, Reload::default()));
}

#[test]
fn reload_report_serializes_camel_case() {
    let report = Reload {
        restart_needed: vec!["supervise"],
        ..Reload::default()
    };
    assert_eq!(
        serde_json::to_value(&report).unwrap(),
        json!({
            "active": false, "supervise": false, "applied": false, "added": [], "removed": [],
            "pinned": [], "unpinned": [], "worktreeOn": [], "worktreeOff": [],
            "restartNeeded": ["supervise"]
        })
    );
    assert_eq!(
        serde_json::to_value(Change::Unchanged).unwrap(),
        json!("unchanged")
    );
}

#[test]
fn reload_changes_name_each_list_in_order() {
    let report = Reload {
        applied: true,
        unpinned: vec!["b".into(), "c".into()],
        added: vec!["a".into()],
        ..Reload::default()
    };
    assert_eq!(report.changes(), ["더함: a", "고정 풂: b, c"]);
    assert!(Reload::default().changes().is_empty());
}

#[test]
fn auto_reload_logs_list_changes_and_pending_switches_only() {
    let applied = Reload {
        applied: true,
        pinned: vec!["a".into()],
        ..Reload::default()
    };
    assert_eq!(
        auto_reload_line(&applied).as_deref(),
        Some("바뀐 rocky.json 의 rc 대상 목록을 다시 읽었다(고정: a)")
    );
    let both = Reload {
        restart_needed: vec!["supervise", "nightly"],
        ..applied
    };
    assert_eq!(
        auto_reload_line(&both).as_deref(),
        Some("바뀐 rocky.json 의 rc 대상 목록을 다시 읽었다(고정: a). rc 의 supervise · nightly 는 데몬을 다시 띄워야 반영된다")
    );
    // 다른 블록만 고쳤다.
    assert_eq!(auto_reload_line(&Reload::default()), None);
}
