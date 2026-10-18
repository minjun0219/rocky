//! 기본 브랜치 검증 잡 — 임시 원격(bare) + 보드 레포(clone)로 끝까지 돈다: 통과·실패·같은 커밋 생략·복구 알림·시간 초과,
//! 통과한 커밋의 배포(가짜 `rocky` 로).

use std::os::unix::process::CommandExt;
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::common::*;
use rocky_core::config::CommandBridge;
use rocky_core::verify::{DeployRecord, DeployState, VerifyConfig, VerifyState, VerifyTarget};
use rockyd::runner::default_runner;
use rockyd::verify::{tick, verify_target, verify_target_with, DeployEnv, VerifyNotifier};

fn git(dir: &Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .current_dir(dir)
        // pre-push 훅 안에서 돌 때 실제 레포로 새지 않게.
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// 원격(bare)과 그걸 clone 한 보드 레포. `ok.txt` 가 있으면 단계가 통과한다.
fn repos(tmp: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let origin = tmp.join("origin.git");
    let work = tmp.join("work");
    std::fs::create_dir_all(&origin).unwrap();
    git(&origin, &["init", "--bare", "-q"]);
    git(tmp, &["clone", "-q", origin.to_str().unwrap(), "work"]);
    std::fs::write(work.join("ok.txt"), "1").unwrap();
    git(&work, &["add", "."]);
    git(&work, &["commit", "-q", "-m", "첫 커밋"]);
    git(&work, &["push", "-q", "origin", "HEAD:main"]);
    (origin, std::fs::canonicalize(&work).unwrap())
}

fn commit(work: &Path, msg: &str, change: impl FnOnce(&Path)) {
    change(work);
    git(work, &["add", "-A"]);
    git(work, &["commit", "-q", "-m", msg]);
    git(work, &["push", "-q", "origin", "HEAD:main"]);
}

fn step(name: &str, script: &str, timeout_ms: Option<u64>) -> CommandBridge {
    CommandBridge {
        name: name.into(),
        command: vec!["sh".into(), "-c".into(), script.into()],
        timeout_ms,
    }
}

fn capture() -> (VerifyNotifier, Arc<Mutex<Vec<String>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = seen.clone();
    (
        Arc::new(move |title: String, body: String| {
            sink.lock().unwrap().push(format!("{title} | {body}"))
        }),
        seen,
    )
}

#[tokio::test]
async fn verifies_new_commits_in_its_own_worktree_and_notifies_failure_and_recovery() {
    let f = fx();
    let tmp = tempfile::tempdir().unwrap();
    let (_origin, work) = repos(tmp.path());
    f.store.ensure_board("proj", None, "t").unwrap();
    f.store
        .set_board_path("proj", work.to_str().unwrap(), "t")
        .unwrap();
    let root = tmp.path().join("verify");
    let target = VerifyTarget {
        board: "proj".into(),
        branch: "main".into(),
        deploy: false,
        steps: vec![
            step("has-ok", "test -f ok.txt", None),
            step("echo", "echo hi", None),
        ],
    };
    let runner = default_runner();
    let (notifier, seen) = capture();

    verify_target(&f.state, &runner, &notifier, &root, &target).await;
    let status = f.state.verify();
    let rec = status[0].record.clone().expect("기록이 있다");
    assert_eq!(rec.state, VerifyState::Passed, "{status:?}");
    assert_eq!(rec.subject.as_deref(), Some("첫 커밋"));
    assert!(
        rockyd::verify::target_dir(&root, &target)
            .join("tree/ok.txt")
            .exists(),
        "전용 워크트리에서 돈다"
    );
    let log = std::fs::read_to_string(&rec.log).unwrap();
    assert!(log.contains("### echo — sh"), "{log}");
    assert!(
        !log.contains("test -f ok.txt"),
        "단계 argv 는 로그에 남지 않는다"
    );
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(&rec.log).unwrap().permissions().mode() & 0o777,
        0o600,
        "로그는 소유자만 읽는다"
    );
    assert!(seen.lock().unwrap().is_empty(), "첫 통과는 조용하다");
    // 보드 레포의 작업 트리·브랜치는 그대로다.
    assert!(work.join("ok.txt").exists());

    // 깨뜨리는 커밋 → 실패 알림. 같은 커밋은 다시 돌지 않는다.
    commit(&work, "ok 를 지운다", |w| {
        std::fs::remove_file(w.join("ok.txt")).unwrap()
    });
    verify_target(&f.state, &runner, &notifier, &root, &target).await;
    let rec = f.state.verify()[0].record.clone().unwrap();
    assert_eq!(rec.state, VerifyState::Failed);
    assert_eq!(rec.failed_step.as_deref(), Some("has-ok"));
    assert_eq!(seen.lock().unwrap().len(), 1);
    assert!(seen.lock().unwrap()[0].contains("검증 실패"));
    verify_target(&f.state, &runner, &notifier, &root, &target).await;
    assert_eq!(
        seen.lock().unwrap().len(),
        1,
        "같은 커밋의 실패를 또 알리지 않는다"
    );

    // 고치면 복구 알림.
    commit(&work, "ok 를 되살린다", |w| {
        std::fs::write(w.join("ok.txt"), "1").unwrap()
    });
    verify_target(&f.state, &runner, &notifier, &root, &target).await;
    assert_eq!(
        f.state.verify()[0].record.clone().unwrap().state,
        VerifyState::Passed
    );
    assert!(seen.lock().unwrap()[1].contains("다시 초록"));

    // REST 가 같은 상태를 싣는다.
    let (code, body) = get(&f.state, "/api/verify").await;
    assert_eq!(code, 200);
    assert_eq!(body["targets"][0]["record"]["state"], "passed");
    assert_eq!(body["targets"][0]["board"], "proj");
}

