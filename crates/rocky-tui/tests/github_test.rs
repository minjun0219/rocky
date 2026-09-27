//! GitHub 링크 판별과 `gh` JSON → 한 줄. 실제 gh 는 부르지 않는다.

use rocky_tui::github::{github_ref, summarize_issue, summarize_pr, GhKind, GhRef};
use serde_json::json;

#[test]
fn only_issue_and_pull_urls_are_github_refs() {
    assert_eq!(
        github_ref("https://github.com/minjun0219/rocky/pull/143"),
        Some(GhRef {
            kind: GhKind::Pull,
            number: 143
        })
    );
    assert_eq!(
        github_ref("https://github.com/o/r/issues/7#issuecomment-1"),
        Some(GhRef {
            kind: GhKind::Issue,
            number: 7
        })
    );
    assert_eq!(
        github_ref("https://github.com/o/r/pull/9/files?x=1").map(|r| r.number),
        Some(9)
    );
    assert_eq!(github_ref("https://github.com/o/r/blob/main/x.rs"), None);
    assert_eq!(github_ref("https://github.com/o/r/compare/a...b"), None);
    assert_eq!(github_ref("https://gitlab.com/o/r/-/issues/1"), None);
    assert_eq!(github_ref("https://github.com/o/r/pull/abc"), None);
}

#[test]
fn pr_summary_covers_merged_draft_ci_and_review() {
    assert_eq!(
        summarize_pr(1, &json!({"state": "MERGED"})),
        "PR #1 · merged"
    );
    assert_eq!(
        summarize_pr(2, &json!({"state": "CLOSED"})),
        "PR #2 · closed"
    );
    let open = json!({
        "state": "OPEN", "isDraft": true, "reviewDecision": "",
        "statusCheckRollup": [
            {"name": "ci", "status": "COMPLETED", "conclusion": "SUCCESS"},
            {"context": "codeql", "state": "SUCCESS"}
        ]
    });
    assert_eq!(
        summarize_pr(3, &open),
        "PR #3 · open · draft · CI ✓ · 리뷰 대기"
    );
    let failing = json!({
        "state": "OPEN", "reviewDecision": "CHANGES_REQUESTED",
        "statusCheckRollup": [
            {"status": "COMPLETED", "conclusion": "SUCCESS"},
            {"status": "COMPLETED", "conclusion": "FAILURE"},
            {"status": "IN_PROGRESS", "conclusion": null}
        ]
    });
    assert_eq!(summarize_pr(4, &failing), "PR #4 · open · CI ✗ · 변경 요청");
    let pending = json!({
        "state": "OPEN", "reviewDecision": "APPROVED",
        "statusCheckRollup": [{"status": "QUEUED", "conclusion": null}]
    });
    assert_eq!(summarize_pr(5, &pending), "PR #5 · open · CI … · 승인");
    // 체크가 없으면 CI 칸을 비운다.
    assert_eq!(
        summarize_pr(6, &json!({"state": "OPEN", "statusCheckRollup": []})),
        "PR #6 · open · 리뷰 대기"
    );
}

#[test]
fn issue_summary() {
    assert_eq!(
        summarize_issue(12, &json!({"state": "OPEN"})),
        "이슈 #12 · open"
    );
    assert_eq!(
        summarize_issue(13, &json!({"state": "CLOSED"})),
        "이슈 #13 · closed"
    );
    assert_eq!(summarize_issue(14, &json!({})), "이슈 #14 · ?");
}
