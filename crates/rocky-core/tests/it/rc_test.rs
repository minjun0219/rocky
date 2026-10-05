use rocky_core::config::{load_rc_block, RcConfig};
use rocky_core::rc::*;

#[test]
fn server_argv_is_judged_by_structure() {
    assert!(is_server_argv("claude rc --name repo-a"));
    assert!(is_server_argv(
        "/opt/bin/claude remote-control --name=repo-a -c"
    ));
    assert!(is_server_argv(
        "claude rc --no-create-session-in-dir --name repo-a"
    ));
    // 명령을 인자로 품은 셸 — argv[0] 이 zsh 라 서버가 아니다.
    assert!(!is_server_argv(
        "zsh -c cd repo-a && claude rc --name repo-a"
    ));
    // `--name` 없는 일회성 호출.
    assert!(!is_server_argv("claude rc --help"));
    assert!(!is_server_argv("claude rc -c"));
    assert!(!is_server_argv("claude-code rc --name x"));
    assert!(!is_server_argv("claude --print --name x"));
}

#[test]
fn etime_parses_all_shapes() {
    assert_eq!(parse_etime("00:05"), Some(5));
    assert_eq!(parse_etime("12:30"), Some(750));
    assert_eq!(parse_etime("02:10:00"), Some(7800));
    assert_eq!(parse_etime("1-02:10:00"), Some(86_400 + 7800));
    for bad in ["", "5", "??:??", "x-01:00", "1:2:3:4"] {
        assert_eq!(parse_etime(bad), None, "{bad}");
    }
}

#[test]
fn ps_table_finds_servers_and_their_sessions() {
    let out = "\
    1     0 30-00:00:00 /sbin/launchd
 2343     1 01-13:57:39 claude rc --name repo-a
 2400  2343    10:00 /x/versions/2.1 --print --sdk-url https://api.example.com/v1/code/sessions/cse_1
 2401  2343    09:00 /x/versions/2.1 --print --sdk-url https://api.example.com/v1/code/sessions/cse_2
 2402  2343    09:00 /x/helper --watch
  900   500    05:00 zsh -c claude rc --name repo-a
 1701     1 ??:?? claude rc --name repo-b
 1800   500    00:10 claude rc --help
 garbage
";
    let rows = parse_ps(out);
    let found: Vec<(u32, Option<u64>)> = servers(&rows)
        .iter()
        .map(|r| (r.pid, r.uptime_secs))
        .collect();
    assert_eq!(
        found,
        vec![
            (1701, None),
            (2343, Some(86_400 + 13 * 3600 + 57 * 60 + 39))
        ]
    );
    assert_eq!(session_count(&rows, 2343), 2);
    assert_eq!(session_count(&rows, 1701), 0);
}

#[test]
fn lsof_pairs_pid_with_cwd() {
    let out = "p71444\nfcwd\nn/w/repo a\np82509\nfcwd\nn/w/repo-b\n";
    let map = parse_lsof_cwd(out);
    assert_eq!(map.get(&71444).map(String::as_str), Some("/w/repo a"));
    assert_eq!(map.get(&82509).map(String::as_str), Some("/w/repo-b"));
    assert_eq!(map.len(), 2);
}

#[test]
fn session_child_is_sdk_url_to_code_session() {
    assert!(is_session_command(
        "/x/versions/2.1.288 --print --sdk-url https://api.example.com/v1/code/sessions/cse_1 --session-id cse_1"
    ));
    assert!(!is_session_command("/x/versions/2.1.288 --print"));
    // `/code/session` 이 `--sdk-url` 앞에만 있으면 아니다.
    assert!(!is_session_command(
        "/code/session --sdk-url https://x/other"
    ));
}

#[test]
fn targets_resolve_like_the_shell() {
    let config = RcConfig {
        root: None,
        pinned: vec!["repo-a".into(), "~/abs-ish".into()],
        targets: vec![
            "repo-a".into(),
            "repo-a/".into(),
            "/opt/repo-c/".into(),
            "nested/repo-d".into(),
            " ".into(),
        ],
    };
    let got = resolve_targets(&config, "/home/u");
    let view: Vec<(&str, &str, bool)> = got
        .iter()
        .map(|t| (t.label.as_str(), t.dir.as_str(), t.pinned))
        .collect();
    assert_eq!(
        view,
        vec![
            ("repo-a", "/home/u/dev/workspaces/repo-a", true),
            ("abs-ish", "/home/u/abs-ish", true),
            ("repo-c", "/opt/repo-c", false),
            ("repo-d", "/home/u/dev/workspaces/nested/repo-d", false),
        ]
    );
}

