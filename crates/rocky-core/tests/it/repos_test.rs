//! 레포 목록(`rocky_core::repos`) — `rocky.json` 의 `repos` 블록과 보드를 합친다.

use rocky_core::repos::{
    boards_missing_from_config, effective, from_board, load_repos_block, parse_repos_block,
    RepoEntry,
};
use rocky_core::types::Board;

fn board(id: &str, key: &str, repo: Option<&str>, path: Option<&str>) -> Board {
    Board {
        id: id.into(),
        key: key.into(),
        title: key.into(),
        description: None,
        repo: repo.map(str::to_string),
        path: path.map(str::to_string),
        previous_keys: None,
        review_fix: false,
        created_at: "2026-11-11T00:00:00Z".into(),
        archived_at: None,
        pr_authors: Vec::new(),
    }
}

fn parse(json: serde_json::Value) -> Vec<RepoEntry> {
    parse_repos_block(json.as_object().unwrap())
}

/// 칸마다 모양을 본다 — 틀린 칸은 버리고 항목은 살린다. 빈 key 와 객체가 아닌 값은 항목째 버린다. key 순서로 낸다.
#[test]
fn the_block_keeps_good_fields_and_drops_bad_ones() {
    let repos = parse(serde_json::json!({
        "rocky": {
            "repo": "minjun0219/rocky",
            "path": "/w/rocky",
            "prAuthors": ["@me", "bad login!", "logan-usp"],
            "reviewFix": true
        },
        "acorn": { "repo": "not a slug", "path": "  " },
        " ": { "repo": "o/r" },
        "broken": "o/r"
    }));
    assert_eq!(
        repos,
        vec![
            RepoEntry {
                key: "acorn".into(),
                ..Default::default()
            },
            RepoEntry {
                key: "rocky".into(),
                repo: Some("minjun0219/rocky".into()),
                path: Some("/w/rocky".into()),
                pr_authors: vec!["@me".into(), "logan-usp".into()],
                review_fix: true,
                board_id: None,
            },
        ]
    );
}

/// `~` 로 시작하는 path 는 홈으로 푼다.
#[test]
fn a_tilde_path_is_expanded() {
    let repos = parse(serde_json::json!({ "rocky": { "path": "~/dev/rocky" } }));
    let path = repos[0].path.clone().unwrap();
    assert!(!path.starts_with('~'), "{path}");
    assert!(path.ends_with("/dev/rocky"), "{path}");
}

/// 파일이 없거나 JSON 이 아니거나 블록이 없으면 빈 목록이다(fail-open).
#[test]
fn a_missing_or_broken_file_means_no_repos() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rocky.json");
    assert!(load_repos_block(&path).is_empty());
    std::fs::write(&path, "{ not json").unwrap();
    assert!(load_repos_block(&path).is_empty());
    std::fs::write(&path, r#"{ "pr": {} }"#).unwrap();
    assert!(load_repos_block(&path).is_empty());
    std::fs::write(&path, r#"{ "repos": { "rocky": { "repo": "o/r" } } }"#).unwrap();
    assert_eq!(load_repos_block(&path)[0].repo.as_deref(), Some("o/r"));
}

/// 같은 key 면 설정 항목이 통째로 이기고 보드 id 만 붙는다. 설정에 없는 보드는 보드 값 그대로, 보관된 보드는 빠진다.
#[test]
fn config_wins_by_key_and_other_boards_fill_in() {
    let config = parse(serde_json::json!({
        "rocky": { "repo": "o/new", "path": "/new", "reviewFix": true },
        "only-config": { "repo": "o/c" }
    }));
    let mut archived = board("b3", "old", Some("o/old"), None);
    archived.archived_at = Some("2026-11-01T00:00:00Z".into());
    let boards = vec![
        board("b1", "rocky", Some("o/stale"), Some("/stale")),
        board("b2", "acorn", Some("o/acorn"), Some("/acorn")),
        archived,
    ];
    let repos = effective(&config, &boards);
    let keys: Vec<&str> = repos.iter().map(|r| r.key.as_str()).collect();
    assert_eq!(keys, vec!["only-config", "rocky", "acorn"]);
    let rocky = &repos[1];
    assert_eq!(rocky.repo.as_deref(), Some("o/new"));
    assert_eq!(rocky.path.as_deref(), Some("/new"));
    assert!(rocky.review_fix);
    assert_eq!(rocky.board_id.as_deref(), Some("b1"));
    assert_eq!(repos[0].board_id, None);
    assert_eq!(repos[2], from_board(&boards[1]));
}

/// `rocky doctor` 가 알릴 "옮길 보드" — 레포나 path 가 붙었고 설정에 없고 보관되지 않은 보드만.
#[test]
fn boards_left_to_move_are_listed() {
    let config = parse(serde_json::json!({ "rocky": { "repo": "o/r" } }));
    let boards = vec![
        board("b1", "rocky", Some("o/r"), None),
        board("b2", "acorn", None, Some("/acorn")),
        board("b3", "notes", None, None),
    ];
    let left: Vec<&str> = boards_missing_from_config(&config, &boards)
        .into_iter()
        .map(|b| b.key.as_str())
        .collect();
    assert_eq!(left, vec!["acorn"]);
}
