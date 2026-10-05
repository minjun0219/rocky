//! `rocky tokens` — Claude Code 토큰 색인 보기. 데몬의 `/api/tokens/*` 를 읽어 표로 찍는다.
//!
//! - `rocky tokens [--since 30d] [--by model,effort|model|effort|session|branch] [--json]` — 합계
//! - `rocky tokens here [--cwd P] [--json]` — 이 디렉터리의 최근 세션: 턴별 모델·effort·토큰과 추천

use rocky_core::usage::parse_since;
use serde_json::Value;

use crate::client::{request_value, CliContext};
use crate::commands::Printer;
use crate::flags::ParsedFlags;
use crate::format::encode_uri_component;

pub fn cmd_tokens(
    ctx: &CliContext,
    rest: &[String],
    flags: &ParsedFlags,
    printer: &Printer,
) -> Result<(), String> {
    if rest.first().map(String::as_str) == Some("here") {
        let cwd = match flags.str_flag("cwd") {
            Some(c) => c.to_string(),
            None => std::env::current_dir()
                .map_err(|e| format!("현재 디렉터리를 읽지 못했다: {e}"))?
                .to_string_lossy()
                .to_string(),
        };
        let raw = request_value(
            ctx,
            "GET",
            &format!("/api/tokens/current?cwd={}", encode_uri_component(&cwd)),
            None,
        )?;
        printer.emit(&raw, || render_current(&raw));
        return Ok(());
    }
    if let Some(sub) = rest.first() {
        return Err(format!(
            "usage: rocky tokens [--since 30d] [--by model,effort] | rocky tokens here [--cwd P] — 모르는 하위 명령: {sub}"
        ));
    }
    let spec = flags.str_flag("since").unwrap_or("30d");
    let from = parse_since(spec, chrono::Utc::now())
        .ok_or_else(|| format!("--since 는 30d · 12h · 2w 꼴이다 — 받은 값: {spec}"))?;
    let by = flags.str_flag("by").unwrap_or("model,effort");
    let raw = request_value(
        ctx,
        "GET",
        &format!(
            "/api/tokens/summary?from={}&groupBy={}",
            encode_uri_component(&from),
            encode_uri_component(by)
        ),
        None,
    )?;
    printer.emit(&raw, || render_summary(&raw, spec));
    Ok(())
}

/// 1234 → `1.2k`, 3718277 → `3.7M`, 1740448341 → `1.7B`.
pub fn compact(n: u64) -> String {
    let f = n as f64;
    if n >= 1_000_000_000 {
        format!("{:.1}B", f / 1e9)
    } else if n >= 1_000_000 {
        format!("{:.1}M", f / 1e6)
    } else if n >= 10_000 {
        format!("{:.0}k", f / 1e3)
    } else if n >= 1_000 {
        format!("{:.1}k", f / 1e3)
    } else {
        n.to_string()
    }
}

fn num(v: &Value, key: &str) -> u64 {
    v.get(key).and_then(Value::as_u64).unwrap_or(0)
}

fn text<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("-")
}

/// 합계 표. 묶은 칸(모델·effort·세션·브랜치) 다음에 세션·턴·요청·턴당 출력·출력·입력·캐시 읽기/쓰기·도구.
pub fn render_summary(raw: &Value, since: &str) -> String {
    let rows = raw
        .get("rows")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if rows.is_empty() {
        return format!(
            "최근 {since} 동안 색인된 Claude Code 사용이 없다 (색인은 데몬이 1분마다 갱신)"
        );
    }
    let key_of = |r: &Value| -> String {
        ["model", "effort", "sessionId", "gitBranch"]
            .iter()
            .filter_map(|k| r.get(*k).and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join(" · ")
    };
    let keys: Vec<String> = rows.iter().map(key_of).collect();
    let width = keys
        .iter()
        .map(|k| k.chars().count())
        .max()
        .unwrap_or(0)
        .max(4);
    // 한글 머리는 터미널에서 두 칸씩이라 `{:>n}` 로는 어긋난다 — 칸 수를 직접 센다.
    let header: String = [
        ("세션", 5usize),
        ("턴", 5),
        ("요청", 6),
        ("턴당출력", 8),
        ("출력", 7),
        ("입력", 7),
        ("캐시읽기", 8),
        ("캐시쓰기", 7),
        ("도구", 6),
    ]
    .iter()
    .map(|(label, w)| {
        let cols: usize = label
            .chars()
            .map(|c| if c.is_ascii() { 1 } else { 2 })
            .sum();
        format!(" {}{label}", " ".repeat(w.saturating_sub(cols)))
    })
    .collect();
    let mut out = vec![format!(
        "최근 {since} — {}\n{:<width$} {header}",
        text(raw, "groupBy"),
        "",
    )];
    for (row, key) in rows.iter().zip(&keys) {
        let turns = num(row, "turns");
        let per_turn = num(row, "mainOutputTokens")
            .checked_div(turns)
            .map_or_else(|| "-".into(), compact);
        out.push(format!(
            "{key:<width$}  {:>5} {:>5} {:>6} {:>8} {:>7} {:>7} {:>8} {:>7} {:>6}",
            num(row, "sessions"),
            turns,
            num(row, "requests"),
            per_turn,
            compact(num(row, "outputTokens")),
            compact(num(row, "inputTokens")),
            compact(num(row, "cacheReadTokens")),
            compact(num(row, "cacheWriteTokens")),
            num(row, "toolCalls"),
        ));
    }
    out.join("\n")
}

/// 현재 세션 — 머리, 최근 턴(최대 10), effort 변경, 추천.
pub fn render_current(raw: &Value) -> String {
    let session = raw.get("session").cloned().unwrap_or(Value::Null);
    let id = text(&session, "sessionId");
    let mut out = vec![format!(
        "세션 {} · {} · {}",
        &id[..id.len().min(8)],
        text(&session, "cwd"),
        text(&session, "gitBranch"),
    )];
    let turns = raw
        .get("turns")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for t in turns.iter().rev().take(10).rev() {
        out.push(format!(
            "  {}  {:<20} {:<7} 출력 {:>6}  도구 {:>3}",
            text(t, "startedAt").get(..16).unwrap_or("-"),
            text(t, "model"),
            text(t, "effort"),
            compact(num(t, "outputTokens")),
            num(t, "toolCalls"),
        ));
    }
    if let Some(changes) = raw.get("effortChanges").and_then(Value::as_array) {
        for c in changes {
            out.push(format!(
                "  effort {} → {} ({})",
                text(c, "from"),
                text(c, "to"),
                text(c, "ts").get(..16).unwrap_or("-")
            ));
        }
    }
    if let Some(rec) = raw.get("recommendation") {
        let suggestions = rec.get("suggestions").and_then(Value::as_array);
        match suggestions.filter(|s| !s.is_empty()) {
            Some(list) => {
                for s in list {
                    out.push(format!("→ {}", text(s, "message")));
                }
            }
            None => out.push(match rec.get("held").and_then(Value::as_str) {
                Some(why) => format!("추천 없음 — {why}"),
                None => "추천 없음 — 지금 설정이 맞아 보인다".into(),
            }),
        }
    }
    out.join("\n")
}
