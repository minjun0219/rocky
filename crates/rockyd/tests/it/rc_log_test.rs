//! rc 서버 기동 로그 — 커진 로그만 비우고, 비운 뒤 서버가 처음부터 다시 쓰는지(append) 본다.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rocky_core::config::RcConfig;
use rocky_core::rc::SERVER_LOG_CAP;
use rockyd::rc::{default_ops, RcController};

fn controller(rc_dir: &Path) -> Arc<RcController> {
    Arc::new(RcController::new(
        Some(RcConfig {
            root: Some("/w".into()),
            pinned: vec![],
            targets: vec![],
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
    for name in ["a.out", "handoff-rocky-1.err", "nightly.json", "a.version"] {
        std::fs::write(rc_dir.join(name), big()).unwrap();
    }
    std::fs::write(rc_dir.join("b.out"), "·✔︎· Ready · b · main").unwrap();
    // 링크는 따라가지 않는다 — 가리키는 파일이 크더라도. 링크 자신의 크기(lstat)가 작아 크기 검사에서 먼저 빠지고,
    // `is_file()` 은 그 검사가 링크를 따라가게 바뀌어도 막는 두 번째 방어다.
    let outside = dir.path().join("outside.txt");
    std::fs::write(&outside, big()).unwrap();
    std::os::unix::fs::symlink(&outside, rc_dir.join("link.out")).unwrap();

    let control = controller(&rc_dir);
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
    for name in ["nightly.json", "a.version"] {
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

/// 여러 스레드가 같은 `events.jsonl` 에 동시에 써도 줄이 섞이지 않는다 — 야간 재시작이 대상 여럿을 함께 내리고
/// 띄우며 이벤트를 남기는 모양. 줄마다 따로 JSON 으로 읽혀야 하고 개수가 맞아야 한다.
#[test]
fn concurrent_event_lines_do_not_interleave() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.jsonl");
    let threads: Vec<_> = (0..8)
        .map(|t| {
            let path = path.clone();
            std::thread::spawn(move || {
                for i in 0..200 {
                    let value = serde_json::json!({
                        "ts": "2026-10-08T00:00:00+00:00",
                        "event": "start",
                        "label": format!("target-{t}"),
                        "fields": { "i": i, "message": "떴다 — 그 세션 이어받기(--session-id)" },
                    });
                    rockyd::rc::append_jsonl(&path, &value).unwrap();
                }
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    let text = std::fs::read_to_string(&path).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 8 * 200);
    for line in lines {
        serde_json::from_str::<serde_json::Value>(line)
            .unwrap_or_else(|e| panic!("섞인 줄: {e}: {line}"));
    }
}
