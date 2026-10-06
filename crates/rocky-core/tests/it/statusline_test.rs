//! TS 원본 `src/statusline.test.ts` 포팅.

use rocky_core::statusline::{
    board_key_for_cwd, render_statusline, truncate_title, BoardLocation, StatuslineData,
    StatuslineMine, DEFAULT_STATUSLINE_TEMPLATE, STATUSLINE_TITLE_MAX,
};

fn empty() -> StatuslineData {
    StatuslineData::default()
}

fn with_mine() -> StatuslineData {
    StatuslineData {
        mine: Some(StatuslineMine {
            r#ref: "rocky-todo-12".into(),
            title: "statusline API 추가".into(),
            comments: 0,
        }),
        doing: 1,
        ..StatuslineData::default()
    }
}

fn render(template: &str, data: &StatuslineData) -> String {
    render_statusline(template, data, STATUSLINE_TITLE_MAX)
}

#[test]
fn nothing_to_show_renders_empty() {
    assert_eq!(render(DEFAULT_STATUSLINE_TEMPLATE, &empty()), "");
}

#[test]
fn zero_counts_erase_their_group() {
    assert_eq!(
        render(
            "[⏺{doing}][ ✉{inbox}]",
            &StatuslineData {
                doing: 3,
                ..empty()
            }
        ),
        "⏺3"
    );
}

#[test]
fn leading_whitespace_from_erased_groups_is_trimmed() {
    assert_eq!(
        render(
            "[⏺ {mine.ref}][  ✉{inbox}]",
            &StatuslineData {
                inbox: 2,
                ..empty()
            }
        ),
        "✉2"
    );
}

#[test]
fn group_survives_when_any_placeholder_filled() {
    assert_eq!(
        render("[{mine.ref} {mine.title}]", &with_mine()),
        "rocky-todo-12 statusline API 추가"
    );
}

#[test]
fn decoration_only_group_always_stays() {
    assert_eq!(render("[보드]", &empty()), "보드");
}

#[test]
fn unknown_placeholder_stays_visible() {
    assert_eq!(render("[{mine.titel}]", &with_mine()), "{mine.titel}");
}

#[test]
fn unclosed_bracket_reverts_to_literal() {
    assert_eq!(
        render(
            "[⏺{doing}",
            &StatuslineData {
                doing: 2,
                ..empty()
            }
        ),
        "[⏺2"
    );
}

#[test]
fn esc_bracket_is_literal_not_group() {
    let yellow = "\u{001b}[33m";
    let reset = "\u{001b}[0m";
    assert_eq!(
        render(
            &format!("[{yellow}⏺{{doing}}{reset}]"),
            &StatuslineData {
                doing: 4,
                ..empty()
            }
        ),
        format!("{yellow}⏺4{reset}")
    );
}

#[test]
fn colored_group_disappears_with_escapes_when_empty() {
    let yellow = "\u{001b}[33m";
    let reset = "\u{001b}[0m";
    assert_eq!(
        render(
            &format!("[{yellow}⏺{{doing}}{reset}][ ✉{{inbox}}]"),
            &StatuslineData {
                inbox: 1,
                ..empty()
            }
        ),
        "✉1"
    );
}

#[test]
fn default_template_carries_mine_with_title() {
    assert_eq!(
        render(DEFAULT_STATUSLINE_TEMPLATE, &with_mine()),
        "⏺ rocky-todo-12 statusline API 추가"
    );
}

#[test]
fn comment_segment_appears_when_comments_arrive() {
    let mut data = with_mine();
    data.mine.as_mut().unwrap().comments = 3;
    assert_eq!(
        render(DEFAULT_STATUSLINE_TEMPLATE, &data),
        "⏺ rocky-todo-12 statusline API 추가 💬 3"
    );
}

#[test]
fn without_mine_all_mine_groups_disappear() {
    assert_eq!(
        render(
            DEFAULT_STATUSLINE_TEMPLATE,
            &StatuslineData {
                stale: 2,
                ..empty()
            }
        ),
        "⚠ 2"
    );
}

#[test]
fn truncate_short_title_unchanged() {
    assert_eq!(truncate_title("짧다", STATUSLINE_TITLE_MAX), "짧다");
}

