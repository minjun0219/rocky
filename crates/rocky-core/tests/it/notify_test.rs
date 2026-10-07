//! TS `src/notify.test.ts` 포팅.

use rocky_core::notify::{
    build_notify_context, build_pr_context, drop_absorbed, filter_changes_for_antigravity,
    filter_human_changes, hold_cursor, merge_context, page_cursor, peer_messages,
    pr_channel_events, pr_entries_for_board, pr_entries_for_session, read_cursor,
    subscribed_pr_entries, write_cursor, BoardLookup, PeerMessage,
};
use rocky_core::peer_inbox::pr_session_message;
use rocky_core::prwatch::{PrEvent, PrEventKind, PrSubscription};
use rocky_core::types::{ChangeFeedEntry, Changes, HistoryEntity, HistoryEntry};
use serde_json::json;

fn entry(id: i64, actor: &str, action: &str) -> ChangeFeedEntry {
    ChangeFeedEntry {
        history: HistoryEntry {
            id,
            entity: HistoryEntity::Todo,
            entity_id: "abcd1234".into(),
            actor: actor.into(),
            action: action.into(),
            changes: None,
            at: "2026-07-23T10:00:00.000Z".into(),
        },
        title: "제목".into(),
        board_key: Some("rocky".into()),
    }
}

fn with_changes(mut e: ChangeFeedEntry, changes: serde_json::Value) -> ChangeFeedEntry {
    let map: Changes = changes.as_object().cloned().unwrap_or_default();
    e.history.changes = Some(map);
    e
}

// ── filterHumanChanges ──────────────────────────────────────────────────────

#[test]
fn drops_agent_actors_keeps_human_actors() {
    let entries = vec![
        entry(1, "claude-code", "update"),
        entry(2, "logan", "update"),
        entry(3, "codex", "update"),
        entry(4, "web", "update"),
    ];
    let kept: Vec<i64> = filter_human_changes(entries)
        .iter()
        .map(|e| e.history.id)
        .collect();
    assert_eq!(kept, vec![2, 4]);
}

/// agy 대화에는 agy 자신(`antigravity`)의 변경을 되돌려 보내지 않는다 — 에이전트도 그대로 빠지고 사람은 남는다.
#[test]
fn antigravity_does_not_hear_its_own_changes() {
    let entries = vec![
        entry(1, "antigravity", "start"),
        entry(2, "logan", "comment"),
        entry(3, "claude-code", "update"),
        entry(4, "web", "update"),
    ];
    let kept: Vec<i64> = filter_changes_for_antigravity(entries)
        .iter()
        .map(|e| e.history.id)
        .collect();
    assert_eq!(kept, vec![2, 4]);
}

// ── buildNotifyContext ──────────────────────────────────────────────────────

#[test]
fn none_when_no_entries() {
    assert!(build_notify_context(&[]).is_none());
}

#[test]
fn formats_compact_korean_lines_with_board_action_and_diff() {
    let mut note = entry(2, "logan", "create");
    note.history.entity = HistoryEntity::Note;
    note.title = "메모".into();
    note.board_key = None;
    let mut done = entry(3, "logan", "done");
    done.title = "끝난 일".into();

    let context = build_notify_context(&[
        with_changes(entry(1, "logan", "update"), json!({ "title": ["a", "b"] })),
        note,
        done,
    ])
    .expect("컨텍스트가 있어야 한다");
    for needle in [
        "rocky",
        "[rocky]",
        "logan",
        "제목",
        "title: a → b",
        "메모",
        "완료",
    ] {
        assert!(context.contains(needle), "{needle} 이 없다:\n{context}");
    }
}

// ── 댓글 렌더 ───────────────────────────────────────────────────────────────

#[test]
fn renders_a_comment_with_its_body_instead_of_a_field_diff() {
    let mut e = with_changes(
        entry(1, "logan", "comment"),
        json!({ "comment": [null, "이거 SSE 로도 흘러가나?"] }),
    );
    e.title = "댓글 기능 추가".into();
    let context = build_notify_context(&[e]).unwrap();
    assert!(
        context.contains("\"댓글 기능 추가\" 댓글 · \"이거 SSE 로도 흘러가나?\""),
        "{context}"
    );
    assert!(!context.contains("comment:"), "{context}");
}

