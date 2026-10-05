//! `rocky rc` — `claude rc` 서버. 대상 목록은 `rocky.json` 의 `rc` 블록이다.
//!
//! - `rocky rc [status] [--json]` — 현황(`GET /api/rc/servers`)
//! - `rocky rc start <라벨> [--wait]` · `rocky rc restart <라벨> [--fresh] [--wait]` — 데몬이 띄우거나 다시 띄운다.
//!   재시작은 그 서버에 붙은 원격 세션을 끊는다. `--wait` 면 결과가 날 때까지 기다려 찍는다.
//! - `rocky rc agy [start|stop]` — Antigravity 원격 제어(`agy remote-control`) 보기·켜기·끄기. rc 블록과 상관없다.

use std::time::{Duration, Instant};

use rocky_core::rc::AgyAction;
use serde_json::{json, Value};

use crate::client::{request_value, CliContext};
use crate::commands::Printer;
use crate::flags::ParsedFlags;
use crate::format::encode_uri_component;

const USAGE: &str = "usage: rocky rc [status] | rocky rc start <라벨> [--wait] | rocky rc restart <라벨> [--fresh] [--wait] | rocky rc agy [start|stop]";
/// `--wait` 상한 — 등록 판정 40초 + `already served` 재시도 45·90초 + 정지 유예를 넉넉히 덮는다.
const WAIT_LIMIT: Duration = Duration::from_secs(300);

pub fn cmd_rc(
    ctx: &CliContext,
    rest: &[String],
    flags: &ParsedFlags,
    printer: &Printer,
) -> Result<(), String> {
    match rest.first().map(String::as_str) {
        None | Some("status") => {
            let raw = request_value(ctx, "GET", "/api/rc/servers", None)?;
            printer.emit(&raw, || render_status(&raw));
            Ok(())
        }
        Some("agy") => {
            let raw = match rest.get(1).map(String::as_str) {
                None => request_value(ctx, "GET", "/api/rc/servers", None)?,
                Some(name) => {
                    let action = AgyAction::parse(name).ok_or_else(|| {
                        format!("usage: rocky rc agy [start|stop] — 모르는 동작: {name}")
                    })?;
                    let path = format!("/api/rc/antigravity/{}", action.as_str());
                    request_value(ctx, "POST", &path, Some(&json!({})))?
                }
            };
            printer.emit(&raw, || {
                agy_line(&raw).unwrap_or_else(|| "antigravity: agy 가 설치돼 있지 않다".into())
            });
            Ok(())
        }
        Some(verb @ ("start" | "restart")) => {
            let label = rest
                .get(1)
                .ok_or_else(|| format!("{USAGE} — 라벨이 필요하다"))?;
            let body = serde_json::json!({ "fresh": flags.bool_flag("fresh") });
            let path = format!("/api/rc/servers/{}/{verb}", encode_uri_component(label));
            let accepted = request_value(ctx, "POST", &path, Some(&body))?;
            if let Some(err) = accepted.get("error").and_then(Value::as_str) {
                return Err(err.to_string());
            }
            if !flags.bool_flag("wait") {
                println!("{label}: 받았다 — 진행은 `rocky rc` 로 본다");
                return Ok(());
            }
            let row = wait_for_result(ctx, label)?;
            printer.emit(&row, || render_result(label, &row));
            Ok(())
        }
        Some(sub) => Err(format!("{USAGE} — 모르는 하위 명령: {sub}")),
    }
}

/// 진행 표시(`action`)가 풀릴 때까지 2초 간격으로 현황을 읽는다.
fn wait_for_result(ctx: &CliContext, label: &str) -> Result<Value, String> {
    let started = Instant::now();
    loop {
        std::thread::sleep(Duration::from_secs(2));
        let raw = request_value(ctx, "GET", "/api/rc/servers", None)?;
        let row = raw
            .get("servers")
            .and_then(Value::as_array)
            .and_then(|rows| rows.iter().find(|r| str_of(r, "label") == label))
            .cloned()
            .ok_or_else(|| format!("현황에 {label} 이(가) 없다"))?;
        if row.get("action").is_none() {
            return Ok(row);
        }
        if started.elapsed() > WAIT_LIMIT {
            return Err(format!(
                "{label}: {}초 안에 끝나지 않았다 — `rocky rc` 로 계속 본다",
                WAIT_LIMIT.as_secs()
            ));
        }
    }
}

/// 마지막 결과 한 줄 — `✓ repo-a: 떴다 — 열린 세션 이어받기(-c)(pid 123)`.
pub fn render_result(label: &str, row: &Value) -> String {
    match row.get("lastResult") {
        Some(r) => format!(
            "{} {label}: {}",
            if r.get("ok").and_then(Value::as_bool) == Some(true) {
                "✓"
            } else {
                "✗"
            },
            str_of(r, "message")
        ),
        None => format!("{label}: 결과가 없다(데몬이 다시 떴을 수 있다)"),
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
    match str_of(v, "action") {
        "starting" => parts.push("띄우는 중…".into()),
        "restarting" => parts.push("재시작 중…".into()),
        "retrying" => parts.push("다시 시도 중…".into()),
        _ if v.get("authSuspect").and_then(Value::as_bool) == Some(true) => {
            parts.push("⚠ 자격 의심 — 다시 띄우기를 권한다".into())
        }
        _ => {
            // 실패만 남긴다 — 성공은 ● 가 이미 말한다.
            if let Some(r) = v.get("lastResult") {
                if r.get("ok").and_then(Value::as_bool) == Some(false) {
                    parts.push(format!("✗ {}", str_of(r, "message")));
                }
            }
        }
    }
    parts.join("  ")
}

/// `antigravity: running (mac-1)` — agy 가 없으면 None.
pub fn agy_line(raw: &Value) -> Option<String> {
    let agy = raw.get("antigravity").filter(|v| !v.is_null())?;
    let state = agy.get("state").and_then(Value::as_str).unwrap_or("꺼짐");
    Some(match agy.get("instance").and_then(Value::as_str) {
        Some(name) => format!("antigravity: {state} ({name})"),
        None => format!("antigravity: {state}"),
    })
}

pub fn render_status(raw: &Value) -> String {
    if raw.get("configured").and_then(Value::as_bool) != Some(true) {
        let hint = "rc 가 꺼져 있다 — rocky.json 에 \"rc\": { \"pinned\": [...], \"targets\": [...] } 를 두고(enabled 가 false 가 아니게) 데몬을 다시 띄운다";
        // agy 줄은 rc 블록과 상관없이 보인다.
        return [Some(hint.to_string()), agy_line(raw)]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join("\n");
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
    if let Some(err) = raw.get("probeError").and_then(Value::as_str) {
        out.insert(0, format!("⚠ {err} — ○ 는 꺼짐이 아니라 모름이다"));
    }
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
    if let Some(sup) = raw.get("supervise").filter(|v| !v.is_null()) {
        out.push(match sup.get("lastTick").and_then(Value::as_str) {
            // 데몬은 UTC 로 남긴다 — 이 기기의 현지 시각으로 보인다.
            Some(at) => match chrono::DateTime::parse_from_rfc3339(at) {
                Ok(t) => format!(
                    "감시: 켜짐 — 마지막 {}",
                    t.with_timezone(&chrono::Local).format("%H:%M")
                ),
                Err(_) => format!("감시: 켜짐 — 마지막 {at}"),
            },
            None => "감시: 켜짐 — 첫 바퀴 전".into(),
        });
    }
    out.extend(agy_line(raw));
    out.join("\n")
}
