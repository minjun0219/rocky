//! `rocky statusline` — statusline 자리는 실패해도 조용해야 한다: 데몬이 없거나 입력이 깨져도 빈 출력 +
//! 성공. 에러 줄이 statusline 에 새면 사용자 화면이 망가진다.

use std::io::Write;
use std::process::{Command, Stdio};

/// 테스트가 띄우는 바이너리를 사용자 환경에서 떼어 낸다 — 사용 로그(`~/.config/rocky/usage`)와 홈을 임시
/// 디렉터리로. 안 그러면 테스트 실행이 실제 `rocky usage` 숫자에 섞인다(한때 `board pr-authors` 오류 177건).
fn isolate<'a>(cmd: &'a mut Command, dir: &std::path::Path) -> &'a mut Command {
    cmd.env("HOME", dir)
        .env("ROCKY_USAGE_DIR", dir.join("usage"))
}

fn run(args: &[&str], stdin: &str) -> (i32, String, String) {
    let dir = tempfile::tempdir().unwrap();
    // 아무도 안 듣는 포트 — 데몬 없음.
    let config = dir.path().join("rocky.json");
    std::fs::write(
        &config,
        r#"{"todo":{"port":1,"dir":"/nonexistent","expose":"off"}}"#,
    )
    .unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rocky"));
    isolate(&mut cmd, dir.path());
    let mut child = cmd
        .args(args)
        .env("ROCKY_CONFIG", &config)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn no_daemon_or_bad_input_is_silent_success() {
    for (args, stdin) in [
        (vec!["statusline", "--cwd", "/tmp", "--session", "s"], ""),
        (
            vec!["statusline"],
            r#"{"workspace":{"current_dir":"/tmp"},"session_id":"s"}"#,
        ),
        (vec!["statusline"], "not json"),
    ] {
        let (code, out, err) = run(&args, stdin);
        assert_eq!(code, 0, "{args:?}: {err}");
        assert_eq!(out, "", "{args:?}");
        assert_eq!(err, "", "{args:?}");
    }
}

/// `board pr-authors` 의 인자 조합 — 로그인들, 또는 `--clear` 중 하나만. 둘 다·둘 다 없음은 사용법 오류로
/// 데몬에 가기 전에 막는다(데몬이 없으니 통과한 조합은 연결 오류로 끝난다 — 사용법 오류가 아니면 통과다).
#[test]
fn board_pr_authors_accepts_logins_or_clear_but_not_both() {
    let usage = |args: &[&str]| {
        let (code, _, err) = run(args, "");
        code != 0 && err.contains("usage: rocky board")
    };
    assert!(!usage(&["board", "pr-authors", "@me", "--board", "x"]));
    assert!(!usage(&["board", "pr-authors", "--clear", "--board", "x"]));
    assert!(usage(&[
        "board",
        "pr-authors",
        "@me",
        "--clear",
        "--board",
        "x"
    ]));
    assert!(usage(&["board", "pr-authors", "--board", "x"]));
}

/// 예전 `rocky update REF --title …`(할 일 수정)을 습관대로 부르면 플러그인 업데이트를 돌리지 않고 `edit` 으로
/// 안내한다 — 업데이트는 플러그인을 올리고 데몬을 교체해 되돌리기 어렵다.
#[test]
fn old_todo_update_form_is_refused_with_a_pointer_to_edit() {
    for args in [
        &["update", "rocky-1"][..],
        &["update", "rocky-1", "--title", "새 제목"][..],
        &["update", "--priority", "p1"][..],
    ] {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("rocky.json");
        std::fs::write(
            &config,
            r#"{"todo":{"port":1,"dir":"/nonexistent","expose":"off"}}"#,
        )
        .unwrap();
        // PATH 를 비워 둔다 — 만에 하나 안내 대신 업데이트로 빠져도 `gh`·`claude` 를 못 찾아 아무것도 바꾸지 못한다.
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_rocky"));
        isolate(&mut cmd, dir.path());
        let out = cmd
            .args(args)
            .env("ROCKY_CONFIG", &config)
            .env("PATH", dir.path())
            .output()
            .unwrap();
        let err = String::from_utf8_lossy(&out.stderr);
        assert_ne!(out.status.code(), Some(0), "{args:?}");
        assert!(err.contains("rocky edit REF"), "{args:?}: {err}");
        // 업데이트는 첫 줄에 `최신 릴리스 X · 이 CLI Y · 데몬 Z` 를 찍는다 — 그 줄이 없어야 한다.
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            !stdout.contains("이 CLI") && !err.contains("이 CLI"),
            "{args:?}: 업데이트로 빠졌다 — {stdout}{err}"
        );
    }
}

