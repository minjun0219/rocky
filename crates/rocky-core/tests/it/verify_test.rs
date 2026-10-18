//! 기본 브랜치 검증의 순수 판정 — 설정 모양, 원격 커밋 읽기, 다시 돌지, 알릴지, 통과한 커밋을 배포할지.

use rocky_core::verify::{
    deploy_base, deploy_notification, deploy_plan, dir_name, is_branch_name, load_verify_block,
    notification, parse_ls_remote, session_notice, settle_deploy, should_run, DeployInput,
    DeployPlan, DeployRecord, DeploySkip, DeployState, VerifyRecord, VerifyRun, VerifyRunEvent,
    VerifyState,
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
        attempt: 1,
        rerun: false,
    }
}

#[test]
fn reruns_only_new_or_interrupted_commits_unless_asked() {
    assert!(should_run(None, "aaa", false));
    assert!(!should_run(
        Some(&record("aaa", VerifyState::Passed)),
        "aaa",
        false
    ));
    assert!(
        !should_run(Some(&record("aaa", VerifyState::Failed)), "aaa", false),
        "같은 커밋의 실패는 다시 돌지 않는다"
    );
    assert!(
        should_run(Some(&record("aaa", VerifyState::Failed)), "aaa", true),
        "다시 돌려 달라고 하면 같은 커밋도 돈다"
    );
    assert!(
        should_run(Some(&record("aaa", VerifyState::Running)), "aaa", false),
        "데몬이 도중에 내려갔다"
    );
    assert!(should_run(
        Some(&record("aaa", VerifyState::Passed)),
        "bbb",
        false
    ));
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

    let twice = VerifyRecord {
        attempt: 2,
        ..record("eeeeeeeeee", VerifyState::Failed)
    };
    let (_, body) = notification(None, &twice).unwrap();
    assert!(
        body.contains("종료 코드 101 (다시 돌려도 실패 — 2번 연속)"),
        "{body}"
    );
}

#[test]
fn the_attempt_count_is_written_only_after_a_retry() {
    let first = serde_json::to_value(record("aaa", VerifyState::Passed)).unwrap();
    assert!(first.get("attempt").is_none(), "{first}");
    let back: VerifyRecord = serde_json::from_value(first).unwrap();
    assert_eq!(back.attempt, 1, "옛 기록(필드 없음)은 첫 시도로 읽는다");
    let second = VerifyRecord {
        attempt: 2,
        ..record("aaa", VerifyState::Passed)
    };
    assert_eq!(serde_json::to_value(second).unwrap()["attempt"], 2);
}

#[test]
fn run_lines_name_the_outcome_and_stay_flat_for_errors() {
    let at = "2026-10-05T01:00:00.000Z".to_string();
    let interrupted = VerifyRun::of(at.clone(), record("aaa", VerifyState::Running));
    assert_eq!(interrupted.event, VerifyRunEvent::Interrupted);
    let failed = VerifyRun::of(
        at.clone(),
        VerifyRecord {
            rerun: true,
            ..record("aaa", VerifyState::Failed)
        },
    );
    let line = serde_json::to_value(&failed).unwrap();
    assert_eq!(line["event"], "failed");
    assert_eq!(line["record"]["rerun"], true);
    assert_eq!(line["record"]["failedStep"], "cargo-test");

    let error = serde_json::to_value(VerifyRun::error(at, "원격을 못 읽음".into())).unwrap();
    assert_eq!(
        error,
        serde_json::json!({ "at": "2026-10-05T01:00:00.000Z", "event": "error", "error": "원격을 못 읽음" }),
        "record 없음은 줄에 싣지 않는다"
    );
    let back: VerifyRun = serde_json::from_value(line).unwrap();
    assert_eq!(back, failed);
}

#[test]
fn subscribed_sessions_hear_every_finished_run_once_per_commit() {
    let passed = record("aaaaaaa111", VerifyState::Passed);
    // 첫 판정 — 배너와 달리 통과도 보낸다(배포를 맡은 세션이 기다린다)
    let (kind, text) = session_notice(None, &passed).unwrap();
    assert_eq!(kind, "verify-passed");
    assert!(
        text.contains("rocky main 검증 aaaaaaa 통과 · feat: x"),
        "{text}"
    );
    assert!(text.contains("사용자가 직접 쓴 것이 아니다"));
    // 같은 커밋·같은 결과(다시 돌렸는데 그대로)는 보내지 않는다
    assert!(session_notice(Some(&passed), &passed).is_none());
    // 다시 돌려 결과가 바뀌면 보낸다
    let failed_same = record("aaaaaaa111", VerifyState::Failed);
    let (kind, text) = session_notice(Some(&passed), &failed_same).unwrap();
    assert_eq!(kind, "verify-failed");
    assert!(text.contains("실패(cargo-test) — 종료 코드 101"), "{text}");
    assert!(text.contains("/tmp/x.log"), "실패면 로그 자리를 싣는다");
    // 새 커밋은 결과가 같아도 보낸다
    let next = record("bbbbbbb222", VerifyState::Passed);
    assert!(session_notice(Some(&passed), &next).is_some());
    // 도는 중은 판정이 아니다
    assert!(session_notice(None, &record("c", VerifyState::Running)).is_none());
}

