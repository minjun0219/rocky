//! rc 서버 기동 로그 — 커진 로그만 비우고, 비운 뒤 서버가 처음부터 다시 쓰는지(append) 본다.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rocky_core::config::RcConfig;
use rocky_core::rc::SERVER_LOG_CAP;
use rockyd::rc::{default_ops, RcCommand, RcController};

fn controller(rc_dir: &Path) -> Arc<RcController> {
    Arc::new(RcController::new(
        Some(RcConfig {
            root: Some("/w".into()),
            pinned: vec![],
            targets: vec!["busy".into()],
            supervise: false,
            nightly: None,
        }),
        "/nonexistent-home".into(),
        rc_dir.to_path_buf(),
        rockyd::runner::default_runner(),
        default_ops(),
    ))
}

fn big() -> Vec<u8> {
    vec![b'x'; SERVER_LOG_CAP as usize + 1]
}

fn wait_until(what: &str, mut ok: impl FnMut() -> bool) {
    let start = Instant::now();
    while !ok() {
        assert!(start.elapsed() < Duration::from_secs(5), "{what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn trims_only_grown_server_logs() {
    let dir = tempfile::tempdir().unwrap();
    let rc_dir = dir.path().join("rc");
    std::fs::create_dir_all(&rc_dir).unwrap();
    for name in [
        "a.out",
        "handoff-rocky-1.err",
        "busy.out",
        "nightly.json",
        "a.version",
    ] {
        std::fs::write(rc_dir.join(name), big()).unwrap();
    }
    std::fs::write(rc_dir.join("b.out"), "·✔︎· Ready · b · main").unwrap();
    // 링크는 따라가지 않는다 — 가리키는 파일이 크더라도. 링크 자신의 크기(lstat)가 작아 크기 검사에서 먼저 빠지고,
    // `is_file()` 은 그 검사가 링크를 따라가게 바뀌어도 막는 두 번째 방어다.
    let outside = dir.path().join("outside.txt");
    std::fs::write(&outside, big()).unwrap();
    std::os::unix::fs::symlink(&outside, rc_dir.join("link.out")).unwrap();

    let control = controller(&rc_dir);
    // 지금 띄우는 중인 대상 — 새 서버의 등록 출력을 지우지 않게 건너뛴다.
    control.begin("busy", RcCommand::Start).unwrap();
    let mut trimmed = control.trim_logs();
    trimmed.sort();
    assert_eq!(trimmed, vec!["a.out", "handoff-rocky-1.err"]);
    let len = |p: &Path| std::fs::metadata(p).unwrap().len();
    assert_eq!(len(&rc_dir.join("a.out")), 0);
    assert_eq!(len(&rc_dir.join("handoff-rocky-1.err")), 0);
    assert_eq!(
        len(&rc_dir.join("b.out")),
        "·✔︎· Ready · b · main".len() as u64,
        "작은 로그는 그대로"
    );
    for name in ["busy.out", "nightly.json", "a.version"] {
        assert_eq!(len(&rc_dir.join(name)), SERVER_LOG_CAP + 1, "{name}");
    }
    assert_eq!(len(&outside), SERVER_LOG_CAP + 1);

    let events = std::fs::read_to_string(rc_dir.join("events.jsonl")).unwrap();
    let line: serde_json::Value = serde_json::from_str(events.lines().last().unwrap()).unwrap();
    assert_eq!(line["event"], "log-trim");
    let mut files: Vec<&str> = line["fields"]["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f.as_str().unwrap())
        .collect();
    files.sort();
    assert_eq!(files, vec!["a.out", "handoff-rocky-1.err"]);

    assert!(control.trim_logs().is_empty());
    let after = std::fs::read_to_string(rc_dir.join("events.jsonl")).unwrap();
    assert_eq!(after, events, "비운 게 없으면 기록하지 않는다");
}

#[test]
fn a_trimmed_log_starts_over_instead_of_leaving_a_hole() {
    let dir = tempfile::tempdir().unwrap();
    let (out, err) = (dir.path().join("x.out"), dir.path().join("x.err"));
    std::fs::write(&out, "지난 기동의 출력").unwrap();
    let ops = default_ops();
    let argv: Vec<String> = ["sh", "-c", "printf aaaa; sleep 1; printf b"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let pid = (ops.spawn)(&argv, dir.path(), &out, &err).unwrap();
    wait_until("첫 출력", || std::fs::read(&out).unwrap() == b"aaaa");
    std::fs::OpenOptions::new()
        .write(true)
        .open(&out)
        .unwrap()
        .set_len(0)
        .unwrap();
    wait_until("프로세스가 끝난다", || !(ops.signal)(pid, 0));
    assert_eq!(
        std::fs::read(&out).unwrap(),
        b"b",
        "앞이 0 으로 채워지지 않는다"
    );
}
