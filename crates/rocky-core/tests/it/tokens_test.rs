//! 토큰 색인 — 트랜스크립트 파서, 멱등 적재, 요약, 추천 규칙.

use std::io::Write;
use std::path::Path;

use rocky_core::logindex::LogIndex;
use rocky_core::tokens::{
    effort_rank, latest_session_for_cwd, parse_line, recent_turns, recommend, session_detail,
    summary, GroupBy, RecommendConfig, TranscriptLine, TurnStat,
};
use serde_json::json;

const SID: &str = "sess-1";

fn prompt(uuid: &str, ts: &str, text: &str) -> String {
    json!({
        "type": "user", "uuid": uuid, "sessionId": SID, "timestamp": ts,
        "cwd": "/repo", "gitBranch": "main", "isSidechain": false,
        "message": { "role": "user", "content": text }
    })
    .to_string()
}

fn tool_result(ts: &str) -> String {
    json!({
        "type": "user", "uuid": format!("tr-{ts}"), "sessionId": SID, "timestamp": ts,
        "message": { "role": "user", "content": [{ "type": "tool_result", "tool_use_id": "t", "content": "ok" }] }
    })
    .to_string()
}

#[allow(clippy::too_many_arguments)]
fn assistant(
    id: &str,
    ts: &str,
    model: &str,
    effort: &str,
    output: u64,
    block: serde_json::Value,
    sidechain: bool,
) -> String {
    json!({
        "type": "assistant", "uuid": format!("a-{id}-{ts}"), "sessionId": SID, "timestamp": ts,
        "cwd": "/repo/sub", "gitBranch": "feat/x", "isSidechain": sidechain,
        "effort": effort, "perTurnEffort": effort,
        "message": {
            "id": id, "model": model, "stop_reason": "end_turn",
            "content": [block],
            "usage": {
                "input_tokens": 2, "output_tokens": output,
                "cache_read_input_tokens": 100, "cache_creation_input_tokens": 10
            }
        }
    })
    .to_string()
}

fn text() -> serde_json::Value {
    json!({ "type": "text", "text": "hi" })
}

fn tool(id: &str) -> serde_json::Value {
    json!({ "type": "tool_use", "id": id, "name": "Bash", "input": {} })
}

fn append(path: &Path, lines: &[String]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    for l in lines {
        writeln!(f, "{l}").unwrap();
    }
}

#[test]
fn parser_reads_assistant_usage_effort_and_tool_uses() {
    let line = assistant(
        "m1",
        "2026-10-01T00:00:00Z",
        "claude-opus-5-5",
        "high",
        42,
        tool("tu1"),
        false,
    );
    let Some(TranscriptLine::Message(m)) = parse_line(&line) else {
        panic!("message expected");
    };
    assert_eq!(m.message_id, "m1");
    assert_eq!(m.model, "claude-opus-5-5");
    assert_eq!(m.effort.as_deref(), Some("high"));
    assert_eq!(
        (
            m.input_tokens,
            m.output_tokens,
            m.cache_read_tokens,
            m.cache_write_tokens
        ),
        (2, 42, 100, 10)
    );
    assert_eq!(m.tool_use_ids, vec!["tu1".to_string()]);
    assert_eq!(m.meta.git_branch.as_deref(), Some("feat/x"));
}

#[test]
fn parser_accepts_hook_style_effort_object() {
    let mut v: serde_json::Value = serde_json::from_str(&assistant(
        "m1",
        "2026-10-01T00:00:00Z",
        "claude-opus-5-5",
        "x",
        1,
        text(),
        false,
    ))
    .unwrap();
    v["effort"] = json!({ "level": "xhigh" });
    v.as_object_mut().unwrap().remove("perTurnEffort");
    let Some(TranscriptLine::Message(m)) = parse_line(&v.to_string()) else {
        panic!("message expected");
    };
    assert_eq!(m.effort.as_deref(), Some("xhigh"));
}