#[test]
fn truncate_long_title_with_ellipsis() {
    let long = "가".repeat(40);
    assert_eq!(
        truncate_title(&long, STATUSLINE_TITLE_MAX),
        format!("{}…", "가".repeat(30))
    );
}

#[test]
fn truncate_cuts_at_code_points() {
    assert_eq!(truncate_title(&"🙂".repeat(5), 3), "🙂🙂🙂…");
}

// ── boardKeyForCwd ──

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

#[test]
fn due_and_collect_placeholders_render_and_vanish_when_zero() {
    let data = StatuslineData {
        due: 2,
        collect: 3,
        ..StatuslineData::default()
    };
    assert_eq!(render("[⏰{due}][ 📥{collect}]", &data), "⏰2 📥3");
    assert_eq!(render("[⏰{due}][ 📥{collect}]", &empty()), "");
    // 기본 템플릿에도 들어 있다.
    assert_eq!(render(DEFAULT_STATUSLINE_TEMPLATE, &data), "⏰ 2  📥 3");
}

/// 기본 템플릿 — 핸드오프는 🤝(받은편지함 메시지가 아니다), 구독한 PR 의 머지 후보 ✅ · 충돌 ⛔.
#[test]
fn default_template_shows_handoffs_and_my_pr_states() {
    let data = StatuslineData {
        inbox: 1,
        pr_ready: 2,
        pr_conflict: 1,
        ..empty()
    };
    assert_eq!(
        render(DEFAULT_STATUSLINE_TEMPLATE, &data),
        "🤝 1  ✅ 2  ⛔ 1"
    );
    assert_eq!(render("[✅{pr.ready}][⛔{pr.conflict}]", &empty()), "");
}

fn pr(
    repo: &str,
    number: i64,
    state: &str,
    ready: bool,
    merge_state: &str,
) -> rocky_core::prwatch::PrSnapshot {
    rocky_core::prwatch::PrSnapshot {
        repo: repo.into(),
        number,
        title: format!("PR {number}"),
        url: String::new(),
        state: state.into(),
        is_draft: false,
        base: "main".into(),
        head: "h".into(),
        merge_state: merge_state.into(),
        ci: rocky_core::prwatch::CiState::Pass,
        unhandled: 0,
        unhandled_ids: vec![],
        decision: 0,
        ready,
        updated_at: String::new(),
        author: None,
    }
}

fn sub(repo: &str, number: i64, session: Option<&str>) -> rocky_core::prwatch::PrSubscription {
    rocky_core::prwatch::PrSubscription {
        repo: repo.into(),
        number,
        session_id: session.map(str::to_string),
        created_at: String::new(),
        filter_id: None,
    }
}

/// 이 세션(별칭 포함)이 구독한 열린 PR 만 센다 — 충돌이면 머지 후보로 세지 않는다, 닫힌 PR·남의 구독·세션 없는 구독은 뺀다.
#[test]
fn session_pr_counts_only_my_open_prs() {
    use rocky_core::statusline::session_pr_counts;
    let prs = [
        pr("O/R", 1, "OPEN", true, "CLEAN"),
        pr("o/r", 2, "OPEN", false, "DIRTY"),
        pr("o/r", 3, "MERGED", true, "CLEAN"),
        pr("o/r", 4, "OPEN", true, "CLEAN"),
        pr("o/r", 5, "OPEN", true, "CLEAN"),
        pr("o/r", 6, "OPEN", false, "BLOCKED"),
    ];
    let subs = [
        sub("o/r", 1, Some("short")),
        sub("o/r", 2, Some("full-uuid")),
        sub("o/r", 3, Some("full-uuid")),
        sub("o/r", 4, Some("someone-else")),
        sub("o/r", 5, None),
        sub("o/r", 6, Some("full-uuid")),
    ];
    let aliases = ["full-uuid".to_string(), "short".to_string()];
    assert_eq!(session_pr_counts(&subs, &prs, &aliases), (1, 1));
    assert_eq!(
        session_pr_counts(&subs, &prs, &["nobody".to_string()]),
        (0, 0)
    );
}
