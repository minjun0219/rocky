use rocky_core::rc_targets::{edit_rc_targets, RcEdit};
use serde_json::{json, Value};

const HOME: &str = "/home/u";

fn config(rc: Value) -> String {
    serde_json::to_string_pretty(&json!({ "todo": { "port": 8636 }, "rc": rc, "pr": {} })).unwrap()
}

fn lists(out: &str) -> (Value, Value) {
    let v: Value = serde_json::from_str(out).unwrap();
    (v["rc"]["pinned"].clone(), v["rc"]["targets"].clone())
}

#[test]
fn add_appends_and_keeps_the_rest_of_the_file() {
    let raw = config(json!({ "enabled": true, "pinned": [], "targets": ["rocky"] }));
    let out = edit_rc_targets(&raw, HOME, "ogpeek", RcEdit::Add { pin: false }).unwrap();
    assert_eq!(lists(&out.json), (json!([]), json!(["rocky", "ogpeek"])));
    assert_eq!(out.dir, "/home/u/dev/workspaces/ogpeek");
    // rc 밖의 키와 순서는 그대로.
    let keys: Vec<String> = serde_json::from_str::<Value>(&out.json)
        .unwrap()
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    assert_eq!(keys, vec!["todo", "rc", "pr"]);
    assert!(out.json.ends_with('\n'));
}

#[test]
fn add_pin_goes_to_pinned_and_paths_under_root_become_relative() {
    let raw = config(json!({ "pinned": [], "targets": [] }));
    let out = edit_rc_targets(
        &raw,
        HOME,
        "~/dev/workspaces/cabinet/",
        RcEdit::Add { pin: true },
    )
    .unwrap();
    assert_eq!(lists(&out.json), (json!(["cabinet"]), json!([])));
    // root 밖은 받은 그대로.
    let out = edit_rc_targets(&raw, HOME, "/srv/x", RcEdit::Add { pin: false }).unwrap();
    assert_eq!(lists(&out.json).1, json!(["/srv/x"]));
}

#[test]
fn add_refuses_what_is_already_there() {
    let raw = config(json!({ "pinned": ["cabinet"], "targets": ["rocky"] }));
    for (name, pin) in [("rocky", false), ("cabinet", false), ("cabinet", true)] {
        assert!(
            edit_rc_targets(&raw, HOME, name, RcEdit::Add { pin }).is_err(),
            "{name} {pin}"
        );
    }
    // 대상을 고정으로 넣는 것은 받는다(고정에 더한다).
    let out = edit_rc_targets(&raw, HOME, "rocky", RcEdit::Add { pin: true }).unwrap();
    assert_eq!(
        lists(&out.json),
        (json!(["cabinet", "rocky"]), json!(["rocky"]))
    );
}

#[test]
fn remove_by_label_drops_both_lists() {
    let raw = config(json!({
        "pinned": ["cabinet/notes"],
        "targets": ["rocky", "cabinet/notes"]
    }));
    let out = edit_rc_targets(&raw, HOME, "notes", RcEdit::Remove).unwrap();
    assert_eq!(lists(&out.json), (json!([]), json!(["rocky"])));
    assert!(
        out.message.contains("rocky rc stop notes"),
        "{}",
        out.message
    );
    assert!(edit_rc_targets(&raw, HOME, "nope", RcEdit::Remove).is_err());
}

#[test]
fn ambiguous_label_is_refused() {
    let raw = config(json!({ "pinned": [], "targets": ["a/web", "b/web"] }));
    let err = edit_rc_targets(&raw, HOME, "web", RcEdit::Remove).unwrap_err();
    assert!(err.contains("둘 이상"), "{err}");
    // 경로로 고르면 된다.
    let out = edit_rc_targets(&raw, HOME, "a/web", RcEdit::Remove).unwrap();
    assert_eq!(lists(&out.json).1, json!(["b/web"]));
}

#[test]
fn pin_and_unpin() {
    let raw = config(json!({ "pinned": ["only-pinned"], "targets": ["rocky"] }));
    let out = edit_rc_targets(&raw, HOME, "rocky", RcEdit::Pin).unwrap();
    assert_eq!(
        lists(&out.json),
        (json!(["only-pinned", "rocky"]), json!(["rocky"]))
    );
    assert!(edit_rc_targets(&raw, HOME, "only-pinned", RcEdit::Pin).is_err());
    // 고정에만 있던 것을 풀면 대상으로 남긴다.
    let out = edit_rc_targets(&raw, HOME, "only-pinned", RcEdit::Unpin).unwrap();
    assert_eq!(
        lists(&out.json),
        (json!([]), json!(["rocky", "only-pinned"]))
    );
    assert!(edit_rc_targets(&raw, HOME, "rocky", RcEdit::Unpin).is_err());
}

#[test]
fn refuses_without_rc_block_or_on_broken_json() {
    let no_rc = serde_json::to_string(&json!({ "todo": {} })).unwrap();
    let err = edit_rc_targets(&no_rc, HOME, "x", RcEdit::Add { pin: false }).unwrap_err();
    assert!(err.contains("rc 블록이 없다"), "{err}");
    assert!(edit_rc_targets("{", HOME, "x", RcEdit::Add { pin: false }).is_err());
    assert!(edit_rc_targets(&no_rc, HOME, "  ", RcEdit::Remove).is_err());
}

#[test]
fn custom_root_is_respected() {
    let raw = config(json!({ "root": "ws", "pinned": [], "targets": [] }));
    let out = edit_rc_targets(&raw, HOME, "/home/u/ws/app", RcEdit::Add { pin: false }).unwrap();
    assert_eq!(lists(&out.json).1, json!(["app"]));
    assert_eq!(out.dir, "/home/u/ws/app");
}

#[test]
fn label_outside_root_keeps_the_original_entry() {
    // 라벨로 고른 칸이 root 밖이면 라벨이 아니라 원래 경로를 적는다 — 라벨은 `<root>/<라벨>` 로 풀린다.
    let raw = config(json!({ "pinned": ["/srv/foo"], "targets": [] }));
    let out = edit_rc_targets(&raw, HOME, "foo", RcEdit::Unpin).unwrap();
    assert_eq!(lists(&out.json), (json!([]), json!(["/srv/foo"])));
    let raw = config(json!({ "pinned": [], "targets": ["/srv/foo"] }));
    let out = edit_rc_targets(&raw, HOME, "foo", RcEdit::Pin).unwrap();
    assert_eq!(lists(&out.json), (json!(["/srv/foo"]), json!(["/srv/foo"])));
    assert_eq!(out.dir, "/srv/foo");
}