#[test]
fn parser_skips_tool_results_meta_synthetic_and_garbage() {
    assert!(matches!(
        parse_line(&prompt("p1", "2026-10-01T00:00:00Z", "고쳐줘")),
        Some(TranscriptLine::Prompt(_))
    ));
    assert_eq!(parse_line(&tool_result("2026-10-01T00:00:01Z")), None);
    let mut meta: serde_json::Value =
        serde_json::from_str(&prompt("p2", "2026-10-01T00:00:00Z", "skill body")).unwrap();
    meta["isMeta"] = json!(true);
    assert_eq!(parse_line(&meta.to_string()), None);
    let synthetic = assistant(
        "m9",
        "2026-10-01T00:00:00Z",
        "<synthetic>",
        "high",
        0,
        text(),
        false,
    );
    assert_eq!(parse_line(&synthetic), None);
    assert_eq!(parse_line("{\"type\":\"assistant\""), None);
    // 하네스가 user 자리에 넣은 것은 턴이 아니다 — 사람이 친 슬래시 커맨드는 턴이다.
    for injected in [
        "<task-notification><task-id>x</task-id></task-notification>",
        "<wake reason=\"external-event\">pr</wake>",
        "<local-command-stdout>ok</local-command-stdout>",
    ] {
        assert_eq!(
            parse_line(&prompt("p3", "2026-10-01T00:00:00Z", injected)),
            None,
            "{injected}"
        );
    }
    assert!(matches!(
        parse_line(&prompt(
            "p4",
            "2026-10-01T00:00:00Z",
            "<command-name>/review</command-name>"
        )),
        Some(TranscriptLine::Prompt(_))
    ));
    assert_eq!(parse_line("{\"type\":\"attachment\"}"), None);
}

#[test]
fn ingest_is_incremental_idempotent_and_dedupes_repeated_message_lines() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("projects");
    let main = root.join("-repo").join(format!("{SID}.jsonl"));
    // 한 메시지가 블록마다 한 줄씩 — 같은 usage 가 세 번 나온다.
    append(
        &main,
        &[
            prompt("p1", "2026-10-01T00:00:00Z", "첫 요청"),
            assistant(
                "m1",
                "2026-10-01T00:00:01Z",
                "claude-opus-5-5",
                "high",
                100,
                text(),
                false,
            ),
            assistant(
                "m1",
                "2026-10-01T00:00:01Z",
                "claude-opus-5-5",
                "high",
                100,
                tool("tu1"),
                false,
            ),
            assistant(
                "m1",
                "2026-10-01T00:00:01Z",
                "claude-opus-5-5",
                "high",
                100,
                tool("tu2"),
                false,
            ),
            tool_result("2026-10-01T00:00:02Z"),
            assistant(
                "m2",
                "2026-10-01T00:00:03Z",
                "claude-opus-5-5",
                "high",
                50,
                text(),
                false,
            ),
        ],
    );
    let db = tmp.path().join("logs.db");
    let mut index = LogIndex::open(&db).unwrap();
    let touched = index.ingest_transcripts(&root).touched;
    assert_eq!(
        touched.into_iter().collect::<Vec<_>>(),
        vec![SID.to_string()]
    );

    // 다음 바퀴에 새 턴 — 열린 턴이 커서로 이어지고, 다시 읽어도 늘지 않는다.
    append(
        &main,
        &[
            prompt("p2", "2026-10-01T01:00:00Z", "둘째 요청"),
            assistant(
                "m3",
                "2026-10-01T01:00:01Z",
                "claude-sonnet-5-5",
                "medium",
                30,
                text(),
                false,
            ),
        ],
    );
    // 서브에이전트 — 토큰엔 들어가고 턴 수엔 안 들어간다.
    append(
        &root
            .join("-repo")
            .join(SID)
            .join("subagents")
            .join("agent-a.jsonl"),
        &[assistant(
            "s1",
            "2026-10-01T00:30:00Z",
            "claude-haiku-4-5",
            "low",
            7,
            tool("tu3"),
            true,
        )],
    );
    assert!(index.ingest_transcripts(&root).errors.is_empty());
    assert!(index.ingest_transcripts(&root).touched.is_empty());
    drop(index);
    let index = LogIndex::open(&db).unwrap();

    let rows = summary(
        index.conn(),
        "2026-10-01",
        "2026-10-02",
        GroupBy::ModelEffort,
    )
    .unwrap();
    let opus = rows
        .iter()
        .find(|r| r.model.as_deref() == Some("claude-opus-5-5"))
        .unwrap();
    assert_eq!(opus.effort.as_deref(), Some("high"));
    assert_eq!(
        (
            opus.requests,
            opus.output_tokens,
            opus.tool_calls,
            opus.turns
        ),
        (2, 150, 2, 1)
    );
    let haiku = rows
        .iter()
        .find(|r| r.model.as_deref() == Some("claude-haiku-4-5"))
        .unwrap();
    assert_eq!(
        (haiku.turns, haiku.output_tokens, haiku.tool_calls),
        (0, 7, 1)
    );

    let all = summary(index.conn(), "2026-10-01", "2026-10-02", GroupBy::Session).unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(
        (all[0].turns, all[0].requests, all[0].output_tokens),
        (2, 4, 187)
    );

    let turns = recent_turns(index.conn(), SID, 10).unwrap();
    assert_eq!(turns.len(), 2);
    assert_eq!(
        (
            turns[0].output_tokens,
            turns[0].tool_calls,
            turns[0].requests
        ),
        (150, 2, 2)
    );
    assert_eq!(turns[1].model, "claude-sonnet-5-5");

    let detail = session_detail(index.conn(), SID, 10).unwrap().unwrap();
    assert_eq!(detail.effort_changes.len(), 1);
    assert_eq!(detail.effort_changes[0].from.as_deref(), Some("high"));
    assert_eq!(detail.effort_changes[0].to.as_deref(), Some("medium"));
    assert_eq!(detail.session.started_at, "2026-10-01T00:00:00Z");
    assert_eq!(detail.session.ended_at, "2026-10-01T01:00:01Z");

    assert_eq!(
        latest_session_for_cwd(index.conn(), "/repo")
            .unwrap()
            .as_deref(),
        Some(SID)
    );
    assert_eq!(latest_session_for_cwd(index.conn(), "/rep").unwrap(), None);
}