#[test]
fn renders_an_edited_comment_with_the_new_body() {
    let e = with_changes(
        entry(1, "logan", "comment-edit"),
        json!({ "comment": ["오타", "고침"] }),
    );
    let context = build_notify_context(&[e]).unwrap();
    assert!(context.contains("댓글 수정 · \"고침\""), "{context}");
}

#[test]
fn folds_newlines_and_truncates_a_long_body() {
    let body = format!("{}\n둘째 줄", "가".repeat(250));
    let e = with_changes(
        entry(1, "logan", "comment"),
        json!({ "comment": [null, body] }),
    );
    let context = build_notify_context(&[e]).unwrap();
    assert!(context.contains('…'), "{context}");
    assert!(!context.contains("\n둘째 줄"), "{context}");
    let line = context.lines().find(|l| l.contains("댓글")).unwrap_or("");
    assert!(line.chars().count() < 300, "{}", line.chars().count());
}

#[test]
fn agent_comments_are_filtered_out_before_formatting() {
    let entries = vec![
        with_changes(
            entry(1, "claude-code", "comment"),
            json!({ "comment": [null, "봇"] }),
        ),
        with_changes(
            entry(2, "logan", "comment"),
            json!({ "comment": [null, "사람"] }),
        ),
    ];
    let context = build_notify_context(&filter_human_changes(entries)).unwrap();
    assert!(context.contains("\"사람\""), "{context}");
    assert!(!context.contains("\"봇\""), "{context}");
}

// ── 커서 저장 ───────────────────────────────────────────────────────────────

#[test]
fn read_missing_is_none_and_write_then_read_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("hook-cursors.json");
    assert!(read_cursor(&file, "sess-1").is_none());
    write_cursor(&file, "sess-1", 42);
    assert_eq!(read_cursor(&file, "sess-1"), Some(42));
    write_cursor(&file, "sess-1", 50);
    assert_eq!(read_cursor(&file, "sess-1"), Some(50));
    assert!(read_cursor(&file, "sess-2").is_none());
}

/// 개수만 보면 "어느 100개가 남았는지"를 놓친다 — `at` 이 밀리초라 세션들이 같은 값을
/// 갖기 쉽고, 그때 잘려나가는 구간이 오래된 쪽이 아니라 임의의 밴드가 되던 버그가
/// 있었다. 남은 집합이 정확히 최신 100개인지까지 못 박는다.
#[test]
fn prunes_to_the_most_recent_100_sessions() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("hook-cursors.json");
    for i in 0..120 {
        write_cursor(&file, &format!("sess-{i}"), i);
    }
    let raw: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    let keys = raw.as_object().unwrap();
    assert!(keys.len() <= 100, "{}", keys.len());
    assert_eq!(read_cursor(&file, "sess-119"), Some(119));
    assert!(read_cursor(&file, "sess-0").is_none());

    let survivors: Vec<i64> = (0..120)
        .filter(|i| keys.contains_key(&format!("sess-{i}")))
        .collect();
    let expected: Vec<i64> = (20..120).collect();
    assert_eq!(survivors, expected);
}

#[test]
fn corrupt_cursor_file_is_treated_as_empty() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("hook-cursors.json");
    write_cursor(&file, "a", 1);
    std::fs::write(&file, "{broken").unwrap();
    assert!(read_cursor(&file, "a").is_none());
    write_cursor(&file, "a", 2);
    assert_eq!(read_cursor(&file, "a"), Some(2));
}

// ── mergeContext ────────────────────────────────────────────────────────────

#[test]
fn both_present_are_joined_with_a_blank_line() {
    assert_eq!(
        merge_context(&[Some("A".into()), Some("B".into())]).as_deref(),
        Some("A\n\nB")
    );
}