#[test]
fn custom_root_is_used_for_relative_names() {
    let config = RcConfig {
        root: Some("/srv/ws/".into()),
        pinned: vec![],
        targets: vec!["x".into()],
    };
    assert_eq!(resolve_targets(&config, "/home/u")[0].dir, "/srv/ws/x");
    // 상대 root 는 홈 기준, `~foo` 는 홈이 아니라 root 아래 이름이다.
    let config = RcConfig {
        root: Some("ws".into()),
        pinned: vec![],
        targets: vec!["x".into(), "~foo".into()],
    };
    let dirs: Vec<String> = resolve_targets(&config, "/home/u/")
        .into_iter()
        .map(|t| t.dir)
        .collect();
    assert_eq!(dirs, vec!["/home/u/ws/x", "/home/u/ws/~foo"]);
}

#[test]
fn auth_status_unknown_unless_explicit() {
    assert_eq!(parse_auth_status(r#"{"loggedIn": true}"#), AuthState::In);
    assert_eq!(parse_auth_status(r#"{"loggedIn": false}"#), AuthState::Out);
    assert_eq!(parse_auth_status(r#"{"other": 1}"#), AuthState::Unknown);
    assert_eq!(parse_auth_status("Not logged in"), AuthState::Unknown);
}

#[test]
fn agy_status_reads_first_values() {
    let out = "Daemon state = running\nDaemon pid = 82906\nDaemon state = active\nInstance name: mac-1 (find it at https://x)\n";
    assert_eq!(
        parse_agy_status(out),
        AgyStatus {
            state: Some("running".into()),
            pid: Some(82906),
            instance: Some("mac-1".into()),
        }
    );
    assert_eq!(
        parse_agy_status(""),
        AgyStatus {
            state: None,
            pid: None,
            instance: None
        }
    );
}

#[test]
fn rows_match_by_dir_string_and_list_strays() {
    let targets = vec![
        Target {
            label: "a".into(),
            dir: "/w/a".into(),
            pinned: true,
        },
        Target {
            label: "b".into(),
            dir: "/w/b".into(),
            pinned: false,
        },
    ];
    let live = vec![
        LiveServer {
            pid: 20,
            dir: "/w/a".into(),
            uptime_secs: Some(60),
            sessions: 2,
        },
        LiveServer {
            pid: 10,
            dir: "/w/a".into(),
            uptime_secs: Some(600),
            sessions: 0,
        },
        // 개명 전 경로를 문 옛 서버.
        LiveServer {
            pid: 30,
            dir: "/w/old-name".into(),
            uptime_secs: None,
            sessions: 1,
        },
        // 정규화하지 않는다 — 끝 슬래시가 다르면 다른 폴더다.
        LiveServer {
            pid: 40,
            dir: "/w/b/".into(),
            uptime_secs: None,
            sessions: 0,
        },
    ];
    let (rows, strays) = build_rows(&targets, &live);
    assert!(rows[0].running);
    assert_eq!(rows[0].pid, Some(10), "먼저 뜬 쪽");
    assert_eq!(rows[0].sessions, 0);
    assert!(!rows[1].running);
    // 대상 폴더의 두 번째 서버(20)도 사라지지 않고 여기 보인다.
    assert_eq!(
        strays.iter().map(|s| s.pid).collect::<Vec<_>>(),
        vec![20, 30, 40]
    );
    assert_eq!(strays[1].label, "old-name");
}

#[test]
fn rc_block_is_optional_and_lenient() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rocky.json");
    assert_eq!(load_rc_block(&path), None);
    std::fs::write(&path, r#"{"todo":{}}"#).unwrap();
    assert_eq!(load_rc_block(&path), None);
    std::fs::write(
        &path,
        r#"{"rc":{"root":" ","pinned":["a",3," b "],"targets":"nope"}}"#,
    )
    .unwrap();
    assert_eq!(
        load_rc_block(&path),
        Some(RcConfig {
            root: None,
            pinned: vec!["a".into(), "b".into()],
            targets: vec![],
        })
    );
    // 목록을 남긴 채 끈 기기 — 블록이 없을 때와 같다.
    std::fs::write(&path, r#"{"rc":{"enabled":false,"pinned":["a"]}}"#).unwrap();
    assert_eq!(load_rc_block(&path), None);
    std::fs::write(&path, r#"{"rc":{"enabled":true,"pinned":["a"]}}"#).unwrap();
    assert_eq!(
        load_rc_block(&path).map(|c| c.pinned),
        Some(vec!["a".to_string()])
    );
}

#[test]
fn status_serializes_camel_case() {
    let v = serde_json::to_value(RcStatus::unconfigured()).unwrap();
    assert_eq!(
        v,
        serde_json::json!({"configured": false, "servers": [], "strays": [], "auth": "unknown", "antigravity": null})
    );
}

// ── 띄우기 · 재시작 판정 — 옛 CLI 의 decide_test · registration_test 를 옮겼다 ──

#[test]
fn read_registration_cases() {
    let served = "Error: This folder is already served by a terminal `claude remote-control` on this device. Stop it first.\nExiting in about 45 seconds.\n";
    let cases = [
        ("아직 아무것도 없음", "", "", Registration::Pending),
        (
            "연결 중은 아직",
            "·|· Connecting · acorn-app · main",
            "",
            Registration::Pending,
        ),
        (
            "세션 모드는 Connected",
            "·✔︎· Connected · acorn-app · main",
            "",
            Registration::Connected,
        ),
        (
            "서버만 모드는 Ready 만 찍는다",
            "·|· Connecting · x · main\n·✔︎· Ready · acorn-app · main",
            "",
            Registration::Connected,
        ),
        (
            "등록이 남아 있으면 실패 — 프로세스는 45초쯤 떠 있어 속는다",
            "",
            served,
            Registration::Served,
        ),
        (
            "실패가 연결보다 앞선다",
            "·✔︎· Ready · x",
            served,
            Registration::Served,
        ),
    ];
    for (name, out, err, want) in cases {
        assert_eq!(read_registration(out, err), want, "{name}");
    }
}

#[test]
fn restart_resumes_only_with_live_session() {
    // 열린 세션이 있을 때만 -c — 세션 없이 -c 로 뜬 서버는 할 일이 없어 내려간다(단일 세션 모드).
    assert_eq!(restart_mode(true, true, false), LaunchMode::Resume);
    assert_eq!(restart_mode(false, true, false), LaunchMode::Resume);
    assert_eq!(restart_mode(true, false, false), LaunchMode::Session);
    assert_eq!(restart_mode(false, false, false), LaunchMode::Server);
    // fresh 면 세션이 있어도 이어받지 않는다.
    assert_eq!(restart_mode(true, true, true), LaunchMode::Session);
    assert_eq!(restart_mode(false, true, true), LaunchMode::Server);
    // 이름으로 띄우면 고정이 아니어도 세션까지.
    assert_eq!(START_MODE, LaunchMode::Session);
}

#[test]
fn retry_after_failed_resume() {
    assert_eq!(retry_mode(true), LaunchMode::Session);
    assert_eq!(retry_mode(false), LaunchMode::Server);
    assert!(may_fail_to_start(LaunchMode::Resume));
    assert!(!may_fail_to_start(LaunchMode::Session));
    assert!(!may_fail_to_start(LaunchMode::Server));
}

#[test]
fn server_argv_per_mode_is_a_server_by_structure() {
    let cases = [
        (
            LaunchMode::Resume,
            vec!["claude", "rc", "--name", "x", "-c"],
        ),
        (LaunchMode::Session, vec!["claude", "rc", "--name", "x"]),
        (
            LaunchMode::Server,
            vec!["claude", "rc", "--name", "x", "--no-create-session-in-dir"],
        ),
    ];
    for (mode, want) in cases {
        let argv = server_argv("x", mode);
        assert_eq!(argv, want);
        // 띄운 것을 1조각의 현황이 서버로 알아본다.
        assert!(is_server_argv(&argv.join(" ")));
    }
}

#[test]
fn mode_notes_say_why() {
    assert_eq!(
        mode_note(LaunchMode::Resume, false),
        "열린 세션 이어받기(-c)"
    );
    // fresh 는 세션이 있었어도 버린 것 — "세션 없음" 이 아니다.
    assert_eq!(mode_note(LaunchMode::Session, true), "이어받지 않고 새로");
    assert_eq!(
        mode_note(LaunchMode::Server, false),
        "서버만(세션은 앱에서)"
    );
}

#[test]
fn backoff_stays_within_three_minutes() {
    let total: u64 = REGISTRATION_BACKOFF.iter().map(|d| d.as_secs()).sum();
    assert!(total <= 180);
    assert!(REGISTRATION_FIRST < REGISTRATION_WAIT);
}
#[test]
fn agy_action_takes_only_start_and_stop() {
    assert_eq!(AgyAction::parse("start"), Some(AgyAction::Start));
    assert_eq!(AgyAction::parse("stop"), Some(AgyAction::Stop));
    // 다른 하위 명령(serve·status)이나 플래그를 라우트로 넘길 수 없다
    for name in ["serve", "status", "", "start --name x", "START"] {
        assert_eq!(AgyAction::parse(name), None, "{name:?}");
    }
    assert_eq!(
        AgyAction::Stop.argv(),
        vec!["agy", "remote-control", "stop"]
    );
}

#[test]
fn agy_settles_on_running_or_no_state_line() {
    let at = |state: Option<&str>| AgyStatus {
        state: state.map(str::to_string),
        pid: None,
        instance: Some("mac-1".into()),
    };
    assert!(AgyAction::Start.settled(Some(&at(Some("running")))));
    assert!(!AgyAction::Start.settled(Some(&at(None))));
    // 끈 직후 launchd 의 중간값 — 아직 아니다. `Daemon status: not running` 이 되면(state 줄 없음) 자리 잡았다.
    assert!(!AgyAction::Stop.settled(Some(&at(Some("SIGTERMed")))));
    assert!(AgyAction::Stop.settled(Some(&at(None))));
    assert!(!AgyAction::Start.settled(None));
}
