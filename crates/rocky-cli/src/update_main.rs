//! `rocky update --main [SHA]` · `rocky update --release` — 이 기기를 릴리스 대신 GitHub main 을 직접 빌드해 쓰는
//! **main 빌드 모드**로 바꾸고 올리거나, 릴리스로 되돌린다. 순수 판정은 `rocky_core::main_build`, 여기는 git·cargo·bun·claude·agy 를
//! 부르는 배선이다.
//!
//! main 빌드 순서: 잠금 → `main-src` clone(처음만)·fetch → 원하는 커밋 → (마켓플레이스를 바꿔야 하면 확인) →
//! reset → release 빌드 + 웹 UI → `main.new` 에 모아 `main` 과 rename 으로 교체 → `current` → `main` →
//! 새 빌드로 데몬 재시작 → 플러그인 다시 깔기 → agy 번들 다시 깔기. 버전 문자열이 같아도(main 빌드끼리)
//! 커밋이 다르면 빌드한다 — 훅의 버전 비교로는 교체되지 않으므로 데몬은 직접 재시작한다.

use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use rocky_core::main_build::{
    self, BuildPlan, MarketplaceSource, PluginStep, ReleaseStep, CLONE_URL, MARKETPLACE, PLUGIN,
};

use crate::client::CliContext;
use crate::commands::run_claude;
use crate::config_cmd::data_home;

/// `$XDG_DATA_HOME/rocky/` 아래 자리.
struct Paths {
    root: PathBuf,
    src: PathBuf,
    out: PathBuf,
    nightly: PathBuf,
    current: PathBuf,
}

impl Paths {
    fn new() -> Self {
        let root = data_home().join("rocky");
        Paths {
            src: root.join("main-src"),
            out: root.join("main"),
            nightly: root.join(main_build::NIGHTLY),
            current: root.join("current"),
            root,
        }
    }
}

fn current_target() -> Option<String> {
    std::fs::read_link(Paths::new().current)
        .ok()
        .and_then(|l| l.to_str().map(str::to_string))
}

/// 이 기기가 main 빌드 모드인가 — `current` 링크가 `main` 을 가리킨다.
pub fn is_main_mode() -> bool {
    main_build::is_main_mode(current_target().as_deref())
}

/// 이 기기가 nightly 모드인가 — `current` 링크가 `nightly` 를 가리킨다.
pub fn is_nightly_mode() -> bool {
    main_build::is_nightly_mode(current_target().as_deref())
}

/// 동시에 두 빌드가 `main/` 을 바꾸지 않게 — 디렉터리 만들기는 원자적이다. 기기의 다른 main 빌드 스크립트와
/// 같은 이름을 써서 서로를 막는다. 안에 쥔 프로세스의 pid 를 남겨, Ctrl-C 로 끊긴 빌드(Drop 이 안 돈다)가 남긴
/// 잠금은 알아보고 넘겨받는다. pid 가 없는 잠금(다른 스크립트의 것)은 살아 있다고 본다.
struct Lock(PathBuf);

