//! 보드 요약 — "지금 뭐 봐야 하나" 를 몇 줄로. 순수 판정만.
//!
//! 소비자 둘: `rocky today`(사람이 셸에서, Claude Code 의 `!` 모드 포함)와 SessionStart 훅
//! (세션 컨텍스트에 짧게). 같은 문자열을 내므로 둘이 어긋나지 않는다.

use serde::{Deserialize, Serialize};

use crate::inbox::InboxResponse;
use crate::refs::TodoView;
use crate::types::TodoStatus;

/// 요약에 실리는 항목 수 상한 — 세션 컨텍스트에 들어가는 글이라 짧아야 한다.
pub const SUMMARY_ITEM_MAX: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DueBucket {
    Overdue,
    Today,
}

/// `YYYY-MM-DD` 둘을 문자열로 비교한다 — 형식이 고정이라 사전순이 곧 날짜순이다.
pub fn due_bucket(due: &str, today: &str) -> Option<DueBucket> {
    let due = due.get(..10)?;
    if due.len() != 10 || today.len() != 10 {
        return None;
    }
    match due.cmp(today) {
        std::cmp::Ordering::Less => Some(DueBucket::Overdue),
        std::cmp::Ordering::Equal => Some(DueBucket::Today),
        std::cmp::Ordering::Greater => None,
    }
}

/// 수집함 항목 중 이 보드에 아직 안 올라간 것 — url 이 보드 todos 의 links 에 없는 것.
/// url 없는 항목은 판정 불가라 "미올림" 으로 센다(TUI 의 ✓ 판정과 같은 기준).
pub fn count_unpromoted(inbox: &InboxResponse, todos: &[TodoView]) -> i64 {
    let promoted: std::collections::HashSet<&str> = todos
        .iter()
        .flat_map(|t| t.todo.links.iter().map(|l| l.url.as_str()))
        .collect();
    inbox
        .sources
        .iter()
        .filter(|s| s.available)
        .flat_map(|s| s.items.iter())
        .filter(|i| !i.url.as_deref().is_some_and(|u| promoted.contains(u)))
        .count() as i64
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryItem {
    pub r#ref: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due: Option<String>,
    /// 마감 지남 / 오늘 / 진행중 — 렌더의 글리프.
    pub kind: SummaryKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SummaryKind {
    Overdue,
    Today,
    Doing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    /// cwd 로 풀린 보드 key. 못 풀면 None — 전체 보드 기준.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub board: Option<String>,
    pub doing: i64,
    pub overdue: i64,
    pub today: i64,
    /// 열린 핸드오프(대기 + 미수락 배달).
    pub handoffs_open: i64,
    /// 수집함 미올림 — 수집함 캐시가 없으면 None(모름 ≠ 0).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collect: Option<i64>,
    pub items: Vec<SummaryItem>,
}

/// 요약 조립. `todos` 는 보드의 미보관 항목 전부, `today` 는 `YYYY-MM-DD`.
pub fn build_summary(
    board: Option<String>,
    todos: &[TodoView],
    handoffs_open: i64,
    inbox: Option<&InboxResponse>,
    today: &str,
) -> Summary {
    let mut items: Vec<SummaryItem> = Vec::new();
    let mut overdue = 0;
    let mut today_count = 0;
    let mut doing = 0;
    // 지난 마감 → 오늘 마감 → 진행중 순. 같은 항목이 둘에 걸리면(진행중인데 마감 지남) 마감 쪽으로.
    for t in todos.iter().filter(|t| t.todo.status != TodoStatus::Done) {
        let bucket = t.todo.due.as_deref().and_then(|d| due_bucket(d, today));
        match bucket {
            Some(DueBucket::Overdue) => overdue += 1,
            Some(DueBucket::Today) => today_count += 1,
            None => {}
        }
        if t.todo.status == TodoStatus::Doing {
            doing += 1;
        }
        let kind = match (bucket, t.todo.status) {
            (Some(DueBucket::Overdue), _) => Some(SummaryKind::Overdue),
            (Some(DueBucket::Today), _) => Some(SummaryKind::Today),
            (None, TodoStatus::Doing) => Some(SummaryKind::Doing),
            _ => None,
        };
        if let Some(kind) = kind {
            items.push(SummaryItem {
                r#ref: t.r#ref.clone(),
                title: t.todo.title.clone(),
                due: t.todo.due.clone(),
                kind,
            });
        }
    }
    items.sort_by_key(|i| match i.kind {
        SummaryKind::Overdue => 0,
        SummaryKind::Today => 1,
        SummaryKind::Doing => 2,
    });
    items.truncate(SUMMARY_ITEM_MAX);
    Summary {
        board,
        doing,
        overdue,
        today: today_count,
        handoffs_open,
        collect: inbox.map(|i| count_unpromoted(i, todos)),
        items,
    }
}

/// 사람이 읽는 몇 줄. 첫 줄은 개수, 그 뒤 항목(최대 `SUMMARY_ITEM_MAX`). 전부 0 이면 한 줄뿐.
pub fn render_summary(s: &Summary) -> String {
    let board = s.board.as_deref().unwrap_or("전체");
    let mut head = format!("rocky · {board}");
    let mut parts: Vec<String> = Vec::new();
    if s.overdue > 0 {
        parts.push(format!("마감 지남 {}", s.overdue));
    }
    if s.today > 0 {
        parts.push(format!("오늘 마감 {}", s.today));
    }
    if s.doing > 0 {
        parts.push(format!("진행중 {}", s.doing));
    }
    if s.handoffs_open > 0 {
        parts.push(format!("핸드오프 대기 {}", s.handoffs_open));
    }
    if let Some(n) = s.collect.filter(|n| *n > 0) {
        parts.push(format!("수집함 미올림 {n}"));
    }
    if parts.is_empty() {
        head.push_str(" — 급한 것 없음");
    } else {
        head.push_str(" — ");
        head.push_str(&parts.join(" · "));
    }
    let mut lines = vec![head];
    for item in &s.items {
        let glyph = match item.kind {
            SummaryKind::Overdue => "⚠",
            SummaryKind::Today => "⏰",
            SummaryKind::Doing => "●",
        };
        let due = item
            .due
            .as_deref()
            .map(|d| format!(" ({d})"))
            .unwrap_or_default();
        lines.push(format!("  {glyph} {} {}{due}", item.r#ref, item.title));
    }
    lines.join("\n")
}
