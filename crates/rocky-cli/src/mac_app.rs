//! 릴리스·nightly tarball 에 든 `Rocky.app` 을 기기에 깐다 — `rocky update` 가 바이너리를 받은 뒤 부른다. 판정은
//! `rocky_core::mac_app`, 여기는 ditto·codesign·pgrep·open 을 부르는 배선이다(규칙은 `docs/features/mac.md`).
//!
//! 순서: 받은 폴더의 `Rocky.app` 버전과 깔린 것을 견준다 → 같은 폴더의 `.Rocky.app.new` 로 복사 → 격리 속성 지우기 →
//! (키체인에 인증서가 있으면) 그 인증서로 다시 서명하고 검증 → 돌고 있으면 내린다 → rename 으로 바꿔 끼운다 → 돌고 있었거나
//! 처음 깔았으면 띄운다. 검증에 실패하면 깔린 앱을 건드리지 않는다.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use rocky_core::mac_app::{self, APP, PROCESS_PATTERN};

fn quiet(program: &str, args: &[&str]) -> Command {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    cmd
}

fn ok(program: &str, args: &[&str]) -> bool {
    quiet(program, args).status().is_ok_and(|s| s.success())
}

fn must(program: &str, args: &[&str]) -> Result<(), String> {
    let out = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("{program} 를 실행하지 못했다(PATH 확인): {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{program} {} 실패 ({}): {}",
            args.join(" "),
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

fn installed_version(app: &Path) -> Option<String> {
    std::fs::read_to_string(app.join("Contents/Info.plist"))
        .ok()
        .and_then(|xml| mac_app::plist_version(&xml))
}

/// `/Applications` 에 쓸 수 있나 — 권한 비트로는 uid·ACL 을 다 못 보니 빈 폴더를 만들어 본다.
fn writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".rocky-probe-{}", std::process::id()));
    let made = std::fs::create_dir(&probe).is_ok();
    let _ = std::fs::remove_dir(&probe);
    made
}

fn running() -> bool {
    ok("pgrep", &["-f", PROCESS_PATTERN])
}

/// 돌고 있는 앱을 SIGTERM 으로 내리고 5초까지 기다린다 — 앱은 아무것도 저장하지 않는 데몬의 클라이언트다.
/// AppleScript `quit` 은 자동화 권한 창을 띄워서 쓰지 않는다.
fn stop() -> bool {
    let _ = quiet("pkill", &["-TERM", "-f", PROCESS_PATTERN]).status();
    let until = Instant::now() + Duration::from_secs(5);
    while Instant::now() < until {
        if !running() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    !running()
}

/// `from` 폴더(받아 푼 tarball)의 `Rocky.app` 을 깐다. 앱이 없는 tarball(맥 앱을 넣기 전 릴리스)이거나 macOS 가 아니면
/// `Ok(None)`, 그대로 두거나 깔았으면 사람에게 보일 한 줄.
pub fn sync(from: &Path, force: bool) -> Result<Option<String>, String> {
    if !cfg!(target_os = "macos") {
        return Ok(None);
    }
    let src = from.join(APP);
    let Some(bundled) = installed_version(&src) else {
        return Ok(None);
    };
    let home = PathBuf::from(std::env::var("HOME").map_err(|_| "HOME 이 없다".to_string())?);
    let system = Path::new("/Applications").join(APP);
    let user = home.join("Applications").join(APP);
    let dest = mac_app::choose_dest(
        &home,
        system.exists(),
        user.exists(),
        writable(Path::new("/Applications")),
    );
    let installed = installed_version(&dest);
    if !mac_app::needs_install(installed.as_deref(), &bundled, force) {
        return Ok(Some(format!("맥 앱 {bundled} 그대로 — {}", dest.display())));
    }
    let parent = dest
        .parent()
        .ok_or_else(|| format!("{} 의 상위 폴더가 없다", dest.display()))?;
    std::fs::create_dir_all(parent)
        .map_err(|e| format!("{} 을 만들지 못했다: {e}", parent.display()))?;

    let stage = parent.join(format!(".{APP}.new"));
    let old = parent.join(format!(".{APP}.old"));
    let _ = std::fs::remove_dir_all(&stage);
    let staged = (|| {
        let stage_s = stage.to_string_lossy();
        // ditto 는 확장 속성·서명을 그대로 옮긴다.
        must("ditto", &[&src.to_string_lossy(), &stage_s])?;
        // curl 로 받은 tarball 은 격리 속성이 없지만, 손으로 받은 파일에서 왔을 때 Gatekeeper 가 막지 않게.
        let _ = quiet("xattr", &["-dr", "com.apple.quarantine", &stage_s]).status();
        let listing = Command::new("security")
            .args(["find-identity", "-p", "codesigning"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default();
        let env = std::env::var("ROCKY_MAC_SIGN_IDENTITY").ok();
        let identity = mac_app::sign_identity(env.as_deref(), &listing);
        let signed = match &identity {
            Some(id) => {
                must(
                    "codesign",
                    &["--force", "--sign", id, "--timestamp=none", &stage_s],
                )
                .map_err(|e| {
                    format!(
                        "'{id}' 로 서명하지 못했다 — `security find-identity -p codesigning` 으로 이름을 확인하거나 \
                         ROCKY_MAC_SIGN_IDENTITY=- 로 CI 의 ad-hoc 서명을 그대로 쓴다: {e}"
                    )
                })?;
                id.clone()
            }
            None => "ad-hoc(CI)".to_string(),
        };
        must("codesign", &["--verify", "--strict", &stage_s])?;
        Ok::<String, String>(signed)
    })();
    let signed = match staged {
        Ok(s) => s,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&stage);
            return Err(format!("맥 앱을 깔지 않았다(깔린 앱은 그대로) — {e}"));
        }
    };

    let was_running = running();
    if was_running && !stop() {
        let _ = std::fs::remove_dir_all(&stage);
        return Err(format!(
            "돌고 있는 맥 앱이 5초 안에 내려가지 않아 바꾸지 않았다 — 앱을 끈 뒤 다시 `rocky update` ({})",
            dest.display()
        ));
    }
    let _ = std::fs::remove_dir_all(&old);
    if dest.exists() {
        std::fs::rename(&dest, &old)
            .map_err(|e| format!("옛 맥 앱 치우기 실패 {}: {e}", dest.display()))?;
    }
    if let Err(e) = std::fs::rename(&stage, &dest) {
        // 바꿔 끼우기에 실패하면 옛 앱을 되돌려 둔다.
        let _ = std::fs::rename(&old, &dest);
        return Err(format!("새 맥 앱 걸기 실패 {}: {e}", dest.display()));
    }
    let _ = std::fs::remove_dir_all(&old);

    let note = if !mac_app::should_launch(was_running, installed.is_some()) {
        " · 꺼져 있던 앱이라 띄우지 않았다"
    } else if ok("open", &[&dest.to_string_lossy()]) {
        ""
    } else {
        " · 띄우지 못했다 — Finder 에서 연다"
    };
    Ok(Some(format!(
        "✓ 맥 앱 {} → {bundled} — {} (서명: {signed}){note}",
        installed.as_deref().unwrap_or("(없음)"),
        dest.display(),
    )))
}

/// [`sync`] 를 돌고 결과를 찍는다. 맥 앱 실패는 바이너리·데몬 업데이트를 되돌리지 않으니 경고로만 남긴다.
pub fn sync_and_report(from: &Path, force: bool) {
    match sync(from, force) {
        Ok(Some(line)) => println!("{line}"),
        Ok(None) => {}
        Err(e) => eprintln!("⚠ {e}"),
    }
}
