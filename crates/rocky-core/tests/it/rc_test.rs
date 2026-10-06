use std::collections::HashSet;
use std::time::Duration;

use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use rocky_core::config::{load_rc_block, NightlyConfig, RcConfig};
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
        supervise: false,
        nightly: None,
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
        supervise: false,
        nightly: None,
    };
    assert_eq!(resolve_targets(&config, "/home/u")[0].dir, "/srv/ws/x");
    // 상대 root 는 홈 기준, `~foo` 는 홈이 아니라 root 아래 이름이다.
    let config = RcConfig {
        root: Some("ws".into()),
        pinned: vec![],
        targets: vec!["x".into(), "~foo".into()],
        supervise: false,
        nightly: None,
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
            supervise: false,
            nightly: None,
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

// ── 감시(되살리기) 판정 ──

fn row(label: &str, pinned: bool, running: bool) -> ServerRow {
    ServerRow {
        label: label.into(),
        dir: format!("/w/{label}"),
        pinned,
        running,
        pid: running.then_some(1),
        uptime_secs: None,
        sessions: 0,
        action: None,
        last_result: None,
        auth_suspect: false,
        stale: false,
    }
}

fn status_with(servers: Vec<ServerRow>) -> RcStatus {
    RcStatus {
        configured: true,
        servers,
        ..RcStatus::unconfigured()
    }
}

#[test]
fn revives_only_stopped_pinned_idle_targets() {
    let mut busy = row("busy", true, false);
    busy.action = Some(RcAction::Restarting);
    let status = status_with(vec![
        row("up", true, true),
        row("down", true, false),
        row("other", false, false),
        busy,
    ]);
    assert_eq!(labels(&status, &[]), vec!["down"]);
}

fn labels(status: &RcStatus, marked: &[&str]) -> Vec<String> {
    let marked: HashSet<String> = marked.iter().map(|s| s.to_string()).collect();
    revive_candidates(status, &marked)
        .into_iter()
        .map(|r| r.label)
        .collect()
}

#[test]
fn revives_nothing_when_unsure_or_logged_out() {
    let base = status_with(vec![row("down", true, false)]);
    let mut probe_failed = base.clone();
    probe_failed.probe_error = Some("ps 실패".into());
    assert!(
        labels(&probe_failed, &["down"]).is_empty(),
        "꺼짐이 모름이다"
    );
    let mut logged_out = base.clone();
    logged_out.auth = AuthState::Out;
    assert!(
        labels(&logged_out, &["down"]).is_empty(),
        "띄워도 곧 내려간다"
    );
    let mut unknown = base.clone();
    unknown.auth = AuthState::Unknown;
    assert_eq!(labels(&unknown, &[]), vec!["down"], "모르면 띄워 본다");
    let off = RcStatus::unconfigured();
    assert!(labels(&off, &["down"]).is_empty());
}

#[test]
fn failure_backoff_doubles_to_thirty_minutes() {
    let mins: Vec<u64> = (0..8).map(|n| failure_backoff(n).as_secs() / 60).collect();
    assert_eq!(mins, vec![0, 2, 4, 8, 16, 30, 30, 30]);
    assert_eq!(failure_backoff(u32::MAX), SUPERVISE_BACKOFF_MAX);
}

#[test]
fn auth_mark_reports_each_transition_once() {
    let empty = AuthMark::default();
    // 처음 본 것이 로그아웃이어도 알린다.
    let (m, t) = next_auth_mark(AuthState::Out, &empty, 100);
    assert_eq!(t, AuthTransition::LoggedOut);
    assert!(m.still_out());
    // 이어지는 로그아웃은 조용히 시각만 민다.
    let (m, t) = next_auth_mark(AuthState::Out, &m, 220);
    assert_eq!((t, m.last_out), (AuthTransition::None, Some(220)));
    // 모름은 기록을 건드리지 않는다.
    let (m2, t) = next_auth_mark(AuthState::Unknown, &m, 300);
    assert_eq!((t, &m2), (AuthTransition::None, &m));
    // 회복은 한 번.
    let (m, t) = next_auth_mark(AuthState::In, &m, 400);
    assert_eq!(t, AuthTransition::Recovered);
    assert!(m.recovered() && !m.still_out());
    let (m, t) = next_auth_mark(AuthState::In, &m, 520);
    assert_eq!((t, m.last_in), (AuthTransition::None, Some(400)));
    // 다시 끊기면 다시 알린다.
    let (_, t) = next_auth_mark(AuthState::Out, &m, 600);
    assert_eq!(t, AuthTransition::LoggedOut);
    // 로그인만 계속 보던 기기는 아무 일도 없다.
    let (m, t) = next_auth_mark(AuthState::In, &empty, 100);
    assert_eq!((t, m), (AuthTransition::None, AuthMark::default()));
}

#[test]
fn suspects_servers_started_before_the_logout_only_after_recovery() {
    let still_out = AuthMark {
        last_out: Some(1_000),
        last_in: None,
    };
    let recovered = AuthMark {
        last_out: Some(1_000),
        last_in: Some(1_500),
    };
    // now 2000 — 1500초 떠 있었으면 500 에 떴다(로그아웃 1000 보다 앞).
    assert!(auth_suspect(Some(1_500), &recovered, 2_000));
    // 회복 뒤 다시 띄운 서버(300초 전 = 1700 에 떴다)는 의심하지 않는다.
    assert!(!auth_suspect(Some(300), &recovered, 2_000));
    // 아직 로그아웃 중이면 의심하지 않는다 — 다른 신호다.
    assert!(!auth_suspect(Some(1_500), &still_out, 2_000));
    assert!(!auth_suspect(None, &recovered, 2_000));
}

#[test]
fn auth_mark_round_trips_as_camel_case_json() {
    let mark = AuthMark {
        last_out: Some(10),
        last_in: Some(20),
    };
    let v = serde_json::to_value(&mark).unwrap();
    assert_eq!(v, serde_json::json!({"lastOut": 10, "lastIn": 20}));
    let back: AuthMark = serde_json::from_value(v).unwrap();
    assert_eq!(back, mark);
    let empty: AuthMark = serde_json::from_str("{}").unwrap();
    assert_eq!(empty, AuthMark::default());
}

#[test]
fn supervise_is_off_unless_set() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rocky.json");
    std::fs::write(&path, r#"{"rc":{"pinned":["a"]}}"#).unwrap();
    assert!(!load_rc_block(&path).unwrap().supervise);
    std::fs::write(&path, r#"{"rc":{"pinned":["a"],"supervise":true}}"#).unwrap();
    assert!(load_rc_block(&path).unwrap().supervise);
}

#[test]
fn revives_marked_unpinned_as_server_only() {
    let status = status_with(vec![
        row("pin", true, false),
        row("marked", false, false),
        row("plain", false, false),
    ]);
    let marked: HashSet<String> = ["marked".to_string(), "pin".to_string()].into();
    let got = revive_candidates(&status, &marked);
    assert_eq!(
        got,
        vec![
            // 표식이 있어도 고정은 원래대로 새 세션.
            Revive {
                label: "pin".into(),
                mode: LaunchMode::Session
            },
            // 야간이 내리고 못 띄운 비고정은 서버만 — 사람이 부른 기동이 아니다.
            Revive {
                label: "marked".into(),
                mode: LaunchMode::Server
            },
        ]
    );
    let mut busy = status.clone();
    busy.servers[1].action = Some(RcAction::Retrying);
    assert_eq!(
        labels(&busy, &["marked"]),
        vec!["pin"],
        "진행 중이면 건너뛴다"
    );
}

#[test]
fn clears_marks_of_running_or_dropped_targets_only_on_a_good_probe() {
    let status = status_with(vec![row("up", false, true), row("down", false, false)]);
    let marked: HashSet<String> = ["up", "down", "gone"].map(String::from).into();
    assert_eq!(
        stale_revive_marks(&status, &marked),
        vec!["gone", "up"],
        "누가 이미 띄웠거나 설정에서 뺀 라벨"
    );
    let mut restarting = status.clone();
    restarting.servers[0].action = Some(RcAction::Restarting);
    assert_eq!(
        stale_revive_marks(&restarting, &marked),
        vec!["gone"],
        "야간이 내리고 띄우는 중인 대상의 표식은 남긴다"
    );
    let mut failed = status.clone();
    failed.probe_error = Some("lsof 실패".into());
    assert!(
        stale_revive_marks(&failed, &marked).is_empty(),
        "꺼짐이 모름이면 지우지 않는다"
    );
    assert!(stale_revive_marks(&RcStatus::unconfigured(), &marked).is_empty());
}

#[test]
fn claude_version_is_read_as_numbers() {
    assert_eq!(
        parse_claude_version("2.1.288 (Claude Code)\n").as_deref(),
        Some("2.1.288")
    );
    assert_eq!(
        parse_claude_version("\n2.1.288\n").as_deref(),
        Some("2.1.288")
    );
    for bad in [
        "",
        "error: timeout",
        "2.1",
        "2.1.x (Claude Code)",
        "v2.1.288",
    ] {
        assert_eq!(parse_claude_version(bad), None, "{bad:?}");
    }
    assert_eq!(
        version_from_path("/h/.local/share/claude/versions/2.1.287").as_deref(),
        Some("2.1.287")
    );
    assert_eq!(version_from_path("/h/.local/share/claude/claude"), None);
    assert_eq!(
        newest_version(["2.1.283", "2.1.287", "2.1.29", ".DS_Store"]).as_deref(),
        Some("2.1.287"),
        "문자열이 아니라 숫자로 비교한다"
    );
    assert_eq!(newest_version([".DS_Store"]), None);
}

#[test]
fn project_dir_name_matches_claude_code() {
    for (dir, want) in [
        (
            "/Users/minjun/dev/workspaces/hail-mary",
            "-Users-minjun-dev-workspaces-hail-mary",
        ),
        (
            "/Users/minjun/dev/workspaces/minjun.kim",
            "-Users-minjun-dev-workspaces-minjun-kim",
        ),
        (
            "/Users/minjun/dev/workspaces/static/.claude/worktrees/cors-dev",
            "-Users-minjun-dev-workspaces-static--claude-worktrees-cors-dev",
        ),
    ] {
        assert_eq!(project_dir_name(dir), want);
    }
}

#[test]
fn nightly_decision_follows_the_old_order() {
    const NOW: i64 = 1_000_000;
    let quiet = Duration::from_secs(60 * 60);
    let stale = NightlyState {
        current: Some("2.1.286".into()),
        recorded: Some("2.1.283\n".into()),
        ..NightlyState::default()
    };
    let with = |f: &dyn Fn(&mut NightlyState)| {
        let mut s = stale.clone();
        f(&mut s);
        s
    };
    let ago = |mins: i64| Some(NOW - mins * 60);
    use LaunchMode::*;
    use NightlyReason::*;
    let cases: Vec<(&str, NightlyState, NightlyReason, Option<LaunchMode>)> = vec![
        ("서버만 — 쉰다", stale.clone(), Restart, Some(Server)),
        (
            "고정 · 서버만 — 새 세션",
            with(&|s| s.pinned = true),
            Restart,
            Some(Session),
        ),
        (
            "세션 · 61분 조용 — 이어받기",
            with(&|s| (s.live_session, s.last_write) = (true, ago(61))),
            Restart,
            Some(Resume),
        ),
        (
            "세션 · 59분 전 대화 — 바쁨",
            with(&|s| (s.live_session, s.last_write) = (true, ago(59))),
            Busy,
            None,
        ),
        (
            "세션 · 대화 기록 없음 — 잃을 대화가 없다",
            with(&|s| s.live_session = true),
            Restart,
            Some(Resume),
        ),
        (
            "서버만 · 같은 폴더 터미널이 방금 기록 — 여전히 쉰다",
            with(&|s| s.last_write = ago(1)),
            Restart,
            Some(Server),
        ),
        (
            "최신",
            with(&|s| s.recorded = Some("2.1.286\n".into())),
            Current,
            None,
        ),
        (
            "설치 버전 모름",
            with(&|s| s.current = None),
            VersionUnknown,
            None,
        ),
        ("기록 없음", with(&|s| s.recorded = None), NoRecord, None),
        (
            "기록 빔",
            with(&|s| s.recorded = Some("\n".into())),
            NoRecord,
            None,
        ),
        (
            "네트워크 없음",
            with(&|s| s.offline = true),
            NoNetwork,
            None,
        ),
        (
            "네트워크가 없어도 바쁨이 먼저",
            with(&|s| (s.offline, s.live_session, s.last_write) = (true, true, ago(1))),
            Busy,
            None,
        ),
    ];
    for (name, state, reason, mode) in cases {
        let d = decide_nightly(&state, NOW, quiet);
        assert_eq!((d.reason, d.mode), (reason, mode), "{name}");
    }
    let d = decide_nightly(
        &with(&|s| (s.live_session, s.last_write) = (true, ago(90))),
        NOW,
        quiet,
    );
    assert_eq!(d.from.as_deref(), Some("2.1.283"));
    assert_eq!(d.idle_secs, Some(90 * 60));
    // 시계가 뒤로 가 기록이 미래면 방금 대화한 것으로 본다.
    let d = decide_nightly(
        &with(&|s| (s.live_session, s.last_write) = (true, Some(NOW + 30))),
        NOW,
        quiet,
    );
    assert_eq!((d.reason, d.idle_secs), (Busy, Some(0)));
    assert_eq!(
        serde_json::to_value(VersionUnknown).unwrap(),
        "version-unknown"
    );
}

#[test]
fn nightly_backoff_is_longer_than_daytime() {
    let day: Duration = REGISTRATION_BACKOFF.iter().sum();
    let night: Duration = NIGHTLY_REGISTRATION_BACKOFF.iter().sum();
    // 옛 CLI 에서 45초+90초로 안 풀리고 5분쯤 뒤 풀린 적이 있다.
    assert!(night > Duration::from_secs(5 * 60) && night > day);
}

fn at(date: (i32, u32, u32), hm: (u32, u32)) -> NaiveDateTime {
    NaiveDate::from_ymd_opt(date.0, date.1, date.2)
        .unwrap()
        .and_hms_opt(hm.0, hm.1, 0)
        .unwrap()
}

fn hm(h: u32, m: u32) -> NaiveTime {
    NaiveTime::from_hms_opt(h, m, 0).unwrap()
}

#[test]
fn nightly_runs_once_a_day_and_catches_up_after_sleep() {
    let today = NaiveDate::from_ymd_opt(2026, 10, 7).unwrap();
    let yesterday = today.pred_opt().unwrap();
    let four_thirty = hm(4, 30);
    assert!(!nightly_due(
        at((2026, 10, 7), (4, 29)),
        four_thirty,
        yesterday
    ));
    assert!(nightly_due(
        at((2026, 10, 7), (4, 30)),
        four_thirty,
        yesterday
    ));
    assert!(
        nightly_due(at((2026, 10, 7), (9, 0)), four_thirty, yesterday),
        "04:30 을 자고 넘겼으면 깬 뒤 한 번"
    );
    assert!(
        !nightly_due(at((2026, 10, 7), (9, 0)), four_thirty, today),
        "오늘 이미 돌았다"
    );
    assert!(
        nightly_due(
            at((2026, 10, 7), (5, 0)),
            four_thirty,
            today - chrono::Days::new(3)
        ),
        "며칠 꺼져 있었어도 한 번"
    );
    // 처음 켠 날 — 시각이 지났으면 오늘 돈 것으로 쳐서 낮에 서버를 내리지 않는다.
    let noon = at((2026, 10, 7), (12, 0));
    assert_eq!(nightly_first_mark(noon, four_thirty), today);
    assert!(!nightly_due(
        noon,
        four_thirty,
        nightly_first_mark(noon, four_thirty)
    ));
    // 시각 전이면 어제로 쳐서 오늘 그 시각에 돈다.
    let three = at((2026, 10, 7), (3, 0));
    assert_eq!(nightly_first_mark(three, four_thirty), yesterday);
    assert!(nightly_due(
        at((2026, 10, 7), (4, 30)),
        four_thirty,
        nightly_first_mark(three, four_thirty)
    ));
}

#[test]
fn busy_deadline_does_not_wait_past_until() {
    let seven = hm(7, 0);
    assert_eq!(
        busy_deadline(at((2026, 10, 7), (4, 30)), seven),
        at((2026, 10, 7), (7, 0))
    );
    let late = at((2026, 10, 7), (9, 0));
    assert_eq!(busy_deadline(late, seven), late, "지났으면 기다리지 않는다");
}

#[test]
fn recovery_waits_double_to_sixteen_minutes() {
    let mins: Vec<u64> = (0..=7).map(|n| recovery_wait(n).as_secs() / 60).collect();
    assert_eq!(mins, vec![1, 1, 2, 4, 8, 16, 16, 16]);
}

#[test]
fn canary_is_the_first_pinned() {
    let order = canary_first(vec![("a", false), ("b", true), ("c", true)], |t| t.1);
    assert_eq!(
        order.iter().map(|t| t.0).collect::<Vec<_>>(),
        ["b", "a", "c"]
    );
    let none = canary_first(vec![("a", false), ("b", false)], |t| t.1);
    assert_eq!(none.iter().map(|t| t.0).collect::<Vec<_>>(), ["a", "b"]);
    assert!(canary_first(Vec::<(&str, bool)>::new(), |t| t.1).is_empty());
}

#[test]
fn nightly_is_blocked_when_unsure_or_logged_out() {
    let ok = status_with(vec![row("a", true, true)]);
    assert_eq!(nightly_blocked(&ok), None);
    let mut failed = ok.clone();
    failed.probe_error = Some("ps 실패".into());
    assert_eq!(nightly_blocked(&failed), Some("probe-failed"));
    let mut out = ok.clone();
    out.auth = AuthState::Out;
    assert_eq!(nightly_blocked(&out), Some("logged-out"));
    assert_eq!(
        nightly_blocked(&RcStatus::unconfigured()),
        Some("unconfigured")
    );
}

#[test]
fn hhmm_accepts_one_or_two_digit_hours() {
    assert_eq!(parse_hhmm("04:30"), Some(hm(4, 30)));
    assert_eq!(parse_hhmm(" 4:30 "), Some(hm(4, 30)));
    assert_eq!(parse_hhmm("23:59"), Some(hm(23, 59)));
    for bad in [
        "24:00", "4:3", "4:300", "+4:30", "04-30", "", "ab:cd", "004:30",
    ] {
        assert_eq!(parse_hhmm(bad), None, "{bad:?}");
    }
}

#[test]
fn nightly_is_off_unless_the_block_is_an_object() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rocky.json");
    let load = |raw: &str| {
        std::fs::write(&path, raw).unwrap();
        load_rc_block(&path).unwrap().nightly
    };
    assert_eq!(load(r#"{"rc":{"pinned":["a"]}}"#), None);
    assert_eq!(load(r#"{"rc":{"nightly":true}}"#), None);
    assert_eq!(
        load(r#"{"rc":{"nightly":{}}}"#),
        Some(NightlyConfig::default())
    );
    let base = NightlyConfig::default();
    assert_eq!((base.at, base.busy_until), (hm(4, 30), hm(7, 0)));
    assert_eq!(base.quiet, Duration::from_secs(3600));
    assert_eq!(
        load(r#"{"rc":{"nightly":{"at":"3:15","busyUntil":"06:45","quietMinutes":30}}}"#),
        Some(NightlyConfig {
            at: hm(3, 15),
            busy_until: hm(6, 45),
            quiet: Duration::from_secs(30 * 60),
        })
    );
    // 모양이 틀린 칸은 기본값(fail-open).
    assert_eq!(
        load(r#"{"rc":{"nightly":{"at":"4시","quietMinutes":0}}}"#),
        Some(NightlyConfig::default())
    );
}
