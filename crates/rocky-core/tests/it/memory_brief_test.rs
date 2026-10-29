//! 레포 기억 요약 — 다이제스트 앵커와 그 뒤의 decision · blocker 고르기, 렌더.

use rocky_core::memory_brief::{
    build_memory_brief, render_memory_brief, MemoryBrief, BRIEF_DIGEST_LINES, BRIEF_PER_KIND,
};
use rocky_core::worklog::WorklogEntry;

fn entry(n: u32, kind: &str, content: &str) -> WorklogEntry {
    WorklogEntry {
        id: format!("{n}-abcdef"),
        timestamp: format!("2026-10-{:02}T00:00:00.000Z", n.min(28)),
        kind: kind.into(),
        content: content.into(),
        tags: vec![],
        page_id: None,
    }
}

#[test]
fn empty_journal_renders_nothing() {
    let brief = build_memory_brief(&[]);
    assert!(brief.is_empty());
    assert_eq!(render_memory_brief(&brief), None);
}

#[test]
fn turns_and_notes_alone_render_nothing() {
    let entries = vec![entry(2, "turn", "user: hi"), entry(1, "note", "memo")];
    assert_eq!(render_memory_brief(&build_memory_brief(&entries)), None);
}

#[test]
fn only_entries_after_the_last_digest_are_picked() {
    // 최근부터: 결정 하나, 다이제스트, 그 앞의 결정(다이제스트가 정리했으니 빠진다).
    let entries = vec![
        entry(5, "blocker", "CI 의 mac 잡이 큐에 묶임"),
        entry(4, "decision", "로키 세션의 cwd 는 캐비닛"),
        entry(3, "turn", "user: ..."),
        entry(
            2,
            "digest",
            "## digest — 12 entries, a … b\n\n- D안으로 결정 → id:1-a (t)\n- 인증은 API 키 먼저 → id:1-b (t)",
        ),
        entry(1, "decision", "옛 결정"),
    ];
    let brief = build_memory_brief(&entries);
    assert_eq!(
        brief,
        MemoryBrief {
            digest: Some((
                "2026-10-02".into(),
                vec![
                    "D안으로 결정 → id:1-a (t)".into(),
                    "인증은 API 키 먼저 → id:1-b (t)".into()
                ]
            )),
            decisions: vec!["로키 세션의 cwd 는 캐비닛".into()],
            blockers: vec!["CI 의 mac 잡이 큐에 묶임".into()],
        }
    );
    let text = render_memory_brief(&brief).unwrap();
    assert!(text.starts_with("🧠 이 레포의 기억(워크로그)"));
    assert!(text.contains("  정리(2026-10-02):\n    · D안으로 결정"));
    assert!(!text.contains("옛 결정"));
    assert!(text.contains("worklog_search"));
}

#[test]
fn without_a_digest_all_entries_count_and_are_capped() {
    let entries: Vec<WorklogEntry> = (1..=6)
        .rev()
        .map(|n| entry(n, "decision", &format!("결정 {n}")))
        .collect();
    let brief = build_memory_brief(&entries);
    assert_eq!(brief.digest, None);
    assert_eq!(brief.decisions.len(), BRIEF_PER_KIND);
    assert_eq!(brief.decisions[0], "결정 6");
}

#[test]
fn multiline_content_is_flattened_and_cut() {
    let long = format!("첫 줄\n둘째 줄 {}", "가".repeat(300));
    let brief = build_memory_brief(&[entry(1, "decision", &long)]);
    let line = &brief.decisions[0];
    assert!(!line.contains('\n'));
    assert!(line.starts_with("첫 줄 둘째 줄"));
    assert!(line.ends_with('…'));
}

#[test]
fn digest_without_anchors_falls_back_to_plain_lines_and_is_capped() {
    let plain = "## digest\n\n하나\n둘\n셋\n넷\n다섯\n여섯";
    let brief = build_memory_brief(&[entry(1, "digest", plain)]);
    let (_, lines) = brief.digest.unwrap();
    assert_eq!(lines.len(), BRIEF_DIGEST_LINES);
    assert_eq!(lines[0], "하나");
}
