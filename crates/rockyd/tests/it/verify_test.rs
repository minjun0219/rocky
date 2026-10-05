//! 기본 브랜치 검증 잡 — 임시 원격(bare) + 보드 레포(clone)로 끝까지 돈다: 통과·실패·같은 커밋 생략·복구 알림·시간 초과.

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
        root.join("proj/tree/ok.txt").exists(),
        "전용 워크트리에서 돈다"
    );
    assert!(std::fs::read_to_string(&rec.log)
        .unwrap()
        .contains("### echo"));
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