fn turn(output: u64, tools: u64, model: &str, effort: &str) -> TurnStat {
    TurnStat {
        model: model.into(),
        effort: Some(effort.into()),
        output_tokens: output,
        tool_calls: tools,
        ..Default::default()
    }
}

fn rules(rec: &rocky_core::tokens::Recommendation) -> Vec<&str> {
    rec.suggestions.iter().map(|s| s.rule).collect()
}

#[test]
fn rule_lower_effort_fires_on_short_turns_at_xhigh() {
    let cfg = RecommendConfig::default();
    let turns: Vec<_> = (0..15)
        .map(|_| turn(800, 3, "claude-opus-5-5", "xhigh"))
        .collect();
    let rec = recommend(SID, &turns, &cfg);
    assert_eq!(rules(&rec), vec!["lower-effort"]);
    assert!(rec.suggestions[0].message.contains("15턴"));
    assert!(rec.suggestions[0].message.contains("800"));
    // 세션 중간 전환은 캐시를 깨므로 작업 경계에서 바꾸라고 말한다.
    assert!(rec.suggestions[0].message.contains("작업 경계"));
    assert_eq!(rec.evidence.avg_output_tokens, 800);

    // 길면, 혹은 effort 가 high 면 내지 않는다.
    let long: Vec<_> = (0..15)
        .map(|_| turn(5_000, 3, "claude-opus-5-5", "max"))
        .collect();
    assert!(recommend(SID, &long, &cfg).suggestions.is_empty());
    let high: Vec<_> = (0..15)
        .map(|_| turn(800, 3, "claude-opus-5-5", "high"))
        .collect();
    assert!(recommend(SID, &high, &cfg).suggestions.is_empty());
}

