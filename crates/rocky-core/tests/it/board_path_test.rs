//! cwd → 보드 판정(`rocky_core::board_path`). TS 원본 `src/statusline.test.ts` 의 boardKeyForCwd 부분에서 옮겨 왔다.

use rocky_core::board_path::{board_key_for_cwd, BoardLocation};

fn boards() -> Vec<BoardLocation> {
    vec![
        BoardLocation {
            key: "rocky-todo".into(),
            path: Some("/Users/x/dev/rocky-todo".into()),
        },
        BoardLocation {
            key: "rocky".into(),
            path: None,
        },
        BoardLocation {
            key: "ogpeek".into(),
            path: Some("/Users/x/dev/ogpeek".into()),
        },
    ]
}

#[test]
fn no_cwd_picks_nothing() {
    assert_eq!(board_key_for_cwd(&boards(), None), None);
}

#[test]
fn under_board_path_wins() {
    assert_eq!(
        board_key_for_cwd(&boards(), Some("/Users/x/dev/rocky-todo")).as_deref(),
        Some("rocky-todo")
    );
}

#[test]
fn path_comparison_respects_boundaries() {
    let og = vec![BoardLocation {
        key: "og".into(),
        path: Some("/Users/x/dev/og".into()),
    }];
    assert_eq!(board_key_for_cwd(&og, Some("/Users/x/dev/ogpeek")), None);
}

#[test]
fn trailing_slash_is_normalized() {
    let slashed = vec![BoardLocation {
        key: "proj".into(),
        path: Some("/Users/x/dev/proj/".into()),
    }];
    assert_eq!(
        board_key_for_cwd(&slashed, Some("/Users/x/dev/proj")).as_deref(),
        Some("proj")
    );
    assert_eq!(
        board_key_for_cwd(&slashed, Some("/Users/x/dev/proj/sub")).as_deref(),
        Some("proj")
    );
}

#[test]
fn trailing_slash_does_not_skew_length_comparison() {
    let both = vec![
        BoardLocation {
            key: "outer".into(),
            path: Some("/Users/x/dev/".into()),
        },
        BoardLocation {
            key: "inner".into(),
            path: Some("/Users/x/dev/proj".into()),
        },
    ];
    assert_eq!(
        board_key_for_cwd(&both, Some("/Users/x/dev/proj/sub")).as_deref(),
        Some("inner")
    );
}

#[test]
fn key_as_path_segment_matches_without_path() {
    assert_eq!(
        board_key_for_cwd(&boards(), Some("/Users/x/orca/rocky/eelpout")).as_deref(),
        Some("rocky")
    );
}

#[test]
fn worktree_matches_by_segment_not_basename() {
    let cwd = "/Users/x/dev/rocky-todo/.claude/worktrees/todo-12";
    assert_eq!(
        board_key_for_cwd(&boards(), Some(cwd)).as_deref(),
        Some("rocky-todo")
    );
}

#[test]
fn multiple_segment_candidates_pick_longest_key() {
    let bs = vec![
        BoardLocation {
            key: "rocky".into(),
            path: None,
        },
        BoardLocation {
            key: "rocky-todo".into(),
            path: None,
        },
    ];
    assert_eq!(
        board_key_for_cwd(&bs, Some("/Users/x/rocky/rocky-todo/sub")).as_deref(),
        Some("rocky-todo")
    );
}
