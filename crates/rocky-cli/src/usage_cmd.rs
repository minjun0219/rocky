//! `rocky usage` — 사용 로그 보고. 데몬 없이 파일만 읽는다. 집계는 `rocky_core::usage`.
//!
//! CLI·훅·worklog MCP 가 자기 호출을 여기(`record`)로 남긴다 — 데몬을 안 거치는 표면이라
//! 데몬 싱크가 볼 수 없다. 데몬 쪽(REST·MCP)은 `rockyd::usage_sink` 가 같은 파일에 쓴다.

use std::path::PathBuf;
use std::time::Instant;

use rocky_core::config::{env_snapshot, load_usage_block, resolve_usage_dir, user_config_path};
use rocky_core::usage::{
    append_event, build_report, now_iso, parse_since, read_events, render_report, UsageEvent,
    UsageSource, KNOWN_SURFACES,
};
use serde_json::Value;

use crate::commands::Printer;
use crate::flags::ParsedFlags;

/// 설정된 사용 로그 디렉터리 — None 이면 꺼진 것.
pub fn usage_dir() -> Option<PathBuf> {
    resolve_usage_dir(&env_snapshot(), &load_usage_block(&user_config_path()))
}

/// 한 건 기록. 꺼져 있거나 실패하면 조용히 넘어간다 — 로그가 본업을 막지 않는다.
pub fn record(
    source: UsageSource,
    name: &str,
    ok: bool,
    started: Option<Instant>,
    meta: Option<Value>,
) {
    let Some(dir) = usage_dir() else {
        return;
    };
    let mut event = UsageEvent::new(source, name, ok);
    event.ms = started.map(|s| s.elapsed().as_millis() as u64);
    event.meta = meta;
    let _ = append_event(&dir, &event);
}

/// `rocky usage [--since 30d] [--json]`.
pub fn cmd_usage(flags: &ParsedFlags, printer: &Printer) -> Result<(), String> {
    let spec = flags.str_flag("since").unwrap_or("30d");
    let since = parse_since(spec, chrono::Utc::now())
        .ok_or_else(|| format!("--since 는 30d · 12h · 2w 꼴이다 — 받은 값: {spec}"))?;
    let until = now_iso();
    let Some(dir) = usage_dir() else {
        printer.line("사용 로그가 꺼져 있다 (usage.enabled: false 또는 ROCKY_USAGE=0)");
        return Ok(());
    };
    let events = read_events(&dir, &since);
    let report = build_report(&events, &since, &until, KNOWN_SURFACES);
    let raw = serde_json::to_value(&report).unwrap_or(Value::Null);
    printer.emit(&raw, || {
        format!("{}\n  파일: {}", render_report(&report), dir.display())
    });
    Ok(())
}
