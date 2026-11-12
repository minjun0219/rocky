//! 맥 앱(`Rocky.app`)을 `rocky app` 으로 띄울 때의 순수 판정 — 무엇으로 서명할지, 다시 서명할지. 앱은 버전 폴더
//! (`current/Rocky.app`)에 그대로 있고 `/Applications` 로 옮기지 않는다(A안, 2026-11-10). 서명·띄우기·내리기는
//! `rocky_cli::mac_app` 이 한다(규칙은 `docs/features/mac.md`).

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

/// 다시 서명할까 — 쓸 인증서가 있고, 그 버전 폴더의 앱을 마지막으로 서명한 인증서(`signed`, 표식 파일)와 다를 때만.
/// 같은 폴더의 앱은 한 번만 서명한다 — 해시가 바뀔 때마다 EDR·Gatekeeper 가 새 파일로 본다.
pub fn needs_resign(identity: Option<&str>, signed: Option<&str>) -> bool {
    identity.is_some_and(|id| signed.map(str::trim) != Some(id))
}
