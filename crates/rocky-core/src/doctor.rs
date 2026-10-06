//! `rocky doctor` 의 실행 상태 점검 — 데몬이 낸 JSON(`/api/health` · `/api/verify` · `/api/deliveries` ·
//! `/api/rc/servers`)을 읽어 항목마다 ✓/⚠ 한 줄과 고치는 명령을 낸다. 설치·설정 점검은 `setup` 이 맡고, 여기는
//! "떠 있는 데몬이 제대로 돌고 있나" 만 본다. 읽기만 한다 — 고치는 명령은 안내로만 낸다.
//!
//! 여기는 I/O 가 없다 — 요청은 CLI(`rocky_cli::doctor_cmd`)가 한다.

use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::setup::{Check, CheckKind, SetupReport};

/// PR 감시는 3분마다 돈다 — 마지막 tick 이 이보다 오래되면 멈춘 것으로 본다.
pub const PR_WATCH_STALE_SECS: i64 = 15 * 60;

/// 세션 전달 실패를 이만큼 거슬러 센다 — 그보다 오래된 실패는 지난 일이다.
pub const DELIVERY_FAILURE_WINDOW_SECS: i64 = 24 * 3600;

/// 데몬에서 받은 것. 못 받은 것(옛 데몬·로컬 아님·요청 실패)은 `None` 이고 그 항목은 건너뛴다.
#[derive(Debug, Clone, Default)]
pub struct RuntimeInput {
    pub health: Value,
    pub verify: Option<Value>,
    pub deliveries: Option<Value>,
    pub rc: Option<Value>,
}

fn check(id: &str, kind: CheckKind, ok: bool, detail: String, fix: Option<&str>) -> Check {
    Check {
        id: id.into(),
        kind,
        ok,
        detail,
        fix: fix.map(Into::into),
    }
}

fn str_of<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

fn age_secs(at: &str, now: DateTime<Utc>) -> Option<i64> {
    DateTime::parse_from_rfc3339(at)
        .ok()
        .map(|t| (now - t.with_timezone(&Utc)).num_seconds())
}

/// 실행 상태 점검 — 항목 순서는 데몬 → PR 감시 → 기본 브랜치 검증 → 세션 전달 → rc.
pub fn runtime_checks(input: &RuntimeInput, now: DateTime<Utc>) -> Vec<Check> {
    let mut out = vec![db_check(&input.health), pr_watch_check(&input.health, now)];
    out.extend(input.verify.as_ref().map(verify_check));
    out.extend(input.deliveries.as_ref().map(|d| deliveries_check(d, now)));
    out.extend(input.rc.as_ref().map(rc_check));
    out
}

fn db_check(health: &Value) -> Check {
    match str_of(health, "dbIntegrity") {
        Some("ok") => check("db", CheckKind::Required, true, "DB 무결성 ok".into(), None),
        // 옛 데몬은 싣지 않는다 — 모르는 것을 문제로 치지 않는다.
        None => check(
            "db",
            CheckKind::Info,
            true,
            "DB 무결성 — 데몬이 싣지 않음".into(),
            None,
        ),
        Some(other) => check(
            "db",
            CheckKind::Required,
            false,
            format!("DB 무결성: {other}"),
            Some("~/.config/rocky/todo/todo.db 를 백업해 두고 데몬 로그를 본다"),
        ),
    }
}

fn pr_watch_check(health: &Value, now: DateTime<Utc>) -> Check {
    let Some(watch) = health.get("prWatch").filter(|w| !w.is_null()) else {
        return check(
            "pr-watch",
            CheckKind::Info,
            true,
            "PR 감시 꺼짐".into(),
            None,
        );
    };
    if watch.get("available").and_then(Value::as_bool) != Some(true) {
        let reason = str_of(watch, "reason").unwrap_or("이유 모름");
        return check(
            "pr-watch",
            CheckKind::Required,
            false,
            format!("PR 감시 멈춤 — {reason}"),
            Some("gh auth status"),
        );
    }
    let repos = watch
        .get("repos")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    match str_of(watch, "lastTick").and_then(|at| age_secs(at, now)) {
        Some(age) if age > PR_WATCH_STALE_SECS => check(
            "pr-watch",
            CheckKind::Required,
            false,
            format!("PR 감시 — 마지막 tick {}분 전(3분마다 돈다)", age / 60),
            Some("rocky daemon restart"),
        ),
        Some(age) => check(
            "pr-watch",
            CheckKind::Required,
            true,
            format!("PR 감시 — 레포 {repos}개, 마지막 tick {}분 전", age / 60),
            None,
        ),
        None => check(
            "pr-watch",
            CheckKind::Required,
            true,
            format!("PR 감시 — 레포 {repos}개, 첫 tick 전"),
            None,
        ),
    }
}