#[test]
fn rule_hold_after_raise_suppresses_when_turns_got_longer() {
    let cfg = RecommendConfig::default();
    let mut turns: Vec<_> = (0..8)
        .map(|_| turn(500, 2, "claude-opus-5-5", "medium"))
        .collect();
    turns.extend((0..7).map(|_| turn(2_500, 2, "claude-opus-5-5", "max")));
    let rec = recommend(SID, &turns, &cfg);
    assert!(rec.suggestions.is_empty());
    let held = rec.held.unwrap();
    assert!(held.contains("medium → max"), "{held}");

    // 올렸는데 짧아졌으면 억제하지 않는다 — 규칙 1 이 다시 산다.
    let mut shorter: Vec<_> = (0..8)
        .map(|_| turn(2_000, 2, "claude-opus-5-5", "medium"))
        .collect();
    shorter.extend((0..7).map(|_| turn(400, 2, "claude-opus-5-5", "max")));
    assert_eq!(rules(&recommend(SID, &shorter, &cfg)), vec!["lower-effort"]);

    // 규칙을 끄면 억제하지 않는다.
    let off = RecommendConfig {
        hold_after_raise: false,
        ..cfg
    };
    assert_eq!(rules(&recommend(SID, &turns, &off)), vec!["lower-effort"]);
}

#[test]
fn rule_switch_to_sonnet_needs_opus_no_tools_and_short_output() {
    let cfg = RecommendConfig::default();
    let chat: Vec<_> = (0..15)
        .map(|_| turn(600, 0, "claude-opus-5-5", "high"))
        .collect();
    let rec = recommend(SID, &chat, &cfg);
    assert_eq!(rules(&rec), vec!["switch-to-sonnet"]);
    assert!(rec.suggestions[0].message.contains("Sonnet medium"));
    assert!(rec.suggestions[0].message.contains("작업 경계"));

    let mut one_tool = chat.clone();
    one_tool[3].tool_calls = 1;
    assert!(recommend(SID, &one_tool, &cfg).suggestions.is_empty());
    let sonnet: Vec<_> = (0..15)
        .map(|_| turn(600, 0, "claude-sonnet-5-5", "high"))
        .collect();
    assert!(recommend(SID, &sonnet, &cfg).suggestions.is_empty());
}

#[test]
fn too_few_turns_are_held_and_window_limits_the_view() {
    let cfg = RecommendConfig::default();
    let few: Vec<_> = (0..3)
        .map(|_| turn(100, 0, "claude-opus-5-5", "max"))
        .collect();
    let rec = recommend(SID, &few, &cfg);
    assert!(rec.suggestions.is_empty());
    assert!(rec.held.unwrap().contains("3개"));

    // 창 밖의 긴 턴은 평균에 들어가지 않는다.
    let mut turns: Vec<_> = (0..10)
        .map(|_| turn(50_000, 0, "claude-opus-5-5", "max"))
        .collect();
    turns.extend((0..15).map(|_| turn(100, 1, "claude-opus-5-5", "max")));
    let rec = recommend(SID, &turns, &cfg);
    assert_eq!(rec.evidence.turns, 15);
    assert_eq!(rules(&rec), vec!["lower-effort"]);
}

#[test]
fn effort_rank_orders_levels() {
    assert!(effort_rank(Some("low")) < effort_rank(Some("medium")));
    assert!(effort_rank(Some("xhigh")) < effort_rank(Some("max")));
    assert_eq!(effort_rank(Some("turbo")), None);
    assert_eq!(effort_rank(None), None);
}

#[test]
fn a_failing_file_does_not_stop_the_rest() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("projects");
    append(
        &root.join("-a").join(format!("{SID}.jsonl")),
        &[
            prompt("p1", "2026-10-01T00:00:00Z", "요청"),
            assistant(
                "m1",
                "2026-10-01T00:00:01Z",
                "claude-opus-5-5",
                "high",
                5,
                text(),
                false,
            ),
        ],
    );
    // 읽을 수 없는 .jsonl — 디렉터리 순서와 무관하게 다른 파일은 옮겨져야 한다.
    let broken = root.join("-b").join("broken.jsonl");
    append(&broken, &["{}".to_string()]);
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&broken, std::fs::Permissions::from_mode(0o000)).unwrap();
    let mut index = LogIndex::open(&tmp.path().join("logs.db")).unwrap();
    let ingest = index.ingest_transcripts(&root);
    std::fs::set_permissions(&broken, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(ingest.errors.len(), 1, "{:?}", ingest.errors);
    assert!(ingest.errors[0].contains("broken.jsonl"));
    assert!(ingest.touched.contains(SID));
}