impl Lock {
    fn take(root: &Path) -> Result<Lock, String> {
        let path = root.join("main-sync.lock");
        for _ in 0..2 {
            match std::fs::create_dir(&path) {
                Ok(()) => {
                    let _ = std::fs::write(path.join("pid"), std::process::id().to_string());
                    return Ok(Lock(path));
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    let holder = std::fs::read_to_string(path.join("pid"))
                        .ok()
                        .and_then(|s| s.trim().parse::<u32>().ok());
                    match holder {
                        Some(pid) if !alive(pid) => {
                            let _ = std::fs::remove_dir_all(&path);
                        }
                        _ => {
                            return Err(format!(
                                "다른 main 빌드가 돌고 있다({}{}) — 끝난 뒤 다시. 아무것도 안 돌고 있으면 그 폴더를 지운다",
                                path.display(),
                                holder.map(|p| format!(", pid {p}")).unwrap_or_default()
                            ))
                        }
                    }
                }
                Err(e) => return Err(format!("잠금 {} 을 만들지 못했다: {e}", path.display())),
            }
        }
        Err(format!("잠금 {} 을 넘겨받지 못했다", path.display()))
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// 그 pid 의 프로세스가 살아 있나(`kill -0`).
fn alive(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// 자식 명령 — 바깥 git(훅·래퍼)의 `GIT_*` 가 새면 다른 레포를 건드리므로 지운다.
fn command(program: &str, args: &[&str], cwd: Option<&Path>) -> Command {
    let mut c = Command::new(program);
    c.args(args);
    if let Some(dir) = cwd {
        c.current_dir(dir);
    }
    for var in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
    ] {
        c.env_remove(var);
    }
    c
}

/// 실행하고 실패하면 무엇이 어디서 실패했는지. stdout 은 버리고(빌드 잡음) stderr 는 그대로 보인다.
fn run(program: &str, args: &[&str], cwd: Option<&Path>) -> Result<(), String> {
    let status = command(program, args, cwd)
        .stdout(Stdio::null())
        .status()
        .map_err(|e| format!("{program} 를 실행하지 못했다(PATH 확인): {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "{program} {} 실패 ({status}){}",
            args.join(" "),
            cwd.map(|d| format!(" — {}", d.display()))
                .unwrap_or_default()
        ))
    }
}

/// stdout 한 덩어리(앞뒤 공백 뺀 것).
fn output(program: &str, args: &[&str], cwd: Option<&Path>) -> Result<String, String> {
    let out = command(program, args, cwd)
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| format!("{program} 를 실행하지 못했다(PATH 확인): {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "{program} {} 실패 ({})",
            args.join(" "),
            out.status
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn has(program: &str) -> bool {
    command(program, &["--version"], None)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(7)]
}

/// 사람이 확인한다. `--yes` 면 묻지 않는다. 터미널이 아니면(훅·배치) 묻지 못하므로 멈춘다.
pub(crate) fn confirm(question: &str, yes: bool) -> Result<bool, String> {
    if yes {
        return Ok(true);
    }
    if !std::io::stdin().is_terminal() {
        return Err(format!(
            "{question}\n  확인이 필요하다 — 터미널에서 다시 실행하거나 --yes 를 붙인다"
        ));
    }
    print!("{question} [y/N] ");
    let _ = std::io::stdout().flush();
    let mut answer = String::new();
    std::io::stdin()
        .read_line(&mut answer)
        .map_err(|e| format!("답을 읽지 못했다: {e}"))?;
    Ok(matches!(answer.trim().to_lowercase().as_str(), "y" | "yes"))
}

/// `rocky-marketplace` 가 지금 가리키는 곳. claude 가 없거나 출력이 JSON 이 아니면 에러 — 모르는 건 바꾸지 않는다.
pub(crate) fn marketplace_source() -> Result<MarketplaceSource, String> {
    let json = output("claude", &["plugin", "marketplace", "list", "--json"], None)?;
    main_build::parse_marketplace(&json)
        .ok_or_else(|| format!("claude plugin marketplace list --json 을 읽지 못했다: {json}"))
}

/// 데몬 한 줄 — `v0.49.0 (abc1234)` 또는 `(없음)`.
fn daemon_label(ctx: &CliContext) -> String {
    crate::client::daemon_health(&ctx.base_url)
        .and_then(|h| {
            let v = h.version?;
            Some(format!(
                "v{}",
                rocky_core::version::label(&v, h.commit.as_deref())
            ))
        })
        .unwrap_or_else(|| "(없음)".into())
}

/// `link` 를 `target` 을 가리키는 심볼릭 링크로 바꿔 끼운다 — 임시 링크를 만든 뒤 rename 해서 읽는 쪽이 링크가 없는
/// 순간을 보지 않는다(부트스트랩과 같은 방식).
fn relink(link: &Path, target: &str) -> Result<(), String> {
    let tmp = link.with_extension(format!("tmp{}", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    std::os::unix::fs::symlink(target, &tmp)
        .and_then(|()| std::fs::rename(&tmp, link))
        .map_err(|e| format!("{} → {target} 링크를 걸지 못했다: {e}", link.display()))
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let dest = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &dest)?;
        } else {
            std::fs::copy(entry.path(), dest)?;
        }
    }
    Ok(())
}

/// 빌드한 것을 `main.new` 에 모은 뒤 `main` 과 바꿔 끼운다. 지금 도는 이 rocky 가 옛 `main/rocky` 여도 괜찮다 —
/// 디렉터리를 옮기고 지워도 실행 중인 파일은 끝까지 돈다.
fn install(p: &Paths, sha: &str) -> Result<(), String> {
    let stage = p.root.join("main.new");
    let old = p.root.join("main.old");
    let fail = |what: &str, e: std::io::Error| format!("{what} 실패: {e}");
    let _ = std::fs::remove_dir_all(&stage);
    std::fs::create_dir_all(&stage).map_err(|e| fail("main.new 만들기", e))?;
    for bin in ["rocky", "rockyd"] {
        std::fs::copy(p.src.join("target/release").join(bin), stage.join(bin))
            .map_err(|e| fail(&format!("{bin} 복사"), e))?;
    }
    copy_dir(&p.src.join("dist"), &stage.join("dist")).map_err(|e| fail("웹 UI dist 복사", e))?;
    let tui = p.src.join("tui/dist").join(crate::tui_cmd::TUI_SCRIPT);
    if tui.is_file() {
        std::fs::copy(&tui, stage.join(crate::tui_cmd::TUI_SCRIPT))
            .map_err(|e| fail("TUI 번들 복사", e))?;
    }
    std::fs::write(stage.join(".sha"), format!("{sha}\n")).map_err(|e| fail(".sha 쓰기", e))?;
    let _ = std::fs::remove_dir_all(&old);
    if p.out.exists() {
        std::fs::rename(&p.out, &old).map_err(|e| fail("옛 main 치우기", e))?;
    }
    std::fs::rename(&stage, &p.out).map_err(|e| fail("새 main 걸기", e))?;
    let _ = std::fs::remove_dir_all(&old);
    Ok(())
}

/// 원하는 커밋 — `pin` 이 있으면 origin/main 위의 그 커밋, 없으면 origin/main 최신.
fn resolve_want(src: &Path, pin: Option<&str>) -> Result<String, String> {
    let Some(pin) = pin else {
        return output("git", &["rev-parse", "origin/main"], Some(src));
    };
    let want = output(
        "git",
        &["rev-parse", "-q", "--verify", &format!("{pin}^{{commit}}")],
        Some(src),
    )
    .map_err(|_| format!("커밋 {pin} 을 찾지 못했다"))?;
    run(
        "git",
        &["merge-base", "--is-ancestor", &want, "origin/main"],
        Some(src),
    )
    .map_err(|_| format!("{want} 은 origin/main 위의 커밋이 아니다"))?;
    Ok(want)
}

/// `rocky update --main [SHA] [--force] [--yes]`.
pub fn update_main(
    ctx: &CliContext,
    pin: Option<&str>,
    force: bool,
    yes: bool,
) -> Result<(), String> {
    let p = Paths::new();
    let missing: Vec<&str> = ["git", "cargo", "bun", "claude"]
        .into_iter()
        .filter(|t| !has(t))
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "main 빌드에 필요한 것이 없다: {} (PATH 확인)",
            missing.join(", ")
        ));
    }
    std::fs::create_dir_all(&p.root)
        .map_err(|e| format!("{} 을 만들지 못했다: {e}", p.root.display()))?;
    let _lock = Lock::take(&p.root)?;

    if !p.src.join(".git").exists() {
        println!("main 을 처음 받는다 — {}", p.src.display());
        run(
            "git",
            &["clone", "--quiet", CLONE_URL, &p.src.to_string_lossy()],
            None,
        )?;
    }
    run("git", &["fetch", "-q", "origin", "main"], Some(&p.src))?;
    let want = resolve_want(&p.src, pin)?;
    let have = std::fs::read_to_string(p.out.join(".sha")).ok();
    let finished = std::fs::read_to_string(p.out.join(".finished")).ok();
    // TUI 를 모르던 옛 rocky 가 이 커밋을 빌드했으면 `tui.js` 없이 깔려 있다 — 원하는 커밋에 번들 스크립트가 있는데
    // 깔린 쪽에 번들이 없으면 다시 빌드한다(`main_build::plan`).
    let bundle_missing = !p.out.join(crate::tui_cmd::TUI_SCRIPT).is_file()
        && command(
            "git",
            &["cat-file", "-e", &format!("{want}:scripts/build-tui.ts")],
            Some(&p.src),
        )
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    let plan = main_build::plan(
        &want,
        have.as_deref(),
        finished.as_deref(),
        force,
        is_main_mode(),
        bundle_missing,
    );
    if plan == BuildPlan::Skip {
        println!("✓ 이미 main {} 빌드를 쓰고 있다", short(&want));
        return Ok(());
    }

    // 플러그인을 어떻게 맞출지 빌드 전에 정한다 — 마켓플레이스를 바꿔야 하면 몇 분짜리 빌드 전에 묻는다.
    let src = p
        .src
        .canonicalize()
        .unwrap_or_else(|_| p.src.clone())
        .to_string_lossy()
        .to_string();
    let source = marketplace_source()?;
    let step = main_build::plugin_step(&source, &src);
    if step == PluginStep::Switch {
        let question = format!(
            "플러그인 마켓플레이스 {MARKETPLACE} 를 GitHub 릴리스에서 이 기기의 main 빌드 폴더({src})로 바꾼다.\n  \
             플러그인을 뺐다 다시 깔고, 그 플러그인의 저장된 옵션·데이터는 지워진다(rocky 의 보드·워크로그는 \
             ~/.config/rocky 라 그대로). 진행할까?"
        );
        if !confirm(&question, yes)? {
            return Err("중단 — 아무것도 바꾸지 않았다".into());
        }
    }

    let before = daemon_label(ctx);
    if plan == BuildPlan::Build {
        println!(
            "main {} 을 빌드한다 — release 빌드라 몇 분 걸린다",
            short(&want)
        );
        run("git", &["reset", "-q", "--hard", &want], Some(&p.src))?;
        run(
            "cargo",
            &[
                "build",
                "--release",
                "--locked",
                "-q",
                "-p",
                "rocky-cli",
                "-p",
                "rockyd",
            ],
            Some(&p.src),
        )?;
        run("bun", &["install", "--frozen-lockfile"], Some(&p.src))?;
        run("bun", &["run", "build:ui"], Some(&p.src))?;
        // TUI 번들 — TUI 가 들어오기 전 커밋을 고정해 빌드할 때는 없다. 지난 빌드의 것은 먼저 지운다: `reset --hard` 는
        // gitignore 된 `tui/dist/` 를 남겨서, 안 그러면 다른 버전의 번들이 함께 깔린다.
        let _ = std::fs::remove_file(p.src.join("tui/dist").join(crate::tui_cmd::TUI_SCRIPT));
        if p.src.join("scripts/build-tui.ts").is_file() {
            run("bun", &["run", "build:tui"], Some(&p.src))?;
        }
        install(&p, &want)?;
    } else {
        // 빌드 뒤 단계가 실패했던 실행이거나 모드를 벗어났다가 같은 커밋으로 돌아온 것 — 빌드는 있다.
        println!(
            "main {} 빌드는 이미 있다 — 링크·데몬 재시작·플러그인만 마저 한다",
            short(&want)
        );
        let _ = std::fs::remove_file(p.out.join(".finished"));
    }
    relink(&p.current, "main")?;

    // 데몬 — main 빌드끼리는 버전 문자열이 같아 훅이 교체하지 않는다. 새 빌드로 직접 재시작하고, 재시작이
    // 같은 빌드(커밋까지)가 떴는지 확인한다.
    // `warnings` 는 실패 — 하나라도 있으면 끝까지 간 표식을 남기지 않는다. `notes` 는 의도대로 건너뛴 것(개발용
    // 마켓플레이스 보존 등)이라 성공으로 친다 — 실패로 세면 다음 실행마다 뒤 단계를 되풀이하고 끝나지 않는다.
    let mut warnings = Vec::new();
    let mut notes = Vec::new();
    let restarted = command(
        &p.out.join("rocky").to_string_lossy(),
        &["daemon", "restart"],
        None,
    )
    .status()
    .is_ok_and(|s| s.success());
    if !restarted {
        warnings.push("데몬 재시작 실패 — `rocky daemon status` 로 확인".to_string());
    }

    apply_plugin(&p.src, &step, &source, &src, &mut warnings, &mut notes);

    println!(
        "✓ main {} — 데몬 {before} → {}",
        short(&want),
        daemon_label(ctx)
    );
    println!("  열려 있는 세션은 /reload-plugins 또는 재시작해야 새 훅·커맨드가 적용된다.");
    for note in &notes {
        println!("  참고: {note}");
    }
    if warnings.is_empty() {
        // 끝까지 갔다는 표식 — 없으면 다음 실행이 빌드 없이 뒤 단계만 마저 한다(`BuildPlan::Finish`).
        let _ = std::fs::write(p.out.join(".finished"), format!("{want}\n"));
        Ok(())
    } else {
        Err(format!(
            "{}\n  고친 뒤 다시 `rocky update --main` — 빌드는 건너뛰고 남은 단계만 한다",
            warnings.join("\n")
        ))
    }
}

