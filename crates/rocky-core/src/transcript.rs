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

/// 사람이 쓴 요청이 아니라 하네스가 넣은 메시지 — 턴의 경계는 그대로 두되(나누지 않으면 다음 턴 기록이 앞 턴과
/// 겹친다) 요청 칸에는 원문 대신 짧은 이름을 남긴다. 원문(`<task-notification>` 의 경로·id, 셸 출력)은 회고에서
/// 잡음이다(실측: 레포 하나의 턴 기록에서 49건·6건).
pub fn label_injected(req: String) -> String {
    let head = req.trim_start();
    if head.starts_with("<task-notification>") {
        "(백그라운드 작업 알림)".to_string()
    } else if head.starts_with("<bash-stdout>") || head.starts_with("<bash-stderr>") {
        "(셸 출력)".to_string()
    } else {
        req
    }
}

/// 마지막 실제 사용자 프롬프트부터 끝까지를 한 턴으로 본다. 프롬프트가 없거나 셋 다
/// 비면 `None`. 손상/부분 라인은 건너뛴다.
pub fn extract_turn(transcript: &str) -> Option<TurnParts> {
    extract_turn_found(transcript).flatten()
}

/// `extract_turn` 의 속 — 바깥 `None` 은 "이 글에 실제 프롬프트가 없다"(더 앞을 읽어야 한다), 안쪽 `None` 은
/// "찾았는데 남길 게 없다". 끝에서부터 읽는 [`extract_turn_from_tail`] 이 둘을 가른다.
fn extract_turn_found(transcript: &str) -> Option<Option<TurnParts>> {
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
    Some(turn_from(&entries, start))
}

/// 끝에서부터 읽는 첫 창 — 한 턴은 대개 이 안이다.
pub const TAIL_WINDOW: u64 = 256 * 1024;

/// 트랜스크립트 **파일 끝**에서 마지막 턴을 뽑는다. 세션 트랜스크립트는 수십 MB 까지 자라는데(실측 41MB) 필요한 건
/// 마지막 프롬프트부터 끝까지라, 끝의 창만 읽고 프롬프트가 없으면 창을 두 배로 넓힌다. 창의 첫 줄은 잘렸을 수 있어
/// 버린다(파일 맨 앞까지 읽은 경우만 그대로). 결과는 파일 전체를 [`extract_turn`] 한 것과 같다.
pub fn extract_turn_from_tail(path: &std::path::Path, first_window: u64) -> Option<TurnParts> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let mut window = first_window.max(1);
    loop {
        let start = len.saturating_sub(window);
        file.seek(SeekFrom::Start(start)).ok()?;
        let mut buf = Vec::with_capacity((len - start) as usize);
        file.read_to_end(&mut buf).ok()?;
        let text = String::from_utf8_lossy(&buf);
        let text = if start == 0 {
            &text[..]
        } else {
            text.split_once('\n').map_or("", |(_, rest)| rest)
        };
        if let Some(found) = extract_turn_found(text) {
            return found;
        }
        if start == 0 {
            return None;
        }
        window = window.saturating_mul(2);
    }
}

fn turn_from(entries: &[Value], start: usize) -> Option<TurnParts> {
    let req = label_injected(text_of(
        entries[start].get("message").and_then(|m| m.get("content")),
    ));
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