/// 격리가 실제로 먹는지 — 사용 로그가 임시 디렉터리에 쓰인다(= 사용자 로그에는 안 쓰인다).
#[test]
fn test_runs_log_usage_into_the_temp_dir_not_the_users() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("rocky.json");
    std::fs::write(
        &config,
        r#"{"todo":{"port":1,"dir":"/nonexistent","expose":"off"}}"#,
    )
    .unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rocky"));
    isolate(&mut cmd, dir.path());
    let _ = cmd
        .args(["edit"])
        .env("ROCKY_CONFIG", &config)
        .output()
        .unwrap();
    let logged = std::fs::read_dir(dir.path().join("usage"))
        .map(|entries| entries.count())
        .unwrap_or(0);
    assert!(
        logged > 0,
        "사용 로그가 임시 디렉터리에 없다 — 격리가 안 먹었다"
    );
}

/// `statusline --full` 을 프로세스째 돌려 cc-usage 골든(`rocky-core/tests/fixtures/cc-usage`)과 바이트 단위로 비교한다 —
/// 설정 블록 읽기·환경 변수(폭·색·시간대)·stdin 읽기까지 실제 경로를 탄다. 크레딧 캐시를 심은 케이스는 CLI 가 아직
/// usage 캐시를 읽지 않아 건너뛴다(렌더는 `rocky-core` 의 대조 테스트가 본다).
#[test]
fn full_replays_cc_usage_goldens() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../rocky-core/tests/fixtures/cc-usage");
    let mut dirs: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    dirs.sort();
    let mut ran = 0;
    let mut failures = Vec::new();
    for case_dir in &dirs {
        let case: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(case_dir.join("case.json")).unwrap())
                .unwrap();
        if !case["usage"].is_null() {
            continue;
        }
        ran += 1;
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        let config = dir.path().join("rocky.json");
        let statusline = serde_json::json!({
            "source": case["config"]["source"],
            "alertPercent": case["config"]["alert_percent"],
        });
        std::fs::write(
            &config,
            serde_json::json!({
                "todo": {"port": 1, "dir": "/nonexistent", "expose": "off"},
                "statusline": statusline,
            })
            .to_string(),
        )
        .unwrap();
        // 환경을 비우고 케이스가 정한 것만 준다 — 사용자 터미널의 TERM·COLUMNS 가 섞이면 바이트가 달라진다.
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_rocky"));
        cmd.env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("HOME", &home)
            .env("ROCKY_USAGE_DIR", dir.path().join("usage"))
            .env("ROCKY_CONFIG", &config)
            .env("ROCKY_STATUSLINE_NOW", case["now"].as_str().unwrap())
            .env("TZ", case["tz"].as_str().unwrap());
        for (k, v) in case["env"].as_object().unwrap() {
            cmd.env(k, v.as_str().unwrap());
        }
        let mut child = cmd
            .args(["statusline", "--full"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdin = case["stdin"]
            .as_str()
            .unwrap()
            .replace("{{HOME}}", home.to_str().unwrap());
        child
            .stdin
            .take()
            .unwrap()
            .write_all(stdin.as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        let mut want = std::fs::read_to_string(case_dir.join("expected.txt")).unwrap();
        for pair in case["allow"].as_array().unwrap() {
            want = want.replace(pair[0].as_str().unwrap(), pair[1].as_str().unwrap());
        }
        let got = String::from_utf8_lossy(&out.stdout);
        if got != want || !out.status.success() {
            failures.push(format!(
                "{}\n  want {want:?}\n  got  {got:?}\n  err  {}",
                case_dir.file_name().unwrap().to_string_lossy(),
                String::from_utf8_lossy(&out.stderr)
            ));
        }
    }
    assert!(ran >= 30, "크레딧 없는 케이스가 {ran}건뿐이다");
    assert!(
        failures.is_empty(),
        "{}건 다름:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
