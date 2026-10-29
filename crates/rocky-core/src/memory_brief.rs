//! 레포 기억 요약 — 세션이 시작할 때 그 레포의 워크로그에서 몇 줄을 골라 컨텍스트에 싣는다. 순수 판정만.
//!
//! 워크로그는 쌓이기만 하고 다시 읽히지 않았다(`worklog_read` 를 부르는 세션이 드물다). SessionStart 의
//! stdout 은 세션 컨텍스트로 들어가므로, 마지막 `/rocky:recall` 다이제스트의 앵커 몇 줄과 그 뒤에 남은
//! `decision` · `blocker` 몇 개를 실으면 새 세션이 묻지 않고도 레포의 최근 결정을 안다.

use crate::summary::one_line;
use crate::worklog::WorklogEntry;

/// 다이제스트에서 싣는 앵커 줄 수 상한.
pub const BRIEF_DIGEST_LINES: usize = 5;

/// `decision` · `blocker` 를 kind 마다 싣는 개수 상한.
pub const BRIEF_PER_KIND: usize = 3;

/// 한 줄의 글자 수 상한 — 세션 컨텍스트에 들어가는 글이라 짧아야 한다.
pub const BRIEF_LINE_MAX: usize = 140;

/// 고른 기억. 비어 있으면 아무것도 싣지 않는다.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemoryBrief {
    /// 마지막 다이제스트의 시각(`YYYY-MM-DD`)과 앵커 줄.
    pub digest: Option<(String, Vec<String>)>,
    /// 다이제스트 뒤(없으면 전체)의 결정, 최근부터.
    pub decisions: Vec<String>,
    /// 다이제스트 뒤(없으면 전체)의 blocker, 최근부터.
    pub blockers: Vec<String>,
}

impl MemoryBrief {
    pub fn is_empty(&self) -> bool {
        self.digest.is_none() && self.decisions.is_empty() && self.blockers.is_empty()
    }
}

/// 다이제스트 본문에서 앵커 줄(`- ` 로 시작)만 고른다. `## digest — …` 머리와 빈 줄은 뺀다.
/// 앵커가 하나도 없으면(손으로 쓴 다이제스트) 비지 않은 첫 줄들을 쓴다.
fn digest_lines(content: &str) -> Vec<String> {
    let anchors: Vec<&str> = content
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("- ") || l.starts_with("* "))
        .map(|l| l[2..].trim())
        .filter(|l| !l.is_empty())
        .collect();
    let lines: Vec<&str> = if anchors.is_empty() {
        content
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .collect()
    } else {
        anchors
    };
    lines
        .into_iter()
        .take(BRIEF_DIGEST_LINES)
        .map(|l| one_line(l, BRIEF_LINE_MAX))
        .collect()
}

/// 워크로그 항목(최근부터, `Worklog::read` 순서)에서 기억을 고른다. 마지막 `digest` 가 있으면 그 앵커와
/// 그보다 뒤의 결정·blocker 만 — 앞의 것은 다이제스트가 이미 정리했다.
pub fn build_memory_brief(newest_first: &[WorklogEntry]) -> MemoryBrief {
    let digest_at = newest_first.iter().position(|e| e.kind == "digest");
    let recent = &newest_first[..digest_at.unwrap_or(newest_first.len())];
    let pick = |kind: &str| -> Vec<String> {
        recent
            .iter()
            .filter(|e| e.kind == kind)
            .take(BRIEF_PER_KIND)
            .map(|e| one_line(&e.content, BRIEF_LINE_MAX))
            .filter(|l| !l.is_empty())
            .collect()
    };
    let digest = digest_at.and_then(|i| {
        let entry = &newest_first[i];
        let lines = digest_lines(&entry.content);
        if lines.is_empty() {
            return None;
        }
        let date = entry
            .timestamp
            .get(..10)
            .unwrap_or(&entry.timestamp)
            .to_string();
        Some((date, lines))
    });
    MemoryBrief {
        digest,
        decisions: pick("decision"),
        blockers: pick("blocker"),
    }
}

/// 세션 컨텍스트에 싣는 모양. 비었으면 `None` — 아무 줄도 찍지 않는다.
pub fn render_memory_brief(brief: &MemoryBrief) -> Option<String> {
    if brief.is_empty() {
        return None;
    }
    let mut out = vec!["🧠 이 레포의 기억(워크로그)".to_string()];
    if let Some((date, lines)) = &brief.digest {
        out.push(format!("  정리({date}):"));
        out.extend(lines.iter().map(|l| format!("    · {l}")));
    }
    if !brief.decisions.is_empty() {
        out.push("  최근 결정:".to_string());
        out.extend(brief.decisions.iter().map(|l| format!("    · {l}")));
    }
    if !brief.blockers.is_empty() {
        out.push("  최근 blocker(풀렸는지는 확인):".to_string());
        out.extend(brief.blockers.iter().map(|l| format!("    · {l}")));
    }
    out.push("  (더 보려면 worklog_search · worklog_read, 정리는 /rocky:recall)".to_string());
    Some(out.join("\n"))
}
