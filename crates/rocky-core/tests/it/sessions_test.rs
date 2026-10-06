//! TS 원본 `src/sessions.test.ts` 의 파싱·매칭 구간 포팅.
//! (RunCommand 실행·TTL 캐시는 데몬 쪽이라 Phase 2 에서 포팅한다.)

use rocky_core::sessions::{
    claude_jobs_dir, job_state_path, match_board, parse_job_state, parse_sessions, SessionsResult,
};

const SAMPLE: &str = r#"[
  {"pid":19921,"cwd":"/Users/minjun/dev/workspaces/rocky-todo","kind":"interactive","startedAt":1784964736538,"sessionId":"bc29bdd3-ba90-4547-96eb-9db0af935e6c","name":"rocky-todo-1e","status":"idle"},
  {"pid":32551,"cwd":"/Users/minjun/orca/workspaces/rocky-todo/eelpout","kind":"interactive","startedAt":1785067158470,"sessionId":"5591d3d2-9ac5-49c4-96b2-2b3e7bdcfce6","name":"eelpout-a3","status":"busy"}
]"#;

const SAMPLE_BACKGROUND: &str = r#"[
  {"pid":24075,"id":"5acaaaeb","cwd":"/repo/.claude/worktrees/todo-16","kind":"background","startedAt":1785151478042,"sessionId":"5acaaaeb-1275-48d1-8f4c-3970c33ff6dc","name":"rocky-todo-16","status":"idle","state":"done"}
]"#;

/// Claude Code 2.1.289 — 잠든 background 행에는 `pid`·`status` 가 없고 `state` 에 `blocked` 가 온다. cwd 는 워크트리가
/// 아니라 레포 루트다(워크트리 경로는 `~/.claude/jobs/<id>/state.json` 의 `worktreePath` 에만 있다).
const SAMPLE_DORMANT: &str = r#"[
  {"id":"0da6a98a","cwd":"/repo","kind":"background","startedAt":1786318742359,"sessionId":"0da6a98a-ed68-4ca3-a447-9ae91c1be8e9","name":"repo-25","state":"blocked"},
  {"pid":82536,"cwd":"/repo","kind":"interactive","startedAt":1791192384395,"sessionId":"92a79b99-aebd-55bc-b4a0-335c04e5848e","name":"Rocky","status":"idle"}
]"#;

#[test]
fn parses_agents_json() {
    let result = parse_sessions(SAMPLE);
    assert!(result.available);
    assert_eq!(result.sessions.len(), 2);
    assert_eq!(result.sessions[0].name, "rocky-todo-1e");
    assert_eq!(result.sessions[1].status, "busy");
}

#[test]
fn empty_array_is_available_with_no_sessions() {
    let result = parse_sessions("[]");
    assert!(result.available);
    assert!(result.sessions.is_empty());
}

#[test]
fn run_failure_maps_to_unavailable_with_reason() {
    // 실행 실패 → 호출자(데몬)가 이 헬퍼로 만든다.
    let result = SessionsResult::unavailable("command not found");
    assert!(!result.available);
    assert!(result
        .reason
        .as_deref()
        .unwrap()
        .contains("command not found"));
    assert!(result.sessions.is_empty());
}

#[test]
fn broken_json_is_unavailable() {
    let result = parse_sessions("not json");
    assert!(!result.available);
    assert!(result.sessions.is_empty());
}

#[test]
fn rows_missing_required_fields_are_skipped() {
    let mixed = r#"[{"pid":1},{"pid":19921,"cwd":"/x","kind":"interactive","startedAt":0,"sessionId":"s","name":"rocky-todo-1e","status":"idle"}]"#;
    let result = parse_sessions(mixed);
    assert_eq!(result.sessions.len(), 1);
    assert_eq!(result.sessions[0].name, "rocky-todo-1e");
}

#[test]
fn match_board_by_path_segment_including_worktrees() {
    let sessions = parse_sessions(SAMPLE).sessions;
    let matched: Vec<_> = match_board(&sessions, "rocky-todo")
        .iter()
        .map(|s| s.name.clone())
        .collect();
    assert_eq!(matched, vec!["rocky-todo-1e", "eelpout-a3"]);
}

