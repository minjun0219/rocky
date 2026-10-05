//! `rocky rc [status] [--json]` — `claude rc` 서버 현황. 데몬의 `GET /api/rc/servers` 를 읽어 찍는다.
//! 대상 목록은 `rocky.json` 의 `rc` 블록이다. 띄우기·재시작은 아직 없다(보기만).

use serde_json::Value;

use crate::client::{request_value, CliContext};
use crate::commands::Printer;

pub fn cmd_rc(ctx: &CliContext, rest: &[String], printer: &Printer) -> Result<(), String> {
    match rest.first().map(String::as_str) {
        None | Some("status") => {
            let raw = request_value(ctx, "GET", "/api/rc/servers", None)?;
            printer.emit(&raw, || render_status(&raw));
            Ok(())
        }
        Some(sub) => Err(format!(
            "usage: rocky rc [status] [--json] — 모르는 하위 명령: {sub}"
        )),
    }
}

/// 떠 있은 시간 → `45초` · `12분` · `3시간` · `2일`.
pub fn human_uptime(secs: u64) -> String {
    match secs {
        0..=59 => format!("{secs}초"),
        60..=3599 => format!("{}분", secs / 60),
        3600..=86_399 => format!("{}시간", secs / 3600),
        _ => format!("{}일", secs / 86_400),
    }
}

fn str_of<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

fn line(v: &Value, running: bool) -> String {
    let mut parts = vec![format!(
        "{} {}",
        if running { "●" } else { "○" },
        str_of(v, "label")
    )];
    if v.get("pinned").and_then(Value::as_bool) == Some(true) {
        parts.push("고정".into());
    }
    let sessions = v.get("sessions").and_then(Value::as_u64).unwrap_or(0);
    if sessions > 0 {
        parts.push(format!("세션 {sessions}"));
    }
    if let Some(up) = v.get("uptimeSecs").and_then(Value::as_u64) {
        parts.push(human_uptime(up));
    }
    parts.join("  ")
}

pub fn render_status(raw: &Value) -> String {
    if raw.get("configured").and_then(Value::as_bool) != Some(true) {
        return "rc 대상이 없다 — rocky.json 에 \"rc\": { \"pinned\": [...], \"targets\": [...] } 를 둔다".into();
    }
    let list = |key: &str| {
        raw.get(key)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    };
    let mut out: Vec<String> = list("servers")
        .iter()
        .map(|s| line(s, s.get("running").and_then(Value::as_bool) == Some(true)))
        .collect();
    let strays = list("strays");
    if !strays.is_empty() {
        out.push("대상 밖:".into());
        out.extend(
            strays
                .iter()
                .map(|s| format!("  {}  {}", line(s, true), str_of(s, "dir"))),
        );
    }
    out.push(match str_of(raw, "auth") {
        "in" => "자격: 로그인됨".into(),
        "out" => "자격: ⚠ 로그아웃 — 새로 띄우는 서버가 로그인 안 된 채 뜬다".into(),
        _ => "자격: 확인 못 함".into(),
    });
    if let Some(agy) = raw.get("antigravity").filter(|v| !v.is_null()) {
        let state = agy.get("state").and_then(Value::as_str).unwrap_or("꺼짐");
        let instance = agy.get("instance").and_then(Value::as_str);
        out.push(match instance {
            Some(name) => format!("antigravity: {state} ({name})"),
            None => format!("antigravity: {state}"),
        });
    }
    out.join("\n")
}
