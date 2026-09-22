//! `mcp worklog` 표면 가드 — TS `src/index.test.ts` 의 도구 목록 검사 대응.

use rocky_todo_cli::worklog_mcp::WorklogMcp;

#[test]
fn exposes_exactly_the_four_worklog_tools() {
    let mut names = WorklogMcp::tool_names();
    names.sort();
    assert_eq!(
        names,
        vec![
            "worklog_append",
            "worklog_read",
            "worklog_search",
            "worklog_status"
        ]
    );
}