#[test]
fn a_single_part_is_returned_alone() {
    assert_eq!(
        merge_context(&[None, Some("B".into())]).as_deref(),
        Some("B")
    );
    assert_eq!(
        merge_context(&[Some("A".into()), None]).as_deref(),
        Some("A")
    );
}

#[test]
fn nothing_to_inject_is_none() {
    assert!(merge_context(&[None, None]).is_none());
}

#[test]
fn empty_strings_count_as_absent() {
    assert_eq!(
        merge_context(&[Some(String::new()), Some("B".into())]).as_deref(),
        Some("B")
    );
}

#[test]
fn pr_context_lists_only_actionable_transitions() {
    use rocky_core::types::{ChangeFeedEntry, HistoryEntity, HistoryEntry};
    let entry = |action: &str, number: i64| {
        ChangeFeedEntry {
        history: HistoryEntry {
            id: number,
            entity: HistoryEntity::Board,
            entity_id: "b1".into(),
            actor: "rocky".into(),
            action: action.into(),
            changes: Some(
                serde_json::json!({ "number": number, "title": format!("PR {number}"), "url": format!("https://x/pull/{number}"), "repo": "o/r" })
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
            at: "2026-09-28T10:00:00Z".into(),
        },
        title: "rocky".into(),
        board_key: Some("rocky".into()),
    }
    };
    assert!(build_pr_context(&[]).is_none());
    assert!(build_pr_context(&[
        entry("pr-opened", 1),
        entry("pr-unready", 2),
        entry("pr-merged", 7),
        entry("pr-closed", 8)
    ])
    .is_none());
    let text = build_pr_context(&[
        entry("pr-ready", 3),
        entry("pr-conflict", 4),
        entry("pr-merged", 5),
        entry("update", 6),
    ])
    .unwrap();
    assert!(text.starts_with("# rocky: PR 상태 변화"));
    assert!(text.contains("- o/r #3 머지 후보 — PR 3 (https://x/pull/3)"));
    assert!(text.contains("#4 충돌"));
    assert!(
        !text.contains("#5"),
        "머지·닫힘은 히스토리에만 — 세션에 넣지 않는다"
    );
    assert!(!text.contains("#6"));
    assert!(text.contains("감시를 따로 돌리지 말고"));

    // 채널용 — 같은 규칙(ready·conflict 만), 모양은 <channel> 태그(content + meta 속성).
    let events = pr_channel_events(&[
        entry("pr-ready", 3),
        entry("pr-conflict", 4),
        entry("pr-merged", 5),
        entry("update", 6),
    ]);
    assert_eq!(events.len(), 2);
    assert_eq!(
        events[0].content,
        "o/r #3 머지 후보 — PR 3\nhttps://x/pull/3"
    );
    assert_eq!(events[0].meta["kind"], "ready");
    assert_eq!(events[0].meta["repo"], "o/r");
    assert_eq!(events[0].meta["number"], "3");
    assert_eq!(events[0].meta["url"], "https://x/pull/3");
    assert_eq!(events[1].meta["kind"], "conflict");
    assert!(
        events.iter().all(|e| e
            .meta
            .keys()
            .all(|k| k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))),
        "meta 키는 식별자만 — 하이픈이면 Claude Code 가 버린다"
    );
    assert!(pr_channel_events(&[entry("pr-merged", 7)]).is_empty());
}

