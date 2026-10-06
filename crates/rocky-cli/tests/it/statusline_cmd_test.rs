//! `rocky statusline` — statusline 자리는 실패해도 조용해야 한다: 데몬이 없거나 입력이 깨져도 빈 출력 +
//! 성공. 에러 줄이 statusline 에 새면 사용자 화면이 망가진다.

use std::io::Write;
use std::process::{Command, Stdio};

/// `--full` 이 띄우는 detached 갱신이 테스트에서 **실제 토큰·실제 API 에 닿지 않게** 막는 두 겹 — 없는 keychain 항목
/// (macOS keychain 은 HOME 과 무관해 임시 HOME 으로는 못 막는다)과 아무도 안 듣는 usage API 주소.
const TEST_KEYCHAIN: &str = "rocky-test-absent";
const DEAD_USAGE_URL: &str = "http://127.0.0.1:1/usage";

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
/// 설정 블록 읽기·환경 변수(폭·색·시간대)·stdin 읽기·git 과 `extraCommands` 실행까지 실제 경로를 탄다.
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
        ran += 1;
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        // 캡처 때 cc-usage 캐시에 심은 것을 rocky 의 같은 자리(그 계정의 캐시 폴더)에 심는다. 캐시 JSON 은 cc-usage 와
        // 같은 모양이라 그대로 쓴다.
        // 계정 파일이 있으면 그 계정의 캐시 폴더, 없으면 `_`.
        let account = case["account"].as_str();
        if let Some(email) = account {
            std::fs::write(
                home.join(".claude.json"),
                serde_json::json!({"oauthAccount": {"emailAddress": email}}).to_string(),
            )
            .unwrap();
        }
        let bucket = rocky_core::claude_account::cache_bucket(
            &rocky_core::claude_account::cache_slot(&home.join(".cache"), &home.join(".claude")),
            account,
        );
        for (key, file) in [("usage", "usage.json"), ("state", "state.json")] {
            if !case[key].is_null() {
                std::fs::create_dir_all(&bucket).unwrap();
                std::fs::write(bucket.join(file), case[key].to_string()).unwrap();
            }
        }
        // 캡처 때와 같은 셸 줄·git 환경으로 repo 를 다시 만든다(작성자·날짜 고정이라 커밋 해시까지 같다).
        let repo = case["repo"].as_array().unwrap();
        if !repo.is_empty() {
            let project = home.join("project");
            std::fs::create_dir_all(&project).unwrap();
            for line in repo {
                let mut step = Command::new("sh");
                step.args(["-c", line.as_str().unwrap()])
                    .current_dir(&project)
                    .env_clear()
                    .env("PATH", std::env::var("PATH").unwrap_or_default())
                    .env("HOME", &home);
                for (k, v) in case["gitEnv"].as_object().unwrap() {
                    step.env(k, v.as_str().unwrap());
                }
                let out = step.output().unwrap();
                assert!(
                    out.status.success(),
                    "{line}: {}",
                    String::from_utf8_lossy(&out.stderr)
                );
            }
        }
        let config = dir.path().join("rocky.json");
        // cc-usage 설정 키(snake_case)를 rocky.json 의 키(camelCase)로 옮긴다.
        let extra_commands: Vec<serde_json::Value> = case["config"]["extra_commands"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|c| serde_json::json!({ "command": c["command"], "timeoutMs": c["timeout_ms"] }))
            .collect();
        let statusline = serde_json::json!({
            "keychainService": TEST_KEYCHAIN,
            // rocky 만의 크레딧 페이드는 끄고 대조한다(끄면 cc-usage 와 같은 색).
            "creditFade": false,
            "source": case["config"]["source"],
            "alertPercent": case["config"]["alert_percent"],
            "extraCommands": extra_commands,
            "badges": case["config"]["badges"],
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
            .env("ROCKY_STATUSLINE_USAGE_URL", DEAD_USAGE_URL)
            .env("ROCKY_STATUSLINE_NOW", case["now"].as_str().unwrap())
            .env("TZ", case["tz"].as_str().unwrap());
        for (k, v) in case["env"].as_object().unwrap() {
            cmd.env(k, v.as_str().unwrap());
        }
        if let Some(source) = case["sourceEnv"].as_str() {
            cmd.env("ROCKY_STATUSLINE_SOURCE", source);
        }
        let args = case["args"].as_array().unwrap();
        let mut child = cmd
            .args(["statusline", "--full"])
            .args(args.iter().map(|a| a.as_str().unwrap()))
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
        // 바이트로 비교한다 — extra 출력은 UTF-8 이 아닐 수 있다(extra-bytes).
        let mut want = std::fs::read(case_dir.join("expected.txt")).unwrap();
        for pair in case["allow"].as_array().unwrap() {
            let (from, to) = (pair[0].as_str().unwrap(), pair[1].as_str().unwrap());
            want = String::from_utf8(want)
                .unwrap()
                .replace(from, to)
                .into_bytes();
        }
        let (got_bytes, want_bytes) = (out.stdout.clone(), want.clone());
        let want = String::from_utf8_lossy(&want_bytes);
        let got = String::from_utf8_lossy(&got_bytes);
        if got_bytes != want_bytes || !out.status.success() {
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

/// git 이 멈추면 500ms 에서 **프로세스 그룹째** 끊고 git 세그먼트만 뺀다. 가짜 git 은 자식을 하나 띄워 두는데,
/// git 프로세스만 죽이면 그 자식이 고아로 남는다 — 1초마다 도는 자리라 쌓인다.
#[test]
fn full_kills_a_hung_git_with_its_process_group() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let git = bin.join("git");
    let child_pid = dir.path().join("child.pid");
    std::fs::write(
        &git,
        format!(
            "#!/bin/sh\n[ \"$1\" = warm ] && exit 0\nsleep 10 &\necho $! > {}\nsleep 10\n",
            child_pid.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&git, std::fs::Permissions::from_mode(0o755)).unwrap();
    // 새 실행 파일의 첫 실행은 macOS 검사(syspolicyd)로 수백 ms 늦을 수 있다 — 마감 전에 시작하도록 한 번 데워 둔다.
    assert!(Command::new(&git).arg("warm").status().unwrap().success());
    let config = dir.path().join("rocky.json");
    std::fs::write(
        &config,
        r#"{"todo":{"port":1,"dir":"/nonexistent","expose":"off"},"statusline":{"keychainService":"rocky-test-absent"}}"#,
    )
    .unwrap();

    let started = std::time::Instant::now();
    let mut child = Command::new(env!("CARGO_BIN_EXE_rocky"))
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("HOME", dir.path())
        .env("ROCKY_USAGE_DIR", dir.path().join("usage"))
        .env("ROCKY_CONFIG", &config)
        .env("ROCKY_STATUSLINE_USAGE_URL", DEAD_USAGE_URL)
        .env("NO_COLOR", "1")
        .args(["statusline", "--full"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(
            br#"{"workspace":{"current_dir":"/somewhere"},"model":{"display_name":"Opus 5"}}"#,
        )
        .unwrap();
    let out = child.wait_with_output().unwrap();
    let elapsed = started.elapsed();
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "/somewhere\nOpus 5 · usage …\n"
    );
    assert!(
        elapsed < std::time::Duration::from_secs(3),
        "마감에서 끊지 못했다 — {elapsed:?}"
    );
    // 가짜 git 이 띄운 자식도 같이 죽었다 — kill(pid, 0) 이 실패해야 한다(SIGKILL 이 도착할 틈을 잠깐 준다).
    let pid: libc::pid_t = std::fs::read_to_string(&child_pid)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(200));
    // SAFETY: 시그널 0 은 존재 확인만 한다.
    let alive = unsafe { libc::kill(pid, 0) } == 0;
    if alive {
        // SAFETY: 테스트가 남긴 고아를 치운다.
        unsafe {
            libc::kill(pid, libc::SIGKILL);
        }
    }
    assert!(!alive, "git 이 띄운 자식({pid})이 고아로 남았다");
}

/// `statusline --full` 한 번 — 환경을 비우고 주어진 것만 넣는다. 데몬·keychain·usage API 에 닿지 않는다.
fn full_once(home: &std::path::Path, env: &[(&str, &str)], config: &str, stdin: &str) -> String {
    let rocky_json = home.join("rocky.json");
    std::fs::write(&rocky_json, config).unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rocky"));
    cmd.env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", home)
        .env("ROCKY_USAGE_DIR", home.join("usage"))
        .env("ROCKY_CONFIG", &rocky_json)
        .env("ROCKY_STATUSLINE_USAGE_URL", DEAD_USAGE_URL)
        .env("NO_COLOR", "1")
        .env("TZ", "UTC");
    for (k, v) in env {
        cmd.env(k, v);
    }
    let mut child = cmd
        .args(["statusline", "--full"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    String::from_utf8(child.wait_with_output().unwrap().stdout).unwrap()
}

const API_CONFIG: &str = r#"{"todo":{"port":1,"dir":"/nonexistent","expose":"off"},"statusline":{"source":"api","keychainService":"rocky-test-absent"}}"#;
const NOW: &str = "2026-09-16T07:40:00Z";

/// 계정 하나의 usage 캐시를 심는다 — 5분 전 응답, 5h 사용률 `used`.
fn seed_usage(bucket: &std::path::Path, used: f64) {
    std::fs::create_dir_all(bucket).unwrap();
    let usage = serde_json::json!({
        "usage": {"fetched_at": "2026-09-16T07:35:00Z", "five_hour": {"percent": used}}
    });
    std::fs::write(bucket.join("usage.json"), usage.to_string()).unwrap();
}

fn slot(home: &std::path::Path, config_dir: &std::path::Path) -> std::path::PathBuf {
    rocky_core::claude_account::cache_slot(&home.join(".cache"), config_dir)
}

#[test]
fn full_keeps_caches_apart_per_config_dir() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let work = home.join("work-claude");
    seed_usage(
        &rocky_core::claude_account::cache_bucket(&slot(home, &home.join(".claude")), None),
        42.0,
    );
    seed_usage(
        &rocky_core::claude_account::cache_bucket(&slot(home, &work), None),
        10.0,
    );
    let stdin = r#"{"model":{"display_name":"M"}}"#;
    let now = [("ROCKY_STATUSLINE_NOW", NOW)];
    assert_eq!(full_once(home, &now, API_CONFIG, stdin), "M · 5h 58%\n");
    let work_env = [
        ("ROCKY_STATUSLINE_NOW", NOW),
        ("CLAUDE_CONFIG_DIR", work.to_str().unwrap()),
    ];
    assert_eq!(
        full_once(home, &work_env, API_CONFIG, stdin),
        "M · 5h 90%\n"
    );
}

/// 같은 설정 폴더 안의 계정 전환(claude-swap · `/login`) — 계정 파일의 이메일이 바뀌면 그 계정의 캐시로 갈아탄다.
/// 계정 파일은 1분마다(또는 한도 숫자가 바뀔 때)만 다시 읽고, 못 읽으면 계정 캐시를 덮지 않는다.
#[test]
fn full_follows_an_account_switch_inside_one_config_dir() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let config_dir = home.join(".claude");
    std::fs::create_dir_all(&config_dir).unwrap();
    let account = config_dir.join(".claude.json");
    let set_email = |email: &str| {
        std::fs::write(
            &account,
            format!(r#"{{"oauthAccount":{{"emailAddress":"{email}"}}}}"#),
        )
        .unwrap();
    };
    let slot = slot(home, &config_dir);
    seed_usage(
        &rocky_core::claude_account::cache_bucket(&slot, Some("a@example.com")),
        42.0,
    );
    seed_usage(
        &rocky_core::claude_account::cache_bucket(&slot, Some("b@example.com")),
        10.0,
    );
    let stdin = r#"{"model":{"display_name":"M"}}"#;
    let at = |t: &'static str| [("ROCKY_STATUSLINE_NOW", t)];

    set_email("a@example.com");
    assert_eq!(
        full_once(home, &at("2026-09-16T07:40:00Z"), API_CONFIG, stdin),
        "M · 5h 58%\n"
    );
    // 전환 직후 — 1분 안이고 한도 숫자도 그대로라 아직 a 의 캐시다.
    set_email("b@example.com");
    assert_eq!(
        full_once(home, &at("2026-09-16T07:40:30Z"), API_CONFIG, stdin),
        "M · 5h 58%\n"
    );
    // 1분이 지나 다시 읽으면 b 로 갈아탄다.
    assert_eq!(
        full_once(home, &at("2026-09-16T07:41:01Z"), API_CONFIG, stdin),
        "M · 5h 90%\n"
    );
    // 계정 파일이 깨져 있으면(원자적 재작성 중 등) 계정 캐시를 덮지 않는다 — b 그대로.
    std::fs::write(&account, "{ not json").unwrap();
    assert_eq!(
        full_once(home, &at("2026-09-16T07:43:00Z"), API_CONFIG, stdin),
        "M · 5h 90%\n"
    );
}

/// stdin 의 한도는 그 계정의 `state.json` 에 남아, stdin 이 한 번 비어도 6시간 동안 그린다.
#[test]
fn full_records_stdin_limits_for_the_six_hour_fallback() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let config = r#"{"todo":{"port":1,"dir":"/nonexistent","expose":"off"},"statusline":{"source":"stdin","keychainService":"rocky-test-absent"}}"#;
    let now = [("ROCKY_STATUSLINE_NOW", NOW)];
    let with =
        r#"{"model":{"display_name":"M"},"rate_limits":{"five_hour":{"used_percentage":30}}}"#;
    assert_eq!(full_once(home, &now, config, with), "M · 5h 70%\n");
    let later = [("ROCKY_STATUSLINE_NOW", "2026-09-16T09:40:00Z")];
    assert_eq!(
        full_once(home, &later, config, r#"{"model":{"display_name":"M"}}"#),
        "M · 5h 70%\n"
    );
    let bucket = rocky_core::claude_account::cache_bucket(&slot(home, &home.join(".claude")), None);
    let state: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(bucket.join("state.json")).unwrap()).unwrap();
    assert_eq!(state["five_hour"]["percent"], 30.0);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(bucket.join("state.json"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "캐시 파일은 본인만 읽는다");
    }
}

/// 단계가 오른 시각을 `state.json` 에 남겨야 다음 렌더가 깜빡임 구간인지 안다 — 0.7초 뒤 렌더는 꺼진 프레임이다.
#[test]
fn full_records_when_an_alert_rose_and_blinks_from_there() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let config = r#"{"todo":{"port":1,"dir":"/nonexistent","expose":"off"},"statusline":{"source":"stdin","keychainService":"rocky-test-absent"}}"#;
    let hit =
        r#"{"model":{"display_name":"M"},"rate_limits":{"five_hour":{"used_percentage":100}}}"#;
    // 처음 — 켜진 프레임(배지)이고 시각을 적는다. NO_COLOR 라 배지·굵은 빨강 모두 " 0% ".
    let first = full_once(home, &[("ROCKY_STATUSLINE_NOW", NOW)], config, hit);
    assert!(first.starts_with("M · 5h  0% \n"), "{first:?}");
    let bucket = rocky_core::claude_account::cache_bucket(&slot(home, &home.join(".claude")), None);
    let state: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(bucket.join("state.json")).unwrap()).unwrap();
    assert_eq!(state["alert_key"], "2@5h");
    assert_eq!(state["alert_at"], NOW);
}

/// 크레딧을 쓰기 시작하면 그 시각을 `state.json` 에 적고, 다음 렌더들이 거기서부터 옅은 색 → 원래 색으로 이어 그린다.
#[test]
fn full_fades_the_credit_amount_in_when_credits_start_burning() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let config = home.join("rocky.json");
    std::fs::write(
        &config,
        r#"{"todo":{"port":1,"dir":"/nonexistent","expose":"off"},"statusline":{"source":"stdin","keychainService":"rocky-test-absent"}}"#,
    )
    .unwrap();
    // 방금(15분 안에) 크레딧이 늘었다 — 쓰는 중.
    let bucket = rocky_core::claude_account::cache_bucket(&slot(home, &home.join(".claude")), None);
    std::fs::create_dir_all(&bucket).unwrap();
    std::fs::write(
        bucket.join("usage.json"),
        r#"{"usage":{"fetched_at":"2026-09-16T07:39:00Z","extra":{"enabled":true,"used_credits":1160,"monthly_limit":5000}},"credits_rising_at":"2026-09-16T07:39:00Z"}"#,
    )
    .unwrap();
    let render = |now: &str| {
        let mut child = Command::new(env!("CARGO_BIN_EXE_rocky"))
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("HOME", home)
            .env("ROCKY_USAGE_DIR", home.join("usage"))
            .env("ROCKY_CONFIG", &config)
            .env("ROCKY_STATUSLINE_USAGE_URL", DEAD_USAGE_URL)
            .env("ROCKY_STATUSLINE_NOW", now)
            .env("COLORTERM", "truecolor")
            .args(["statusline", "--full"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(br#"{"model":{"display_name":"M"},"rate_limits":{"five_hour":{"used_percentage":30}}}"#)
            .unwrap();
        String::from_utf8(child.wait_with_output().unwrap().stdout).unwrap()
    };
    // 처음 — 옅은 색(105,149,76)에서 출발하고 시각을 적는다.
    assert!(render("2026-09-16T07:40:00Z").contains("\x1b[38;2;105;149;76m$38.40"));
    let state: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(bucket.join("state.json")).unwrap()).unwrap();
    assert_eq!(state["credits_spending_at"], "2026-09-16T07:40:00Z");
    // 3초 뒤 — 원래 색(82,170,23).
    assert!(render("2026-09-16T07:40:03Z").contains("\x1b[38;2;82;170;23m$38.40"));
}

/// 폴더 아래 파일 전부(상대 경로, 정렬) — 캐시를 건드렸는지 본다.
fn tree(root: &std::path::Path) -> Vec<String> {
    fn walk(root: &std::path::Path, dir: &std::path::Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                out.push(path.strip_prefix(root).unwrap().display().to_string());
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

/// agy 의 stdin — 1.2.14 실측 꼴. gemini 5h 86% 남음.
const AGY_STDIN: &str = r#"{"cwd":"/w","session_id":"agy-s","product":"antigravity","model":{"display_name":"Gemini 3.8 Flash (High)"},"context_window":{"used_percentage":12},"quota":{"gemini-5h":{"remaining_fraction":0.86,"reset_time":"2026-09-16T09:00:00Z"},"3p-5h":{"remaining_fraction":1}}}"#;

/// 같은 머신의 Claude 세션이 남긴 것 — 계정 파일(배지)과 그 계정의 stdin 관측(5h 70% 남음).
fn seed_claude_session(home: &std::path::Path) {
    std::fs::write(
        home.join(".claude.json"),
        r#"{"oauthAccount":{"emailAddress":"a@example.com"}}"#,
    )
    .unwrap();
    let bucket = rocky_core::claude_account::cache_bucket(
        &slot(home, &home.join(".claude")),
        Some("a@example.com"),
    );
    std::fs::create_dir_all(&bucket).unwrap();
    let state = serde_json::json!({
        "observed_at": NOW, "stdin_limits_seen": NOW,
        "five_hour": {"percent": 30, "resets_at": "2026-09-16T09:00:00Z"}
    });
    std::fs::write(bucket.join("state.json"), state.to_string()).unwrap();
}

fn config_with(statusline: &str) -> String {
    format!(
        r#"{{"todo":{{"port":1,"dir":"/nonexistent","expose":"off"}},"statusline":{statusline}}}"#
    )
}

/// agy 는 source 설정과 무관하게 Claude 쪽(갱신·캐시·계정 파일)을 건드리지 않고 quota 로만 그린다 — 같은 머신 Claude
/// 세션의 5h(70% 남음)·배지가 새면 안 된다. 갱신을 띄웠다면 잠시 뒤 usage.json 이 생기므로 기다렸다 본다.
#[test]
fn full_agy_never_touches_the_claude_side() {
    for source in ["auto", "stdin", "api"] {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        seed_claude_session(home);
        let before = tree(&home.join(".cache"));
        let config = config_with(&format!(
            r#"{{"source":"{source}","keychainService":"rocky-test-absent","badges":{{"a@example.com":{{"emoji":"🏢"}}}}}}"#
        ));
        let out = full_once(home, &[("ROCKY_STATUSLINE_NOW", NOW)], &config, AGY_STDIN);
        assert_eq!(
            out, "/w\nGemini 3.8 Flash (High) · ctx 12% · 5h 86% (↻09:00)\n",
            "{source}"
        );
        std::thread::sleep(std::time::Duration::from_millis(500));
        assert_eq!(
            tree(&home.join(".cache")),
            before,
            "{source}: 캐시가 바뀌었다"
        );
    }
}

/// `--source none`·환경 변수 none 은 Claude 쪽을 하나도 건드리지 않는다. 모르는 플래그 값은 한 줄 안내만 내고 아무것도
/// 건드리지 않는다.
#[test]
fn full_source_none_and_bad_flag_touch_nothing() {
    let claude =
        r#"{"model":{"display_name":"Opus 5"},"rate_limits":{"five_hour":{"used_percentage":30}}}"#;
    let config = config_with(
        r#"{"source":"auto","keychainService":"rocky-test-absent","badges":{"a@example.com":{"emoji":"🏢"}}}"#,
    );
    for (args, env, want) in [
        (&["--source", "none"][..], None, "Opus 5\n"),
        (&[][..], Some("none"), "Opus 5\n"),
        (
            &["--source", "bogus"][..],
            None,
            "[rocky] --source \"bogus\": auto|stdin|api|none 중 하나\n",
        ),
        // 명령줄이 틀려도 statusline 은 비지 않는다.
        (
            &["--source"][..],
            None,
            "[rocky] flag --source requires a value\n",
        ),
        (
            &["--source=none"][..],
            None,
            "[rocky] unknown flag: --source=none\n",
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        seed_claude_session(home);
        let before = tree(&home.join(".cache"));
        let rocky_json = home.join("rocky.json");
        std::fs::write(&rocky_json, &config).unwrap();
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_rocky"));
        cmd.env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("HOME", home)
            .env("ROCKY_USAGE_DIR", home.join("usage"))
            .env("ROCKY_CONFIG", &rocky_json)
            .env("ROCKY_STATUSLINE_USAGE_URL", DEAD_USAGE_URL)
            .env("NO_COLOR", "1");
        if let Some(env) = env {
            cmd.env("ROCKY_STATUSLINE_SOURCE", env);
        }
        let mut child = cmd
            .args(["statusline", "--full"])
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(claude.as_bytes())
            .unwrap();
        let out = String::from_utf8(child.wait_with_output().unwrap().stdout).unwrap();
        assert_eq!(out, want, "{args:?} {env:?}");
        std::thread::sleep(std::time::Duration::from_millis(500));
        assert_eq!(tree(&home.join(".cache")), before, "{args:?} {env:?}");
    }
}

/// 갱신 자식은 부모가 정한 source 로 돈다 — 설정이 `none` 이어도 `--source api` 로 띄운 갱신은 실제로 조회를 시도해
/// (토큰이 없어 실패) 그 계정의 usage.json 에 이유를 남긴다. 넘기지 않으면 자식은 설정의 `none` 을 보고 아무것도 안 한다.
#[test]
fn full_passes_its_source_to_the_refresh_it_spawns() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    std::fs::write(
        home.join(".claude.json"),
        r#"{"oauthAccount":{"emailAddress":"a@example.com"}}"#,
    )
    .unwrap();
    let config = config_with(&format!(
        r#"{{"source":"none","keychainService":"rocky-test-absent","credentialsFile":"{}"}}"#,
        home.join("absent.json").display()
    ));
    let rocky_json = home.join("rocky.json");
    std::fs::write(&rocky_json, &config).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_rocky"))
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", home)
        .env("ROCKY_USAGE_DIR", home.join("usage"))
        .env("ROCKY_CONFIG", &rocky_json)
        .env("ROCKY_STATUSLINE_USAGE_URL", DEAD_USAGE_URL)
        .env("ROCKY_STATUSLINE_SOURCE", "none")
        .args(["statusline", "--full", "--source", "api"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"{}").unwrap();
    child.wait().unwrap();
    let usage = rocky_core::claude_account::cache_bucket(
        &slot(home, &home.join(".claude")),
        Some("a@example.com"),
    )
    .join("usage.json");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !usage.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let written = std::fs::read_to_string(&usage).expect("갱신이 usage.json 을 남기지 않았다");
    assert!(written.contains("token not found"), "{written}");
}

/// `statusline refresh` 를 직접 불러도 환경 변수 `none` 이면 아무것도 하지 않는다(토큰이 있어도 조회하지 않는다).
#[test]
fn refresh_with_source_none_from_the_parent_is_a_noop() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    std::fs::write(
        home.join(".claude.json"),
        r#"{"oauthAccount":{"emailAddress":"a@example.com"}}"#,
    )
    .unwrap();
    let rocky_json = home.join("rocky.json");
    std::fs::write(
        &rocky_json,
        config_with(r#"{"source":"api","tokenEnv":"ROCKY_TEST_TOKEN"}"#),
    )
    .unwrap();
    let status = Command::new(env!("CARGO_BIN_EXE_rocky"))
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", home)
        .env("ROCKY_USAGE_DIR", home.join("usage"))
        .env("ROCKY_CONFIG", &rocky_json)
        .env("ROCKY_STATUSLINE_USAGE_URL", DEAD_USAGE_URL)
        .env("ROCKY_TEST_TOKEN", "t")
        .env("ROCKY_STATUSLINE_SOURCE", "none")
        .args(["statusline", "refresh"])
        .status()
        .unwrap();
    assert!(status.success());
    assert!(
        tree(&home.join(".cache")).is_empty(),
        "{:?}",
        tree(&home.join(".cache"))
    );
}
