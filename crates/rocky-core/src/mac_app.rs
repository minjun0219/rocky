//! 맥 앱(`Rocky.app`)을 릴리스·nightly tarball 에서 기기에 까는 순수 판정 — 어디에 깔지, 바꿀지, 무엇으로 서명할지.
//! 복사·서명·다시 띄우기는 `rocky_cli::mac_app` 이 한다(규칙은 `docs/features/mac.md`).

use std::path::{Path, PathBuf};

/// tarball 안과 설치 폴더 안의 앱 이름.
pub const APP: &str = "Rocky.app";

/// 오너가 만든 자체 서명 인증서의 이름 — `ROCKY_MAC_SIGN_IDENTITY` 가 없을 때 키체인에 이 이름이 있으면 그것으로 다시 서명한다.
pub const DEFAULT_IDENTITY: &str = "rocky";

/// 돌고 있는 앱을 찾는 `pgrep -f` 패턴 — 번들 안의 실행 파일 경로.
pub const PROCESS_PATTERN: &str = "/Rocky.app/Contents/MacOS/Rocky";

/// `Info.plist` 의 `CFBundleVersion`(없으면 `CFBundleShortVersionString`). `bundle.sh` 가 쓰는 XML plist 만 읽는다.
pub fn plist_version(xml: &str) -> Option<String> {
    ["CFBundleVersion", "CFBundleShortVersionString"]
        .iter()
        .find_map(|key| plist_string(xml, key))
}

fn plist_string(xml: &str, key: &str) -> Option<String> {
    let after = &xml[xml.find(&format!("<key>{key}</key>"))? + key.len() + 11..];
    let rest = after.trim_start().strip_prefix("<string>")?;
    let value = rest[..rest.find("</string>")?].trim();
    (!value.is_empty()).then(|| value.to_string())
}

/// 깔 자리 — 이미 깔린 곳이 있으면 그곳(`/Applications` 먼저), 없으면 쓸 수 있는 `/Applications`, 그것도 아니면 `~/Applications`.
pub fn choose_dest(
    home: &Path,
    system_has: bool,
    user_has: bool,
    system_writable: bool,
) -> PathBuf {
    let system = Path::new("/Applications").join(APP);
    let user = home.join("Applications").join(APP);
    if system_has {
        system
    } else if user_has {
        user
    } else if system_writable {
        system
    } else {
        user
    }
}

/// 바꿀까 — 없거나 버전이 다르면. `force` 는 nightly 처럼 버전 문자열이 같아도 빌드가 다른 경우.
pub fn needs_install(installed: Option<&str>, bundled: &str, force: bool) -> bool {
    force || installed != Some(bundled)
}

/// 다시 서명할 인증서 — `ROCKY_MAC_SIGN_IDENTITY` 가 있으면 그것(`-` 면 CI 의 ad-hoc 서명 그대로), 없으면
/// `security find-identity -p codesigning` 출력에 [`DEFAULT_IDENTITY`] 가 있을 때만 그것. `None` 이면 다시 서명하지 않는다.
pub fn sign_identity(env: Option<&str>, keychain_listing: &str) -> Option<String> {
    match env.map(str::trim).filter(|v| !v.is_empty()) {
        Some("-") => None,
        Some(name) => Some(name.to_string()),
        None => keychain_listing
            .contains(&format!("\"{DEFAULT_IDENTITY}\""))
            .then(|| DEFAULT_IDENTITY.to_string()),
    }
}

/// 깐 뒤 띄울까 — 돌고 있었으면 다시, 처음 깔았으면 새로. 깔려 있는데 꺼져 있으면 사람이 끈 것이라 두고 본다.
pub fn should_launch(was_running: bool, was_installed: bool) -> bool {
    was_running || !was_installed
}

/// 앱 번들 안에 함께 싣는 실행 파일 — `Contents/Helpers/` 에 둔다(설계 `2026-11-09-mac-app-bundle-design.md`). 앱 실행 파일 `Contents/MacOS/Rocky`
/// 옆에 두면 대소문자를 가리지 않는 볼륨(macOS 기본)에서 CLI `rocky` 가 앱을 덮어쓴다.
pub const PAYLOAD_BINARIES: [&str; 2] = ["rocky", "rockyd"];

/// 실행 파일 곁의 리소스(`dist`·`tui.js`)를 찾을 자리, 먼저 볼 것부터 — 실행 파일과 같은 폴더, 그리고 실행 파일이 앱 번들의
/// `Contents/Helpers/`(또는 `Contents/MacOS/`)에 있으면 그 번들의 `Contents/Resources/`. `exe` 는 링크를 푼 실제 경로여야 한다.
pub fn resource_candidates(exe: &Path, name: &str) -> Vec<PathBuf> {
    let Some(dir) = exe.parent() else {
        return Vec::new();
    };
    let mut out = vec![dir.join(name)];
    if dir
        .file_name()
        .is_some_and(|n| n == "Helpers" || n == "MacOS")
    {
        if let Some(contents) = dir
            .parent()
            .filter(|c| c.file_name().is_some_and(|n| n == "Contents"))
        {
            out.push(contents.join("Resources").join(name));
        }
    }
    out
}

/// 서명 순서 — 번들 안쪽의 실행 파일([`PAYLOAD_BINARIES`] 중 있는 것)을 먼저, 앱 번들을 마지막에. 안쪽이 서명되지 않은 채 번들을
/// 서명하면 `codesign` 이 "code object is not signed at all — In subcomponent" 로 실패한다. `--deep` 은 쓰지 않는다.
pub fn sign_order(app: &Path, exists: impl Fn(&Path) -> bool) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = PAYLOAD_BINARIES
        .iter()
        .map(|name| app.join("Contents/Helpers").join(name))
        .filter(|path| exists(path))
        .collect();
    out.push(app.to_path_buf());
    out
}