/// 페이지가 꽉 찼으면 받은 마지막 id 까지만 전진하고 "더 있음" — 전역 last_id 로 뛰면 그 사이를 잃는다.
#[test]
fn page_cursor_advances_through_full_pages_only() {
    use rocky_core::types::{ChangesSince, HistoryEntity, HistoryEntry};
    let entry = |id: i64| ChangeFeedEntry {
        history: HistoryEntry {
            id,
            entity: HistoryEntity::Todo,
            entity_id: "x".into(),
            actor: "rocky".into(),
            action: "update".into(),
            changes: None,
            at: "2026-09-28T10:00:00Z".into(),
        },
        title: "t".into(),
        board_key: None,
    };
    let full = ChangesSince {
        last_id: 250,
        entries: (1..=100).map(entry).collect(),
    };
    assert_eq!(page_cursor(&full, 100), (100, true));
    let partial = ChangesSince {
        last_id: 250,
        entries: (101..=140).map(entry).collect(),
    };
    assert_eq!(page_cursor(&partial, 100), (250, false));
    let exact_end = ChangesSince {
        last_id: 100,
        entries: (1..=100).map(entry).collect(),
    };
    assert_eq!(
        page_cursor(&exact_end, 100),
        (100, false),
        "꽉 찼지만 끝까지 왔다"
    );
    let empty = ChangesSince {
        last_id: 7,
        entries: vec![],
    };
    assert_eq!(page_cursor(&empty, 100), (7, false));
}

/// 사고 회귀(2026-09-29) — rocky 의 "머지 가능" 이 tally 세션에 주입됐다. PR 전이는 그 세션의 보드
/// 것만, 보드를 모르는 세션에는 아무것도.
#[test]
fn pr_transitions_are_scoped_to_the_sessions_board() {
    let pr = |board: &str, number: i64| {
        let mut e = with_changes(
            entry(number, "rocky", "pr-ready"),
            serde_json::json!({ "number": number, "title": "t", "url": "u", "repo": "o/r" }),
        );
        e.history.entity = HistoryEntity::Board;
        e.board_key = Some(board.into());
        e
    };
    let entries = vec![pr("rocky", 1), pr("tally", 2), entry(3, "logan", "update")];
    let rocky: Vec<i64> = pr_entries_for_board(&entries, Some("rocky"))
        .iter()
        .map(|e| e.history.id)
        .collect();
    assert_eq!(
        rocky,
        vec![1],
        "rocky 세션엔 rocky PR 만 — 보드 변경(update)은 여기서 다루지 않는다"
    );
    let tally: Vec<i64> = pr_entries_for_board(&entries, Some("tally"))
        .iter()
        .map(|e| e.history.id)
        .collect();
    assert_eq!(tally, vec![2]);
    assert!(
        pr_entries_for_board(&entries, None).is_empty(),
        "보드를 모르는 세션엔 아무것도"
    );
    assert!(build_pr_context(&pr_entries_for_board(&entries, Some("blip-a"))).is_none());
}

/// 사고 회귀(2026-09-29 #208) — 한동안 조용했던 세션은 "머지 가능" 과 그 뒤의 "머지됨" 을 한꺼번에
/// 받는다. PR 마다 마지막 전이로 판단해야 이미 머지된 PR 을 머지하라고 하지 않는다.
#[test]
fn a_ready_followed_by_merged_in_the_same_batch_is_not_announced() {
    let pr = |id: i64, action: &str, number: i64| {
        let mut e = with_changes(
            entry(id, "rocky", action),
            serde_json::json!({ "number": number, "title": "t", "url": "u", "repo": "o/r" }),
        );
        e.history.entity = HistoryEntity::Board;
        e
    };
    let batch = vec![
        pr(1, "pr-ready", 208),
        pr(2, "pr-opened", 209),
        pr(3, "pr-merged", 208),
        pr(4, "pr-ready", 210),
    ];
    let text = build_pr_context(&batch).unwrap();
    assert!(!text.contains("#208"), "머지된 PR 의 옛 머지 가능은 빠진다");
    assert!(text.contains("#210"));
    // 머지 가능 → 다시 대기면 알리지 않는다(마지막이 unready).
    let back = vec![pr(1, "pr-ready", 7), pr(2, "pr-unready", 7)];
    assert!(build_pr_context(&back).is_none());
    // 충돌 → 머지 가능이면 마지막(머지 가능)만.
    let fixed = vec![pr(1, "pr-conflict", 8), pr(2, "pr-ready", 8)];
    let text = build_pr_context(&fixed).unwrap();
    assert!(text.contains("#8 머지 후보") && !text.contains("충돌"));
}

