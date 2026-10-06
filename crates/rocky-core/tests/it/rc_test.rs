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
        let argv = server_argv("x", mode, None);
        assert_eq!(argv, want);
        // 띄운 것을 1조각의 현황이 서버로 알아본다.
        assert!(is_server_argv(&argv.join(" ")));
    }
    let pinned = server_argv("x", LaunchMode::Pin, Some("cse_01ab"));
    assert_eq!(
        pinned,
        vec!["claude", "rc", "--name", "x", "--session-id", "cse_01ab"]
    );
    assert!(is_server_argv(&pinned.join(" ")));
    assert!(
        may_fail_to_start(LaunchMode::Pin),
        "만료 · 틀린 id 면 뜨자마자 내려간다"
    );
}

#[test]
fn session_ids_are_claude_ai_ids_only() {
    for ok in ["cse_01HX2abc", "session_01ABC-def_9"] {
        assert!(valid_session_id(ok), "{ok}");
    }
    // 로컬 전사본 UUID · 빈 꼬리 · 셸 글자는 거른다.
    for bad in [
        "3f2c9a10-1b2c-4d5e-8f90-0a1b2c3d4e5f",
        "cse_",
        "session_",
        "cse_a b",
        "cse_x;rm",
        "",
    ] {
        assert!(!valid_session_id(bad), "{bad:?}");
    }
}

#[test]
fn a_turn_is_in_progress_only_with_a_live_session_that_just_talked() {
    let now = 10_000;
    assert!(turn_in_progress(true, Some(now - 60), now, TURN_QUIET));
    assert!(
        !turn_in_progress(true, Some(now - 121), now, TURN_QUIET),
        "2분 넘게 조용하다"
    );
    assert!(
        !turn_in_progress(false, Some(now - 1), now, TURN_QUIET),
        "세션이 없으면 같은 폴더 터미널이 방금 썼어도 아니다"
    );
    assert!(
        !turn_in_progress(true, None, now, TURN_QUIET),
        "기록이 없으면 끊길 대화도 없다"
    );
    assert!(TURN_POLL < TURN_QUIET && TURN_QUIET < TURN_WAIT);
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
    assert_eq!(
        mode_note(LaunchMode::Pin, false),
        "고른 세션 이어받기(--session-id)"
    );
}

