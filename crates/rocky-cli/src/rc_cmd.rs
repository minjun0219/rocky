//! `rocky rc` — `claude rc` 서버. 대상 목록은 `rocky.json` 의 `rc` 블록이다.
//!
//! - `rocky rc [status] [--json]` — 현황(`GET /api/rc/servers`)
//! - `rocky rc start <라벨> [--wait]` · `rocky rc restart <라벨> [--fresh | --session <cse_…>] [--wait]` — 데몬이 띄우거나
//!   다시 띄운다. 재시작은 그 서버에 붙은 원격 세션을 끊는다(막 대화하는 중이면 턴이 끝날 때까지 기다린다). `--session` 은
//!   이어받을 세션을 claude.ai 쪽 id 로 못 박는다. `--wait` 면 결과가 날 때까지 기다려 찍는다.
//! - `rocky rc start --all` — 꺼진 대상 전부를 띄운다 — 비고정은 서버만(세션을 만들지 않는다), 고정은 세션과 함께. 비상용(옛 CLI `-a`).
//! - `rocky rc --activity` — 현황에 대상마다 최근 활동(마지막 커밋 · 작업 중 · 곁가지 브랜치, 꺼진 것은 정박 여부).
//! - `rocky rc agy [start|stop]` — Antigravity 원격 제어(`agy remote-control`) 보기·켜기·끄기. rc 블록과 상관없다.
//! - `rocky rc nightly [--dry-run]` — 야간 재시작을 지금 한 번 돌린다(데몬이 백그라운드로, 결과는 `rocky rc`).
//!   `--dry-run` 은 리허설: 지금 설치 버전으로 서버마다 무엇을 할지(손대지 않는다).

use std::time::{Duration, Instant};

use rocky_core::rc::AgyAction;
use serde_json::{json, Value};

use crate::client::{request_value, request_value_within, CliContext};
use crate::commands::Printer;
use crate::flags::ParsedFlags;
use crate::format::encode_uri_component;

const USAGE: &str = "usage: rocky rc [status] [--activity] | rocky rc start --all | rocky rc start <라벨> [--wait] | rocky rc restart <라벨> [--fresh | --session <cse_…>] [--wait] | rocky rc agy [start|stop] | rocky rc nightly [--dry-run]";
/// `--wait` 상한 — 턴 대기 10분 + 정지 유예 20초 + `already served` 재시도 60·120초 + 등록 판정 40초를 덮는다.
const WAIT_LIMIT: Duration = Duration::from_secs(15 * 60);
/// `--activity` 응답 한도 — 데몬이 대상마다 git 을 띄워 기다렸다 답한다.
const ACTIVITY_LIMIT: Duration = Duration::from_secs(60);
/// 리허설 응답 한도 — 데몬의 `claude --version` 한도(40초)와 프로브를 덮는다.
const PREVIEW_LIMIT: Duration = Duration::from_secs(90);

