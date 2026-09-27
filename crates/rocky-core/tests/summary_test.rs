//! 보드 요약 — 마감 판정, 수집함 미올림 집계, 조립·렌더.

use rocky_core::inbox::{InboxItem, InboxResponse, InboxSourceResult};
use rocky_core::refs::TodoView;
use rocky_core::summary::{
    build_summary, count_unpromoted, due_bucket, render_summary, DueBucket, SummaryKind,
    SUMMARY_ITEM_MAX,
};
use rocky_core::types::{Todo, TodoLink, TodoPriority, TodoStatus};

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

#[test]
fn unpromoted_counts_items_without_a_matching_board_link() {
    let mut on_board = todo(1, "x", TodoStatus::Todo, None);
    on_board.todo.links = vec![TodoLink {
        url: "https://x/a".into(),
        title: None,
    }];
    let inbox = InboxResponse {
        sources: vec![
            InboxSourceResult {
                name: "s".into(),
                available: true,
                reason: None,
                fetched_at: "t".into(),
                items: vec![
                    InboxItem {
                        id: "a".into(),
                        title: "올라감".into(),
                        url: Some("https://x/a".into()),
                        note: None,
                        due: None,
                        created_at: None,
                    },
                    InboxItem {
                        id: "b".into(),
                        title: "안 올라감".into(),
                        url: Some("https://x/b".into()),
                        note: None,
                        due: None,
                        created_at: None,
                    },
                    InboxItem {
                        id: "c".into(),
                        title: "url 없음".into(),
                        url: None,
                        note: None,
                        due: None,
                        created_at: None,
                    },
                ],
            },
            // 실패한 소스는 세지 않는다.
            InboxSourceResult {
                name: "dead".into(),
                available: false,
                reason: Some("exit 1".into()),
                fetched_at: "t".into(),
                items: vec![],
            },
        ],
    };
    assert_eq!(count_unpromoted(&inbox, &[on_board]), 2);
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