#[tokio::test]
async fn a_step_past_its_timeout_fails_and_a_board_without_path_is_reported() {
    let f = fx();
    let tmp = tempfile::tempdir().unwrap();
    let (_origin, work) = repos(tmp.path());
    f.store.ensure_board("proj", None, "t").unwrap();
    f.store
        .set_board_path("proj", work.to_str().unwrap(), "t")
        .unwrap();
    let root = tmp.path().join("verify");
    let (notifier, _seen) = capture();
    let slow = VerifyTarget {
        board: "proj".into(),
        branch: "main".into(),
        deploy: false,
        steps: vec![step("slow", "sleep 30", Some(300))],
    };
    let started = std::time::Instant::now();
    verify_target(&f.state, &default_runner(), &notifier, &root, &slow).await;
    assert!(
        started.elapsed() < std::time::Duration::from_secs(20),
        "시간 초과면 기다리지 않는다"
    );
    let rec = f.state.verify()[0].record.clone().unwrap();
    assert_eq!(rec.state, VerifyState::Failed);
    assert!(rec.reason.unwrap().contains("끝나지 않았다"));

    f.store.ensure_board("bare", None, "t").unwrap();
    let nopath = VerifyTarget {
        board: "bare".into(),
        branch: "main".into(),
        deploy: false,
        steps: slow.steps.clone(),
    };
    verify_target(&f.state, &default_runner(), &notifier, &root, &nopath).await;
    let status = f.state.verify();
    let bare = status.iter().find(|s| s.board == "bare").unwrap();
    assert!(bare.error.as_deref().unwrap().contains("path"), "{bare:?}");
    assert!(bare.record.is_none());
}

#[tokio::test]
async fn a_timed_out_step_takes_its_grandchildren_with_it() {
    let f = fx();
    let tmp = tempfile::tempdir().unwrap();
    let (_origin, work) = repos(tmp.path());
    f.store.ensure_board("proj", None, "t").unwrap();
    f.store
        .set_board_path("proj", work.to_str().unwrap(), "t")
        .unwrap();
    let root = tmp.path().join("verify");
    let pid_file = tmp.path().join("grandchild.pid");
    // sh 가 손자(sleep)를 띄우고 기다린다 — 리더만 죽이면 손자가 남는다.
    let script = format!("sleep 60 & echo $! > {}; wait", pid_file.display());
    let (notifier, _seen) = capture();
    let target = VerifyTarget {
        board: "proj".into(),
        branch: "main".into(),
        deploy: false,
        steps: vec![step("slow", &script, Some(500))],
    };
    verify_target(&f.state, &default_runner(), &notifier, &root, &target).await;
    let pid: i32 = std::fs::read_to_string(&pid_file)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let alive = std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .status()
        .unwrap()
        .success();
    assert!(!alive, "시간 초과면 손자까지 끝낸다(pid {pid})");
    let signals =
        std::fs::read_to_string(rockyd::verify::target_dir(&root, &target).join("signals.log"))
            .unwrap();
    assert!(signals.contains("시간 초과 step=slow"), "{signals}");
    assert!(!rockyd::verify::target_dir(&root, &target)
        .join("running.pgid")
        .exists());
}

