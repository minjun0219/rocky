//! 보드 요약 — 마감 판정, 수집함 미올림 집계, 조립·렌더.

use std::collections::HashSet;

use rocky_core::inbox::{mark_promoted, InboxItem, InboxResponse, InboxSourceResult};
use rocky_core::refs::TodoView;
use rocky_core::summary::{
    build_summary, count_unpromoted, due_bucket, one_line, render_summary, DueBucket, SummaryKind,
    SUMMARY_INBOX_MAX, SUMMARY_ITEM_MAX,
};
use rocky_core::types::{Todo, TodoPriority, TodoStatus};

fn todo(n: i64, title: &str, status: TodoStatus, due: Option<&str>) -> TodoView {
    TodoView {
        todo: Todo {
            id: format!("id{n}"),
            number: n,
            board_id: "b".into(),
            section_id: None,
            parent_id: None,
            title: title.into(),
            description: String::new(),
            status,
            priority: TodoPriority::P2,
            due: due.map(str::to_string),
            labels: vec![],
            links: vec![],
            doing_by: None,
            doing_since: None,
            doing_session_id: None,
            doing_session_claimed: false,
            position: n,
            created_at: String::new(),
            updated_at: String::new(),
            completed_at: None,
            archived_at: None,
        },
        r#ref: format!("rocky-{n}"),
        comment_count: 0,
        last_comment_at: None,
        doing_state: None,
    }
}

#[test]
fn due_bucket_compares_dates_and_ignores_future_or_bad_input() {
    assert_eq!(
        due_bucket("2026-09-26", "2026-09-27"),
        Some(DueBucket::Overdue)
    );
    assert_eq!(
        due_bucket("2026-09-27", "2026-09-27"),
        Some(DueBucket::Today)
    );
    assert_eq!(due_bucket("2026-09-28", "2026-09-27"), None);
    // datetime 이어도 날짜 10자만 본다.
    assert_eq!(
        due_bucket("2026-09-27T09:00:00", "2026-09-27"),
        Some(DueBucket::Today)
    );
    assert_eq!(due_bucket("bad", "2026-09-27"), None);
    assert_eq!(due_bucket("2026-09-27", ""), None);
}

fn item(id: &str, title: &str, url: Option<&str>) -> InboxItem {
    InboxItem {
        id: id.into(),
        title: title.into(),
        url: url.map(str::to_string),
        note: None,
        due: None,
        created_at: None,
        promoted: false,
    }
}

fn source(name: &str, items: Vec<InboxItem>) -> InboxSourceResult {
    InboxSourceResult {
        name: name.into(),
        board: None,
        available: true,
        reason: None,
        fetched_at: "t".into(),
        items,
    }
}

#[test]
fn unpromoted_counts_items_not_linked_from_any_board() {
    let mut inbox = InboxResponse {
        sources: vec![
            source(
                "s",
                vec![
                    item("a", "올라감", Some("https://x/a")),
                    item("b", "안 올라감", Some("https://x/b")),
                    item("c", "url 없음", None),
                ],
            ),
            // 실패한 소스는 세지 않는다.
            InboxSourceResult {
                name: "dead".into(),
                board: None,
                available: false,
                reason: Some("exit 1".into()),
                fetched_at: "t".into(),
                items: vec![],
            },
        ],
    };
    // 판정 근거는 전 보드(보관 포함)의 링크 — 데몬이 `Store::linked_urls` 로 모은다.
    let linked: HashSet<String> = ["https://x/a".to_string()].into();
    mark_promoted(&mut inbox, &linked);
    assert!(inbox.sources[0].items[0].promoted);
    assert!(
        !inbox.sources[0].items[2].promoted,
        "url 없는 항목은 판정 불가 — 미올림"
    );
    assert_eq!(count_unpromoted(&inbox), 2);
}

