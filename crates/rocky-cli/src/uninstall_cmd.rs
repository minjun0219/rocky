//! `rocky uninstall [--purge] [--dry-run] [--yes]` — rocky 가 이 기기에 깐 것을 걷어낸다. 무엇을 지우고 남길지는
//! `rocky_core::uninstall`, 여기는 디스크·launchd·claude 를 읽고 부르는 배선이다.
//!
//! launchd 는 bootout 하나로 내린다(`launchd::uninstall_launchd`) — 보안 솔루션(EDR)이 악성코드로 본 흐름은 plist 를
//! 쓰고 bootstrap 하는 쪽이라(2026-10-19) 여기서는 그 둘을 하지 않는다. 데몬은 pid 로만 내린다.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rocky_core::config::{data_home, expand_tilde};
use rocky_core::main_build::{MarketplaceSource, MARKETPLACE, PLUGIN};
use rocky_core::uninstall::{self, DataDir, Step, UninstallInput};

use crate::client::CliContext;
use crate::commands::run_claude;
use crate::flags::ParsedFlags;

/// 데몬이 SIGTERM 뒤 포트를 놓기를 기다리는 시간.
const STOP_WAIT: Duration = Duration::from_secs(10);

pub fn cmd_uninstall(
    ctx: &CliContext,
    flags: &ParsedFlags,
    expose_tailscale: bool,
) -> Result<(), String> {
    let purge = flags.bool_flag("purge");
    let plan = uninstall::plan(&gather(ctx, purge, expose_tailscale));
    println!("{}", uninstall::render_plan(&plan));
    if flags.bool_flag("dry-run") {
        return Ok(());
    }
    // 이 세션의 다음 훅(Stop 의 log-turn)이 바이너리가 없는 걸 보고 다시 받아 데몬을 띄운다 — 지운 것이 곧 되살아난다.
    if std::env::var("CLAUDECODE").is_ok_and(|v| v == "1") {
        return Err(
            "Claude Code 세션 안에서는 걷어내지 않는다 — 이 세션의 훅이 바이너리를 다시 받는다. 세션을 모두 닫고 터미널에서 `rocky uninstall`"
                .into(),
        );
    }
    let question = if purge {
        "설정·보드·워크로그까지 지운다 — 되돌릴 수 없다. 진행할까?"
    } else {
        "진행할까?"
    };
    if !crate::update_main::confirm(question, flags.bool_flag("yes"))? {
        return Err("중단 — 아무것도 바꾸지 않았다".into());
    }
    let mut failed = 0;
    for step in &plan.steps {
        match run_step(ctx, step) {
            Ok(done) => println!("✓ {done}"),
            // 데몬이 남았는데 폴더(DB)를 지우면 도는 데몬 밑에서 파일이 사라진다 — 여기서 멈춘다.
            Err(e) if matches!(step, Step::Launchd(_) | Step::StopDaemon) => {
                return Err(format!(
                    "⚠ {e}\n데몬이 남아 있어 폴더는 지우지 않았다 — 데몬을 내린 뒤 다시 `rocky uninstall`"
                ));
            }
            Err(e) => {
                failed += 1;
                println!("⚠ {e}");
            }
        }
    }
    if failed > 0 {
        return Err(format!(
            "{failed}개 단계가 끝나지 않았다 — 위 ⚠ 를 보고 손으로 마저 하거나 다시 `rocky uninstall`"
        ));
    }
    println!("✓ rocky 를 걷어냈다");
    Ok(())
}

fn gather(ctx: &CliContext, purge: bool, tailscale: bool) -> UninstallInput {
    let home = std::env::var("HOME").map(PathBuf::from).unwrap_or_default();
    let install_dir = data_home().join("rocky");
    let link = expand_tilde("~/.local/bin/rocky");
    let cli_link = std::fs::symlink_metadata(&link).ok().map(|meta| {
        let target = meta
            .file_type()
            .is_symlink()
            .then(|| std::fs::read_link(&link).ok())
            .flatten()
            .map(|t| match link.parent() {
                Some(parent) if t.is_relative() => parent.join(t),
                _ => t,
            })
            .map(|t| real(&t));
        (link.clone(), target)
    });
    let worklog = std::env::var("ROCKY_WORKLOG_DIR")
        .ok()
        .filter(|d| !d.trim().is_empty())
        .map(|d| expand_tilde(d.trim()))
        .unwrap_or_else(rocky_core::worklog::default_worklog_root);
    let mut data_dirs: Vec<PathBuf> =
        vec![expand_tilde("~/.config/rocky"), ctx.dir.clone(), worklog];
    data_dirs.extend(crate::usage_cmd::usage_dir());
    let mut data_dirs: Vec<PathBuf> = data_dirs
        .into_iter()
        .filter(|p| p.is_dir())
        .map(|p| real(&p))
        .collect();
    data_dirs.sort();
    data_dirs.dedup();
    let data_dirs = data_dirs
        .into_iter()
        .map(|path| DataDir {
            repo: in_repo(&path),
            path,
        })
        .collect();
    let (command, script) = crate::config_cmd::claude_statusline();
    UninstallInput {
        home,
        install_dir_exists: install_dir.exists(),
        install_dir_repo: in_repo(&install_dir),
        install_dir,
        cli_link,
        plist: crate::launchd::registered_plist(),
        purge,
        data_dirs,
        statusline_wired: rocky_core::setup::statusline_wired(
            command.as_deref(),
            script.as_deref(),
        ),
        tailscale,
        rc: ctx.dir.join("rc").is_dir(),
    }
}

