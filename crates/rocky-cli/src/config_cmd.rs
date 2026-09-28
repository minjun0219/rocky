//! `rocky config` — 설치·설정 점검과 기본 파일 생성. 판정은 `rocky_core::setup`, 여기는 재료
//! 수집과 파일 쓰기뿐이다. `/rocky:config` 커맨드가 이 출력을 보고 빠진 것을 채운다.

use std::path::{Path, PathBuf};

use rocky_core::config::{expand_tilde, user_config_path, TodoConfig};
use rocky_core::setup::{
    build_report, default_config_json, render_report, ConfigFileState, DaemonState, SetupInput,
};
use rocky_core::statusline::BoardLocation;
use serde_json::Value;

use crate::client::{daemon_health, request_value, CliContext};
use crate::commands::Printer;
use crate::context::infer_board_key;
use crate::launchd::is_launchd_registered;

const USAGE: &str = "usage: rocky config show [--json] | config init | config path\n  show = 설치·설정 점검(설정 파일·데몬·launchd·statusline·보드), init = 기본 rocky.json 생성(있으면 그대로), path = 설정 파일 경로";

pub fn cmd_config(
    ctx: &CliContext,
    rest: &[String],
    todo: &TodoConfig,
    printer: &Printer,
) -> Result<(), String> {
    match rest.first().map(String::as_str).unwrap_or("show") {
        "show" => {
            let input = gather(ctx, todo);
            let report = build_report(&input);
            let raw = serde_json::to_value(&report).unwrap_or(Value::Null);
            printer.emit(&raw, || render_report(&report));
            Ok(())
        }
        "init" => {
            printer.line(&init_config_file(&user_config_path())?);
            Ok(())
        }
        "path" => {
            printer.line(&user_config_path().to_string_lossy());
            Ok(())
        }
        _ => Err(USAGE.into()),
    }
}

/// 기본 `rocky.json` 을 **없을 때만** 만든다. 있으면 손대지 않고 그 사실을 돌려준다.
pub fn init_config_file(path: &Path) -> Result<String, String> {
    if path.exists() {
        return Ok(format!("= {} 이미 있음 — 그대로 둔다", path.display()));
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{} 생성 실패: {e}", dir.display()))?;
    }
    std::fs::write(path, default_config_json())
        .map_err(|e| format!("{} 쓰기 실패: {e}", path.display()))?;
    Ok(format!(
        "✓ {} 생성 (expose off · sessionSummary on)",
        path.display()
    ))
}

/// 점검 재료 수집 — 전부 fail-open. 데몬이 없거나 파일이 없으면 그 자리만 비운다.
fn gather(ctx: &CliContext, todo: &TodoConfig) -> SetupInput {
    let config_path = user_config_path();
    let config = config_file_state(&config_path);
    let daemon = daemon_health(&ctx.base_url).map(|h| DaemonState {
        version: h.version,
        pid: h.pid,
    });
    let (statusline_command, statusline_script) = claude_statusline();
    let cwd = std::env::current_dir()
        .ok()
        .map(|p| p.to_string_lossy().to_string());
    // 레포 밖(홈 등)이면 보드 판정을 건너뛴다 — 디렉터리 이름이 곧 보드는 아니다.
    let repo_key =
        crate::context::git(&["rev-parse", "--show-toplevel"]).map(|_| infer_board_key());
    let boards = if daemon.is_some() {
        request_value(ctx, "GET", "/api/boards", None)
            .ok()
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default()
            .iter()
            .filter_map(|b| {
                Some(BoardLocation {
                    key: b.get("key")?.as_str()?.to_string(),
                    path: b.get("path").and_then(Value::as_str).map(str::to_string),
                })
            })
            .collect()
    } else {
        Vec::new()
    };
    SetupInput {
        config,
        todo: todo.clone(),
        port: ctx.port,
        cli_version: env!("CARGO_PKG_VERSION").to_string(),
        install_current: std::fs::read_link(expand_tilde("~/.local/share/rocky/current"))
            .ok()
            .map(|p| p.to_string_lossy().to_string()),
        daemon,
        launchd_registered: is_launchd_registered(),
        statusline_command,
        statusline_script,
        cwd,
        repo_key,
        boards,
    }
}

fn config_file_state(path: &Path) -> ConfigFileState {
    let text = std::fs::read_to_string(path).ok();
    ConfigFileState {
        path: path.to_string_lossy().to_string(),
        exists: text.is_some(),
        valid_json: text
            .as_deref()
            .map(|t| serde_json::from_str::<Value>(t).is_ok())
            .unwrap_or(false),
    }
}

/// `~/.claude/settings.json` 의 `statusLine.command` 와, 그게 실행 파일 경로 하나면 그 본문.
/// `cc-usage statusline` 처럼 인자가 붙은 명령은 본문을 읽지 않는다 — 첫 토큰이 파일이면 그것만.
fn claude_statusline() -> (Option<String>, Option<String>) {
    let settings = expand_tilde("~/.claude/settings.json");
    let command = std::fs::read_to_string(settings)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| {
            v.get("statusLine")?
                .get("command")?
                .as_str()
                .map(str::to_string)
        });
    let script = command.as_deref().and_then(|c| {
        let first = c.split_whitespace().next()?;
        let p: PathBuf = expand_tilde(first);
        std::fs::read_to_string(p).ok()
    });
    (command, script)
}
