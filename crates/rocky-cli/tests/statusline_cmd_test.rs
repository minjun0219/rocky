//! `rocky statusline` — statusline 자리는 실패해도 조용해야 한다: 데몬이 없거나 입력이 깨져도 빈 출력 +
//! 성공. 에러 줄이 statusline 에 새면 사용자 화면이 망가진다.

use std::io::Write;
use std::process::{Command, Stdio};

fn run(args: &[&str], stdin: &str) -> (i32, String, String) {
    let dir = tempfile::tempdir().unwrap();
    // 아무도 안 듣는 포트 — 데몬 없음.
    let config = dir.path().join("rocky.json");
    std::fs::write(
        &config,
        r#"{"todo":{"port":1,"dir":"/nonexistent","expose":"off"}}"#,
    )
    .unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_rocky"))
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