#[tokio::test]
async fn setup_failures_retry_and_an_interrupted_rerun_still_announces_recovery() {
    let f = fx();
    let tmp = tempfile::tempdir().unwrap();
    let (_origin, work) = repos(tmp.path());
    f.store.ensure_board("proj", None, "t").unwrap();
    f.store
        .set_board_path("proj", work.to_str().unwrap(), "t")
        .unwrap();
    let root = tmp.path().join("verify");
    let target = VerifyTarget {
        board: "proj".into(),
        branch: "main".into(),
        deploy: false,
        steps: vec![step("has-ok", "test -f ok.txt", None)],
    };
    let dir = rockyd::verify::target_dir(&root, &target);
    let (notifier, seen) = capture();

    // 준비 실패(fetch 만 실패하는 runner — 네트워크가 잠깐 끊긴 셈) — 커밋을 빨강으로 남기지 않고 error 만, 알림 없음.
    let real = default_runner();
    let flaky: rockyd::runner::Runner = Arc::new(move |argv: Vec<String>, stdin, timeout| {
        if argv.iter().any(|a| a == "fetch") {
            Box::pin(async { rockyd::runner::CmdOutput::failure("could not resolve host") })
        } else {
            real(argv, stdin, timeout)
        }
    });
    verify_target(&f.state, &flaky, &notifier, &root, &target).await;
    let status = f.state.verify()[0].clone();
    assert!(
        status.error.as_deref().unwrap_or("").contains("준비 실패"),
        "{status:?}"
    );
    assert!(status.record.is_none(), "준비 실패는 기록을 만들지 않는다");
    assert!(seen.lock().unwrap().is_empty());

    // 앞 커밋이 실패로 끝났고, 지금 커밋은 도는 중에 데몬이 내려갔다(last=Running, finished=Failed).
    let head = String::from_utf8(
        std::process::Command::new("git")
            .args(["-C", work.to_str().unwrap(), "rev-parse", "HEAD"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_string();
    let rec = |sha: &str, state: &str| {
        serde_json::json!({ "board": "proj", "branch": "main", "sha": sha, "state": state,
                            "startedAt": "2026-10-05T00:00:00.000Z", "log": "/tmp/x.log" })
        .to_string()
    };
    std::fs::write(
        dir.join("finished.json"),
        rec("0000000000000000000000000000000000000000", "failed"),
    )
    .unwrap();
    // 끊긴 실행은 사람이 다시 돌려 달라고 한 것이었다 — 그 표시도 이력에 그대로 실린다.
    let mut running: serde_json::Value = serde_json::from_str(&rec(&head, "running")).unwrap();
    running["rerun"] = true.into();
    std::fs::write(dir.join("last.json"), running.to_string()).unwrap();
    verify_target(&f.state, &default_runner(), &notifier, &root, &target).await;
    assert_eq!(
        f.state.verify()[0].record.clone().unwrap().state,
        VerifyState::Passed
    );
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert!(seen[0].contains("다시 초록"), "{seen:?}");
    // 이력에는 준비 실패·끊긴 실행·통과가 차례로 남는다.
    let events: Vec<String> = runs(&dir).iter().map(|r| r["event"].to_string()).collect();
    assert_eq!(
        events,
        [r#""error""#, r#""interrupted""#, r#""passed""#],
        "{events:?}"
    );
    let lines = runs(&dir);
    assert_eq!(lines[1]["record"]["rerun"], true);
    assert!(
        lines[2]["record"].get("rerun").is_none(),
        "이어 돈 실행은 자동이다"
    );
}

/// `runs.jsonl` 의 줄들.
fn runs(dir: &Path) -> Vec<serde_json::Value> {
    std::fs::read_to_string(dir.join("runs.jsonl"))
        .unwrap_or_default()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

#[tokio::test]
async fn a_failed_commit_runs_again_only_when_asked_and_every_run_is_kept() {
    let f = fx();
    let tmp = tempfile::tempdir().unwrap();
    let (_origin, work) = repos(tmp.path());
    f.store.ensure_board("proj", None, "t").unwrap();
    f.store
        .set_board_path("proj", work.to_str().unwrap(), "t")
        .unwrap();
    let root = tmp.path().join("verify");
    // 트리 밖의 파일을 보는 단계 — 커밋은 그대로인데 환경만 바뀌는 거짓 실패를 흉내 낸다.
    let flag = tmp.path().join("env-ok");
    let target = VerifyTarget {
        board: "proj".into(),
        branch: "main".into(),
        deploy: false,
        steps: vec![step("env", &format!("test -f {}", flag.display()), None)],
    };
    let dir = rockyd::verify::target_dir(&root, &target);
    let runner = default_runner();
    let (notifier, seen) = capture();

    verify_target(&f.state, &runner, &notifier, &root, &target).await;
    assert_eq!(
        f.state.verify()[0].record.clone().unwrap().state,
        VerifyState::Failed
    );
    std::fs::write(&flag, "").unwrap();
    verify_target(&f.state, &runner, &notifier, &root, &target).await;
    assert_eq!(
        f.state.verify()[0].record.clone().unwrap().state,
        VerifyState::Failed,
        "부탁하지 않으면 같은 커밋은 다시 돌지 않는다"
    );

    // 원격(프록시를 거친) 요청은 프로세스를 띄우지 못한다; 없는 대상은 404.
    let (code, _) = call(
        &f.state,
        "POST",
        "/api/verify/rerun",
        Some(serde_json::json!({})),
        ReqOptions {
            headers: vec![("x-forwarded-for", "203.0.113.7")],
            ..ReqOptions::default()
        },
    )
    .await;
    assert_eq!(code, 403);
    let (code, body) = post(
        &f.state,
        "/api/verify/rerun",
        serde_json::json!({ "board": "nope" }),
    )
    .await;
    assert_eq!(code, 404, "{body}");
    for bad in [
        serde_json::json!({ "board": 123 }),
        serde_json::json!({ "board": "  " }),
    ] {
        let (code, body) = post(&f.state, "/api/verify/rerun", bad.clone()).await;
        assert_eq!(code, 400, "잘못 준 필터는 '전부' 가 아니다: {bad} → {body}");
    }
    assert!(!f.state.verify_rerun_requested("proj", "main"));

    let (code, body) = call(
        &f.state,
        "POST",
        "/api/verify/rerun",
        None,
        ReqOptions::default(),
    )
    .await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body["queued"][0]["board"], "proj");
    verify_target(&f.state, &runner, &notifier, &root, &target).await;
    assert_eq!(
        f.state.verify()[0].record.clone().unwrap().state,
        VerifyState::Passed
    );
    assert!(
        !f.state.verify_rerun_requested("proj", "main"),
        "돈 뒤에는 요청을 지운다"
    );
    assert!(seen.lock().unwrap()[1].contains("다시 초록"));

    // 첫 실행은 자동 재시도까지 두 번 실패, 다시 돌린 실행은 통과.
    let lines = runs(&dir);
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert_eq!(lines[0]["event"], "failed");
    assert_eq!(lines[0]["record"]["failedStep"], "env");
    assert!(lines[0]["record"].get("attempt").is_none());
    assert!(lines[0]["record"].get("rerun").is_none());
    assert_eq!(lines[1]["event"], "failed");
    assert_eq!(lines[1]["record"]["attempt"], 2);
    assert_eq!(lines[2]["event"], "passed");
    assert_eq!(lines[2]["record"]["rerun"], true);
    assert_eq!(lines[0]["record"]["sha"], lines[2]["record"]["sha"]);
    // 다시 돌린 실행은 같은 로그에 이어 쓴다 — 앞선 실패의 출력이 남는다.
    assert_eq!(lines[0]["record"]["log"], lines[2]["record"]["log"]);
    let log = std::fs::read_to_string(lines[2]["record"]["log"].as_str().unwrap()).unwrap();
    assert!(log.contains("### 자동 재시도 — env 실패"), "{log}");
    assert!(
        log.contains("### 같은 커밋을 다시 — 다시 돌리기 요청"),
        "{log}"
    );

    // 데몬이 죽어 `running` 기록만 남은 대상은 도는 중이 아니다 — 맡는다.
    let mut status = f.state.verify();
    status[0].record.as_mut().unwrap().state = VerifyState::Running;
    f.state.set_verify(status);
    let (code, body) = post(&f.state, "/api/verify/rerun", serde_json::json!({})).await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body["queued"][0]["board"], "proj");

    // 실제로 도는 중인 대상은 맡지 않는다 — 돌기 시작하며 앞서 맡긴 요청은 흡수된다.
    f.state.begin_verify("proj", "main");
    assert!(!f.state.verify_rerun_requested("proj", "main"));
    let (code, body) = post(
        &f.state,
        "/api/verify/rerun",
        serde_json::json!({ "board": "proj", "branch": "main" }),
    )
    .await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body["running"][0]["board"], "proj");
    assert!(body["queued"].as_array().unwrap().is_empty());
    assert!(!f.state.verify_rerun_requested("proj", "main"));
}

#[tokio::test]
async fn a_failure_that_passes_on_the_automatic_retry_stays_green_and_quiet() {
    let f = fx();
    let tmp = tempfile::tempdir().unwrap();
    let (_origin, work) = repos(tmp.path());
    f.store.ensure_board("proj", None, "t").unwrap();
    f.store
        .set_board_path("proj", work.to_str().unwrap(), "t")
        .unwrap();
    let root = tmp.path().join("verify");
    // 처음 한 번만 실패하는 단계 — 부하로 시간 초과가 난 셈.
    let marker = tmp.path().join("tried");
    let script = format!(
        "if [ -f {m} ]; then exit 0; else touch {m}; exit 1; fi",
        m = marker.display()
    );
    let target = VerifyTarget {
        board: "proj".into(),
        branch: "main".into(),
        deploy: false,
        steps: vec![step("flaky", &script, None)],
    };
    let dir = rockyd::verify::target_dir(&root, &target);
    let (notifier, seen) = capture();

    verify_target(&f.state, &default_runner(), &notifier, &root, &target).await;
    let rec = f.state.verify()[0].record.clone().unwrap();
    assert_eq!(rec.state, VerifyState::Passed);
    assert_eq!(rec.attempt, 2);
    assert!(seen.lock().unwrap().is_empty(), "첫 실패는 알리지 않는다");
    let log = std::fs::read_to_string(&rec.log).unwrap();
    assert!(
        log.contains("### 자동 재시도 — flaky 실패: 종료 코드 1"),
        "{log}"
    );
    let lines = runs(&dir);
    let events: Vec<_> = lines.iter().map(|l| l["event"].as_str().unwrap()).collect();
    assert_eq!(events, ["failed", "passed"], "{lines:?}");
    assert_eq!(lines[1]["record"]["attempt"], 2);
}

#[tokio::test]
async fn the_same_reason_for_not_verifying_is_kept_once() {
    let f = fx();
    let tmp = tempfile::tempdir().unwrap();
    f.store.ensure_board("bare", None, "t").unwrap();
    let root = tmp.path().join("verify");
    let target = VerifyTarget {
        board: "bare".into(),
        branch: "main".into(),
        deploy: false,
        steps: vec![step("noop", "true", None)],
    };
    let (notifier, _seen) = capture();
    for _ in 0..3 {
        verify_target(&f.state, &default_runner(), &notifier, &root, &target).await;
    }
    // 데몬이 다시 떠도(상태를 파일에서 다시 읽음) 이어지는 같은 이유는 다시 쓰지 않는다.
    let cfg = rocky_core::verify::VerifyConfig {
        enabled: None,
        interval_seconds: None,
        targets: vec![target.clone()],
    };
    f.state
        .set_verify(rockyd::verify::load_statuses(&cfg, &root));
    verify_target(&f.state, &default_runner(), &notifier, &root, &target).await;
    let lines = runs(&rockyd::verify::target_dir(&root, &target));
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0]["event"], "error");
    assert!(lines[0]["error"].as_str().unwrap().contains("path"));
}

