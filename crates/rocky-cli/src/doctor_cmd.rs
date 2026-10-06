//! `rocky doctor` — 설치·설정(`rocky config show` 와 같은 점검)과 실행 상태(PR 감시·기본 브랜치 검증·세션 전달·rc)를
//! 한 번에 본다. 판정은 `rocky_core::setup`·`rocky_core::doctor`, 여기는 재료 수집뿐이다. 읽기만 한다 — 데몬이 꺼져
//! 있어도 띄우지 않고(실행 상태는 "닿지 못함" 으로 남긴다) 고치는 명령은 안내로만 낸다.

use rocky_core::config::TodoConfig;
use rocky_core::doctor::{render, runtime_checks, RuntimeInput};
use rocky_core::setup::build_report;
use serde_json::{json, Value};

use crate::client::{request_value, CliContext};
use crate::commands::Printer;
use crate::config_cmd::gather;

pub fn cmd_doctor(ctx: &CliContext, todo: &TodoConfig, printer: &Printer) -> Result<(), String> {
    let input = gather(ctx, todo);
    // 데몬이 떠 있을 때만 묻는다 — `request_value` 는 꺼진 데몬을 띄우므로, 점검이 데몬을 띄우지 않게 먼저 거른다.
    let daemon_up = input.daemon.is_some();
    let setup = build_report(&input);
    let runtime = if daemon_up {
        let get = |path: &str| request_value(ctx, "GET", path, None).ok();
        get("/api/health").map(|health| {
            runtime_checks(
                &RuntimeInput {
                    health,
                    verify: get("/api/verify"),
                    // 로컬 전용 라우트 — 거절(403)이면 본문이 `{ "error" }` 라 건너뛴다.
                    deliveries: get("/api/deliveries").filter(|v| v.get("error").is_none()),
                    rc: get("/api/rc/servers").filter(|v| v.get("error").is_none()),
                },
                chrono::Utc::now(),
            )
        })
    } else {
        None
    };
    let raw = json!({
        "setup": setup,
        "runtime": runtime.as_ref().map_or(Value::Null, |c| json!(c)),
    });
    printer.emit(&raw, || render(&setup, runtime.as_deref()));
    Ok(())
}
