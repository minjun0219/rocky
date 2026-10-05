//! 기본 브랜치 검증 잡 — 임시 원격(bare) + 보드 레포(clone)로 끝까지 돈다: 통과·실패·같은 커밋 생략·복구 알림·시간 초과.

use std::os::unix::process::CommandExt;
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::common::*;
use rocky_core::config::CommandBridge;
use rocky_core::verify::{VerifyState, VerifyTarget};
use rockyd::runner::default_runner;
use rockyd::verify::{verify_target, VerifyNotifier};

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
