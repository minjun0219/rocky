//! Antigravity(`agy`) 트랜스크립트(Stop 훅 입력의 `transcriptPath` — `…/.system_generated/logs/transcript_full.jsonl`)에서
//! 마지막 한 턴을 뽑는다. `rocky hook log-turn agy` 의 재료이고, 결과는 Claude Code 와 같은 [`TurnParts`] 다.
//!
//! 한 줄이 한 단계다: `{ step_index, source, type, content?, tool_calls?, thinking? }`(실측 agy 1.2.17).
//! - 턴의 시작은 사람이 보낸 입력 — `source: USER_EXPLICIT`, `type: USER_INPUT`. 본문은 `<USER_REQUEST>` 로 감싸이고 뒤에
//!   `<ADDITIONAL_METADATA>` 같은 하네스 블록이 붙는다. 훅이 넣은 `userMessage` 도 `USER_INPUT` 이지만 `source: SYSTEM_SDK`
//!   라 턴 중간에 끼어든 것으로 보고 경계로 삼지 않는다.
//! - 도구는 모델 응답(`type: PLANNER_RESPONSE`)의 `tool_calls[].name`. MCP 도구는 모두 `call_mcp_tool` 로 오므로 인자의
//!   `ServerName`·`ToolName` 으로 `mcp__<서버>__<도구>` 를 만든다(Claude Code 와 같은 꼴 — 워크로그 검색이 한 이름으로 잡힌다).
//! - 한 일은 마지막 모델 응답의 `content`(`thinking` 은 보지 않는다).

use serde_json::Value;

use super::{finish_turn, tail_search, ToolTally, TurnParts};

/// 마지막 사람 입력부터 끝까지를 한 턴으로 본다. 입력이 없거나 셋 다 비면 `None`. 손상/부분 라인은 건너뛴다.
pub fn extract_turn(transcript: &str) -> Option<TurnParts> {
    found(transcript).flatten()
}

/// 트랜스크립트 **파일 끝**에서 마지막 턴을 뽑는다 — Claude Code 판([`super::extract_turn_from_tail`])과 같은 창 넓히기.
pub fn extract_turn_from_tail(path: &std::path::Path, first_window: u64) -> Option<TurnParts> {
    tail_search(path, first_window, found)
}

fn str_of<'a>(step: &'a Value, key: &str) -> &'a str {
    step.get(key).and_then(Value::as_str).unwrap_or("")
}

fn is_user_turn(step: &Value) -> bool {
    str_of(step, "type") == "USER_INPUT" && str_of(step, "source") == "USER_EXPLICIT"
}

/// `<USER_REQUEST>` 안쪽 — 태그가 없으면 본문 전체.
fn request_text(content: &str) -> String {
    let inner = content
        .split_once("<USER_REQUEST>")
        .and_then(|(_, rest)| rest.split_once("</USER_REQUEST>"))
        .map_or(content, |(inner, _)| inner);
    inner.trim().to_string()
}

fn tool_name(call: &Value) -> Option<String> {
    let name = call.get("name").and_then(Value::as_str)?;
    if name == "call_mcp_tool" {
        let args = call.get("args");
        let arg = |key: &str| args.and_then(|a| a.get(key)).and_then(Value::as_str);
        if let (Some(server), Some(tool)) = (arg("ServerName"), arg("ToolName")) {
            return Some(format!("mcp__{server}__{tool}"));
        }
    }
    Some(name.to_string())
}

/// 바깥 `None` 은 "이 글에 사람 입력이 없다"(더 앞을 읽어야 한다), 안쪽 `None` 은 "찾았는데 남길 게 없다".
fn found(transcript: &str) -> Option<Option<TurnParts>> {
    let steps: Vec<Value> = transcript
        .split('\n')
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|v| v.is_object())
        .collect();
    let start = steps.iter().rposition(is_user_turn)?;
    let req = request_text(str_of(&steps[start], "content"));
    let mut tools = ToolTally::default();
    let mut did = String::new();
    for step in &steps[start + 1..] {
        if str_of(step, "type") != "PLANNER_RESPONSE" {
            continue;
        }
        if let Some(Value::Array(calls)) = step.get("tool_calls") {
            for name in calls.iter().filter_map(tool_name) {
                tools.add(&name);
            }
        }
        let text = str_of(step, "content").trim();
        if !text.is_empty() {
            did = text.to_string();
        }
    }
    Some(finish_turn(req, tools, did))
}