#[test]
fn backoff_stays_within_three_minutes() {
    let total: u64 = REGISTRATION_BACKOFF.iter().map(|d| d.as_secs()).sum();
    assert!(total <= 180);
    // 2026-10-06 실측 — 내린 뒤 3분쯤에야 등록이 풀렸다(45 · 90초로는 못 풀었다).
    assert!(total >= 180);
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
        activity: None,
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

#[test]
fn ancestors_follow_ppid_up_to_launchd() {
    let row = |pid, ppid| PsRow {
        pid,
        ppid,
        uptime_secs: None,
        args: String::new(),
    };
    // launchd(1) ← 서버(100) ← 세션(101) ← 셸(102) ← CLI(103)
    let rows = vec![
        row(1, 0),
        row(100, 1),
        row(101, 100),
        row(102, 101),
        row(103, 102),
        row(200, 1),
    ];
    let up = ancestors(&rows, 103);
    assert!(
        up.contains(&100) && up.contains(&103),
        "자기 자신과 서버까지"
    );
    assert!(!up.contains(&200), "남의 서버는 아니다");
    assert!(!up.contains(&1), "launchd 는 넣지 않는다");
    // 고리가 있어도 끝난다.
    let looped = vec![row(5, 6), row(6, 5)];
    assert_eq!(ancestors(&looped, 5).len(), 2);
}

#[test]
fn activity_reads_git_like_the_old_status() {
    const DAY: i64 = 86_400;
    let now = 100 * DAY;
    let a = parse_git_facts(
        true,
        "",
        "main\n",
        "origin/main\n",
        &format!("{}\nfeat: 무엇을 했다\n", now - 3 * DAY),
        now,
    );
    assert_eq!(a.branch.as_deref(), Some("main"));
    assert_eq!(a.default_branch.as_deref(), Some("main"));
    assert_eq!(a.subject.as_deref(), Some("feat: 무엇을 했다"));
    assert!(a.active && !a.dirty && !a.on_side_branch());

    // 기본 브랜치에서 15일 조용하면 정박.
    let old = parse_git_facts(
        true,
        "",
        "main",
        "origin/main",
        &format!("{}\nx", now - 15 * DAY),
        now,
    );
    assert!(!old.active);
    assert!(
        parse_git_facts(
            true,
            "",
            "main",
            "origin/main",
            &format!("{}\nx", now - 14 * DAY),
            now
        )
        .active,
        "14일 째는 아직 활성"
    );
    // 오래돼도 작업 중이거나 곁가지면 활성.
    assert!(
        parse_git_facts(
            true,
            " M src/a.rs\n",
            "main",
            "origin/main",
            &format!("{}\nx", now - 90 * DAY),
            now
        )
        .active
    );
    let side = parse_git_facts(
        true,
        "",
        "feat/x",
        "origin/main",
        &format!("{}\nx", now - 90 * DAY),
        now,
    );
    assert!(side.on_side_branch() && side.active);
    // detached 이거나 기본 브랜치를 모르면 곁가지로 치지 않는다.
    assert!(
        !parse_git_facts(
            true,
            "",
            "",
            "origin/main",
            &format!("{}\nx", now - 90 * DAY),
            now
        )
        .active
    );
    // git 아님 · 커밋 없음 · 미래 커밋은 활성(판정 못 한 것을 후보에서 빼지 않는다).
    assert!(parse_git_facts(false, "", "", "", "", now).active);
    let empty = parse_git_facts(true, "", "main", "", "", now);
    assert!(empty.active && empty.commit_at.is_none() && empty.subject.is_none());
    assert!(
        parse_git_facts(
            true,
            "",
            "main",
            "origin/main",
            &format!("{}\nx", now + DAY),
            now
        )
        .active
    );
}

#[test]
fn activity_age_and_subject_read_like_the_old_status() {
    let now = 1_000_000;
    assert_eq!(activity_age(now, now - 59 * 60), "방금");
    assert_eq!(activity_age(now, now - 5 * 3600), "5시간 전");
    assert_eq!(activity_age(now, now - 3 * 86_400), "3일 전");
    let long = "가".repeat(40);
    assert_eq!(
        short_subject(&long),
        format!("{}…", "가".repeat(38)),
        "글자로 자른다"
    );
    assert_eq!(short_subject("짧다"), "짧다");
}

#[test]
fn rocky_versions_are_read_like_the_old_nightly() {
    let plugins = r#"[{"id":"other@m","version":"1.0.0"},{"id":"rocky@rocky-marketplace","version":"0.42.0","scope":"user"}]"#;
    assert_eq!(
        parse_plugin_version(plugins, "rocky@rocky-marketplace").as_deref(),
        Some("0.42.0")
    );
    assert_eq!(
        parse_plugin_version("not json", "rocky@rocky-marketplace"),
        None
    );
    assert_eq!(
        parse_tool_version("rocky 0.42.0\n").as_deref(),
        Some("0.42.0")
    );
    assert_eq!(parse_tool_version("v1.2.3").as_deref(), Some("1.2.3"));
    assert_eq!(parse_tool_version(""), None);
    let tags = "a\trefs/tags/v0.9.0\nb\trefs/tags/v0.42.0\nc\trefs/tags/v0.41.10\nd\trefs/tags/v0.43.0-rc.1\ne\trefs/tags/vX\n";
    assert_eq!(
        latest_tag(tags).as_deref(),
        Some("0.42.0"),
        "숫자로 비교하고 사전 릴리스는 뺀다"
    );
    assert_eq!(latest_tag(""), None);

    let v = |plugin: Option<&str>, latest: Option<&str>| RockyVersions {
        plugin: plugin.map(str::to_string),
        cli: Some("0.42.0".into()),
        daemon: "0.42.0".into(),
        latest: latest.map(str::to_string),
    };
    assert_eq!(v(Some("0.42.0"), Some("0.42.0")).status(), "current");
    assert_eq!(v(Some("0.41.0"), Some("0.42.0")).status(), "behind");
    assert_eq!(v(None, Some("0.42.0")).status(), "unknown");
    assert_eq!(
        v(Some("0.42.0"), None).status(),
        "unknown",
        "태그를 못 받았다"
    );
}

#[test]
fn agy_old_binary_only_when_the_file_changed_after_start() {
    assert!(
        agy_old_binary(Some(100), Some(200)),
        "업데이트가 기동보다 나중"
    );
    assert!(!agy_old_binary(Some(200), Some(100)));
    assert!(!agy_old_binary(Some(100), Some(100)));
    assert!(!agy_old_binary(None, Some(200)), "모르면 아니다");
    assert!(!agy_old_binary(Some(100), None));
}

#[test]
fn handoff_server_name_is_ref_and_a_short_summary() {
    assert_eq!(
        handoff_server_name("rocky", 41, "핸드오프 작업"),
        "rocky-41: 핸드오프 작업"
    );
    assert_eq!(
        handoff_server_name(
            "rocky",
            41,
            "핸드오프 세션을 rc 로 띄우고 데몬이 첫 메시지를 넣는다"
        ),
        "rocky-41: 핸드오프 세션을 rc 로 띄우고 데몬이 첫…",
        "단어가 끝난 자리에서 24 글자가 찼다"
    );
    assert_eq!(
        handoff_server_name(
            "rocky",
            41,
            "핸드오프 세션을 rc 로 띄우고 데몬이 첫메시지를 넣는다"
        ),
        "rocky-41: 핸드오프 세션을 rc 로 띄우고 데몬이…",
        "단어 중간이면 마지막 띄어쓰기까지 물러난다"
    );
    assert_eq!(
        handoff_server_name("rocky", 7, "  줄\n바꿈  "),
        "rocky-7: 줄 바꿈"
    );
    assert_eq!(handoff_server_name("rocky", 7, "   "), "rocky-7");
    let argv = handoff_server_argv("rocky-41: 핸드오프 작업");
    assert_eq!(argv[..4], ["claude", "rc", "--spawn", "session"]);
    assert_eq!(argv[4], "--name=rocky-41: 핸드오프 작업", "이름은 한 인자");
    assert!(is_server_argv(&argv.join(" ")));
    assert_eq!(handoff_log_label("rocky", 41), "handoff-rocky-41");
    assert_eq!(
        handoff_log_label("../x/y", 2),
        "handoff-___x_y-2",
        "보드 key 의 / · . 은 파일 이름에 넣지 않는다"
    );
}

#[test]
fn handoff_session_is_the_child_of_that_server_in_that_dir() {
    use rocky_core::peer_inbox::InboxRegistration;
    let reg = |id: &str, pid: u32, cwd: &str, seen_at: i64| InboxRegistration {
        session_id: id.into(),
        socket: format!("/tmp/cc-socks/{pid}.sock"),
        cwd: cwd.into(),
        seen_at,
        restored: false,
    };
    let regs = vec![
        reg("old", 300, "/w/a", 30),       // 그 서버의 자식이 아니다
        reg("reused-pid", 200, "/w/a", 5), // 같은 소켓(pid 재사용)인데 서버를 띄우기 전 등록
        reg("new", 200, "/w/a/", 20),
    ];
    let rows = parse_ps(
        "  100     1 01:00 claude rc --spawn session --name rocky-41: x\n  200   100 00:50 /x/claude --print --sdk-url https://a/v1/code/sessions/cse_1\n  300     1 05:00 /x/claude\n",
    );
    assert_eq!(
        handoff_session(&regs, &rows, 100, 10).map(|r| r.session_id.as_str()),
        Some("new"),
        "서버를 띄운 뒤(10) 등록한 그 서버의 자식"
    );
    assert_eq!(handoff_session(&regs, &rows, 999, 10), None, "다른 서버");
    assert_eq!(
        handoff_session(&regs, &rows, 100, 25),
        None,
        "띄운 뒤의 등록이 아직 없다"
    );
    assert_eq!(socket_pid("/tmp/cc-socks/200.sock"), Some(200));
    assert_eq!(socket_pid("/tmp/other/200.sock"), None);
    assert_eq!(
        worktree_base("origin/main\n").as_deref(),
        Some("origin/main")
    );
    assert_eq!(worktree_base(""), None);
    assert_eq!(worktree_base("origin/"), None);
    assert_eq!(worktree_branch("todo-41"), "worktree-todo-41");
}
