//! main 빌드 모드 — 모드 판정·다시 빌드할지·마켓플레이스 원천 읽기.

use rocky_core::main_build::{
    is_main_mode, lock_busy, parse_marketplace, plan, BuildPlan, MarketplaceSource, CLONE_URL, REPO,
};

#[test]
fn main_mode_is_the_current_link_pointing_at_main() {
    assert!(is_main_mode(Some("main")));
    assert!(!is_main_mode(Some("v0.49.0")));
    assert!(!is_main_mode(None));
}

#[test]
fn the_repo_is_the_one_main_builds_clone() {
    assert_eq!(CLONE_URL, format!("https://github.com/{REPO}.git"));
}

#[test]
fn the_build_lock_is_held_like_update_main_takes_it() {
    let alive = |pid: u32| pid == 42;
    assert!(
        !lock_busy(false, None, alive),
        "잠금 폴더가 없으면 비어 있다"
    );
    assert!(lock_busy(true, Some("42\n"), alive));
    assert!(
        !lock_busy(true, Some("7"), alive),
        "죽은 pid 의 잠금은 update 가 넘겨받는다"
    );
    assert!(
        lock_busy(true, None, alive),
        "pid 가 없는 잠금(기기의 다른 빌드 스크립트)은 쥔 것으로 본다"
    );
    assert!(lock_busy(true, Some("?"), alive));
}

#[test]
fn plan_builds_only_when_the_wanted_commit_is_not_installed() {
    let sha = "40da51ef1234";
    let done = Some("40da51ef1234\n");
    assert_eq!(plan(sha, done, done, false, true, false), BuildPlan::Skip);
    assert_eq!(
        plan(sha, Some("0a465ffb"), done, false, true, false),
        BuildPlan::Build
    );
    assert_eq!(plan(sha, None, None, false, true, false), BuildPlan::Build);
    assert_eq!(plan(sha, done, done, true, true, false), BuildPlan::Build);
    // 지난 실행이 빌드 뒤(재시작·플러그인)에서 실패했다 — 다시 부르면 빌드 없이 뒤 단계만 마저 한다
    assert_eq!(plan(sha, done, None, false, true, false), BuildPlan::Finish);
    assert_eq!(
        plan(sha, done, Some("0a465ffb"), false, true, false),
        BuildPlan::Finish
    );
    // `--release` 뒤 같은 커밋으로 돌아왔다 — 빌드는 있으니 링크·재시작·플러그인만
    assert_eq!(
        plan(sha, done, done, false, false, false),
        BuildPlan::Finish
    );
    // TUI 를 모르던 옛 rocky 가 이 커밋을 빌드해 tui.js 없이 깔았다 — 커밋이 같아도 다시 빌드한다
    assert_eq!(plan(sha, done, done, false, true, true), BuildPlan::Build);
    assert_eq!(plan(sha, done, None, false, true, true), BuildPlan::Build);
}

#[test]
fn reads_where_the_marketplace_points() {
    let dir = r#"[{"name":"cabinet","source":"github","repo":"x/y"},
        {"name":"rocky-marketplace","source":"directory","path":"/d/main-src","installLocation":"/d/main-src"}]"#;
    assert_eq!(
        parse_marketplace(dir),
        Some(MarketplaceSource::Directory("/d/main-src".into()))
    );
    let gh = r#"[{"name":"rocky-marketplace","source":"github","repo":"minjun0219/rocky"}]"#;
    assert_eq!(
        parse_marketplace(gh),
        Some(MarketplaceSource::Github("minjun0219/rocky".into()))
    );
    let git = r#"[{"name":"rocky-marketplace","source":"git","url":"https://x"}]"#;
    assert_eq!(
        parse_marketplace(git),
        Some(MarketplaceSource::Other("git".into()))
    );
    assert_eq!(parse_marketplace("[]"), Some(MarketplaceSource::Missing));
    // JSON 이 아니면 모른다 — 호출자는 마켓플레이스를 건드리지 않는다
    assert_eq!(parse_marketplace("Error: boom"), None);
}