/// 보드 조회 실패는 "보드 없음" 과 다르다 — 실패면 커서를 넘기지 않아 다음 턴에 같은 PR 전이를
/// 다시 받는다. 매칭 없음·PR 전이 없음이면 평소처럼 넘긴다.
#[test]
fn a_failed_board_lookup_holds_the_cursor_only_when_pr_transitions_are_waiting() {
    let mut pr = entry(1, "rocky", "pr-ready");
    pr.history.entity = HistoryEntity::Board;
    pr.board_key = Some("rocky".into());
    let with_pr = vec![pr, entry(2, "logan", "update")];
    let without_pr = vec![entry(3, "logan", "update")];

    assert!(hold_cursor(&with_pr, &BoardLookup::Failed, false));
    assert!(!hold_cursor(&with_pr, &BoardLookup::Unmatched, false));
    assert!(!hold_cursor(
        &with_pr,
        &BoardLookup::Found("rocky".into()),
        false
    ));
    assert!(!hold_cursor(&without_pr, &BoardLookup::Failed, false));
    // PR 구독 조회 실패도 같다 — 구독을 모르면 이 세션 것인지 가를 수 없다.
    assert!(hold_cursor(
        &with_pr,
        &BoardLookup::Found("rocky".into()),
        true
    ));
    assert!(!hold_cursor(&without_pr, &BoardLookup::Unmatched, true));
    assert_eq!(BoardLookup::Found("rocky".into()).key(), Some("rocky"));
    assert_eq!(BoardLookup::Failed.key(), None);
}

/// 작성자 필터에 걸린 전이(`quiet`)는 기록에는 있지만 세션 훅 주입·채널이 건너뛴다.
#[test]
fn quiet_pr_transitions_are_not_injected() {
    let pr = |id: i64, number: i64, quiet: bool| {
        let mut changes =
            serde_json::json!({ "number": number, "title": "t", "url": "u", "repo": "o/r" });
        if quiet {
            changes["quiet"] = serde_json::json!(true);
        }
        let mut e = with_changes(entry(id, "rocky", "pr-ready"), changes);
        e.history.entity = HistoryEntity::Board;
        e
    };
    let text = build_pr_context(&[pr(1, 11, false), pr(2, 12, true)]).unwrap();
    assert!(text.contains("#11") && !text.contains("#12"));
    assert!(build_pr_context(&[pr(3, 13, true)]).is_none());
}

fn pr_at(id: i64, action: &str, repo: &str, number: i64, at: &str) -> ChangeFeedEntry {
    let mut e = with_changes(
        entry(id, "rocky", action),
        serde_json::json!({ "number": number, "title": "t", "url": format!("https://github.com/{repo}/pull/{number}"), "repo": repo }),
    );
    e.history.entity = HistoryEntity::Board;
    e.history.at = at.into();
    e
}

fn sub(repo: &str, number: i64, session: Option<&str>) -> PrSubscription {
    PrSubscription {
        repo: repo.into(),
        number,
        session_id: session.map(str::to_string),
        created_at: "2026-10-05T00:00:00Z".into(),
        filter_id: None,
    }
}

/// 사고 회귀(2026-10-05) — 같은 보드의 모든 세션이 남의 PR(#310·#316 …) 머지 후보를 받았다. 훅 주입은
/// 받은편지함처럼 **그 PR 을 구독한 세션**에만 간다.
#[test]
fn pr_transitions_reach_only_the_subscribing_session() {
    let at = "2026-10-05T10:00:00.000Z";
    let entries = vec![
        pr_at(1, "pr-ready", "o/r", 310, at),
        pr_at(2, "pr-conflict", "o/r", 316, at),
        pr_at(3, "pr-ready", "O/R", 320, at),
        entry(4, "logan", "update"),
    ];
    let subs = vec![
        sub("o/r", 316, Some("mine")),
        sub("o/r", 320, Some("mine")),
        sub("o/r", 310, Some("other")),
        sub("o/r", 999, None),
    ];
    let ids: Vec<i64> = subscribed_pr_entries(&entries, "mine", &subs)
        .iter()
        .map(|e| e.history.id)
        .collect();
    assert_eq!(
        ids,
        vec![2, 3],
        "구독한 #316·#320 만 — 레포 대소문자는 무시, 보드 변경은 다루지 않는다"
    );
    let text = build_pr_context(&subscribed_pr_entries(&entries, "mine", &subs)).unwrap();
    assert!(!text.contains("#310"), "남이 구독한 PR 은 빠진다");
    assert!(
        subscribed_pr_entries(&entries, "nobody", &subs).is_empty(),
        "구독이 없는 세션엔 아무것도"
    );
    // 채널도 같은 거름을 쓴다.
    let events = pr_channel_events(&subscribed_pr_entries(&entries, "other", &subs));
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].meta["number"], "310");
}

