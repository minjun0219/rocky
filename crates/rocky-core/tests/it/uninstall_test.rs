//! `rocky uninstall` 계획 — 무엇을 걷고 무엇을 남기는지.

use std::path::{Path, PathBuf};

use rocky_core::uninstall::{plan, removable, render_plan, DataDir, Step, UninstallInput};

fn p(s: &str) -> PathBuf {
    PathBuf::from(s)
}

fn dir(s: &str) -> DataDir {
    DataDir {
        path: p(s),
        repo: false,
    }
}

fn input() -> UninstallInput {
    UninstallInput {
        home: p("/Users/me"),
        install_dir: p("/Users/me/.local/share/rocky"),
        install_dir_exists: true,
        cli_link: Some((
            p("/Users/me/.local/bin/rocky"),
            Some(p("/Users/me/.local/share/rocky/current/rocky")),
        )),
        plist: Some(p("/Users/me/Library/LaunchAgents/com.rocky.daemon.plist")),
        data_dirs: vec![
            dir("/Users/me/.config/rocky"),
            dir("/Users/me/.config/rocky/todo"),
        ],
        ..UninstallInput::default()
    }
}

#[test]
fn takes_down_the_install_and_keeps_the_data_by_default() {
    let out = plan(&input());
    assert_eq!(
        out.steps,
        vec![
            Step::Plugin,
            Step::Launchd(p("/Users/me/Library/LaunchAgents/com.rocky.daemon.plist")),
            Step::StopDaemon,
            Step::RemoveLink(p("/Users/me/.local/bin/rocky")),
            Step::RemoveDir(p("/Users/me/.local/share/rocky")),
        ]
    );
    // 안에 든 todo 폴더는 따로 적지 않는다.
    assert_eq!(out.keep.len(), 1);
    assert!(out.keep[0].starts_with("/Users/me/.config/rocky — "));
    assert!(out.keep[0].contains("--purge"));
}

#[test]
fn the_daemon_stops_before_any_folder_goes() {
    let out = plan(&UninstallInput {
        purge: true,
        ..input()
    });
    let stop = out.steps.iter().position(|s| *s == Step::StopDaemon);
    let first_dir = out
        .steps
        .iter()
        .position(|s| matches!(s, Step::RemoveDir(_)));
    assert!(stop < first_dir);
    assert_eq!(
        out.steps.last(),
        Some(&Step::RemoveDir(p("/Users/me/.config/rocky")))
    );
}

#[test]
fn purge_spares_repos_and_folders_outside_home_or_not_rocky() {
    let out = plan(&UninstallInput {
        purge: true,
        data_dirs: vec![
            DataDir {
                path: p("/Users/me/src/rocky"),
                repo: true,
            },
            dir("/srv/rocky-data"),
            dir("/Users/me/notes"),
            dir("/Users/me/.config/rocky"),
        ],
        ..input()
    });
    let install = Step::RemoveDir(p("/Users/me/.local/share/rocky"));
    let removed: Vec<&Step> = out
        .steps
        .iter()
        .filter(|s| matches!(s, Step::RemoveDir(_)) && **s != install)
        .collect();
    assert_eq!(
        removed,
        vec![&Step::RemoveDir(p("/Users/me/.config/rocky"))]
    );
    assert_eq!(out.keep.len(), 3);
}

#[test]
fn a_link_or_file_that_is_not_ours_stays() {
    let foreign = plan(&UninstallInput {
        cli_link: Some((p("/Users/me/.local/bin/rocky"), Some(p("/opt/other/rocky")))),
        ..input()
    });
    assert!(!foreign
        .steps
        .iter()
        .any(|s| matches!(s, Step::RemoveLink(_))));
    let file = plan(&UninstallInput {
        cli_link: Some((p("/Users/me/.local/bin/rocky"), None)),
        ..input()
    });
    assert!(!file.steps.iter().any(|s| matches!(s, Step::RemoveLink(_))));
    assert!(file.keep[0].contains("실제 파일"));
}

