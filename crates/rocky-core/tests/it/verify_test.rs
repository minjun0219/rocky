//! 기본 브랜치 검증의 순수 판정 — 설정 모양, 원격 커밋 읽기, 다시 돌지, 알릴지.

use rocky_core::verify::{
    dir_name, is_branch_name, load_verify_block, notification, parse_ls_remote, should_run,
    VerifyRecord, VerifyState,
};

fn config(text: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rocky.json");
    std::fs::write(&path, text).unwrap();
    (dir, path)
}

#[test]
fn verify_block_is_opt_in_and_drops_bad_targets() {
    let (_d, path) = config(r#"{}"#);
    let c = load_verify_block(&path);
    assert!(!c.active());
    assert_eq!(c.interval_seconds(), 60);

    let (_d, path) = config(
        r#"{ "verify": { "intervalSeconds": 5, "targets": [
            { "board": "rocky", "steps": [ { "name": "check", "command": ["bun", "run", "check"], "timeoutMs": 1000 } ] },
            { "board": "web", "branch": "--upload-pack=x", "steps": [ { "name": "t", "command": ["true"] } ] },
            { "board": "empty", "steps": [] },
            { "board": "bad-step", "steps": [ { "name": "Bad Name", "command": ["true"] } ] }
        ] } }"#,
    );
    let c = load_verify_block(&path);
    assert!(c.active());
    assert_eq!(c.interval_seconds(), 60, "10초 밑은 기본값");
    assert_eq!(
        c.targets.len(),
        1,
        "단계가 없거나 전부 잘못됐거나 브랜치가 잘못된 대상은 버린다"
    );
    assert_eq!(c.targets[0].branch, "main");
    assert_eq!(c.targets[0].steps[0].timeout_ms, Some(1000));

    let (_d, path) = config(
        r#"{ "verify": { "enabled": false, "targets": [ { "board": "r", "steps": [ { "name": "t", "command": ["true"] } ] } ] } }"#,
    );
    assert!(!load_verify_block(&path).active());
}

#[test]
fn branch_and_dir_names_are_safe() {
    assert!(is_branch_name("main"));
    assert!(is_branch_name("release/1.2"));
    assert!(!is_branch_name("-x"));
    assert!(!is_branch_name("a..b"));
    assert!(!is_branch_name("a b"));
    assert!(dir_name("rocky").starts_with("rocky-"));
    assert!(
        dir_name("../x").starts_with("___x-"),
        "경로 구분자는 남지 않는다"
    );
    assert_ne!(
        dir_name("release/a"),
        dir_name("release_a"),
        "치환으로 겹치는 이름도 나뉜다"
    );
    assert_eq!(
        dir_name("main"),
        dir_name("main"),
        "같은 이름은 늘 같은 디렉터리"
    );
}

#[test]
fn ls_remote_output_gives_the_branch_sha() {
    let out = "1234567890abcdef1234567890abcdef12345678\trefs/heads/main\n";
    assert_eq!(
        parse_ls_remote(out, "main").as_deref(),
        Some("1234567890abcdef1234567890abcdef12345678")
    );
    assert_eq!(parse_ls_remote(out, "dev"), None);
    assert_eq!(
        parse_ls_remote("not-a-sha\trefs/heads/main\n", "main"),
        None
    );
    assert_eq!(parse_ls_remote("", "main"), None);
}

fn record(sha: &str, state: VerifyState) -> VerifyRecord {
    VerifyRecord {
        board: "rocky".into(),
        branch: "main".into(),
        sha: sha.into(),
        subject: Some("feat: x".into()),
        state,
        failed_step: (state == VerifyState::Failed).then(|| "cargo-test".into()),
        reason: (state == VerifyState::Failed).then(|| "종료 코드 101".into()),
        started_at: "2026-10-05T00:00:00.000Z".into(),
        finished_at: None,
        log: "/tmp/x.log".into(),
    }
}

#[test]
fn reruns_only_new_or_interrupted_commits() {
    assert!(should_run(None, "aaa"));
    assert!(!should_run(
        Some(&record("aaa", VerifyState::Passed)),
        "aaa"
    ));
    assert!(
        !should_run(Some(&record("aaa", VerifyState::Failed)), "aaa"),
        "같은 커밋의 실패는 다시 돌지 않는다"
    );
    assert!(
        should_run(Some(&record("aaa", VerifyState::Running)), "aaa"),
        "데몬이 도중에 내려갔다"
    );
    assert!(should_run(Some(&record("aaa", VerifyState::Passed)), "bbb"));
}

#[test]
fn notifies_failures_and_recoveries_only() {
    let fail = record("bbbbbbbbbb", VerifyState::Failed);
    let (title, body) = notification(None, &fail).unwrap();
    assert!(title.contains("검증 실패"));
    assert!(
        body.starts_with("bbbbbbb cargo-test — 종료 코드 101"),
        "{body}"
    );

    let pass = record("cccccccccc", VerifyState::Passed);
    assert!(notification(None, &pass).is_none(), "첫 통과는 조용하다");
    assert!(notification(Some(&record("a", VerifyState::Passed)), &pass).is_none());
    let (title, _) = notification(Some(&fail), &pass).unwrap();
    assert!(title.contains("다시 초록"));
    assert!(notification(Some(&fail), &record("d", VerifyState::Running)).is_none());
}