fn peer(text: &str, at: &str) -> PeerMessage {
    PeerMessage {
        at: at.into(),
        text: text.into(),
    }
}

/// 받은편지함 본문(데몬이 소켓에 쓰는 것)을 세션이 받아들인 모양 — 쉬던 세션은 `<cross-session-message>` 로 감싼다.
fn inbox_text(kind: PrEventKind, number: i64) -> String {
    let body = pr_session_message(&PrEvent {
        kind,
        repo: "o/r".into(),
        number,
        title: "t".into(),
        url: format!("https://github.com/o/r/pull/{number}"),
        author: None,
        quiet: false,
    })
    .unwrap();
    format!("<cross-session-message from=\"unknown\">\n{body}\n</cross-session-message>")
}

/// 세션이 받은편지함 메시지를 **받아들였으면**(트랜스크립트에 peer 기록) 훅이 또 넣지 않는다. 그 전이보다 앞선
/// 메시지·다른 PR·다른 전이는 빼지 않는다 — 받은편지함 본문의 머리가 바뀌면 이 테스트가 깨진다.
#[test]
fn transitions_the_session_already_absorbed_from_its_inbox_are_not_injected_again() {
    let entries = vec![
        pr_at(1, "pr-ready", "o/r", 1, "2026-10-05T10:00:00.400Z"),
        pr_at(2, "pr-ready", "o/r", 2, "2026-10-05T10:00:00.000Z"),
        pr_at(3, "pr-ready", "o/r", 3, "2026-10-05T10:05:00.000Z"),
        pr_at(4, "pr-conflict", "o/r", 4, "2026-10-05T10:00:00.000Z"),
        pr_at(5, "pr-conflict", "o/r", 5, "2026-10-05T10:00:00.000Z"),
    ];
    let absorbed = vec![
        // 같은 초에 받아들임 — 받은 것(전이 시각의 밀리초는 버린다).
        peer(
            &inbox_text(PrEventKind::Ready, 1),
            "2026-10-05T10:00:00.900Z",
        ),
        // 옛 머지 후보 — 지금 전이(10:05)보다 앞선다.
        peer(
            &inbox_text(PrEventKind::Ready, 3),
            "2026-10-05T10:01:00.000Z",
        ),
        // 같은 PR 이지만 전이가 다르다.
        peer(
            &inbox_text(PrEventKind::Ready, 4),
            "2026-10-05T10:01:00.000Z",
        ),
        peer(
            &inbox_text(PrEventKind::Conflict, 5),
            "2026-10-05T10:01:00.000Z",
        ),
    ];
    let ids: Vec<i64> = drop_absorbed(&entries, &absorbed)
        .iter()
        .map(|e| e.history.id)
        .collect();
    assert_eq!(ids, vec![2, 3, 4], "#1·#5 만 받아들였다");
}

/// 빼기는 PR 마다 마지막 전이를 고른 **뒤**에 한다 — 먼저 빼면 받아들인 "충돌" 앞의 옛 "머지 후보" 가 살아나 주입된다.
#[test]
fn dropping_an_absorbed_latest_transition_does_not_resurrect_an_older_one() {
    let entries = vec![
        pr_at(1, "pr-ready", "o/r", 7, "2026-10-05T10:00:00.000Z"),
        pr_at(2, "pr-conflict", "o/r", 7, "2026-10-05T10:01:00.000Z"),
    ];
    let absorbed = vec![peer(
        &inbox_text(PrEventKind::Conflict, 7),
        "2026-10-05T10:01:00.000Z",
    )];
    assert!(build_pr_context(&drop_absorbed(&entries, &absorbed)).is_none());
}