/// 바이너리를 바꾼 뒤 플러그인(훅·커맨드·스킬)과 Antigravity 번들을 `main-src` 의 지금 커밋으로 맞춘다 — main 빌드와
/// nightly 가 같이 쓴다. 실패는 `warnings`, 의도대로 건너뛴 것은 `notes`.
fn apply_plugin(
    main_src: &Path,
    step: &PluginStep,
    source: &MarketplaceSource,
    src: &str,
    warnings: &mut Vec<String>,
    notes: &mut Vec<String>,
) {
    match step {
        PluginStep::Reinstall => {
            let _ = run_claude(&["plugin", "uninstall", PLUGIN]);
            if let Err(e) = run_claude(&["plugin", "install", PLUGIN]) {
                warnings.push(format!("플러그인 다시 깔기 실패 — {e}"));
            }
        }
        PluginStep::Switch => {
            let switched = (if matches!(source, MarketplaceSource::Missing) {
                Ok(())
            } else {
                run_claude(&["plugin", "marketplace", "remove", MARKETPLACE])
            })
            .and_then(|()| run_claude(&["plugin", "marketplace", "add", src]))
            .and_then(|()| run_claude(&["plugin", "install", PLUGIN]));
            if let Err(e) = switched {
                warnings.push(format!("마켓플레이스 전환 실패 — {e}"));
            }
        }
        PluginStep::Leave(why) => notes.push(why.clone()),
    }

    // Antigravity 번들도 복사본이라 다시 깐다 — 없으면 건너뛴다.
    if has("agy") {
        let bundle = main_src.join("antigravity");
        let ok = command(
            "agy",
            &["plugin", "install", &bundle.to_string_lossy()],
            None,
        )
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
        if !ok {
            warnings.push("agy 번들 다시 깔기 실패".to_string());
        }
    }
}