#[test]
fn range_normalizes_offsets_to_utc() {
    use rocky_core::tokens::range;
    let now = chrono::Utc::now();
    let (from, to) = range(
        Some("2026-10-05T09:00:00+09:00"),
        Some("2026-10-06"),
        None,
        now,
    );
    assert_eq!(from, "2026-10-05T00:00:00.000Z");
    assert_eq!(to, "2026-10-06");
    let (from, to) = range(None, None, Some(7), now);
    assert!(from < now.to_rfc3339());
    assert_eq!(to, "9999");
}

#[test]
fn an_open_turn_carries_over_to_the_next_pass_and_exact_cwd_wins() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("projects");
    let main = root.join("-repo").join(format!("{SID}.jsonl"));
    let mut index = LogIndex::open(&tmp.path().join("logs.db")).unwrap();

    // 첫 바퀴는 프롬프트까지만 — 응답은 다음 바퀴에 읽힌다(1분 경계에 흔하다).
    append(&main, &[prompt("p1", "2026-10-01T00:00:00Z", "요청")]);
    index.ingest_transcripts(&root);
    append(
        &main,
        &[assistant(
            "m1",
            "2026-10-01T00:00:01Z",
            "claude-opus-5-5",
            "high",
            9,
            text(),
            false,
        )],
    );
    index.ingest_transcripts(&root);
    let turns = recent_turns(index.conn(), SID, 10).unwrap();
    assert_eq!(
        turns.len(),
        1,
        "커서가 없으면 응답이 턴 밖(turn_id NULL)으로 빠진다"
    );
    assert_eq!(turns[0].turn_id, "p1");

    // 파일이 줄어 처음부터 다시 읽으면 열린 턴도 처음부터 — 앞 파일의 턴을 물려받지 않는다.
    std::fs::write(&main, "").unwrap();
    append(
        &main,
        &[assistant(
            "m2",
            "2026-10-01T01:00:00Z",
            "claude-opus-5-5",
            "high",
            9,
            text(),
            false,
        )],
    );
    index.ingest_transcripts(&root);
    assert_eq!(
        recent_turns(index.conn(), SID, 10).unwrap().len(),
        1,
        "m2 는 턴이 없다"
    );

    // 하위 디렉터리의 더 최근 세션보다 정확히 그 디렉터리의 세션이 먼저다.
    let sub = root.join("-repo-wt").join("sess-2.jsonl");
    let mut line: serde_json::Value =
        serde_json::from_str(&prompt("q1", "2026-10-02T00:00:00Z", "워크트리")).unwrap();
    line["sessionId"] = json!("sess-2");
    line["cwd"] = json!("/repo/sub/.worktrees/x");
    append(&sub, &[line.to_string()]);
    index.ingest_transcripts(&root);
    assert_eq!(
        latest_session_for_cwd(index.conn(), "/repo/sub")
            .unwrap()
            .as_deref(),
        Some(SID)
    );
    assert_eq!(
        latest_session_for_cwd(index.conn(), "/repo/sub/.worktrees")
            .unwrap()
            .as_deref(),
        Some("sess-2")
    );
}

#[test]
fn a_later_line_adding_a_tool_call_marks_the_session_changed() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("projects");
    let main = root.join("-repo").join(format!("{SID}.jsonl"));
    let mut index = LogIndex::open(&tmp.path().join("logs.db")).unwrap();
    append(
        &main,
        &[
            prompt("p1", "2026-10-01T00:00:00Z", "요청"),
            assistant(
                "m1",
                "2026-10-01T00:00:01Z",
                "claude-opus-5-5",
                "high",
                9,
                text(),
                false,
            ),
        ],
    );
    index.ingest_transcripts(&root);
    // 같은 메시지의 다음 줄(tool_use 블록)이 다음 바퀴에 읽힌다 — 새 메시지는 없어도 도구 호출이 늘었다.
    append(
        &main,
        &[assistant(
            "m1",
            "2026-10-01T00:00:01Z",
            "claude-opus-5-5",
            "high",
            9,
            tool("tu9"),
            false,
        )],
    );
    assert!(index.ingest_transcripts(&root).touched.contains(SID));
    assert_eq!(recent_turns(index.conn(), SID, 5).unwrap()[0].tool_calls, 1);
}