/// 트랜스크립트에서 세션이 **받아들인** peer 메시지만 — 쉬던 세션(`user`)·턴 중(`queued_command` 첨부) 둘 다.
/// 큐에만 들어간 것·훅 주입·사람 프롬프트·깨진 줄은 세지 않는다(Claude Code 2.1.288 실측 모양).
#[test]
fn peer_messages_reads_only_absorbed_peer_records_from_a_transcript() {
    let lines = [
        r#"{"type":"queue-operation","operation":"enqueue","timestamp":"2026-10-05T10:00:00.000Z","content":"o/r #1 머지 후보 — 큐에만"}"#,
        r#"{"type":"user","isMeta":true,"origin":{"kind":"peer","from":"unknown"},"timestamp":"2026-10-05T10:00:01.000Z","message":{"role":"user","content":"o/r #2 머지 후보 — 쉬던 세션"}}"#,
        r#"{"type":"attachment","timestamp":"2026-10-05T10:00:02.000Z","attachment":{"type":"queued_command","prompt":"o/r #3 머지 후보 — 턴 중","origin":{"kind":"peer","from":"unknown"},"isMeta":true}}"#,
        r#"{"type":"attachment","timestamp":"2026-10-05T10:00:03.000Z","attachment":{"type":"hook_additional_context","content":["o/r #4 머지 후보 — 훅 주입 \"peer\""]}}"#,
        r#"{"type":"user","timestamp":"2026-10-05T10:00:04.000Z","message":{"role":"user","content":"사람이 쓴 \"peer\" 프롬프트"}}"#,
        r#"{"type":"user","origin":{"kind":"peer"} 깨진 줄"#,
        r#"{"type":"user","origin":{"kind":"peer"},"timestamp":"2026-10-05T10:00:05.000Z","message":{"role":"user","content":[{"type":"text","text":"o/r #5 충돌"}]}}"#,
    ];
    let got = peer_messages(&lines.join("\n"));
    let texts: Vec<&str> = got.iter().map(|m| m.text.as_str()).collect();
    assert_eq!(texts.len(), 3, "{texts:?}");
    assert!(texts[0].contains("#2") && texts[1].contains("#3") && texts[2].contains("#5"));
    assert_eq!(got[1].at, "2026-10-05T10:00:02.000Z");
}

/// 보드가 풀리면 그 보드 것 중 구독한 PR, 안 풀리면(보드 경로 밖 워크트리) 구독한 PR 만으로 — 받은편지함처럼
/// 보드를 몰라도 구독한 세션은 받는다.
#[test]
fn a_session_outside_any_board_still_gets_its_subscribed_prs() {
    let at = "2026-10-05T10:00:00.000Z";
    let on = |board: &str, id: i64, number: i64| {
        let mut e = pr_at(id, "pr-ready", "o/r", number, at);
        e.board_key = Some(board.into());
        e
    };
    let entries = vec![on("rocky", 1, 10), on("tally", 2, 11), on("rocky", 3, 12)];
    let subs = vec![sub("o/r", 10, Some("mine")), sub("o/r", 11, Some("mine"))];
    let ids = |v: Vec<ChangeFeedEntry>| v.iter().map(|e| e.history.id).collect::<Vec<_>>();
    assert_eq!(
        ids(pr_entries_for_session(
            &entries,
            Some("rocky"),
            "mine",
            &subs
        )),
        vec![1],
        "보드가 풀리면 그 보드의 구독한 PR 만"
    );
    assert_eq!(
        ids(pr_entries_for_session(&entries, None, "mine", &subs)),
        vec![1, 2],
        "보드를 모르면 구독한 PR 전부 — 구독 안 한 #12 는 빠진다"
    );
}