/// `rocky update --check` 의 main 빌드판 — 모드, 깔린 main 빌드의 커밋, origin/main 최신을 나란히. 아무것도 바꾸지 않는다.
pub fn check_main(ctx: &CliContext) -> Result<(), String> {
    let p = Paths::new();
    let have = std::fs::read_to_string(p.out.join(".sha"))
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    let remote = output("git", &["ls-remote", CLONE_URL, "refs/heads/main"], None)?;
    let latest = remote.split_whitespace().next().unwrap_or("");
    println!(
        "{} · main 빌드 {} · origin/main {} · 데몬 {}",
        if is_main_mode() {
            "main 빌드 모드"
        } else {
            "릴리스 모드"
        },
        if have.is_empty() {
            "(없음)"
        } else {
            short(&have)
        },
        short(latest),
        daemon_label(ctx)
    );
    if !have.is_empty() && have == latest {
        println!("✓ 이미 최신이다");
    }
    Ok(())
}

/// `rocky update --release [--yes]` — main 빌드·nightly 모드에서 릴리스로 돌아간다. 실패할 수 있는 단계(판정·조회·마켓플레이스·
/// 설치)를 다 끝낸 뒤에야 `current` 를 풀고, 릴리스를 못 받으면 `main` 으로 되돌려 둔다 — 중간에 멈춰도 main 빌드는
/// 계속 돈다. `main/`·`main-src/` 는 남겨 둔다(다시 `--main` 으로 오면 빌드 없이 링크만 건다).
pub fn update_release(ctx: &CliContext, yes: bool) -> Result<(), String> {
    let p = Paths::new();
    // 되돌아갈 자리 — main 빌드 모드면 `main`, nightly 모드면 `nightly`.
    let Some(pinned) = current_target().filter(|t| main_build::is_pinned_mode(Some(t))) else {
        return Err("이 기기는 이미 릴리스 모드다 — 올리기는 `rocky update`".into());
    };
    // `--main` 과 같은 잠금 — 몇 분짜리 main 빌드와 동시에 돌면 서로 링크·마켓플레이스를 덮어 요청한 모드와 어긋난다.
    let _lock = Lock::take(&p.root)?;
    let src = p
        .src
        .canonicalize()
        .unwrap_or_else(|_| p.src.clone())
        .to_string_lossy()
        .to_string();
    let step = main_build::release_step(&marketplace_source()?, &src);
    if let ReleaseStep::Leave(why) = &step {
        return Err(why.clone());
    }
    let latest = crate::commands::latest_release()?;
    let question = format!(
        "릴리스 v{latest} 로 돌아간다 — 마켓플레이스 {MARKETPLACE} 를 GitHub 로 되돌려 플러그인을 다시 깔고(그 플러그인의 \
         저장된 옵션·데이터는 지워진다), 릴리스 바이너리로 데몬을 바꾼다. 진행할까?"
    );
    if !confirm(&question, yes)? {
        return Err("중단 — 아무것도 바꾸지 않았다".into());
    }
    let before = daemon_label(ctx);
    // 마켓플레이스를 빼면 그 플러그인도 같이 빠진다 — 다시 깔기 전에 빼는 건 GitHub 그대로일 때만.
    match step {
        ReleaseStep::FromFolder => {
            run_claude(&["plugin", "marketplace", "remove", MARKETPLACE])?;
            run_claude(&["plugin", "marketplace", "add", "minjun0219/rocky"])?;
        }
        ReleaseStep::Add => run_claude(&["plugin", "marketplace", "add", "minjun0219/rocky"])?,
        ReleaseStep::Refresh => {
            run_claude(&["plugin", "marketplace", "update", MARKETPLACE])?;
            let _ = run_claude(&["plugin", "uninstall", PLUGIN]);
        }
        ReleaseStep::Leave(_) => unreachable!("위에서 멈췄다"),
    }
    run_claude(&["plugin", "install", PLUGIN])?;
    let bootstrap = crate::commands::bootstrap_of(&latest)?;

    // 이제 `current` 를 푼다 — 그래야 새 플러그인의 부트스트랩이 main 빌드를 놓고 릴리스를 받는다(받으면 `current` 를
    // 그 버전으로 건다). 못 받았으면 main 으로 되돌려 두어 이 기기가 rocky 없이 남지 않게 한다.
    let _ = std::fs::remove_file(&p.current);
    let fetched = crate::commands::run_bootstrap(&bootstrap);
    let release = p.root.join(format!("v{latest}"));
    if fetched.is_err() || !release.join("rocky").is_file() {
        let _ = relink(&p.current, &pinned);
        return Err(format!(
            "릴리스 v{latest} 바이너리를 받지 못했다 — {pinned} 으로 되돌려 두었다(플러그인은 GitHub 마켓에서 깔렸다){}",
            fetched.err().map(|e| format!(": {e}")).unwrap_or_default()
        ));
    }
    relink(&p.current, &format!("v{latest}"))?;

    // 데몬 — 버전 문자열이 main 빌드와 같으면 훅이 바꾸지 않으므로 받은 바이너리로 직접 재시작하고, 최신 릴리스가
    // 떴는지 확인한다. 버전만 보면 재시작이 실패해 같은 버전의 main 빌드 데몬이 남아도 통과하므로, 재시작의 성패
    // (그 안에서 커밋까지 같은 빌드인지 본다)를 같이 본다.
    let restarted = command(
        &release.join("rocky").to_string_lossy(),
        &["daemon", "restart"],
        None,
    )
    .status()
    .is_ok_and(|s| s.success());
    let after = crate::client::daemon_health(&ctx.base_url).and_then(|h| h.version);
    if !restarted || after.as_deref() != Some(latest.as_str()) {
        return Err(format!(
            "데몬이 v{latest} 로 바뀌지 않았다 — 지금 {}. `rocky daemon status` 로 확인한 뒤 `rocky daemon restart`",
            daemon_label(ctx)
        ));
    }
    println!("✓ 릴리스 v{latest} — 데몬 {before} → {}", daemon_label(ctx));
    println!(
        "  {}는 남겨 두었다 — 다시 쓰려면 `rocky update --{pinned}`.",
        if pinned == main_build::NIGHTLY {
            p.nightly.display()
        } else {
            p.out.display()
        }
    );
    println!("  열려 있는 세션은 /reload-plugins 또는 재시작해야 새 훅·커맨드가 적용된다.");
    Ok(())
}

