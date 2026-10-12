//! `rocky tui [--board K] [--actor A]` — 보드를 터미널에 띄운다(cmux 에서 웹 패널 대신). 화면은 TypeScript(Ink) 번들
//! `tui.js` 라, 실행 파일 옆의 그 파일을 `bun --no-install` 로 **exec** 한다(이 프로세스가 bun 으로 바뀐다). 포트·보드는
//! 여기서 정해 넘기고, actor 는 `--actor` 를 준 때만 넘긴다 — 없으면 TUI 가 `ROCKY_TODO_ACTOR` 나 웹과 같은 기본값을
//! 쓴다(CLI 의 기본 actor 는 `agent` 라 사람이 쓰는 화면에는 맞지 않다).

use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::client::CliContext;

/// TUI 번들 파일 이름 — 릴리스 tarball·`rocky update --main` 이 바이너리 옆에 둔다.
pub const TUI_SCRIPT: &str = "tui.js";

/// `bun` 에 넘길 인자. `--no-install` — `node_modules` 없는 자리에서 모르는 import 를 Bun 이 네트워크로 받지 않게.
pub fn tui_args(script: &str, port: u16, board: &str, actor: Option<&str>) -> Vec<String> {
    let mut args = vec![
        "--no-install".to_string(),
        script.to_string(),
        "--port".to_string(),
        port.to_string(),
        "--board".to_string(),
        board.to_string(),
    ];
    if let Some(actor) = actor.map(str::trim).filter(|a| !a.is_empty()) {
        args.push("--actor".to_string());
        args.push(actor.to_string());
    }
    args
}

/// 실행 파일(링크면 실제 파일) 옆의 `tui.js`.
fn script_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let exe = std::fs::canonicalize(&exe).unwrap_or(exe);
    Some(exe.parent()?.join(TUI_SCRIPT))
}

/// # Errors
/// `tui.js`·`bun` 이 없거나 데몬을 못 띄우면 무엇이 없는지 한 줄로. exec 가 성공하면 돌아오지 않고, exec 자체가
/// 실패하면 여기서 알리고 끝낸다 — 사용 기록은 이미 남겼으니 호출자가 한 번 더 남기지 않게.
pub fn cmd_tui(ctx: &CliContext, board: &str, actor: Option<&str>) -> Result<(), String> {
    let script = script_path().filter(|p| p.is_file()).ok_or_else(|| {
        format!(
            "{TUI_SCRIPT} 가 실행 파일 옆에 없다({}) — 릴리스 tarball 과 `rocky update --main` 이 함께 깐다. 레포에서는 `bun run tui`",
            script_path()
                .and_then(|p| p.parent().map(|d| d.display().to_string()))
                .unwrap_or_default()
        )
    })?;
    let has_bun = Command::new("bun")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if !has_bun {
        return Err("TUI 는 bun 으로 돈다 — bun 이 PATH 에 없다(https://bun.sh)".into());
    }
    crate::client::ensure_daemon(ctx)?;
    // exec 하면 이 프로세스가 bun 으로 바뀌어 뒤에서는 기록하지 못한다 — 넘기기 전에 남긴다.
    crate::usage_cmd::record(
        rocky_core::usage::UsageSource::Cli,
        "rocky tui",
        true,
        None,
        None,
    );
    let error = Command::new("bun")
        .args(tui_args(&script.to_string_lossy(), ctx.port, board, actor))
        .exec();
    eprintln!("bun 을 실행하지 못했다: {error}");
    std::process::exit(1)
}
