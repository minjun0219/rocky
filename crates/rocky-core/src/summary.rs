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

/// 요약에 실리는 수집함 항목 수 상한 — 보드 항목과 따로 센다. 합쳐 나눠 쓰면 진행 중인 일이 많을 때
/// 수집함이 통째로 가려진다.
pub const SUMMARY_INBOX_MAX: usize = 3;

/// 요약 속 수집함 제목의 글자 수 상한.
pub const SUMMARY_TITLE_MAX: usize = 60;

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

/// 수집함 항목 중 아직 보드에 안 올라간 것(`InboxItem::promoted` 가 false — 데몬이 `mark_promoted` 로
/// 전 보드의 링크를 보고 채운다). 사용할 수 없는 소스는 세지 않는다.
fn unpromoted(inbox: &InboxResponse) -> impl Iterator<Item = (&str, &crate::inbox::InboxItem)> {
    inbox
        .sources
        .iter()
        .filter(|s| s.available)
        .flat_map(|s| s.items.iter().map(move |i| (s.name.as_str(), i)))
        .filter(|(_, i)| !i.promoted)
}

/// 아직 안 올라간 수집함 항목 수.
pub fn count_unpromoted(inbox: &InboxResponse) -> i64 {
    unpromoted(inbox).count() as i64
}

/// 아직 안 올라간 수집함 항목 전체의 지문 — 출처·id 를 정렬해 sha256 앞 8바이트(16진수). 순서가 바뀌는 것만으로는
/// 바뀌지 않고, 항목이 하나라도 들고 나면 바뀐다.
pub fn collect_token(inbox: &InboxResponse) -> String {
    let mut keys: Vec<String> = unpromoted(inbox)
        .map(|(source, item)| format!("{source}\u{1f}{}", item.id))
        .collect();
    keys.sort();
    let digest = ring::digest::digest(&ring::digest::SHA256, keys.join("\n").as_bytes());
    digest.as_ref()[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// 외부 제목을 요약 한 줄에 싣는 모양으로 — 줄바꿈·제어문자를 공백으로, 연속 공백은 하나로,
/// `max` 자를 넘으면 자르고 `…`. 수집함 제목은 남이 쓴 글이라 세션 컨텍스트에 여러 줄로 들어가면
/// 요약의 모양을 흉내 낼 수 있다.
pub fn one_line(raw: &str, max: usize) -> String {
    let flat: String = raw
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let joined = flat.split_whitespace().collect::<Vec<_>>().join(" ");
    if joined.chars().count() <= max {
        return joined;
    }
    let cut: String = joined.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", cut.trim_end())
}

/// 요약에 실리는 수집함 항목 하나.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectItem {
    pub source: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
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
    /// 미올림 수집함 항목 — 어댑터 순서 그대로, 최대 `SUMMARY_INBOX_MAX`. 넘친 수는 `collect` 로 안다.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub collect_items: Vec<CollectItem>,
    /// 미올림 수집함 전체의 지문(`collect_token`) — 항목이 들고 나면 바뀐다. `collect_items` 는 앞쪽만 싣고 `collect` 는
    /// 개수뿐이라, 하나 빠지고 하나 들어오면 둘 다 그대로다. 웹 피드가 "본 묶음 뒤로 새 것이 왔나" 를 이걸로 가른다.
    /// 수집함 캐시가 없으면 None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collect_token: Option<String>,
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
        collect: inbox.map(count_unpromoted),
        items,
        collect_items: inbox
            .map(|i| {
                unpromoted(i)
                    .take(SUMMARY_INBOX_MAX)
                    .map(|(source, item)| CollectItem {
                        source: source.to_string(),
                        title: one_line(&item.title, SUMMARY_TITLE_MAX),
                        url: item.url.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        collect_token: inbox.map(collect_token),
    }
}

/// 사람이 읽는 몇 줄. 첫 줄은 개수, 그 뒤 보드 항목(최대 `SUMMARY_ITEM_MAX`)과 미올림 수집함 항목
/// (최대 `SUMMARY_INBOX_MAX`, 넘치면 `… 외 N건`). 전부 0 이면 한 줄뿐.
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
    for item in &s.collect_items {
        // 소스 이름은 설정 파일에서 오지만 제목과 같은 규칙으로 — 한 줄이 한 항목이어야 한다.
        lines.push(format!(
            "  📥 {}: {}",
            one_line(&item.source, 20),
            one_line(&item.title, SUMMARY_TITLE_MAX)
        ));
    }
    let rest = s.collect.unwrap_or(0) - s.collect_items.len() as i64;
    if !s.collect_items.is_empty() && rest > 0 {
        lines.push(format!("  … 외 {rest}건"));
    }
    lines.join("\n")
}