#[tokio::test]
async fn a_recorded_group_whose_number_was_reused_is_left_alone_and_logged() {
    let f = fx();
    let tmp = tempfile::tempdir().unwrap();
    let (_origin, work) = repos(tmp.path());
    f.store.ensure_board("proj", None, "t").unwrap();
    f.store
        .set_board_path("proj", work.to_str().unwrap(), "t")
        .unwrap();
    let root = tmp.path().join("verify");
    let target = VerifyTarget {
        board: "proj".into(),
        branch: "main".into(),
        deploy: false,
        steps: vec![step("ok", "true", None)],
    };
    let dir = rockyd::verify::target_dir(&root, &target);
    std::fs::create_dir_all(&dir).unwrap();
    // 남이 쓰는 그룹 — 자기 그룹의 리더로 띄운 sleep. 기록된 시작 시각은 그 프로세스와 다르다(번호 재사용과 같은 상황).
    let mut other = std::process::Command::new("sleep")
        .arg("30")
        .process_group(0)
        .spawn()
        .unwrap();
    std::fs::write(
        dir.join("running.pgid"),
        format!("{}\tThu Jan  1 09:00:00 1970", other.id()),
    )
    .unwrap();
    let (notifier, _seen) = capture();
    verify_target(&f.state, &default_runner(), &notifier, &root, &target).await;
    let alive = other.try_wait().unwrap().is_none();
    let _ = other.kill();
    let _ = other.wait();
    assert!(alive, "번호가 재사용된 그룹은 건드리지 않는다");
    let signals = std::fs::read_to_string(dir.join("signals.log")).unwrap();
    assert!(signals.contains("건너뜀(번호 재사용)"), "{signals}");
    assert!(!dir.join("running.pgid").exists());
}

