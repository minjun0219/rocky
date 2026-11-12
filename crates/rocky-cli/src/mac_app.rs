//! `rocky app [--quit]` — 맥 앱(`Rocky.app`)을 필요할 때만 띄우고 내린다. 앱은 버전 폴더(`current/Rocky.app`)에 그대로
//! 있고 `rocky update` 는 앱을 깔지도 띄우지도 않는다(A안, 2026-11-10 — 메뉴 막대·아바타는 선택). 판정은
//! `rocky_core::mac_app`, 여기는 codesign·pgrep·open 을 부르는 배선이다(규칙은 `docs/features/mac.md`).
//!
//! 띄우기: 이미 돌고 있으면 `open` 으로 앞으로만 → 키체인에 인증서가 있고 그 폴더의 앱을 아직 그것으로 서명하지 않았으면
//! 격리 속성을 지우고 다시 서명·검증(표식 `.Rocky.app.signed`) → `open`. 검증에 실패하면 띄우지 않는다.

use std::path::Path;
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

/// 서명 표식 — 그 버전 폴더의 앱을 마지막으로 서명한 인증서 이름.
const SIGNED_MARK: &str = ".Rocky.app.signed";

/// 키체인에 있는 다시 서명할 인증서(`ROCKY_MAC_SIGN_IDENTITY` 가 먼저).
fn identity() -> Option<String> {
    let listing = Command::new("security")
        .args(["find-identity", "-p", "codesigning"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let env = std::env::var("ROCKY_MAC_SIGN_IDENTITY").ok();
    mac_app::sign_identity(env.as_deref(), &listing)
}

/// `rocky app` — `dir`(지금 버전 폴더, `current`)의 앱을 띄운다. 사람에게 보일 한 줄을 돌려준다.
pub fn open(dir: &Path) -> Result<String, String> {
    if !cfg!(target_os = "macos") {
        return Err("맥 앱은 macOS 에서만 띄운다".into());
    }
    let app = dir.join(APP);
    if !app.join("Contents/Info.plist").is_file() {
        return Err(format!(
            "{} 에 맥 앱이 없다 — 앱을 담은 릴리스·nightly 가 아니거나(main 빌드는 앱을 빌드하지 않는다: `mac/scripts/bundle.sh`) \
             앱 빌드가 실패한 tarball 이다",
            app.display()
        ));
    }
    let app_s = app.to_string_lossy().to_string();
    let version = std::fs::read_to_string(app.join("Contents/Info.plist"))
        .ok()
        .and_then(|xml| mac_app::plist_version(&xml))
        .unwrap_or_else(|| "?".into());
    if running() {
        must("open", &[&app_s])?;
        return Ok(format!(
            "맥 앱 {version} 은 이미 돌고 있다 — 앞으로 가져왔다"
        ));
    }
    let mark = dir.join(SIGNED_MARK);
    let id = identity();
    let signed = std::fs::read_to_string(&mark).ok();
    let mut note = String::new();
    if mac_app::needs_resign(id.as_deref(), signed.as_deref()) {
        let id = id.unwrap_or_default();
        // curl 로 받은 tarball 은 격리 속성이 없지만, 손으로 받은 파일에서 왔을 때 Gatekeeper 가 막지 않게.
        let _ = quiet("xattr", &["-dr", "com.apple.quarantine", &app_s]).status();
        must(
            "codesign",
            &["--force", "--sign", &id, "--timestamp=none", &app_s],
        )
        .map_err(|e| {
            format!(
                "'{id}' 로 서명하지 못해 띄우지 않았다 — `security find-identity -p codesigning` 으로 이름을 확인하거나 \
                 ROCKY_MAC_SIGN_IDENTITY=- 로 CI 의 ad-hoc 서명을 그대로 쓴다: {e}"
            )
        })?;
        let _ = std::fs::write(&mark, format!("{id}\n"));
        note = format!(" (서명: {id})");
    }
    must("codesign", &["--verify", "--strict", &app_s])
        .map_err(|e| format!("서명 검증에 실패해 띄우지 않았다: {e}"))?;
    must("open", &[&app_s])?;
    Ok(format!(
        "✓ 맥 앱 {version} 을 띄웠다 — {}{note}",
        app.display()
    ))
}

/// `rocky app --quit` — 돌고 있는 맥 앱을 내린다.
pub fn quit() -> Result<String, String> {
    if !running() {
        return Ok("맥 앱은 떠 있지 않다".into());
    }
    if stop() {
        Ok("✓ 맥 앱을 내렸다".into())
    } else {
        Err("맥 앱이 5초 안에 내려가지 않았다 — 메뉴 막대에서 종료한다".into())
    }
}

/// 예전 `rocky update` 가 깔아 둔 앱(`/Applications`·`~/Applications`) — 이제 갱신하지 않으므로 남아 있으면 알린다.
/// 로그인 항목에 올라 있으면 로그인 때마다 옛 앱이 뜬다. 지우는 것은 사람 몫이다.
pub fn stale_note() -> Option<String> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let home = std::env::var("HOME").ok()?;
    let found: Vec<String> = [
        Path::new("/Applications").join(APP),
        Path::new(&home).join("Applications").join(APP),
    ]
    .into_iter()
    .filter(|p| p.join("Contents/Info.plist").is_file())
    .map(|p| p.display().to_string())
    .collect();
    (!found.is_empty()).then(|| {
        format!(
            "예전에 깔아 둔 맥 앱({})은 더 이상 갱신하지 않는다 — 앱을 끄고 지운 뒤 `rocky app` 으로 띄운다\
             (로그인할 때 열려면 앱 메뉴의 \"로그인할 때 열기\")",
            found.join(", ")
        )
    })
}

/// [`stale_note`] 가 있으면 한 줄로 찍는다 — `rocky update`·`rocky app` 끝에.
pub fn report_stale() {
    if let Some(note) = stale_note() {
        println!("  참고: {note}");
    }
}