/// 원하는 nightly 의 (태그, 커밋) — `pin` 이 있으면 그 태그, 없으면 가장 최근 것. 태그는 `git ls-remote --tags` 로 찾는다
/// (GitHub API 예산을 쓰지 않는다).
fn resolve_nightly(pin: Option<&str>) -> Result<(String, String), String> {
    let remote = output(
        "git",
        &["ls-remote", "--tags", CLONE_URL, "refs/tags/v*-nightly.*"],
        None,
    )?;
    match pin {
        None => main_build::latest_nightly(&remote)
            .ok_or_else(|| format!("{CLONE_URL} 에 nightly 프리릴리스가 아직 없다")),
        Some(pin) => {
            let want = main_build::parse_nightly_tag(pin).ok_or_else(|| {
                format!("{pin} 은 nightly 태그(v<버전>-nightly.<YYYYMMDD>)가 아니다")
            })?;
            // 그 태그 줄만 남겨 같은 판정(annotated 의 `^{}`)을 탄다.
            let only: String = remote
                .lines()
                .filter(|l| {
                    l.split_whitespace().nth(1).is_some_and(|r| {
                        r == format!("refs/tags/{}", want.tag)
                            || r == format!("refs/tags/{}^{{}}", want.tag)
                    })
                })
                .map(|l| format!("{l}\n"))
                .collect();
            main_build::latest_nightly(&only)
                .ok_or_else(|| format!("nightly {} 를 찾지 못했다(지워졌거나 아직 없다)", want.tag))
        }
    }
}