#[test]
fn summary_lists_unpromoted_inbox_titles_under_board_items_with_overflow() {
    let mut inbox = InboxResponse {
        sources: vec![source(
            "gh-bugs",
            vec![
                item("a", "올라간 것", Some("https://x/a")),
                item("b", "그래프 색상이 디자인과 다름", Some("https://x/b")),
                item("c", "캘린더 제한이\n동작하지 않음", Some("https://x/c")),
                item("d", "셋째", None),
                item("e", "넷째", None),
            ],
        )],
    };
    mark_promoted(&mut inbox, &["https://x/a".to_string()].into());
    let todos = vec![todo(3, "로그인 리팩터", TodoStatus::Doing, None)];
    let s = build_summary(Some("myrepo".into()), &todos, 0, Some(&inbox), "2026-09-27");
    assert_eq!(s.collect, Some(4));
    assert_eq!(s.collect_items.len(), SUMMARY_INBOX_MAX);
    assert_eq!(
        render_summary(&s),
        "rocky · myrepo — 진행중 1 · 수집함 미올림 4\n  ● rocky-3 로그인 리팩터\n  📥 gh-bugs: 그래프 색상이 디자인과 다름\n  📥 gh-bugs: 캘린더 제한이 동작하지 않음\n  📥 gh-bugs: 셋째\n  … 외 1건"
    );
    let json = serde_json::to_value(&s).unwrap();
    assert_eq!(json["collectItems"][0]["source"], "gh-bugs");
    assert_eq!(json["collectItems"][0]["url"], "https://x/b");
    assert!(json["collectItems"][2].get("url").is_none());
}

#[test]
fn summary_has_no_inbox_lines_without_a_cache_or_when_everything_is_promoted() {
    let s = build_summary(Some("b".into()), &[], 0, None, "2026-09-27");
    assert!(s.collect_items.is_empty());
    let mut inbox = InboxResponse {
        sources: vec![source("s", vec![item("a", "x", Some("https://x/a"))])],
    };
    mark_promoted(&mut inbox, &["https://x/a".to_string()].into());
    let s = build_summary(Some("b".into()), &[], 0, Some(&inbox), "2026-09-27");
    assert_eq!(s.collect, Some(0));
    assert_eq!(render_summary(&s), "rocky · b — 급한 것 없음");
}

#[test]
fn one_line_flattens_and_caps_external_titles() {
    assert_eq!(one_line("  a\n\tb\r\n  c ", 60), "a b c");
    assert_eq!(one_line("가나다라마바", 4), "가나다…");
    assert_eq!(one_line("abcd", 4), "abcd");
}

#[test]
fn summary_buckets_items_and_renders_lines() {
    let todos = vec![
        todo(1, "지난 것", TodoStatus::Todo, Some("2026-09-20")),
        todo(2, "오늘 것", TodoStatus::Doing, Some("2026-09-27")), // 진행중이지만 마감 쪽으로
        todo(3, "그냥 진행중", TodoStatus::Doing, None),
        todo(4, "끝난 것", TodoStatus::Done, Some("2026-09-20")), // 완료는 안 센다
        todo(5, "미래", TodoStatus::Todo, Some("2026-12-01")),
    ];
    let s = build_summary(Some("rocky".into()), &todos, 1, None, "2026-09-27");
    assert_eq!((s.overdue, s.today, s.doing, s.handoffs_open), (1, 1, 2, 1));
    assert_eq!(s.collect, None);
    assert_eq!(
        s.items
            .iter()
            .map(|i| (i.r#ref.as_str(), i.kind))
            .collect::<Vec<_>>(),
        vec![
            ("rocky-1", SummaryKind::Overdue),
            ("rocky-2", SummaryKind::Today),
            ("rocky-3", SummaryKind::Doing),
        ]
    );
    let text = render_summary(&s);
    assert_eq!(
        text,
        "rocky · rocky — 마감 지남 1 · 오늘 마감 1 · 진행중 2 · 핸드오프 대기 1\n  ⚠ rocky-1 지난 것 (2026-09-20)\n  ⏰ rocky-2 오늘 것 (2026-09-27)\n  ● rocky-3 그냥 진행중"
    );
}

#[test]
fn summary_is_one_line_when_nothing_is_urgent_and_caps_items() {
    let s = build_summary(None, &[], 0, None, "2026-09-27");
    assert_eq!(render_summary(&s), "rocky · 전체 — 급한 것 없음");

    let many: Vec<TodoView> = (1..=10)
        .map(|n| todo(n, "d", TodoStatus::Doing, None))
        .collect();
    let s = build_summary(Some("b".into()), &many, 0, None, "2026-09-27");
    assert_eq!(s.doing, 10);
    assert_eq!(s.items.len(), SUMMARY_ITEM_MAX);
    assert_eq!(render_summary(&s).lines().count(), 1 + SUMMARY_ITEM_MAX);
}

#[test]
fn summary_json_shape() {
    let s = build_summary(Some("rocky".into()), &[], 0, None, "2026-09-27");
    let json = serde_json::to_value(&s).unwrap();
    assert_eq!(json["board"], "rocky");
    assert_eq!(json["handoffsOpen"], 0);
    assert!(json.get("collect").is_none());
    assert_eq!(json["items"], serde_json::json!([]));
}
