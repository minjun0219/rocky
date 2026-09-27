//! GitHub 링크 판별과 `gh` JSON → 한 줄. 실제 gh 는 부르지 않는다.

use rocky_tui::github::{
    build_query, github_ref, parse_batch, summarize_issue, summarize_pr, GhKind, GhRef,
};
use serde_json::json;

#[test]
fn only_issue_and_pull_urls_are_github_refs() {
    assert_eq!(
        github_ref("https://github.com/minjun0219/rocky/pull/143"),
        Some(GhRef {
            owner: "minjun0219".into(),
            repo: "rocky".into(),
            kind: GhKind::Pull,
            number: 143
        })
    );
    assert_eq!(
        github_ref("https://github.com/o/r/issues/7#issuecomment-1").map(|r| (r.kind, r.number)),
        Some((GhKind::Issue, 7))
    );
    // GraphQL 문자열에 그대로 들어가는 자리 — 이름 규칙 밖 문자는 받지 않는다.
    assert_eq!(github_ref("https://github.com/o\"x/r/pull/1"), None);
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

fn gref(owner: &str, repo: &str, kind: GhKind, number: u64) -> GhRef {
    GhRef {
        owner: owner.into(),
        repo: repo.into(),
        kind,
        number,
    }
}

#[test]
fn query_groups_by_repo_and_dedups_numbers() {
    let refs = vec![
        gref("o", "r", GhKind::Pull, 5),
        gref("o", "r", GhKind::Issue, 2),
        gref("o", "r", GhKind::Pull, 5),
        gref("x", "y", GhKind::Issue, 9),
    ];
    let q = build_query(&refs);
    assert!(q.starts_with("query {"));
    assert_eq!(q.matches("repository(").count(), 2, "{q}");
    assert!(q.contains("r0: repository(owner: \"o\", name: \"r\")"));
    assert!(q.contains("r1: repository(owner: \"x\", name: \"y\")"));
    assert_eq!(q.matches("n5: issueOrPullRequest(number: 5)").count(), 1);
    assert!(q.contains("n2: issueOrPullRequest(number: 2)"));
    assert!(q.contains("n9: issueOrPullRequest(number: 9)"));
    assert!(q.contains("statusCheckRollup { state }"));
}

#[test]
fn batch_response_maps_back_to_each_ref() {
    let refs = vec![
        gref("o", "r", GhKind::Pull, 5),
        gref("o", "r", GhKind::Issue, 2),
        gref("x", "y", GhKind::Pull, 9),
        gref("x", "y", GhKind::Pull, 10),
    ];
    let data = json!({
        "r0": {
            "n5": { "__typename": "PullRequest", "state": "OPEN", "isDraft": false, "reviewDecision": "APPROVED",
                    "commits": { "nodes": [ { "commit": { "statusCheckRollup": { "state": "EXPECTED" } } } ] } },
            "n2": { "__typename": "Issue", "state": "CLOSED" }
        },
        "r1": {
            "n9": { "__typename": "PullRequest", "state": "MERGED" },
            "n10": null
        }
    });
    let out = parse_batch(&refs, &data);
    let lines: Vec<Option<&str>> = out.iter().map(|(_, s)| s.as_deref()).collect();
    assert_eq!(
        lines,
        vec![
            Some("PR #5 · open · CI … · 승인"), // EXPECTED = 아직 대기
            Some("이슈 #2 · closed"),
            Some("PR #9 · merged"),
            None, // 못 찾음(권한 없음·삭제) → 줄 비움
        ]
    );
    // rollup 이 없는 PR 은 CI 칸을 비운다.
    let data = json!({ "r0": { "n5": { "__typename": "PullRequest", "state": "OPEN", "reviewDecision": null,
        "commits": { "nodes": [] } } } });
    let out = parse_batch(&refs[..1], &data);
    assert_eq!(out[0].1.as_deref(), Some("PR #5 · open · 리뷰 대기"));
}

/// 실제 GitHub 에 묻는다 — 네트워크·gh 인증이 필요해 CI 에서는 돌리지 않는다.
/// 로컬 확인: `cargo test -p rocky-tui --test github_test -- --ignored live`
#[test]
#[ignore]
fn live_fetch_batch_hits_github() {
    let urls = vec![
        "https://github.com/minjun0219/rocky/pull/143".to_string(),
        "https://github.com/minjun0219/rocky/issues/1".to_string(),
        "https://github.com/minjun0219/rocky/pull/999999".to_string(),
    ];
    let out = rocky_tui::github::fetch_batch(&urls);
    assert_eq!(out.len(), 3, "{out:?}");
    assert_eq!(out[0].1.as_deref(), Some("PR #143 · merged"), "{out:?}");
    // /issues/1 URL 이지만 rocky#1 은 PR 이다 — 종류는 URL 이 아니라 API(__typename)가 정한다.
    assert_eq!(out[1].1.as_deref(), Some("PR #1 · merged"), "{out:?}");
    assert_eq!(out[2].1, None);
}
