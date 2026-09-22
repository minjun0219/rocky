//! Claude Code 트랜스크립트(JSONL)에서 "마지막 한 턴"을 기계적으로 추출한다.
//! TS 원본 `src/hooks/transcript.ts`. LLM 없이 동작 — Stop 훅이 워크로그 한 줄을 만들
//! 재료(req / tools / did)만 뽑는다.

use serde_json::Value;

/// 한 턴의 재료 — 사용자 요청, 쓰인 도구(횟수 접미), 마지막 어시스턴트 텍스트.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TurnParts {
    pub req: String,
    pub tools: Vec<String>,
    pub did: String,
}

/// message.content 의 텍스트 — 문자열이면 trim, 블록 배열이면 `text` 블록을 `\n` 으로 이음.
fn text_of(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(s)) => s.trim().to_string(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|b| b.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string(),
        _ => String::new(),
    }
}

/// tool_result 만 담긴 user 메시지는 프롬프트 경계가 아니다.
fn is_real_user_prompt(msg: &Value) -> bool {
    if msg.get("role").and_then(Value::as_str) != Some("user") {
        return false;
    }
    match msg.get("content") {
        Some(Value::String(s)) => !s.trim().is_empty(),
        Some(Value::Array(blocks)) => blocks.iter().any(|b| {
            b.get("type").and_then(Value::as_str) == Some("text")
                && b.get("text")
                    .and_then(Value::as_str)
                    .is_some_and(|t| !t.trim().is_empty())
        }),
        _ => false,
    }
}

/// 마지막 실제 사용자 프롬프트부터 끝까지를 한 턴으로 본다. 프롬프트가 없거나 셋 다
/// 비면 `None`. 손상/부분 라인은 건너뛴다.
pub fn extract_turn(transcript: &str) -> Option<TurnParts> {
    let entries: Vec<Value> = transcript
        .split('\n')
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|v| v.is_object())
        .collect();
    let start = entries
        .iter()
        .rposition(|e| e.get("message").is_some_and(is_real_user_prompt))?;
    let req = text_of(entries[start].get("message").and_then(|m| m.get("content")));
    // 도구 이름은 첫 등장 순서를 유지하면서 횟수를 센다 (JS Map 의 삽입 순서).
    let mut tool_names: Vec<String> = Vec::new();
    let mut tool_counts: Vec<usize> = Vec::new();
    let mut did = String::new();
    for entry in &entries[start + 1..] {
        let Some(msg) = entry.get("message") else {
            continue;
        };
        if msg.get("role").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        if let Some(Value::Array(blocks)) = msg.get("content") {
            for b in blocks {
                if b.get("type").and_then(Value::as_str) == Some("tool_use") {
                    if let Some(name) = b.get("name").and_then(Value::as_str) {
                        match tool_names.iter().position(|n| n == name) {
                            Some(i) => tool_counts[i] += 1,
                            None => {
                                tool_names.push(name.to_string());
                                tool_counts.push(1);
                            }
                        }
                    }
                }
            }
        }
        let txt = text_of(msg.get("content"));
        if !txt.is_empty() {
            did = txt;
        }
    }
    let tools: Vec<String> = tool_names
        .iter()
        .zip(tool_counts.iter())
        .map(|(name, n)| {
            if *n > 1 {
                format!("{name}(×{n})")
            } else {
                name.clone()
            }
        })
        .collect();
    if req.is_empty() && did.is_empty() && tools.is_empty() {
        return None;
    }
    Some(TurnParts { req, tools, did })
}

/// `req: … | tools: … | did: …` 한 줄. 공백은 하나로 접고 각 필드는 `max_chars` 에서
/// `…` 로 자른다(문자 단위). 도구는 최대 20개.
pub fn build_turn_content(parts: &TurnParts, max_chars: usize) -> String {
    let clip = |s: &str| -> String {
        let one = s.split_whitespace().collect::<Vec<_>>().join(" ");
        if one.chars().count() > max_chars {
            let head: String = one.chars().take(max_chars).collect();
            format!("{head}…")
        } else {
            one
        }
    };
    let or_none = |s: String| {
        if s.is_empty() {
            "(none)".to_string()
        } else {
            s
        }
    };
    let req = or_none(clip(&parts.req));
    let tools = or_none(
        parts
            .tools
            .iter()
            .take(20)
            .cloned()
            .collect::<Vec<_>>()
            .join(", "),
    );
    let did = or_none(clip(&parts.did));
    format!("req: {req} | tools: {tools} | did: {did}")
}

/// Stop 훅 자동 기록 토글 — env(`ROCKY_WORKLOG_AUTO_CAPTURE`, `0/false/off/no` 만 비활성)
/// 가 config(`worklog.autoCapture`, 기본 true)를 이긴다.
pub fn should_capture(env_value: Option<&str>, config_auto_capture: Option<bool>) -> bool {
    if let Some(raw) = env_value.map(str::trim).filter(|v| !v.is_empty()) {
        let v = raw.to_lowercase();
        return !matches!(v.as_str(), "0" | "false" | "off" | "no");
    }
    config_auto_capture != Some(false)
}
