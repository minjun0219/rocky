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
use crate::launchd::{is_launchd_registered, launchd_loaded};

const USAGE: &str = "usage: rocky config show [--json] | config init | config link | config path\n  show = 설치·설정 점검(설정 파일·데몬·launchd·statusline·보드·PATH), init = 기본 rocky.json 생성(있으면 그대로), link = ~/.local/bin/rocky 링크, path = 설정 파일 경로";

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
        "link" => {
            let home = std::env::var("HOME").map_err(|_| "HOME 이 없다".to_string())?;
            let target = data_home().join("rocky").join("current").join("rocky");
            printer.line(&link_cli(
                &Path::new(&home).join(".local").join("bin"),
                &target,
            )?);
            if !local_bin_on_path() {
                printer.line("  ~/.local/bin 이 PATH 에 없다 — 셸 rc 에: export PATH=\"$HOME/.local/bin:$PATH\"");
            }
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

/// `<bin_dir>/rocky` → `target` 심볼릭 링크. 이미 우리 링크면 그대로, 남의 실제 파일이면 거절.
pub fn link_cli(bin_dir: &Path, target: &Path) -> Result<String, String> {
    let cli = bin_dir.join("rocky");
    if let Ok(existing) = std::fs::read_link(&cli) {
        if existing == target {
            return Ok(format!(
                "= {} → {} 이미 있음",
                cli.display(),
                target.display()
            ));
        }
    } else if cli.exists() {
        return Err(format!(
            "{} 이 링크가 아닌 파일이라 건드리지 않는다 — 치우고 다시 부른다",
            cli.display()
        ));
    }
    std::fs::create_dir_all(bin_dir)
        .map_err(|e| format!("{} 생성 실패: {e}", bin_dir.display()))?;
    let _ = std::fs::remove_file(&cli);
    std::os::unix::fs::symlink(target, &cli)
        .map_err(|e| format!("{} 링크 실패: {e}", cli.display()))?;
    Ok(format!("✓ {} → {}", cli.display(), target.display()))
}

/// 부트스트랩과 같은 규칙의 데이터 홈 — `$XDG_DATA_HOME` > `~/.local/share`.
fn data_home() -> PathBuf {
    std::env::var("XDG_DATA_HOME")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| expand_tilde("~/.local/share"))
}

/// 셸이 실제로 실행할 수 있는 파일인가 — 실행 비트가 없는 `rocky` 는 PATH 에 있어도 안 불린다.
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

/// PATH 디렉터리에서 실행 가능한 `rocky` 를 찾는다.
fn cli_on_path() -> Option<String> {
    let path = std::env::var("PATH").ok()?;
    path.split(':')
        .map(|d| Path::new(d).join("rocky"))
        .find(|p| is_executable(p))
        .map(|p| p.to_string_lossy().to_string())
}

fn local_bin_on_path() -> bool {
    let Ok(home) = std::env::var("HOME") else {
        return false;
    };
    let local_bin = Path::new(&home).join(".local").join("bin");
    std::env::var("PATH")
        .ok()
        .is_some_and(|p| p.split(':').any(|d| Path::new(d) == local_bin))
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
        install_current: std::fs::read_link(data_home().join("rocky").join("current"))
            .ok()
            .map(|p| p.to_string_lossy().to_string()),
        daemon,
        launchd_registered: is_launchd_registered(),
        launchd_loaded: launchd_loaded(),
        statusline_command,
        statusline_script,
        cwd,
        repo_key,
        boards,
        cli_on_path: cli_on_path(),
        cli_link: std::fs::read_link(expand_tilde("~/.local/bin/rocky"))
            .ok()
            .map(|p| p.to_string_lossy().to_string()),
        local_bin_on_path: local_bin_on_path(),
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
