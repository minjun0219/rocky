//! 핸드오프 주입문/poke 생성 — 순수 함수. TS 원본 `src/handoff.ts`.

use crate::types::ClaimedHandoff;

/// 주입문 재료 — claim 결과에서도, spawn 직전에도 같은 모양으로 만든다.
pub struct HandoffPromptInput<'a> {
    pub actor: &'a str,
    pub note: &'a str,
    pub todo_ref: &'a str,
    pub todo_title: &'a str,
    /// 이 세션 앞에 아직 남은 pending 건수. spawn 은 항상 0 이다.
    pub remaining: i64,
}

/// 세션에 주입할 지시문. todo 본문은 싣지 않는다 — 세션이 `todo_list` 로 직접 읽으면
/// 댓글·히스토리까지 최신으로 본다.
pub fn build_handoff_prompt_from(input: &HandoffPromptInput) -> String {
    let mut lines = vec![
        "# rocky: 보드에서 도착한 작업 요청".to_string(),
        String::new(),
        format!(
            "{} → {} \"{}\"",
            input.actor, input.todo_ref, input.todo_title
        ),
    ];
    if !input.note.is_empty() {
        lines.push(format!("메모: {}", input.note));
    }
    lines.push(String::new());
    lines.push(format!(
        "이 항목을 지금 착수해라. 상세는 todo_list {{ id: \"{}\" }} 로 읽고,",
        input.todo_ref
    ));
    lines.push(format!(
        "착수할 때 todo_status {{ id: \"{}\", action: \"start\" }} 로 표시한다.",
        input.todo_ref
    ));
    // 끝을 말하지 않으면 세션은 일을 마치고도 doing 으로 남겨 둔다 — 보드엔 "멈춤" 으로 쌓인다.
    lines.push(format!(
        "끝나면 todo_status {{ id: \"{}\", action: \"done\" }}, 손을 떼면 action: \"stop\" 으로 닫는다.",
        input.todo_ref
    ));
    if input.remaining > 0 {
        lines.push(format!(
            "(대기 중인 요청이 {}건 더 있다 — 이 건을 마치면 이어서 도착한다.)",
            input.remaining
        ));
    }
    lines.join("\n")
}

/// 대상 세션의 **턴을 여는** 짧은 신호 — `SendMessage` 로 보낸다.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct HandoffPoke {
    /// `SendMessage` 의 `to` — 세션 이름.
    pub to: String,
    /// `SendMessage` 의 `message`.
    pub message: String,
}

pub struct HandoffPokeInput<'a> {
    pub session_name: &'a str,
    pub todo_ref: &'a str,
    pub todo_title: &'a str,
}

/// poke 문구 — 짧게 둔다(같은 턴의 훅 주입과 중복 방지). 다만 주입이 실패해도 굴러가야
/// 하니 이것만 읽고도 착수할 수 있을 만큼은 남긴다.
pub fn build_handoff_poke(input: &HandoffPokeInput) -> HandoffPoke {
    HandoffPoke {
        to: input.session_name.to_string(),
        message: [
            format!(
                "# rocky: 보드에서 작업 요청이 도착했다 — {} \"{}\"",
                input.todo_ref, input.todo_title
            ),
            String::new(),
            "이 메시지는 턴을 여는 신호다. 상세 지시는 같은 턴의 훅 주입으로 함께 도착한다 —"
                .to_string(),
            format!(
                "주입이 보이지 않으면 todo_list {{ id: \"{}\" }} 로 직접 읽고 착수해라.",
                input.todo_ref
            ),
        ]
        .join("\n"),
    }
}

/// claim 결과로 주입문을 만든다 — 훅(Stop / UserPromptSubmit)이 쓰는 입구.
pub fn build_handoff_prompt(claimed: &ClaimedHandoff) -> String {
    build_handoff_prompt_from(&HandoffPromptInput {
        actor: &claimed.handoff.actor,
        note: &claimed.handoff.note,
        todo_ref: &claimed.todo_ref,
        todo_title: &claimed.todo_title,
        remaining: claimed.remaining,
    })
}

/// 이 세션이 들고 있는 진행 중 할 일 — Stop 훅이 데몬의 `doing` 목록에서 고른다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeldTodo {
    pub todo_ref: String,
    pub title: String,
    /// GitHub PR 을 링크했다 — 머지를 기다리는 중이라 Stop 확인에서 묻지 않는다(머지되면 데몬이 완료한다).
    pub awaits_pr: bool,
    /// 핸드오프가 아니라 세션이 스스로 `start` 한 것을 훅이 귀속시켰다 — Stop 확인에서 묻지 않는다(턴 태그·statusline 은 쓴다).
    pub claimed: bool,
}