#[test]
fn plugin_step_reinstalls_only_our_folder_and_switches_from_release() {
    use rocky_core::main_build::{plugin_step, PluginStep};
    let src = "/d/rocky/main-src";
    assert_eq!(
        plugin_step(
            &MarketplaceSource::Directory("/d/rocky/main-src/".into()),
            src
        ),
        PluginStep::Reinstall
    );
    assert_eq!(
        plugin_step(&MarketplaceSource::Github("minjun0219/rocky".into()), src),
        PluginStep::Switch
    );
    assert_eq!(
        plugin_step(&MarketplaceSource::Missing, src),
        PluginStep::Switch
    );
    // 개발용으로 작업 트리를 가리키는 폴더 마켓플레이스는 건드리지 않는다
    assert!(matches!(
        plugin_step(&MarketplaceSource::Directory("/w/rocky".into()), src),
        PluginStep::Leave(_)
    ));
    assert!(matches!(
        plugin_step(&MarketplaceSource::Other("git".into()), src),
        PluginStep::Leave(_)
    ));
}

#[test]
fn release_step_undoes_only_our_folder_and_refreshes_github() {
    use rocky_core::main_build::{release_step, ReleaseStep};
    let src = "/d/rocky/main-src";
    assert_eq!(
        release_step(&MarketplaceSource::Directory(src.into()), src),
        ReleaseStep::FromFolder
    );
    assert_eq!(
        release_step(&MarketplaceSource::Missing, src),
        ReleaseStep::Add
    );
    // 지난 전환이 실패해 GitHub 그대로면 옛 클론을 깔지 않게 갱신부터
    assert_eq!(
        release_step(&MarketplaceSource::Github("minjun0219/rocky".into()), src),
        ReleaseStep::Refresh
    );
    // 개발용으로 작업 트리를 가리키는 폴더는 `--release` 도 건드리지 않는다
    assert!(matches!(
        release_step(&MarketplaceSource::Directory("/w/rocky".into()), src),
        ReleaseStep::Leave(_)
    ));
}

#[test]
fn nightly_mode_is_its_own_pinned_link() {
    use rocky_core::main_build::{is_main_mode, is_nightly_mode, is_pinned_mode};
    assert!(is_nightly_mode(Some("nightly")));
    assert!(!is_main_mode(Some("nightly")));
    assert!(is_pinned_mode(Some("nightly")));
    assert!(is_pinned_mode(Some("main")));
    assert!(!is_pinned_mode(Some("v0.54.0")));
    assert!(!is_pinned_mode(None));
}

#[test]
fn nightly_tags_follow_the_ci_script() {
    use rocky_core::main_build::{nightly_asset, parse_nightly_tag};
    let t = parse_nightly_tag("v0.54.0-nightly.20261029").unwrap();
    assert_eq!((t.date.as_str(), t.seq), ("20261029", 1));
    assert_eq!(
        parse_nightly_tag("v0.54.0-nightly.20261029.2").unwrap().seq,
        2
    );
    for bad in [
        "v0.54.0",
        "v0.24.0-next.3",
        "0.54.0-nightly.20261029",
        "v0.54-nightly.20261029",
        "v0.54.0-nightly.2026102",
        "v0.54.0-nightly.20261029.x",
    ] {
        assert!(parse_nightly_tag(bad).is_none(), "{bad}");
    }
    assert_eq!(
        nightly_asset("v0.54.0-nightly.20261029"),
        "rocky-v0.54.0-nightly.20261029-aarch64-apple-darwin.tar.gz"
    );
}

#[test]
fn the_latest_nightly_is_chosen_by_date_then_seq_and_annotated_tags_peel() {
    use rocky_core::main_build::latest_nightly;
    let a = "a".repeat(40);
    let b = "b".repeat(40);
    let c = "c".repeat(40);
    let d = "d".repeat(40);
    let out = format!(
        "{a}\trefs/tags/v0.55.0-nightly.20261027\n\
         {b}\trefs/tags/v0.54.0-nightly.20261029\n\
         {c}\trefs/tags/v0.54.0-nightly.20261029.2\n\
         {a}\trefs/tags/v0.54.0-nightly.20261029.2^{{}}\n\
         {d}\trefs/tags/v0.56.0\n\
         xyz\trefs/tags/v0.54.0-nightly.20261030\n"
    );
    assert_eq!(
        latest_nightly(&out),
        Some(("v0.54.0-nightly.20261029.2".to_string(), a.clone())),
        "버전이 아니라 날짜·순번, annotated 는 ^{{}} 의 커밋, 커밋 모양이 틀린 줄은 버린다"
    );
    assert_eq!(latest_nightly(&format!("{d}\trefs/tags/v0.56.0\n")), None);
}