pub fn cmd_rc(
    ctx: &CliContext,
    rest: &[String],
    flags: &ParsedFlags,
    printer: &Printer,
) -> Result<(), String> {
    match rest.first().map(String::as_str) {
        None | Some("status") => {
            // `--activity` — 대상마다 최근 활동(git). 데몬이 git 을 띄우니 그때만 붙인다.
            // 대상마다 git 을 띄워 기다렸다 답한다 — 명령마다 5초 한도라 기본 30초보다 넉넉히.
            let raw = if flags.bool_flag("activity") {
                request_value_within(
                    ctx,
                    "GET",
                    "/api/rc/servers?activity=1",
                    None,
                    ACTIVITY_LIMIT,
                )?
            } else {
                request_value(ctx, "GET", "/api/rc/servers", None)?
            };
            printer.emit(&raw, || render_status(&raw));
            Ok(())
        }
        Some("start") if flags.bool_flag("all") => start_all(ctx, rest.get(1).map(String::as_str)),
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
            if verb == "start" && flags.str_flag("session").is_some() {
                return Err(
                    "--session 은 restart 에만 쓴다(그 세션으로 이어받아 다시 띄운다)".into(),
                );
            }
            if verb == "restart" {
                refuse_own_server(ctx, label)?;
            }
            let session = flags.str_flag("session").filter(|_| verb == "restart");
            if let Some(id) = session {
                if flags.bool_flag("fresh") {
                    return Err("--fresh(이어받지 않음)와 --session(이 세션으로 이어받음)은 같이 줄 수 없다".into());
                }
                if !rocky_core::rc::valid_session_id(id) {
                    return Err(format!(
                        "--session 은 claude.ai 쪽 세션 id(cse_… · session_…)다 — 로컬 전사본 UUID 가 아니다: {id}"
                    ));
                }
            }
            let body = serde_json::json!({ "fresh": flags.bool_flag("fresh"), "session": session });
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
        Some("nightly") => {
            if !flags.bool_flag("dry-run") {
                let accepted = request_value(ctx, "POST", "/api/rc/nightly", Some(&json!({})))?;
                if let Some(err) = accepted.get("error").and_then(Value::as_str) {
                    return Err(err.to_string());
                }
                println!("야간 재시작을 시작했다 — 서버를 내리고 띄우는 데 수 분이 걸린다. 결과는 `rocky rc` 로 본다");
                return Ok(());
            }
            // 데몬은 `claude --version`(새 바이너리의 첫 실행은 수십 초 멎는다)과 프로브를 기다렸다 답한다.
            let raw =
                request_value_within(ctx, "GET", "/api/rc/nightly/preview", None, PREVIEW_LIMIT)?;
            if let Some(err) = raw.get("error").and_then(Value::as_str) {
                return Err(err.to_string());
            }
            printer.emit(&raw, || render_nightly(&raw));
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
    let activity = v.get("activity").filter(|a| !a.is_null());
    // 꺼졌고 활동이 없는 비고정 대상 — 옛 CLI 의 "정박"(열 만한 후보가 아니다). 고정은 감시가 되살리니 붙이지 않는다.
    let pinned = v.get("pinned").and_then(Value::as_bool) == Some(true);
    if !running
        && !pinned
        && activity
            .and_then(|a| a.get("active"))
            .and_then(Value::as_bool)
            == Some(false)
    {
        parts.push("정박".into());
    }
    let sessions = v.get("sessions").and_then(Value::as_u64).unwrap_or(0);
    if sessions > 0 {
        parts.push(format!("세션 {sessions}"));
    }
    if let Some(up) = v.get("uptimeSecs").and_then(Value::as_u64) {
        parts.push(human_uptime(up));
    }
    if v.get("stale").and_then(Value::as_bool) == Some(true) {
        parts.push("구버전".into());
    }
    match str_of(v, "action") {
        "starting" => parts.push("띄우는 중…".into()),
        "restarting" => parts.push("재시작 중…".into()),
        "retrying" => parts.push("다시 시도 중…".into()),
        "waiting" => parts.push("대화가 끝나길 기다리는 중…".into()),
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
    if let Some(a) = activity {
        parts.push(activity_summary(a, chrono::Utc::now().timestamp()));
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
    if let Some(nightly) = raw.get("nightly").filter(|v| !v.is_null()) {
        out.push(nightly_line(nightly));
    }
    out.extend(agy_line(raw));
    out.join("\n")
}

/// `야간: 04:30 · 마지막 10-07 04:31 — 재시작 3 · 건너뜀 2 · 못 띄움 1` — 시각은 이 기기의 현지 시각.
pub fn nightly_line(n: &Value) -> String {
    let at = n
        .get("at")
        .and_then(Value::as_str)
        .map(|a| format!("{a} · "))
        .unwrap_or_default();
    if n.get("running").and_then(Value::as_bool) == Some(true) {
        return format!("야간: {at}도는 중");
    }
    let Some(last) = n.get("last").filter(|v| !v.is_null()) else {
        return format!("야간: {at}아직 안 돌았다");
    };
    let when = last
        .get("finishedAt")
        .and_then(Value::as_str)
        .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
        .map(|t| {
            t.with_timezone(&chrono::Local)
                .format("%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_else(|| "?".into());
    if let Some(reason) = last.get("blocked").and_then(Value::as_str) {
        return format!("야간: {at}마지막 {when} — 전부 건너뜀({reason})");
    }
    let count = |outcome: &str| {
        last.get("items")
            .and_then(Value::as_array)
            .map_or(0, |items| {
                items
                    .iter()
                    .filter(|i| str_of(i, "outcome") == outcome)
                    .count()
            })
    };
    let mut parts = vec![
        format!("재시작 {}", count("restarted")),
        format!("건너뜀 {}", count("skipped")),
    ];
    if count("down") > 0 {
        parts.push(format!("⚠ 못 띄움 {}", count("down")));
    }
    format!("야간: {at}마지막 {when} — {}", parts.join(" · "))
}

/// 야간 결과(또는 리허설) — 머리 한 줄(update · 설치 버전) 뒤 서버마다 `기호 라벨  메모`.
pub fn render_nightly(raw: &Value) -> String {
    let title = if raw.get("dryRun").and_then(Value::as_bool) == Some(true) {
        "야간 리허설"
    } else {
        "야간 재시작"
    };
    let mut out = vec![format!("{title} — update: {}", str_of(raw, "update"))];
    if let Some(reason) = raw.get("blocked").and_then(Value::as_str) {
        out.push(format!("전부 건너뜀 — {reason}"));
    }
    let items = raw
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let width = items
        .iter()
        .map(|i| str_of(i, "label").chars().count())
        .max()
        .unwrap_or(0);
    out.extend(items.iter().map(|i| {
        let mark = match str_of(i, "outcome") {
            "restarted" => "✓",
            "would-restart" => "↻",
            "would-wait" => "…",
            "current" => "=",
            "down" => "✗",
            _ => "–",
        };
        format!(
            "{mark} {:width$}  {}",
            str_of(i, "label"),
            str_of(i, "note")
        )
    }));
    if items.is_empty() && raw.get("blocked").is_none() {
        out.push("떠 있는 대상이 없다".into());
    }
    out.join("\n")
}

/// 이 CLI 가 그 서버의 세션 안에서 돌고 있으면 거절한다 — 재시작이 이 턴을 끊는다. 데몬의 턴 대기는 대화 기록이 2분
/// 조용하면 끝난 것으로 보는데, 도구 호출(이 명령 자신 포함)이 도는 동안에는 기록이 멈춰 있다. 옛 CLI 의 자가-살해 가드와
/// 같다. 현황이나 `ps` 를 못 읽으면 막지 않는다(데몬의 다른 검사가 남는다).
fn refuse_own_server(ctx: &CliContext, label: &str) -> Result<(), String> {
    let Ok(raw) = request_value(ctx, "GET", "/api/rc/servers", None) else {
        return Ok(());
    };
    let Some(pid) = raw
        .get("servers")
        .and_then(Value::as_array)
        .and_then(|rows| rows.iter().find(|r| str_of(r, "label") == label))
        .and_then(|r| r.get("pid"))
        .and_then(Value::as_u64)
    else {
        return Ok(());
    };
    let Ok(out) = std::process::Command::new("ps")
        .args(["-axww", "-o", "pid=,ppid=,etime=,args="])
        .output()
    else {
        return Ok(());
    };
    let rows = rocky_core::rc::parse_ps(&String::from_utf8_lossy(&out.stdout));
    if rocky_core::rc::ancestors(&rows, std::process::id()).contains(&(pid as u32)) {
        return Err(format!(
            "{label} 은(는) 이 세션이 붙은 서버다 — 재시작하면 이 턴이 끊긴다. 웹 원격 제어 탭 · 다른 세션 · 터미널에서 재시작한다"
        ));
    }
    Ok(())
}

/// `start --all` 이 띄울 것 — 꺼진 대상마다 `(라벨, serverOnly)`. 꺼짐이 확실한 행만(`running: false`) 고르고,
/// 고정은 감시가 되살릴 때처럼 세션과 함께(`serverOnly: false`), 나머지는 서버만.
pub fn start_all_targets(raw: &Value) -> Vec<(String, bool)> {
    raw.get("servers")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter(|r| r.get("running").and_then(Value::as_bool) == Some(false))
                .map(|r| {
                    let pinned = r.get("pinned").and_then(Value::as_bool) == Some(true);
                    (str_of(r, "label").to_string(), !pinned)
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 꺼진 대상 전부를 띄운다 — 비고정은 서버만, 고정은 세션까지(감시가 되살리는 방식과 같게 · 옛 CLI `-a`). 받은 것과 거절된
/// 것을 한 줄씩. 진행은 `rocky rc` 로 본다.
fn start_all(ctx: &CliContext, extra: Option<&str>) -> Result<(), String> {
    if let Some(label) = extra {
        return Err(format!("--all 은 라벨을 받지 않는다: {label}"));
    }
    let raw = request_value(ctx, "GET", "/api/rc/servers", None)?;
    if raw.get("configured").and_then(Value::as_bool) != Some(true) {
        return Err("rc 가 꺼져 있다 — rocky rc 로 본다".into());
    }
    if let Some(err) = raw.get("probeError").and_then(Value::as_str) {
        return Err(format!("현황을 못 읽어 띄우지 않는다 — {err}"));
    }
    let down = start_all_targets(&raw);
    if down.is_empty() {
        println!("꺼진 대상이 없다");
        return Ok(());
    }
    for (label, server_only) in down {
        let path = format!("/api/rc/servers/{}/start", encode_uri_component(&label));
        let res = request_value(
            ctx,
            "POST",
            &path,
            Some(&json!({ "serverOnly": server_only })),
        )?;
        match res.get("error").and_then(Value::as_str) {
            Some(err) => println!("✗ {label}: {err}"),
            None if server_only => println!("… {label}: 서버만 띄우는 중"),
            None => println!("… {label}: 고정 — 세션과 함께 띄우는 중"),
        }
    }
    println!("진행은 `rocky rc` 로 본다");
    Ok(())
}

/// 최근 활동 한 칸 — `3일 전 feat: 무엇… *작업중 @feat/x`. git 이 아니면 `(git 아님)`.
pub fn activity_summary(a: &Value, now: i64) -> String {
    if a.get("repo").and_then(Value::as_bool) != Some(true) {
        return "(git 아님)".into();
    }
    let age = a
        .get("commitAt")
        .and_then(Value::as_i64)
        .map_or_else(|| "-".to_string(), |t| rocky_core::rc::activity_age(now, t));
    let subject = a
        .get("subject")
        .and_then(Value::as_str)
        .map_or_else(|| "(커밋 없음)".to_string(), rocky_core::rc::short_subject);
    let mut out = format!("{age} {subject}");
    if a.get("dirty").and_then(Value::as_bool) == Some(true) {
        out.push_str(" *작업중");
    }
    if let (Some(b), Some(d)) = (
        a.get("branch").and_then(Value::as_str),
        a.get("defaultBranch").and_then(Value::as_str),
    ) {
        if b != d {
            out.push_str(&format!(" @{b}"));
        }
    }
    out
}