/// canonicalize 한 경로 — 없거나 못 풀면 그대로(그런 경로는 `..` 가 남아 `removable` 이 거절한다).
fn real(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// 그 폴더나 조상 어딘가에 `.git` 이 있다 — 레포 안의 폴더는 `remove_dir_all` 하지 않는다.
fn in_repo(path: &Path) -> bool {
    path.ancestors().any(|dir| dir.join(".git").exists())
}

fn run_step(ctx: &CliContext, step: &Step) -> Result<String, String> {
    match step {
        Step::Plugin => remove_plugin(),
        Step::Launchd(_) => {
            let done = crate::launchd::uninstall_launchd();
            if crate::launchd::job_loaded() {
                return Err(format!(
                    "launchd 가 아직 job 을 쥐고 있다 — `launchctl print gui/$(id -u)/{}` 로 본다",
                    rocky_core::config::launchd_label()
                ));
            }
            Ok(done.trim_start_matches("✓ ").to_string())
        }
        Step::StopDaemon => stop_daemon(ctx),
        Step::RemoveLink(path) => std::fs::remove_file(path)
            .map(|()| format!("{} 링크를 지웠다", path.display()))
            .map_err(|e| format!("{} 를 지우지 못했다: {e}", path.display())),
        Step::RemoveDir(path) => remove_dir(path),
    }
}

/// 플러그인 → 마켓플레이스. 마켓플레이스가 등록돼 있지 않으면 플러그인도 없다고 본다.
fn remove_plugin() -> Result<String, String> {
    let probe = std::process::Command::new("claude")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    if matches!(&probe, Err(e) if e.kind() == std::io::ErrorKind::NotFound) {
        return Ok("claude CLI 가 없다 — 플러그인 단계는 건너뛴다".into());
    }
    let source = crate::update_main::marketplace_source()?;
    if source == MarketplaceSource::Missing {
        return Ok(format!("마켓플레이스 {MARKETPLACE} 는 이미 없다"));
    }
    // 플러그인만 먼저 빠져 있을 수 있다 — 그 실패는 마켓플레이스 제거로 판정한다.
    let plugin = run_claude(&["plugin", "uninstall", PLUGIN]);
    run_claude(&["plugin", "marketplace", "remove", MARKETPLACE])?;
    Ok(match plugin {
        Ok(()) => format!("플러그인 {PLUGIN} 와 마켓플레이스 {MARKETPLACE} 를 뺐다"),
        Err(_) => format!("마켓플레이스 {MARKETPLACE} 를 뺐다(플러그인은 이미 없었다)"),
    })
}

/// `/api/health` 가 알려 준 pid(없으면 `daemon.pid`)로만 내린다 — 이름 패턴으로 찾지 않는다.
fn stop_daemon(ctx: &CliContext) -> Result<String, String> {
    let Some(health) = crate::client::daemon_health(&ctx.base_url) else {
        return Ok("데몬은 떠 있지 않다".into());
    };
    let mut down = crate::client::stop_daemon(ctx, health.pid);
    // `stop_daemon` 은 3초쯤만 본다 — 끝내는 중인 데몬(SSE 정리·DB 닫기)을 조금 더 기다린다.
    let deadline = std::time::Instant::now() + STOP_WAIT;
    while !down && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(200));
        down = crate::client::daemon_health(&ctx.base_url).is_none();
    }
    let pid = health
        .pid
        .map(|p| format!("pid {p}"))
        .unwrap_or_else(|| "pid 파일".into());
    if down {
        Ok(format!("데몬({pid})을 내렸다"))
    } else {
        Err(format!(
            "데몬({pid})이 내려가지 않았다 — `rocky daemon status` 로 본다"
        ))
    }
}

fn remove_dir(path: &Path) -> Result<String, String> {
    match std::fs::remove_dir_all(path) {
        Ok(()) => Ok(format!("{} 를 지웠다", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Ok(format!("{} 는 이미 없다", path.display()))
        }
        Err(e) => Err(format!("{} 를 지우지 못했다: {e}", path.display())),
    }
}