/// `GET /api/todos?status=doing` 응답(JSON 배열)에서 **이 세션이 든 것만** 고른다 — `doingSessionId` 가
/// 훅의 `session_id` 와 같은 것. 핸드오프를 받아 `start` 한 세션, 그리고 세션이 스스로 `start` 한 것(PostToolUse 훅
/// `claim-doing` — `claimed`)에 이 귀속이 붙는다. 사람이 누른 start 는 없다. 보관된 것은 뺀다.
pub fn held_by_session(todos: &serde_json::Value, session_id: &str) -> Vec<HeldTodo> {
    todos
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter(|t| t.get("status").and_then(|v| v.as_str()) == Some("doing"))
                .filter(|t| t.get("archivedAt").is_none_or(|v| v.is_null()))
                .filter(|t| t.get("doingSessionId").and_then(|v| v.as_str()) == Some(session_id))
                .filter_map(|t| {
                    Some(HeldTodo {
                        todo_ref: t.get("ref")?.as_str()?.to_string(),
                        title: t.get("title")?.as_str()?.to_string(),
                        awaits_pr: t
                            .get("links")
                            .and_then(|v| v.as_array())
                            .is_some_and(|links| {
                                links.iter().any(|l| {
                                    l.get("url")
                                        .and_then(|u| u.as_str())
                                        .and_then(crate::prwatch::parse_pr_url)
                                        .is_some()
                                })
                            }),
                        claimed: t
                            .get("doingSessionClaimed")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Stop 훅의 마무리 확인 — 세션이 일을 끝내고도 `done` 을 부르지 않아 보드에 "멈춤" 이 쌓인다(2026-10-02
/// 오너). 들고 있는 할 일이 있으면 턴을 한 번 더 열어 묻는다. `stop_hook_active`(이미 Stop 훅 때문에 이어진
/// 턴)면 묻지 않는다 — 안 그러면 "아직 하는 중" 이라 답한 세션을 영영 못 멈춘다. PR 을 링크한 할 일(`awaits_pr`)도
/// 묻지 않는다 — 머지를 기다리는 동안 턴마다 막히고, 머지되면 데몬이 완료한다.
pub fn held_todo_reminder(held: &[HeldTodo], stop_hook_active: bool) -> Option<String> {
    // 스스로 든 것(claimed)은 묻지 않는다 — 오너 결정(2026-10-06): 귀속은 statusline·턴 태그에만, 확인은 핸드오프로 받은 것에만.
    let held: Vec<&HeldTodo> = held.iter().filter(|t| !t.awaits_pr && !t.claimed).collect();
    if stop_hook_active || held.is_empty() {
        return None;
    }
    let mut lines = vec![
        "# rocky: 이 세션이 들고 있는 할 일".to_string(),
        String::new(),
    ];
    for todo in held {
        lines.push(format!("- {} \"{}\" — 진행 중", todo.todo_ref, todo.title));
    }
    lines.push(String::new());
    lines.push(
        "끝났으면 todo_status { id, action: \"done\" }, 손을 뗐으면 action: \"stop\" 으로 닫는다."
            .to_string(),
    );
    lines.push(
        "아직 하는 중이거나 사람의 답을 기다리는 중이면 아무것도 하지 말고 그대로 멈춘다 — 이 확인에 답하지 않는다."
            .to_string(),
    );
    Some(lines.join("\n"))
}

/// PostToolUse 훅 입력 → 세션이 방금 `start` 한 할 일의 id 들. `tool_input.action` 이 start 가 아니면 비고, 시작이 실패했으면
/// (응답에 doing 상태의 할 일이 없으면) 빈다. MCP 도구의 응답은 문자열 JSON 을 품은 content 블록이라 모양을 가리지 않고 훑는다.
pub fn started_todo_ids(input: &serde_json::Value) -> Vec<String> {
    let is_start = input.pointer("/tool_input/action").and_then(|v| v.as_str()) == Some("start");
    if !is_start {
        return Vec::new();
    }
    let mut ids = Vec::new();
    if let Some(response) = input.get("tool_response") {
        collect_doing_ids(response, &mut ids);
    }
    ids.sort();
    ids.dedup();
    ids
}

fn collect_doing_ids(v: &serde_json::Value, out: &mut Vec<String>) {
    match v {
        serde_json::Value::String(s) => {
            if let Ok(inner) = serde_json::from_str::<serde_json::Value>(s) {
                if inner.is_object() || inner.is_array() {
                    collect_doing_ids(&inner, out);
                }
            }
        }
        serde_json::Value::Array(items) => items.iter().for_each(|i| collect_doing_ids(i, out)),
        serde_json::Value::Object(map) => {
            if map.get("status").and_then(|s| s.as_str()) == Some("doing") {
                if let Some(id) = map.get("id").and_then(|s| s.as_str()) {
                    out.push(id.to_string());
                }
            }
            map.values().for_each(|i| collect_doing_ids(i, out));
        }
        _ => {}
    }
}
