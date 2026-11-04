use std::path::Path;

use rocky_core::mac_app::{
    choose_dest, needs_install, plist_version, should_launch, sign_identity,
};

const PLIST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
<dict>
	<key>CFBundleShortVersionString</key>
	<string>0.60.0</string>
	<key>CFBundleVersion</key>
	<string>0.60.1</string>
</dict>
</plist>"#;

#[test]
fn plist_version_reads_bundle_version_first() {
    assert_eq!(plist_version(PLIST).as_deref(), Some("0.60.1"));
}

#[test]
fn plist_version_falls_back_to_short_version() {
    let xml = "<key>CFBundleShortVersionString</key>\n  <string>1.2.3</string>";
    assert_eq!(plist_version(xml).as_deref(), Some("1.2.3"));
}

#[test]
fn plist_version_none_when_missing_or_empty() {
    assert_eq!(plist_version("<dict></dict>"), None);
    assert_eq!(
        plist_version("<key>CFBundleVersion</key><string> </string>"),
        None
    );
    assert_eq!(plist_version("<key>CFBundleVersion</key><true/>"), None);
}

#[test]
fn choose_dest_prefers_existing_then_writable_system() {
    let home = Path::new("/Users/me");
    let system = Path::new("/Applications/Rocky.app");
    let user = Path::new("/Users/me/Applications/Rocky.app");
    assert_eq!(choose_dest(home, true, true, false), system);
    assert_eq!(choose_dest(home, false, true, true), user);
    assert_eq!(choose_dest(home, false, false, true), system);
    assert_eq!(choose_dest(home, false, false, false), user);
}

#[test]
fn needs_install_when_missing_different_or_forced() {
    assert!(needs_install(None, "0.60.0", false));
    assert!(needs_install(Some("0.59.0"), "0.60.0", false));
    assert!(!needs_install(Some("0.60.0"), "0.60.0", false));
    assert!(needs_install(Some("0.60.0"), "0.60.0", true));
}

#[test]
fn sign_identity_env_wins_and_dash_keeps_adhoc() {
    let listing = r#"  1) 0123ABCD "rocky" (CSSMERR_TP_NOT_TRUSTED)"#;
    assert_eq!(
        sign_identity(Some("other"), listing).as_deref(),
        Some("other")
    );
    assert_eq!(sign_identity(Some("-"), listing), None);
    assert_eq!(sign_identity(Some("  "), listing).as_deref(), Some("rocky"));
}

#[test]
fn sign_identity_uses_rocky_only_when_keychain_has_it() {
    let listing = r#"  1) 0123ABCD "rocky" (CSSMERR_TP_NOT_TRUSTED)
     1 identities found"#;
    assert_eq!(sign_identity(None, listing).as_deref(), Some("rocky"));
    assert_eq!(sign_identity(None, r#"  1) 0123ABCD "rocky-old""#), None);
    assert_eq!(sign_identity(None, "0 identities found"), None);
}

#[test]
fn should_launch_when_running_or_first_install() {
    assert!(should_launch(true, true));
    assert!(should_launch(false, false));
    assert!(!should_launch(false, true));
}