fn verify_check(verify: &Value) -> Check {
    let targets = verify
        .get("targets")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if targets.is_empty() {
        return check(
            "verify",
            CheckKind::Info,
            true,
            "기본 브랜치 검증 꺼짐".into(),
            None,
        );
    }
    let failed: Vec<String> = targets
        .iter()
        .filter_map(|t| {
            let record = t.get("record")?;
            (str_of(record, "state") == Some("failed")).then(|| {
                format!(
                    "{}/{} {}",
                    str_of(t, "board").unwrap_or("?"),
                    str_of(t, "branch").unwrap_or("?"),
                    str_of(record, "subject").unwrap_or(""),
                )
            })
        })
        .collect();
    if failed.is_empty() {
        check(
            "verify",
            CheckKind::Optional,
            true,
            format!("기본 브랜치 검증 — 대상 {}개 실패 없음", targets.len()),
            None,
        )
    } else {
        check(
            "verify",
            CheckKind::Optional,
            false,
            format!("기본 브랜치 검증 실패 — {}", failed.join(" · ")),
            Some("rocky verify"),
        )
    }
}

fn deliveries_check(deliveries: &Value, now: DateTime<Utc>) -> Check {
    let receivers = deliveries
        .get("sessions")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let ended = deliveries.get("ended").and_then(Value::as_u64).unwrap_or(0);
    let failures: Vec<&Value> = deliveries
        .get("recent")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter(|d| d.get("ok").and_then(Value::as_bool) == Some(false))
                .filter(|d| {
                    str_of(d, "at")
                        .and_then(|at| age_secs(at, now))
                        .is_some_and(|age| age <= DELIVERY_FAILURE_WINDOW_SECS)
                })
                .collect()
        })
        .unwrap_or_default();
    let ended_note = if ended > 0 {
        format!(", 끝난 세션 {ended}개 뺌")
    } else {
        String::new()
    };
    // 최근 기록은 새 것부터다 — 맨 앞의 실패가 마지막 실패.
    match failures.first() {
        Some(last) => check(
            "deliveries",
            CheckKind::Optional,
            false,
            format!(
                "세션 전달 — 24시간 안에 못 보낸 것 {}건, 마지막: {}",
                failures.len(),
                str_of(last, "reason").unwrap_or("이유 모름"),
            ),
            Some("웹 \"세션 전달\" 카드에서 받는 세션을 본다"),
        ),
        None => check(
            "deliveries",
            CheckKind::Optional,
            true,
            format!("세션 전달 — 받는 세션 {receivers}개{ended_note}"),
            None,
        ),
    }
}

fn rc_check(rc: &Value) -> Check {
    if rc.get("configured").and_then(Value::as_bool) != Some(true) {
        return check("rc", CheckKind::Info, true, "rc 꺼짐".into(), None);
    }
    if let Some(err) = str_of(rc, "probeError") {
        return check(
            "rc",
            CheckKind::Optional,
            false,
            format!("rc 프로브 실패 — {err}"),
            Some("rocky rc"),
        );
    }
    if str_of(rc, "auth") == Some("out") {
        return check(
            "rc",
            CheckKind::Optional,
            false,
            "rc 자격 로그아웃 — 새로 띄우는 서버가 로그인 안 된 채 뜬다".into(),
            Some("claude 에 다시 로그인한 뒤 rocky rc restart"),
        );
    }
    let servers = rc
        .get("servers")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let running = |s: &Value| s.get("running").and_then(Value::as_bool) == Some(true);
    let down: Vec<&str> = servers
        .iter()
        .filter(|s| s.get("pinned").and_then(Value::as_bool) == Some(true) && !running(s))
        .filter_map(|s| str_of(s, "label"))
        .collect();
    if down.is_empty() {
        let up = servers.iter().filter(|s| running(s)).count();
        check(
            "rc",
            CheckKind::Optional,
            true,
            format!("rc — 서버 {}개 중 {up}개 떠 있음", servers.len()),
            None,
        )
    } else {
        check(
            "rc",
            CheckKind::Optional,
            false,
            format!("rc — 고정 서버 {}개 꺼짐: {}", down.len(), down.join(", ")),
            Some("rocky rc start"),
        )
    }
}

/// 사람이 읽는 출력 — 설치·설정(`setup`)과 실행 상태를 나눠 보이고, 문제 항목 아래에 고치는 명령을 단다.
/// `runtime` 이 `None` 이면 데몬에 닿지 못한 것이다.
pub fn render(setup: &SetupReport, runtime: Option<&[Check]>) -> String {
    let mut lines = vec!["설치·설정".to_string()];
    lines.extend(
        crate::setup::render_report(setup)
            .lines()
            .map(|l| format!("  {l}")),
    );
    lines.push(String::new());
    lines.push("실행 상태".into());
    match runtime {
        None => lines.push("  ⚠ 데몬에 닿지 못해 보지 못했다\n      → rocky daemon start".into()),
        Some(checks) => {
            for c in checks {
                let mark = if c.ok { "✓" } else { "⚠" };
                lines.push(format!("  {mark} {:<12} {}", c.id, c.detail));
                if let (false, Some(fix)) = (c.ok, &c.fix) {
                    lines.push(format!("      → {fix}"));
                }
            }
        }
    }
    lines.push(String::new());
    lines.push("statusline 은 `rocky statusline doctor` 로 따로 본다.".into());
    lines.join("\n")
}