const PASSED: &str = "1111111111111111111111111111111111111111";
const INSTALLED: &str = "0000000000000000000000000000000000000000";

fn deploy_record(sha: &str, state: DeployState) -> DeployRecord {
    DeployRecord {
        sha: sha.into(),
        state,
        pid: Some(4242),
        pid_started: Some("Sun Oct 18 10:00:00 2026".into()),
        started_at: "2026-10-18T01:00:00.000Z".into(),
        finished_at: None,
        reason: None,
        log: "/tmp/deploy-111111111111.log".into(),
    }
}

#[test]
fn deploys_a_passed_commit_only_forward_and_once() {
    let passed = record(PASSED, VerifyState::Passed);
    let ready = DeployInput {
        enabled: true,
        branch: "main",
        repo: Some("minjun0219/rocky"),
        main_mode: true,
        finished: Some(&passed),
        finished_build: Some(INSTALLED),
        built: Some(INSTALLED),
        ancestor: Some(true),
        ..DeployInput::default()
    };
    let plan = |input: DeployInput| deploy_plan(&input);
    let skip = |why: DeploySkip| DeployPlan::Skip(why);
    assert_eq!(plan(ready.clone()), DeployPlan::Start(PASSED.into()));

    // 켜질 수 있는 대상·기기
    assert_eq!(
        plan(DeployInput {
            enabled: false,
            ..ready.clone()
        }),
        skip(DeploySkip::Off)
    );
    assert_eq!(
        plan(DeployInput {
            branch: "release",
            ..ready.clone()
        }),
        skip(DeploySkip::NotMainBranch("release".into())),
        "update --main 은 origin/main 위의 커밋만 받는다"
    );
    assert_eq!(
        plan(DeployInput {
            repo: Some("someone/else"),
            ..ready.clone()
        }),
        skip(DeploySkip::NotRocky(Some("someone/else".into())))
    );
    assert_eq!(
        plan(DeployInput {
            repo: None,
            ..ready.clone()
        }),
        skip(DeploySkip::NotRocky(None))
    );
    assert_eq!(
        plan(DeployInput {
            repo: Some("MinJun0219/Rocky"),
            ..ready.clone()
        }),
        DeployPlan::Start(PASSED.into()),
        "GitHub 레포 이름은 대소문자를 가리지 않는다"
    );
    assert_eq!(
        plan(DeployInput {
            main_mode: false,
            ..ready.clone()
        }),
        skip(DeploySkip::NotMainMode)
    );

    // 실행 중·통과 아님·해시 모양
    assert_eq!(
        plan(DeployInput {
            running: true,
            ..ready.clone()
        }),
        skip(DeploySkip::Running)
    );
    for state in [VerifyState::Failed, VerifyState::Running] {
        let r = record(PASSED, state);
        assert_eq!(
            plan(DeployInput {
                finished: Some(&r),
                ..ready.clone()
            }),
            skip(DeploySkip::NotPassed),
            "{state:?}"
        );
    }
    assert_eq!(
        plan(DeployInput {
            finished: None,
            ..ready.clone()
        }),
        skip(DeploySkip::NotPassed)
    );
    let abbreviated = record("1111111", VerifyState::Passed);
    assert!(matches!(
        plan(DeployInput {
            finished: Some(&abbreviated),
            ..ready.clone()
        }),
        DeployPlan::Skip(DeploySkip::Unknown(_))
    ));

    // 이미 깔림·같은 커밋은 한 번만
    assert_eq!(
        plan(DeployInput {
            finished_build: Some(PASSED),
            ..ready.clone()
        }),
        skip(DeploySkip::Installed)
    );
    for state in [DeployState::Failed, DeployState::Succeeded] {
        let last = deploy_record(PASSED, state);
        assert_eq!(
            plan(DeployInput {
                last_deploy: Some(&last),
                ..ready.clone()
            }),
            skip(DeploySkip::AlreadyTried(state)),
            "{state:?}"
        );
    }
    let older = deploy_record(INSTALLED, DeployState::Failed);
    assert_eq!(
        plan(DeployInput {
            last_deploy: Some(&older),
            ..ready.clone()
        }),
        DeployPlan::Start(PASSED.into()),
        "다른 커밋의 실패는 새 커밋을 막지 않는다"
    );

    // 앞으로만
    assert_eq!(
        plan(DeployInput {
            ancestor: Some(false),
            ..ready.clone()
        }),
        skip(DeploySkip::NotForward(INSTALLED.into())),
        "사람이 손으로 올린 더 새 빌드를 되돌리지 않는다"
    );
    assert!(matches!(
        plan(DeployInput {
            ancestor: None,
            ..ready.clone()
        }),
        DeployPlan::Skip(DeploySkip::Unknown(_))
    ));
    assert!(matches!(
        plan(DeployInput {
            finished_build: None,
            built: None,
            ..ready.clone()
        }),
        DeployPlan::Skip(DeploySkip::Unknown(_))
    ));
    assert_eq!(
        plan(DeployInput {
            finished_build: None,
            built: Some(PASSED),
            ancestor: None,
            ..ready.clone()
        }),
        DeployPlan::Start(PASSED.into()),
        "빌드만 있고 뒤 단계가 남았으면 마저 한다"
    );
    assert_eq!(deploy_base(Some(" "), Some(INSTALLED)), Some(INSTALLED));
    assert_eq!(deploy_base(Some(PASSED), Some(INSTALLED)), Some(PASSED));
}