/// 검증 결과 구독 — 구독한 세션 받은편지함에 끝난 실행마다(통과도) 한 줄, 전달 기록에 `verify-*`. 받을 등록이 없으면
/// 사유만 남긴다. 구독 바꾸기는 로컬 전용이고 세션이 꼭 있어야 한다.
#[tokio::test]
async fn subscribed_session_hears_each_finished_run() {
    use std::io::Read;
    use std::os::unix::net::UnixListener;

    let f = fx();
    let tmp = tempfile::tempdir().unwrap();
    let (_origin, work) = repos(tmp.path());
    f.store.ensure_board("proj", None, "t").unwrap();
    f.store
        .set_board_path("proj", work.to_str().unwrap(), "t")
        .unwrap();
    let root = tmp.path().join("verify");
    let target = VerifyTarget {
        board: "proj".into(),
        branch: "main".into(),
        deploy: false,
        steps: vec![step("has-ok", "test -f ok.txt", None)],
    };
    let runner = default_runner();
    let (notifier, _) = capture();

    let dir = std::path::PathBuf::from(format!("/tmp/cc-socks-rockyverify-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let sock = dir.join("800.sock");
    let listener = Arc::new(UnixListener::bind(&sock).unwrap());
    post(
        &f.state,
        "/api/sessions/inbox",
        serde_json::json!({ "sessionId": "deployer", "socket": sock.to_str().unwrap(), "cwd": "/w/proj" }),
    )
    .await;
    f.store
        .subscribe_verify("proj", "main", "deployer")
        .unwrap();
    listener.set_nonblocking(true).unwrap();
    // 10초 안에 안 오면 실패 — 알림이 빠져도 테스트가 멈춰 서지 않게.
    let read_one = |l: Arc<UnixListener>| {
        tokio::task::spawn_blocking(move || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            let mut conn = loop {
                match l.accept() {
                    Ok((conn, _)) => break conn,
                    Err(_) if std::time::Instant::now() < deadline => {
                        std::thread::sleep(std::time::Duration::from_millis(20))
                    }
                    Err(e) => panic!("받은편지함에 10초 안에 아무것도 안 왔다: {e}"),
                }
            };
            conn.set_nonblocking(false).unwrap();
            let mut text = String::new();
            conn.read_to_string(&mut text).unwrap();
            text
        })
    };

    let first = read_one(listener.clone());
    verify_target(&f.state, &runner, &notifier, &root, &target).await;
    let line = first.await.unwrap();
    assert!(
        line.contains("proj main 검증") && line.contains("통과"),
        "{line}"
    );

    commit(&work, "ok 를 지운다", |w| {
        std::fs::remove_file(w.join("ok.txt")).unwrap()
    });
    let second = read_one(listener.clone());
    verify_target(&f.state, &runner, &notifier, &root, &target).await;
    let line = second.await.unwrap();
    assert!(line.contains("실패(has-ok)"), "{line}");
    assert!(line.contains("배포하지 않는다"), "{line}");
    let (_, deliveries) = get(&f.state, "/api/deliveries").await;
    assert_eq!(deliveries["recent"][0]["kind"], "verify-failed");
    assert_eq!(deliveries["recent"][1]["kind"], "verify-passed");

    // 데몬이 배포하는 대상(rocky 레포·main 빌드 모드·deploy)이면 통과 문구가 세션에게 배포하지 말라고 한다
    f.store
        .set_board_repo("proj", "minjun0219/rocky", "t")
        .unwrap();
    let env = deploy_env(tmp.path(), &head(&work));
    let deploying = VerifyTarget {
        deploy: true,
        ..target.clone()
    };
    commit(&work, "ok 를 되살린다", |w| {
        std::fs::write(w.join("ok.txt"), "1").unwrap()
    });
    let third = read_one(listener.clone());
    verify_target_with(&f.state, &runner, &notifier, &root, &deploying, Some(&env)).await;
    let line = third.await.unwrap();
    assert!(line.contains("데몬이 배포한다"), "{line}");
    assert!(!line.contains("배포를 맡은 세션이면"), "{line}");

    // 받을 등록이 없는 세션으로 넘어가면 — 사유만 남긴다
    f.store.subscribe_verify("proj", "main", "gone").unwrap();
    commit(&work, "c 를 더한다", |w| {
        std::fs::write(w.join("c.txt"), "1").unwrap()
    });
    verify_target(&f.state, &runner, &notifier, &root, &target).await;
    let (_, deliveries) = get(&f.state, "/api/deliveries").await;
    assert_eq!(deliveries["recent"][0]["ok"], false);
    assert_eq!(deliveries["recent"][0]["reason"], "받을 세션 등록 없음");

    // 라우트 — 세션 없는 구독·원격 요청은 거절, 원격 조회는 세션 id 를 가린다
    let (code, _) = post(
        &f.state,
        "/api/verify/subscriptions",
        serde_json::json!({ "board": "proj" }),
    )
    .await;
    assert_eq!(code, 400);
    let (code, _) = post(
        &f.state,
        "/api/verify/subscriptions",
        serde_json::json!({ "board": "nope", "sessionId": "x" }),
    )
    .await;
    assert_eq!(code, 404);
    let (code, body) = post(
        &f.state,
        "/api/verify/subscriptions",
        serde_json::json!({ "sessionId": "deployer" }),
    )
    .await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body["subscribed"][0]["sessionId"], "deployer");
    // 다른 세션("gone")이 맡고 있던 것을 넘겨받았다고 알리고, 지금 결과를 같이 싣는다
    assert_eq!(body["subscribed"][0]["previousSessionId"], "gone");
    assert_eq!(body["subscribed"][0]["record"]["state"], "passed");
    let remote = |method: &'static str, path: &'static str, body: Option<serde_json::Value>| {
        call(
            &f.state,
            method,
            path,
            body,
            ReqOptions {
                peer: Some("100.64.0.1"),
                ..ReqOptions::default()
            },
        )
    };
    let (code, _) = remote(
        "POST",
        "/api/verify/subscriptions",
        Some(serde_json::json!({ "sessionId": "x" })),
    )
    .await;
    assert_eq!(code, 403);
    let (_, listed) = remote("GET", "/api/verify/subscriptions", None).await;
    assert_eq!(listed[0]["board"], "proj");
    assert!(listed[0].get("sessionId").is_none());
    // 세션 범위 해지는 남의 구독을 걷지 않는다
    let (_, body) = call(
        &f.state,
        "DELETE",
        "/api/verify/subscriptions?sessionId=someone-else",
        None,
        ReqOptions::default(),
    )
    .await;
    assert_eq!(body["removed"], 0);
    let (code, body) = call(
        &f.state,
        "DELETE",
        "/api/verify/subscriptions?board=proj",
        None,
        ReqOptions::default(),
    )
    .await;
    assert_eq!(code, 200);
    assert_eq!(body["removed"], 1);
    let _ = std::fs::remove_dir_all(&dir);
}