#[test]
fn no_plist_means_no_launchctl() {
    let out = plan(&UninstallInput {
        plist: None,
        ..input()
    });
    assert!(!out.steps.iter().any(|s| matches!(s, Step::Launchd(_))));
    assert!(out.steps.contains(&Step::StopDaemon));
}

#[test]
fn things_rocky_started_but_people_use_are_only_mentioned() {
    let out = plan(&UninstallInput {
        rc: true,
        tailscale: true,
        statusline_wired: true,
        ..input()
    });
    let text = render_plan(&out);
    assert!(text.contains("rc 서버"));
    assert!(text.contains("tailscale serve --https=443 off"));
    assert!(text.contains("statusLine"));
    assert!(text.starts_with("rocky uninstall — 걷어낼 것:"));
}

#[test]
fn removable_needs_a_rocky_folder_inside_home() {
    let home = Path::new("/Users/me");
    assert!(removable(Path::new("/Users/me/.config/rocky"), home));
    assert!(removable(Path::new("/Users/me/rocky-data/todo"), home));
    assert!(!removable(home, home));
    assert!(!removable(Path::new("/Users/me/.config"), home));
    assert!(!removable(Path::new("/tmp/rocky"), home));
    assert!(!removable(Path::new("relative/rocky"), home));
    assert!(!removable(Path::new("/rocky"), Path::new("")));
}

#[test]
fn a_repo_inside_a_purged_folder_keeps_the_outer_folder() {
    let out = plan(&UninstallInput {
        purge: true,
        data_dirs: vec![
            dir("/Users/me/.config/rocky"),
            DataDir {
                path: p("/Users/me/.config/rocky/worklog"),
                repo: true,
            },
            dir("/Users/me/.config/rocky/todo"),
        ],
        ..input()
    });
    assert!(!out
        .steps
        .contains(&Step::RemoveDir(p("/Users/me/.config/rocky"))));
    assert!(!out
        .steps
        .contains(&Step::RemoveDir(p("/Users/me/.config/rocky/worklog"))));
    // 바깥을 남기면 안의 레포 아닌 폴더는 따로 지운다.
    assert!(out
        .steps
        .contains(&Step::RemoveDir(p("/Users/me/.config/rocky/todo"))));
}

#[test]
fn an_install_folder_in_a_repo_or_outside_home_stays() {
    let repo = plan(&UninstallInput {
        install_dir_repo: true,
        ..input()
    });
    let odd = plan(&UninstallInput {
        install_dir: p("/srv/share/rocky"),
        ..input()
    });
    for out in [repo, odd] {
        assert!(!out
            .steps
            .iter()
            .any(|s| matches!(s, Step::RemoveDir(d) if d.ends_with("rocky") && !d.starts_with("/Users/me/.config"))));
    }
}

#[test]
fn dot_dot_paths_are_never_removable() {
    let home = Path::new("/Users/me");
    assert!(!removable(Path::new("/Users/me/rocky/../notes"), home));
    assert!(!removable(Path::new("/Users/me/rocky/../../other"), home));
}

#[test]
fn data_kept_inside_the_install_folder_keeps_the_install_folder() {
    let worklog = dir("/Users/me/.local/share/rocky/worklog");
    let keep = plan(&UninstallInput {
        data_dirs: vec![worklog.clone()],
        ..input()
    });
    assert!(!keep
        .steps
        .contains(&Step::RemoveDir(p("/Users/me/.local/share/rocky"))));
    assert!(keep.keep.iter().any(|k| k.contains("남길 데이터 폴더")));
    // --purge 면 그 데이터도 지우는 실행이라 설치 폴더째 지운다.
    let purge = plan(&UninstallInput {
        purge: true,
        data_dirs: vec![worklog],
        ..input()
    });
    assert!(purge
        .steps
        .contains(&Step::RemoveDir(p("/Users/me/.local/share/rocky"))));
}