#[test]
fn only_skips_that_need_a_hand_show_under_the_target() {
    for quiet in [
        DeploySkip::Off,
        DeploySkip::Running,
        DeploySkip::NotPassed,
        DeploySkip::Installed,
        DeploySkip::AlreadyTried(DeployState::Failed),
    ] {
        assert!(quiet.note().is_none(), "{quiet:?}");
    }
    let note = DeploySkip::NotMainMode.note().unwrap();
    assert!(note.contains("main 빌드 모드가 아니어서"), "{note}");
    assert!(DeploySkip::NotForward(INSTALLED.into())
        .note()
        .unwrap()
        .contains("0000000"));
    // main/.sha 가 깨져 있어도 자르다 패닉하지 않는다
    assert!(DeploySkip::NotForward("가나다라".into())
        .message()
        .contains("가나다라"));
}

#[test]
fn a_finished_deploy_is_judged_by_the_finished_build() {
    let running = deploy_record(PASSED, DeployState::Running);
    let at = "2026-10-18T01:10:00.000Z".to_string();
    let ok = settle_deploy(&running, Some(&format!("{PASSED}\n")), None, at.clone());
    assert_eq!(ok.state, DeployState::Succeeded);
    assert_eq!(ok.finished_at.as_deref(), Some(at.as_str()));
    assert!(
        deploy_notification("rocky", &ok).is_none(),
        "성공은 조용하다"
    );

    let failed = settle_deploy(&running, Some(INSTALLED), Some(1), at.clone());
    assert_eq!(failed.state, DeployState::Failed);
    assert_eq!(failed.reason.as_deref(), Some("update 종료 코드 1"));
    let (title, body) = deploy_notification("rocky", &failed).unwrap();
    assert_eq!(title, "rocky: rocky 배포 실패");
    assert!(
        body.contains("1111111 update 종료 코드 1") && body.contains("rocky update --main 1111111"),
        "{body}"
    );

    // 데몬이 교체돼 종료 코드를 모른다 — .finished 만 본다
    let unknown = settle_deploy(&running, None, None, at);
    assert_eq!(unknown.state, DeployState::Failed);
    assert!(unknown.reason.unwrap().contains("main/.finished"));
}

#[test]
fn deploy_lines_join_the_run_history() {
    let at = "2026-10-18T01:10:00.000Z".to_string();
    let done = DeployRecord {
        state: DeployState::Succeeded,
        ..deploy_record(PASSED, DeployState::Running)
    };
    let line = serde_json::to_value(VerifyRun::deploy(at.clone(), done.clone())).unwrap();
    assert_eq!(line["event"], "deployed");
    assert_eq!(line["deploy"]["pidStarted"], "Sun Oct 18 10:00:00 2026");
    assert!(line.get("record").is_none());
    let failed = VerifyRun::deploy(
        at,
        DeployRecord {
            state: DeployState::Failed,
            ..done
        },
    );
    assert_eq!(failed.event, VerifyRunEvent::DeployFailed);
    let back: VerifyRun = serde_json::from_value(serde_json::to_value(&failed).unwrap()).unwrap();
    assert_eq!(back, failed);
    assert_eq!(
        serde_json::to_value(&failed).unwrap()["event"],
        "deploy-failed"
    );
}