#[test]
fn match_board_counts_middle_segments() {
    let sessions = parse_sessions(SAMPLE).sessions;
    let matched: Vec<_> = match_board(&sessions, "eelpout")
        .iter()
        .map(|s| s.name.clone())
        .collect();
    assert_eq!(matched, vec!["eelpout-a3"]);
}

#[test]
fn match_board_no_match_is_empty() {
    let sessions = parse_sessions(SAMPLE).sessions;
    assert!(match_board(&sessions, "forses").is_empty());
}

#[test]
fn match_board_rejects_substring_matches() {
    let sessions = parse_sessions(SAMPLE).sessions;
    assert!(match_board(&sessions, "rocky").is_empty());
}

#[test]
fn background_fields_are_carried() {
    let result = parse_sessions(SAMPLE_BACKGROUND);
    assert_eq!(result.sessions[0].id.as_deref(), Some("5acaaaeb"));
    assert_eq!(result.sessions[0].state.as_deref(), Some("done"));
}

#[test]
fn interactive_sessions_have_no_id_or_state() {
    let result = parse_sessions(SAMPLE);
    assert!(result.sessions[0].id.is_none());
    assert!(result.sessions[0].state.is_none());
}

#[test]
fn background_rows_without_pid_are_kept() {
    let result = parse_sessions(SAMPLE_DORMANT);
    assert_eq!(result.sessions.len(), 2, "pid 없는 행을 버리면 안 된다");
    let dormant = &result.sessions[0];
    assert_eq!(dormant.pid, None);
    assert_eq!(dormant.id.as_deref(), Some("0da6a98a"));
    assert_eq!(dormant.state.as_deref(), Some("blocked"));
    assert_eq!(dormant.status, "idle", "status 가 없으면 idle 로 읽는다");
    assert_eq!(result.sessions[1].pid, Some(82536));
}

#[test]
fn job_state_keeps_only_what_the_screen_uses() {
    let raw = r##"{"state":"blocked","detail":"3 PR 머지, 결정 대기","tempo":"blocked","inFlight":{"tasks":0},
      "tokens":129916,"needs":"1) 워크트리 정리 2) 룰셋 결정","suggestedReply":"룰셋 끄고 정리해줘",
      "output":{"result":"done"},"intent":"# 긴 첫 프롬프트","updatedAt":"2026-08-10T08:28:00.000Z"}"##;
    let job = parse_job_state(raw).unwrap();
    let json = serde_json::to_value(&job).unwrap();
    assert_eq!(
        json.as_object().unwrap().keys().collect::<Vec<_>>(),
        ["detail", "needs", "updatedAt"],
        "제안 답장·토큰은 싣지 않는다"
    );
    assert_eq!(job.detail.as_deref(), Some("3 PR 머지, 결정 대기"));
    assert_eq!(job.needs.as_deref(), Some("1) 워크트리 정리 2) 룰셋 결정"));
    assert_eq!(job.updated_at.as_deref(), Some("2026-08-10T08:28:00.000Z"));
}

#[test]
fn job_state_without_known_fields_is_none() {
    assert_eq!(parse_job_state("not json"), None);
    assert_eq!(parse_job_state("[]"), None);
    assert_eq!(
        parse_job_state(r#"{"state":"working","detail":"  "}"#),
        None
    );
}

#[test]
fn job_state_path_rejects_ids_that_leave_the_folder() {
    let dir = std::path::Path::new("/jobs");
    assert_eq!(
        job_state_path(dir, "0da6a98a"),
        Some(std::path::PathBuf::from("/jobs/0da6a98a/state.json"))
    );
    for bad in ["", "..", "../x", "a/b", "a.b", "a b"] {
        assert_eq!(job_state_path(dir, bad), None, "{bad:?}");
    }
}

#[test]
fn jobs_dir_follows_claude_config_dir() {
    let mut env = rocky_core::config::EnvMap::new();
    env.insert("CLAUDE_CONFIG_DIR".into(), "/alt/claude".into());
    assert_eq!(
        claude_jobs_dir(&env),
        std::path::PathBuf::from("/alt/claude/jobs")
    );
    env.insert("CLAUDE_CONFIG_DIR".into(), " ".into());
    assert!(claude_jobs_dir(&env).ends_with(".claude/jobs"));
}