fn head(work: &Path) -> String {
    let out = std::process::Command::new("git")
        .args(["-C", work.to_str().unwrap(), "rev-parse", "HEAD"])
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .unwrap();
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

/// 배포 손 — main 빌드 모드 데이터 홈(`current` → `main`, 깔린 빌드 `base`)과 가짜 `rocky`. 가짜는 `update --main <sha>` 를
/// `calls` 에 남기고, `hold` 가 있는 동안 기다렸다가, `fail` 이 있으면 실패하고 아니면 `main/.finished` 에 그 커밋을 쓴다
/// (update 가 끝까지 간 것처럼). 파일은 전부 데이터 홈 기준이다 — update 는 거기서 뜬다.
fn deploy_env(tmp: &Path, base: &str) -> DeployEnv {
    use std::os::unix::fs::PermissionsExt;
    let data = tmp.join("data");
    std::fs::create_dir_all(data.join("main")).unwrap();
    std::os::unix::fs::symlink("main", data.join("current")).unwrap();
    std::fs::write(data.join("main/.sha"), format!("{base}\n")).unwrap();
    std::fs::write(data.join("main/.finished"), format!("{base}\n")).unwrap();
    let rocky = tmp.join("fake-rocky");
    std::fs::write(
        &rocky,
        "#!/bin/sh\necho \"$*\" >> calls\nwhile [ -f hold ]; do sleep 0.05; done\n\
         if [ -f fail ]; then echo 빌드 실패 >&2; exit 1; fi\nprintf '%s\\n' \"$3\" > main/.finished\n",
    )
    .unwrap();
    std::fs::set_permissions(&rocky, std::fs::Permissions::from_mode(0o755)).unwrap();
    DeployEnv::new(data, rocky)
}

/// rocky 레포 보드 — 배포는 이 레포의 main 대상만 돈다.
fn rocky_board(f: &Fx, work: &Path) {
    f.store.ensure_board("rocky", None, "t").unwrap();
    f.store
        .set_board_path("rocky", work.to_str().unwrap(), "t")
        .unwrap();
    f.store
        .set_board_repo("rocky", "minjun0219/rocky", "t")
        .unwrap();
}

fn deploy_target() -> VerifyTarget {
    VerifyTarget {
        board: "rocky".into(),
        branch: "main".into(),
        steps: vec![step("has-ok", "test -f ok.txt", None)],
        deploy: true,
    }
}

fn deploy_status(f: &Fx) -> rockyd::verify::VerifyTargetStatus {
    f.state
        .verify()
        .into_iter()
        .find(|s| s.board == "rocky")
        .expect("rocky 대상 상태")
}

fn calls(env: &DeployEnv) -> String {
    std::fs::read_to_string(env.data_root.join("calls")).unwrap_or_default()
}

/// 배포가 끝날 때까지 바퀴를 돌린다(10초 안에).
async fn until_settled(
    f: &Fx,
    notifier: &VerifyNotifier,
    root: &Path,
    cfg: &VerifyConfig,
    env: &DeployEnv,
) -> DeployRecord {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        tick(&f.state, &default_runner(), notifier, root, cfg, env).await;
        let deploy = deploy_status(f).deploy;
        if let Some(done) = deploy.clone().filter(|d| d.state != DeployState::Running) {
            return done;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "배포가 10초 안에 끝나지 않았다: {deploy:?}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn a_passed_commit_is_deployed_once_by_the_cli_beside_the_daemon() {
    let f = fx();
    let tmp = tempfile::tempdir().unwrap();
    let (_origin, work) = repos(tmp.path());
    rocky_board(&f, &work);
    let env = deploy_env(tmp.path(), &head(&work));
    let root = tmp.path().join("verify");
    let cfg = VerifyConfig {
        targets: vec![deploy_target()],
        ..VerifyConfig::default()
    };
    let (notifier, seen) = capture();
    commit(&work, "새 커밋", |w| {
        std::fs::write(w.join("b.txt"), "1").unwrap()
    });
    let sha = head(&work);

    tick(&f.state, &default_runner(), &notifier, &root, &cfg, &env).await;
    let status = deploy_status(&f);
    assert_eq!(status.record.unwrap().state, VerifyState::Passed);
    let started = status.deploy.expect("통과하면 같은 바퀴에 배포를 띄운다");
    assert_eq!(started.sha, sha);
    assert!(started.pid.is_some());
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(&started.log)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600,
        "배포 로그는 소유자만 읽는다"
    );

    let done = until_settled(&f, &notifier, &root, &cfg, &env).await;
    assert_eq!(done.state, DeployState::Succeeded, "{done:?}");
    assert_eq!(
        calls(&env),
        format!("update --main {sha}\n"),
        "--yes 는 넘기지 않는다"
    );
    assert!(
        seen.lock().unwrap().is_empty(),
        "첫 통과와 배포 성공은 조용하다"
    );
    let dir = rockyd::verify::target_dir(&root, &deploy_target());
    let events: Vec<String> = runs(&dir)
        .iter()
        .map(|r| r["event"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(events, ["passed", "deployed"]);

    // 깔린 커밋은 다시 띄우지 않는다.
    tick(&f.state, &default_runner(), &notifier, &root, &cfg, &env).await;
    assert_eq!(calls(&env).lines().count(), 1);
    let (_, body) = get(&f.state, "/api/verify").await;
    assert_eq!(body["targets"][0]["deploy"]["state"], "succeeded");
    assert!(body["targets"][0].get("deployNote").is_none(), "{body}");
}

#[tokio::test]
async fn a_failed_deploy_is_announced_once_and_not_retried() {
    let f = fx();
    let tmp = tempfile::tempdir().unwrap();
    let (_origin, work) = repos(tmp.path());
    rocky_board(&f, &work);
    let env = deploy_env(tmp.path(), &head(&work));
    std::fs::write(env.data_root.join("fail"), "").unwrap();
    let root = tmp.path().join("verify");
    let cfg = VerifyConfig {
        targets: vec![deploy_target()],
        ..VerifyConfig::default()
    };
    let (notifier, seen) = capture();
    commit(&work, "새 커밋", |w| {
        std::fs::write(w.join("b.txt"), "1").unwrap()
    });

    tick(&f.state, &default_runner(), &notifier, &root, &cfg, &env).await;
    let done = until_settled(&f, &notifier, &root, &cfg, &env).await;
    assert_eq!(done.state, DeployState::Failed);
    assert_eq!(done.reason.as_deref(), Some("update 종료 코드 1"));
    assert!(std::fs::read_to_string(&done.log)
        .unwrap()
        .contains("빌드 실패"));
    {
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 1, "{seen:?}");
        assert!(seen[0].contains("배포 실패"), "{seen:?}");
    }

    // 같은 커밋은 다시 띄우지 않고 배너도 한 번뿐이다.
    for _ in 0..2 {
        tick(&f.state, &default_runner(), &notifier, &root, &cfg, &env).await;
    }
    assert_eq!(calls(&env).lines().count(), 1);
    assert_eq!(seen.lock().unwrap().len(), 1);
    let status = deploy_status(&f);
    assert_eq!(status.deploy.unwrap().state, DeployState::Failed);
    assert!(
        status.deploy_note.is_none(),
        "이미 시도한 커밋은 이유를 따로 보이지 않는다"
    );
}

#[tokio::test]
async fn deploys_wait_for_the_build_lock_and_verification_waits_for_the_deploy() {
    let f = fx();
    let tmp = tempfile::tempdir().unwrap();
    let (_origin, work) = repos(tmp.path());
    rocky_board(&f, &work);
    let env = deploy_env(tmp.path(), &head(&work));
    let root = tmp.path().join("verify");
    let cfg = VerifyConfig {
        targets: vec![deploy_target()],
        ..VerifyConfig::default()
    };
    let (notifier, _seen) = capture();
    commit(&work, "A", |w| {
        std::fs::write(w.join("a.txt"), "1").unwrap()
    });
    let a = head(&work);

    // 세션의 update 가 잠금을 쥐고 있다(pid 없는 잠금은 쥔 것으로 본다) — 띄우지 않고 이유를 보인다.
    let lock = env.data_root.join("main-sync.lock");
    std::fs::create_dir(&lock).unwrap();
    tick(&f.state, &default_runner(), &notifier, &root, &cfg, &env).await;
    let status = deploy_status(&f);
    assert_eq!(status.record.unwrap().sha, a);
    assert!(status.deploy.is_none());
    assert!(
        status
            .deploy_note
            .as_deref()
            .unwrap_or("")
            .contains("main-sync.lock"),
        "{:?}",
        status.deploy_note
    );
    assert_eq!(calls(&env), "");

    // 잠금이 풀리면 띄운다. 도는 동안 들어온 커밋은 검증하지 않는다(update 가 데몬을 교체한다).
    std::fs::remove_dir(&lock).unwrap();
    std::fs::write(env.data_root.join("hold"), "").unwrap();
    tick(&f.state, &default_runner(), &notifier, &root, &cfg, &env).await;
    assert_eq!(
        deploy_status(&f).deploy.unwrap().state,
        DeployState::Running
    );
    assert!(deploy_status(&f).deploy_note.is_none());
    commit(&work, "B", |w| {
        std::fs::write(w.join("b.txt"), "1").unwrap()
    });
    let b = head(&work);
    tick(&f.state, &default_runner(), &notifier, &root, &cfg, &env).await;
    assert_eq!(
        deploy_status(&f).record.unwrap().sha,
        a,
        "배포가 도는 동안 새 검증을 시작하지 않는다"
    );

    // 끝나면 같은 바퀴에 다음 커밋을 검증하고, 깔린 A 가 B 의 조상이라 B 도 올린다.
    std::fs::remove_file(env.data_root.join("hold")).unwrap();
    let last = until_settled(&f, &notifier, &root, &cfg, &env).await;
    assert_eq!(
        (last.sha.as_str(), last.state),
        (b.as_str(), DeployState::Succeeded)
    );
    let dir = rockyd::verify::target_dir(&root, &deploy_target());
    let deployed: Vec<String> = runs(&dir)
        .iter()
        .filter(|r| r["event"] == "deployed")
        .map(|r| r["deploy"]["sha"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(deployed, [a.clone(), b.clone()]);
    assert_eq!(
        calls(&env),
        format!("update --main {a}\nupdate --main {b}\n")
    );
}

#[tokio::test]
async fn a_new_daemon_settles_a_deploy_it_did_not_start_by_the_finished_build() {
    let f = fx();
    let tmp = tempfile::tempdir().unwrap();
    let (_origin, work) = repos(tmp.path());
    rocky_board(&f, &work);
    let sha = head(&work);
    let env = deploy_env(tmp.path(), &sha);
    let root = tmp.path().join("verify");
    let target = deploy_target();
    let dir = rockyd::verify::target_dir(&root, &target);
    std::fs::create_dir_all(&dir).unwrap();
    let cfg = VerifyConfig {
        targets: vec![target],
        ..VerifyConfig::default()
    };
    let (notifier, seen) = capture();
    // 옛 데몬이 띄운 update — 끝났고(이 데몬은 종료 코드를 모른다) main/.finished 가 그 커밋이다.
    let mut gone = std::process::Command::new("true").spawn().unwrap();
    let gone_pid = gone.id();
    gone.wait().unwrap();
    let running = |pid: u32, started: &str| {
        serde_json::json!({ "sha": sha, "state": "running", "pid": pid, "pidStarted": started,
                            "startedAt": "2026-10-18T00:00:00.000Z", "log": "/tmp/deploy.log" })
        .to_string()
    };
    std::fs::write(dir.join("deploy.json"), running(gone_pid, "x")).unwrap();
    tick(&f.state, &default_runner(), &notifier, &root, &cfg, &env).await;
    let done = deploy_status(&f).deploy.unwrap();
    assert_eq!(done.state, DeployState::Succeeded, "{done:?}");

    // 살아 있는 프로세스라도 시작 시각이 다르면 번호를 재사용한 남이다 — 끝난 것으로 판정한다. 이번엔 .finished 가 다르다.
    let mut other = std::process::Command::new("sleep")
        .arg("30")
        .spawn()
        .unwrap();
    std::fs::write(
        env.data_root.join("main/.finished"),
        "0000000000000000000000000000000000000000\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("deploy.json"),
        running(other.id(), "Thu Jan  1 00:00:00 1970"),
    )
    .unwrap();
    tick(&f.state, &default_runner(), &notifier, &root, &cfg, &env).await;
    let _ = other.kill();
    let _ = other.wait();
    let failed = deploy_status(&f).deploy.unwrap();
    assert_eq!(failed.state, DeployState::Failed, "{failed:?}");
    assert!(failed.reason.unwrap().contains("main/.finished"));
    assert_eq!(seen.lock().unwrap().len(), 1, "실패는 배너로");
    let events: Vec<String> = runs(&dir)
        .iter()
        .map(|r| r["event"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        events,
        ["deployed", "passed", "deploy-failed"],
        "{events:?}"
    );
}

#[tokio::test]
async fn nothing_is_deployed_off_main_build_mode_and_the_reason_shows() {
    let f = fx();
    let tmp = tempfile::tempdir().unwrap();
    let (_origin, work) = repos(tmp.path());
    rocky_board(&f, &work);
    let env = deploy_env(tmp.path(), &head(&work));
    std::fs::remove_file(env.data_root.join("current")).unwrap();
    std::os::unix::fs::symlink("v0.52.0", env.data_root.join("current")).unwrap();
    let root = tmp.path().join("verify");
    let cfg = VerifyConfig {
        targets: vec![deploy_target()],
        ..VerifyConfig::default()
    };
    let (notifier, _seen) = capture();
    commit(&work, "새 커밋", |w| {
        std::fs::write(w.join("b.txt"), "1").unwrap()
    });
    tick(&f.state, &default_runner(), &notifier, &root, &cfg, &env).await;
    let status = deploy_status(&f);
    assert_eq!(status.record.unwrap().state, VerifyState::Passed);
    assert!(status.deploy.is_none());
    assert!(
        status
            .deploy_note
            .as_deref()
            .unwrap_or("")
            .contains("main 빌드 모드"),
        "{:?}",
        status.deploy_note
    );
    assert_eq!(calls(&env), "");
}