/// 그 nightly 의 tarball 을 받아 체크섬을 확인하고 `nightly.new` 에 푼 뒤 `nightly` 와 바꿔 끼운다. 받기는 부트스트랩과 같은
/// curl·shasum·tar 다.
fn install_nightly(p: &Paths, tag: &str, sha: &str) -> Result<(), String> {
    let base = std::env::var("ROCKY_RELEASE_BASE")
        .ok()
        .filter(|b| !b.is_empty())
        .unwrap_or_else(|| format!("https://github.com/{}/releases/download", main_build::REPO));
    let asset = main_build::nightly_asset(tag);
    let tmp = p.root.join(format!("nightly.dl{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp)
        .map_err(|e| format!("{} 을 만들지 못했다: {e}", tmp.display()))?;
    let result = (|| {
        for name in [asset.as_str(), "SHA256SUMS"] {
            let url = format!("{base}/{tag}/{name}");
            run(
                "curl",
                &[
                    "-fsSL",
                    "--connect-timeout",
                    "10",
                    "--max-time",
                    "300",
                    "--retry",
                    "2",
                    "-o",
                    &tmp.join(name).to_string_lossy(),
                    &url,
                ],
                None,
            )
            .map_err(|e| format!("다운로드 실패 {url}: {e}"))?;
        }
        let sums = std::fs::read_to_string(tmp.join("SHA256SUMS"))
            .map_err(|e| format!("SHA256SUMS 를 읽지 못했다: {e}"))?;
        let line = sums
            .lines()
            .find(|l| l.trim_end().ends_with(&format!(" {asset}")))
            .ok_or_else(|| format!("SHA256SUMS 에 {asset} 이 없다"))?;
        std::fs::write(tmp.join("SHA256SUMS.one"), format!("{line}\n"))
            .map_err(|e| format!("체크섬 파일을 쓰지 못했다: {e}"))?;
        run(
            "shasum",
            &["-a", "256", "-c", "--status", "SHA256SUMS.one"],
            Some(&tmp),
        )
        .map_err(|_| format!("체크섬 불일치: {asset}"))?;
        let stage = p.root.join("nightly.new");
        let _ = std::fs::remove_dir_all(&stage);
        std::fs::create_dir_all(&stage)
            .map_err(|e| format!("{} 을 만들지 못했다: {e}", stage.display()))?;
        run(
            "tar",
            &[
                "-xzf",
                &tmp.join(&asset).to_string_lossy(),
                "-C",
                &stage.to_string_lossy(),
            ],
            None,
        )?;
        if !(stage.join("rocky").is_file() && stage.join("rockyd").is_file()) {
            return Err(format!("{asset} 에 rocky·rockyd 가 없다"));
        }
        std::fs::write(stage.join(".tag"), format!("{tag}\n"))
            .and_then(|()| std::fs::write(stage.join(".sha"), format!("{sha}\n")))
            .map_err(|e| format!("nightly 표식을 쓰지 못했다: {e}"))?;
        let old = p.root.join("nightly.old");
        let _ = std::fs::remove_dir_all(&old);
        if p.nightly.exists() {
            std::fs::rename(&p.nightly, &old)
                .map_err(|e| format!("옛 nightly 치우기 실패: {e}"))?;
        }
        std::fs::rename(&stage, &p.nightly).map_err(|e| format!("새 nightly 걸기 실패: {e}"))?;
        let _ = std::fs::remove_dir_all(&old);
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    result
}

/// `rocky update --nightly [TAG] [--force] [--yes]` — CI 가 낸 nightly 프리릴리스를 받아 이 기기를 nightly 모드로 바꾸고
/// 올린다. 빌드는 없다: tarball 을 받고, 플러그인은 `main-src` 를 그 커밋으로 맞춰 다시 깐다(`--main` 과 같은 마켓플레이스).
pub fn update_nightly(
    ctx: &CliContext,
    pin: Option<&str>,
    force: bool,
    yes: bool,
) -> Result<(), String> {
    let p = Paths::new();
    let missing: Vec<&str> = ["git", "curl", "shasum", "tar", "claude"]
        .into_iter()
        .filter(|t| !has(t))
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "nightly 받기에 필요한 것이 없다: {} (PATH 확인)",
            missing.join(", ")
        ));
    }
    std::fs::create_dir_all(&p.root)
        .map_err(|e| format!("{} 을 만들지 못했다: {e}", p.root.display()))?;
    let _lock = Lock::take(&p.root)?;

    let (tag, want) = resolve_nightly(pin)?;
    let have = std::fs::read_to_string(p.nightly.join(".tag")).ok();
    let finished = std::fs::read_to_string(p.nightly.join(".finished")).ok();
    let plan = main_build::plan(
        &tag,
        have.as_deref(),
        finished.as_deref(),
        force,
        is_nightly_mode(),
        false,
    );
    if plan == BuildPlan::Skip {
        println!("✓ 이미 {tag} 를 쓰고 있다");
        return Ok(());
    }

    if !p.src.join(".git").exists() {
        println!("플러그인 소스를 처음 받는다 — {}", p.src.display());
        run(
            "git",
            &["clone", "--quiet", CLONE_URL, &p.src.to_string_lossy()],
            None,
        )?;
    }
    run(
        "git",
        &[
            "fetch",
            "-q",
            "origin",
            &format!("refs/tags/{tag}:refs/tags/{tag}"),
        ],
        Some(&p.src),
    )?;

    // 마켓플레이스를 바꿔야 하면 받기 전에 묻는다(`--main` 과 같다).
    let src = p
        .src
        .canonicalize()
        .unwrap_or_else(|_| p.src.clone())
        .to_string_lossy()
        .to_string();
    let source = marketplace_source()?;
    let step = main_build::plugin_step(&source, &src);
    if step == PluginStep::Switch {
        let question = format!(
            "플러그인 마켓플레이스 {MARKETPLACE} 를 GitHub 릴리스에서 이 기기의 플러그인 소스 폴더({src})로 바꾼다.\n  \
             플러그인을 뺐다 다시 깔고, 그 플러그인의 저장된 옵션·데이터는 지워진다(rocky 의 보드·워크로그는 \
             ~/.config/rocky 라 그대로). 진행할까?"
        );
        if !confirm(&question, yes)? {
            return Err("중단 — 아무것도 바꾸지 않았다".into());
        }
    }

    let before = daemon_label(ctx);
    if plan == BuildPlan::Build {
        println!("{tag}({}) 를 받는다", short(&want));
        install_nightly(&p, &tag, &want)?;
    } else {
        println!("{tag} 는 이미 받아 두었다 — 링크·데몬 재시작·플러그인만 마저 한다");
        let _ = std::fs::remove_file(p.nightly.join(".finished"));
    }
    // 플러그인 파일이 바이너리와 같은 커밋이 되게 — main-src 를 그 nightly 커밋으로.
    run("git", &["reset", "-q", "--hard", &want], Some(&p.src))?;
    relink(&p.current, main_build::NIGHTLY)?;

    // nightly 끼리 버전이 다르면 훅도 바꾸지만, 같은 nightly 를 다시 받았을 때(--force)를 위해 직접 재시작한다.
    let mut warnings = Vec::new();
    let mut notes = Vec::new();
    let restarted = command(
        &p.nightly.join("rocky").to_string_lossy(),
        &["daemon", "restart"],
        None,
    )
    .status()
    .is_ok_and(|s| s.success());
    if !restarted {
        warnings.push("데몬 재시작 실패 — `rocky daemon status` 로 확인".to_string());
    }
    apply_plugin(&p.src, &step, &source, &src, &mut warnings, &mut notes);

    println!("✓ {tag} — 데몬 {before} → {}", daemon_label(ctx));
    println!("  열려 있는 세션은 /reload-plugins 또는 재시작해야 새 훅·커맨드가 적용된다.");
    for note in &notes {
        println!("  참고: {note}");
    }
    if warnings.is_empty() {
        let _ = std::fs::write(p.nightly.join(".finished"), format!("{tag}\n"));
        Ok(())
    } else {
        Err(format!(
            "{}\n  고친 뒤 다시 `rocky update --nightly` — 받기는 건너뛰고 남은 단계만 한다",
            warnings.join("\n")
        ))
    }
}

/// `rocky update --check` 의 nightly 판 — 깔린 nightly 와 최신 nightly 를 나란히. 아무것도 바꾸지 않는다.
pub fn check_nightly(ctx: &CliContext) -> Result<(), String> {
    let p = Paths::new();
    let have = std::fs::read_to_string(p.nightly.join(".tag"))
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    let latest = resolve_nightly(None).map(|(tag, _)| tag);
    println!(
        "{} · 받은 nightly {} · 최신 nightly {} · 데몬 {}",
        if is_nightly_mode() {
            "nightly 모드"
        } else if is_main_mode() {
            "main 빌드 모드"
        } else {
            "릴리스 모드"
        },
        if have.is_empty() { "(없음)" } else { &have },
        latest.as_deref().unwrap_or("(없음)"),
        daemon_label(ctx)
    );
    if latest.as_deref().is_ok_and(|l| l == have) {
        println!("✓ 이미 최신이다");
    }
    Ok(())
}
