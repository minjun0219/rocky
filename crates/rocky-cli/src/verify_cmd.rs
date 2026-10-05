//! `rocky verify` — 데몬의 기본 브랜치 검증 결과(`GET /api/verify`)를 한 줄씩.

use serde_json::Value;

use crate::client::{request_value, CliContext};
use crate::commands::Printer;

pub fn cmd_verify(ctx: &CliContext, printer: &Printer) -> Result<(), String> {
    let raw = request_value(ctx, "GET", "/api/verify", None)?;
    printer.emit(&raw, || render_verify(&raw));
    Ok(())
}

fn text<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

/// 대상마다 한 줄 — `✓`/`✗`/`…` + 보드·브랜치·커밋·제목, 실패면 단계·이유·로그.
pub fn render_verify(raw: &Value) -> String {
    let targets = raw
        .get("targets")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if targets.is_empty() {
        return "검증 대상이 없다 — rocky.json 의 verify.targets[] 에 보드와 단계를 적는다".into();
    }
    let mut out = Vec::new();
    for t in &targets {
        let head = format!("{} {}", text(t, "board"), text(t, "branch"));
        let Some(r) = t.get("record") else {
            out.push(format!("· {head} — 아직 돌지 않았다{}", error_suffix(t)));
            continue;
        };
        let sha = text(r, "sha");
        let short = &sha[..sha.len().min(7)];
        let subject = text(r, "subject");
        let when = text(r, "finishedAt").get(11..16).unwrap_or("");
        let line = match text(r, "state") {
            "passed" => format!("✓ {head} {short} 통과 {when} · {subject}"),
            "failed" => format!(
                "✗ {head} {short} {} 실패 — {} · {subject}\n    로그 {}",
                text(r, "failedStep"),
                text(r, "reason"),
                text(r, "log")
            ),
            _ => format!("… {head} {short} 검증 중 · {subject}"),
        };
        out.push(format!("{line}{}", error_suffix(t)));
    }
    out.join("\n")
}

fn error_suffix(t: &Value) -> String {
    match t.get("error").and_then(Value::as_str) {
        Some(e) => format!("\n    ⚠ {e}"),
        None => String::new(),
    }
}
