//! `rocky verify` — 데몬의 기본 브랜치 검증 결과(`GET /api/verify`)를 한 줄씩.

use serde_json::Value;

use crate::client::{request_value, CliContext};
use crate::commands::Printer;
use crate::flags::ParsedFlags;

pub fn cmd_verify(
    ctx: &CliContext,
    rest: &[String],
    flags: &ParsedFlags,
    printer: &Printer,
) -> Result<(), String> {
    match rest.first().map(String::as_str) {
        Some("subscribe") => return subscribe(ctx, &rest[1..], flags, printer),
        Some("unsubscribe") => return unsubscribe(ctx, &rest[1..], flags, printer),
        Some("subscriptions") => {
            let raw = request_value(ctx, "GET", "/api/verify/subscriptions", None)?;
            printer.emit(&raw, || render_subscriptions(&raw));
            return Ok(());
        }
        _ => {}
    }
    if flags.bool_flag("rerun") {
        // 보드를 안 주면 대상 전부 — cwd 에서 유추하지 않는다(대상은 보통 하나뿐이고, 유추가 빗나가면 404 만 남는다).
        let mut body = serde_json::Map::new();
        if let Some(board) = rest.first().map(String::as_str).or(flags.str_flag("board")) {
            body.insert("board".into(), Value::from(board));
        }
        if let Some(branch) = flags.str_flag("branch") {
            body.insert("branch".into(), Value::from(branch));
        }
        let raw = request_value(ctx, "POST", "/api/verify/rerun", Some(&Value::Object(body)))?;
        printer.emit(&raw, || render_rerun(&raw));
        return Ok(());
    }
    let raw = request_value(ctx, "GET", "/api/verify", None)?;
    printer.emit(&raw, || render_verify(&raw));
    Ok(())
}

/// 이 세션이 검증 결과를 받는다 — 끝난 실행마다 받은편지함으로 한 줄. 보드를 안 주면 대상 전부(보통 하나), 이미 다른
/// 세션이 맡았으면 넘겨받는다. 세션 밖(터미널)에서는 받을 곳이 없어 거절한다.
fn subscribe(
    ctx: &CliContext,
    rest: &[String],
    flags: &ParsedFlags,
    printer: &Printer,
) -> Result<(), String> {
    let session = std::env::var("CLAUDE_CODE_SESSION_ID")
        .ok()
        .filter(|s| !s.is_empty())
        .ok_or("세션 안에서만 구독할 수 있다 — 결과를 받을 Claude Code 세션이 없다(CLAUDE_CODE_SESSION_ID)")?;
    let mut body = target_filter(rest, flags);
    body.insert("sessionId".into(), Value::from(session));
    let raw = request_value(
        ctx,
        "POST",
        "/api/verify/subscriptions",
        Some(&Value::Object(body)),
    )?;
    printer.emit(&raw, || {
        let names = raw["subscribed"]
            .as_array()
            .map(|a| a.iter().map(target_name).collect::<Vec<_>>())
            .unwrap_or_default();
        format!(
            "✓ 검증 결과 구독 — {} (끝난 실행마다 이 세션 받은편지함으로)",
            names.join(", ")
        )
    });
    Ok(())
}

fn unsubscribe(
    ctx: &CliContext,
    rest: &[String],
    flags: &ParsedFlags,
    printer: &Printer,
) -> Result<(), String> {
    let filter = target_filter(rest, flags);
    let query: Vec<String> = filter
        .iter()
        .filter_map(|(k, v)| {
            v.as_str()
                .map(|v| format!("{k}={}", crate::format::encode_uri_component(v)))
        })
        .collect();
    let path = if query.is_empty() {
        "/api/verify/subscriptions".to_string()
    } else {
        format!("/api/verify/subscriptions?{}", query.join("&"))
    };
    let raw = request_value(ctx, "DELETE", &path, None)?;
    printer.emit(&raw, || {
        format!(
            "✓ 검증 결과 구독 해지 — {}개",
            raw["removed"].as_i64().unwrap_or(0)
        )
    });
    Ok(())
}

/// `[BOARD] [--branch B]` — 빼면 그 축은 전부.
fn target_filter(rest: &[String], flags: &ParsedFlags) -> serde_json::Map<String, Value> {
    let mut body = serde_json::Map::new();
    if let Some(board) = rest.first().map(String::as_str).or(flags.str_flag("board")) {
        body.insert("board".into(), Value::from(board));
    }
    if let Some(branch) = flags.str_flag("branch") {
        body.insert("branch".into(), Value::from(branch));
    }
    body
}

fn target_name(v: &Value) -> String {
    format!(
        "{} {}",
        v["board"].as_str().unwrap_or("?"),
        v["branch"].as_str().unwrap_or("?")
    )
}

/// `rocky verify subscriptions` — 대상마다 맡은 세션(로컬이면 id 앞 8자).
pub fn render_subscriptions(raw: &Value) -> String {
    let rows = raw.as_array().cloned().unwrap_or_default();
    if rows.is_empty() {
        return "검증 결과를 구독한 세션이 없다 — 세션 안에서 `rocky verify subscribe`".into();
    }
    rows.iter()
        .map(|r| {
            let session = r["sessionId"]
                .as_str()
                .map(|s| &s[..s.len().min(8)])
                .unwrap_or("?");
            format!("{}  → 세션 {session}", target_name(r))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `--rerun` 응답 — 맡긴 대상과 도는 중이라 건너뛴 대상.
pub fn render_rerun(raw: &Value) -> String {
    let names = |key: &str| -> Vec<String> {
        raw.get(key)
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .map(|t| format!("{} {}", text(t, "board"), text(t, "branch")))
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut out: Vec<String> = names("queued")
        .into_iter()
        .map(|t| format!("↻ {t} — 지금 커밋을 다시 돈다(rocky verify 로 결과)"))
        .collect();
    out.extend(
        names("running")
            .into_iter()
            .map(|t| format!("… {t} — 이미 도는 중이라 건너뛴다")),
    );
    out.join("\n")
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
        // 기록은 UTC — 사람이 읽는 시각은 이 머신의 로컬 시계로.
        let when = chrono::DateTime::parse_from_rfc3339(text(r, "finishedAt"))
            .map(|t| t.with_timezone(&chrono::Local).format("%H:%M").to_string())
            .unwrap_or_default();
        // 자동 재시도(두 번째 시도)였으면 — 통과여도 한 번은 떨어졌다는 신호다.
        let retried = r.get("attempt").and_then(Value::as_u64).unwrap_or(1) > 1;
        let line = match text(r, "state") {
            "passed" if retried => format!("✓ {head} {short} 통과(다시 돌려서) {when} · {subject}"),
            "passed" => format!("✓ {head} {short} 통과 {when} · {subject}"),
            "failed" => format!(
                "✗ {head} {short} {} 실패 — {} · {subject}\n    로그 {}",
                text(r, "failedStep"),
                text(r, "reason"),
                text(r, "log")
            ),
            _ if retried => format!("… {head} {short} 검증 중(자동 재시도) · {subject}"),
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
