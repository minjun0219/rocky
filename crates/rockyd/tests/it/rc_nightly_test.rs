//! rc 야간 재시작 — 가짜 프로세스 세계와 가짜 시계로 판정 · 순서를 고정한다(실제 `claude` 는 띄우지 않는다).

use std::collections::{BTreeMap, HashSet, VecDeque};
use std::path::Path;
use std::sync::{Arc, Mutex};

use chrono::NaiveDateTime;
use rocky_core::config::{NightlyConfig, RcConfig};
use rocky_core::rc::{NightlyOutcome, NightlyReport};
use rockyd::rc::{RcCommand, RcController, RcNotifier, RcOps};
use rockyd::runner::{CmdOutput, Runner};

use crate::common::*;

#[derive(Clone, Copy)]
struct Behavior {
    out: &'static str,
    stays: bool,
}

const READY: Behavior = Behavior {
    out: "·✔︎· Ready · x · main",
    stays: true,
};
const DIES: Behavior = Behavior {
    out: "",
    stays: false,
};

struct World {
    alive: HashSet<u32>,
    /// 라벨 → 지금 그 폴더의 서버 pid.
    servers: BTreeMap<String, u32>,
    /// 열린 세션이 붙은 라벨.
    sessions: HashSet<String>,
    next_pid: u32,
    script: VecDeque<Behavior>,
    /// 일어난 순서 — `stop repo-a` · `spawn repo-a <방식 인자>`.
    log: Vec<String>,
    clock: NaiveDateTime,
    online: bool,
    version: Option<&'static str>,
    logged_in: bool,
    ps_calls: usize,
    /// 이 번째 `ps` 는 실패한다(1부터).
    fail_ps_call: Option<usize>,
}

fn at(h: u32, m: u32) -> NaiveDateTime {
    chrono::NaiveDate::from_ymd_opt(2026, 10, 7)
        .unwrap()
        .and_hms_opt(h, m, 0)
        .unwrap()
}

fn world(clock: NaiveDateTime, running: &[&str], script: Vec<Behavior>) -> Arc<Mutex<World>> {
    let mut servers = BTreeMap::new();
    let mut alive = HashSet::new();
    for (i, label) in running.iter().enumerate() {
        let pid = 100 + i as u32;
        servers.insert(label.to_string(), pid);
        alive.insert(pid);
    }
    Arc::new(Mutex::new(World {
        alive,
        servers,
        sessions: HashSet::new(),
        next_pid: 200,
        script: script.into(),
        log: Vec::new(),
        clock,
        online: true,
        version: Some("2.1.300 (Claude Code)"),
        logged_in: true,
        ps_calls: 0,
        fail_ps_call: None,
    }))
}

fn ok(stdout: String) -> CmdOutput {
    CmdOutput {
        code: 0,
        stdout,
        stderr: String::new(),
    }
}

fn runner(w: Arc<Mutex<World>>) -> Runner {
    Arc::new(move |argv: Vec<String>, _stdin, _timeout| {
        let mut w = w.lock().unwrap();
        let out = match (argv[0].as_str(), argv.get(1).map(String::as_str)) {
            ("ps", _)
                if {
                    w.ps_calls += 1;
                    w.fail_ps_call == Some(w.ps_calls)
                } =>
            {
                CmdOutput::failure("ps: 잠깐 실패")
            }
            ("ps", _) => {
                let mut ps = String::from("    1     0 30-00:00:00 /sbin/launchd\n");
                for (label, pid) in &w.servers {
                    if !w.alive.contains(pid) {
                        continue;
                    }
                    ps.push_str(&format!(
                        "  {pid}     1    10:00 claude rc --name {label}\n"
                    ));
                    if w.sessions.contains(label) {
                        ps.push_str(&format!(
                            "  {}   {pid}    05:00 /x/claude --sdk-url https://a/v1/code/sessions/cse_1\n",
                            pid + 1000
                        ));
                    }
                }
                ok(ps)
            }
            ("lsof", _) => ok(w
                .servers
                .iter()
                .filter(|(_, pid)| w.alive.contains(pid))
                .map(|(label, pid)| format!("p{pid}\nfcwd\nn/w/{label}\n"))
                .collect()),
            ("claude", Some("--version")) => match w.version {
                Some(v) => ok(v.to_string()),
                None => CmdOutput::failure("시간 초과"),
            },
            ("claude", Some("update")) => ok("Successfully updated".into()),
            ("claude", Some("auth")) => ok(format!(r#"{{"loggedIn": {}}}"#, w.logged_in)),
            ("curl", _) if w.online => ok(String::new()),
            _ => CmdOutput::failure("없음"),
        };
        Box::pin(async move { out })
    })
}

fn ops(world: Arc<Mutex<World>>) -> RcOps {
    let (w1, w2, w3, w4) = (world.clone(), world.clone(), world.clone(), world);
    RcOps {
        spawn: Arc::new(
            move |argv: &[String], _dir: &Path, out: &Path, err: &Path| {
                let mut w = w1.lock().unwrap();
                let b = w.script.pop_front().expect("예상보다 많이 띄웠다");
                std::fs::write(out, b.out)?;
                std::fs::write(err, "")?;
                w.next_pid += 1;
                let pid = w.next_pid;
                let label = argv[3].clone();
                if b.stays {
                    w.alive.insert(pid);
                    w.servers.insert(label.clone(), pid);
                }
                w.log.push(format!("spawn {}", argv[3..].join(" ")));
                Ok(pid)
            },
        ),
        signal: Arc::new(move |pid, sig| {
            let mut w = w2.lock().unwrap();
            if sig == 0 {
                return w.alive.contains(&pid);
            }
            if !w.alive.remove(&pid) {
                return false;
            }
            let label = w
                .servers
                .iter()
                .find(|(_, p)| **p == pid)
                .map(|(l, _)| l.clone())
                .unwrap_or_default();
            w.log.push(format!("stop {label}"));
            true
        }),
        sleep: Arc::new(move |d| {
            let mut w = w3.lock().unwrap();
            w.clock += d;
            Box::pin(async {})
        }),
        now: Arc::new(move || w4.lock().unwrap().clock),
    }
}

fn config() -> RcConfig {
    RcConfig {
        root: Some("/w".into()),
        pinned: vec!["repo-a".into()],
        targets: vec!["repo-b".into(), "repo-c".into(), "repo-d".into()],
        supervise: true,
        nightly: Some(NightlyConfig::default()),
    }
}

struct Fixture {
    dir: tempfile::TempDir,
    world: Arc<Mutex<World>>,
    control: Arc<RcController>,
    banners: Arc<Mutex<Vec<String>>>,
    notify: RcNotifier,
}

/// repo-a 고정, 나머지 비고정. `records` 는 라벨별 기동 버전 기록.
fn fixture(world: Arc<Mutex<World>>, records: &[(&str, &str)]) -> Fixture {
    fixture_with(world, records, config())
}

fn fixture_with(world: Arc<Mutex<World>>, records: &[(&str, &str)], config: RcConfig) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let rc_dir = dir.path().join("rc");
    std::fs::create_dir_all(&rc_dir).unwrap();
    for (label, v) in records {
        std::fs::write(rc_dir.join(format!("{label}.version")), format!("{v}\n")).unwrap();
    }
    let control = Arc::new(RcController::new(
        Some(config),
        dir.path().join("home").to_string_lossy().into_owned(),
        rc_dir,
        runner(world.clone()),
        ops(world.clone()),
    ));
    let banners = Arc::new(Mutex::new(Vec::new()));
    let sink = banners.clone();
    Fixture {
        dir,
        world,
        control,
        banners,
        notify: Arc::new(move |_title, body| sink.lock().unwrap().push(body)),
    }
}

async fn run(f: &Fixture) -> NightlyReport {
    f.control.begin_nightly().expect("돌고 있지 않다");
    f.control.run_nightly(&f.notify).await
}

fn log(f: &Fixture) -> Vec<String> {
    f.world.lock().unwrap().log.clone()
}

fn outcome(report: &NightlyReport, label: &str) -> (NightlyOutcome, String) {
    let item = report
        .items
        .iter()
        .find(|i| i.label == label)
        .unwrap_or_else(|| panic!("{label} 결과가 없다: {report:?}"));
    (item.outcome, item.note.clone())
}

/// 열린 세션이 방금 대화했다 — 대화 기록을 지금 시각으로 만든다.
fn talk(f: &Fixture, label: &str) {
    let projects = f
        .dir
        .path()
        .join("home/.claude/projects")
        .join(format!("-w-{label}"));
    std::fs::create_dir_all(&projects).unwrap();
    std::fs::write(projects.join("s.jsonl"), "{}\n").unwrap();
    f.world.lock().unwrap().sessions.insert(label.to_string());
}

#[tokio::test]
async fn preview_says_what_the_night_would_do_and_touches_nothing() {
    let w = world(at(4, 30), &["repo-a", "repo-b", "repo-c", "repo-d"], vec![]);
    let f = fixture(
        w,
        &[
            ("repo-a", "2.1.200"),
            ("repo-b", "2.1.200"),
            ("repo-c", "2.1.300"),
        ],
    );
    talk(&f, "repo-b");
    let report = f.control.nightly_preview().await;
    assert!(report.dry_run);
    assert_eq!(report.update, "건너뜀(리허설) · 설치 2.1.300");
    assert_eq!(
        outcome(&report, "repo-a"),
        (
            NightlyOutcome::WouldRestart,
            "2.1.200 → 2.1.300 · 새 세션과 함께".into()
        )
    );
    assert_eq!(
        outcome(&report, "repo-b"),
        (
            NightlyOutcome::WouldWait,
            "작업 중 — 07:00 까지 5분마다 다시 본다".into()
        )
    );
    assert_eq!(outcome(&report, "repo-c").0, NightlyOutcome::Current);
    assert_eq!(
        outcome(&report, "repo-d"),
        (
            NightlyOutcome::Skipped,
            "기동 버전 기록 없음 — 구버전인지 모른다".into()
        )
    );
    let w = f.world.lock().unwrap();
    assert!(w.log.is_empty(), "내리지도 띄우지도 않는다: {:?}", w.log);
    assert_eq!(w.clock, at(4, 30), "기다리지 않는다");
    let files: Vec<String> = std::fs::read_dir(f.dir.path().join("rc"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|n| !n.ends_with(".version"))
        .collect();
    assert!(files.is_empty(), "기록을 남기지 않는다: {files:?}");
}

#[tokio::test]
async fn preview_after_the_deadline_moves_busy_servers_to_tomorrow() {
    let w = world(at(9, 0), &["repo-b"], vec![]);
    let f = fixture(w, &[("repo-b", "2.1.200")]);
    talk(&f, "repo-b");
    let report = f.control.nightly_preview().await;
    assert_eq!(
        outcome(&report, "repo-b"),
        (
            NightlyOutcome::Skipped,
            "작업 중 — 07:00 이 지나 다음 날로".into()
        )
    );
}

#[tokio::test]
async fn installed_version_falls_back_to_the_install_path() {
    let w = world(at(4, 30), &["repo-a"], vec![]);
    w.lock().unwrap().version = None;
    let f = fixture(w, &[("repo-a", "2.1.200")]);
    let report = f.control.nightly_preview().await;
    assert_eq!(report.version, None);
    assert_eq!(
        outcome(&report, "repo-a"),
        (NightlyOutcome::Skipped, "설치 버전을 못 쟀다".into())
    );

    // 새 바이너리의 첫 실행이 멎어 `--version` 을 못 재도 설치 경로(링크 대상)로 읽는다.
    let home = f.dir.path().join("home");
    let versions = home.join(".local/share/claude/versions");
    std::fs::create_dir_all(&versions).unwrap();
    std::fs::write(versions.join("2.1.299"), "").unwrap();
    std::fs::create_dir_all(home.join(".local/bin")).unwrap();
    std::os::unix::fs::symlink(versions.join("2.1.299"), home.join(".local/bin/claude")).unwrap();
    let report = f.control.nightly_preview().await;
    assert_eq!(report.version.as_deref(), Some("2.1.299"));
    assert_eq!(outcome(&report, "repo-a").0, NightlyOutcome::WouldRestart);
}

#[tokio::test]
async fn preview_is_blocked_when_logged_out() {
    let w = world(at(4, 30), &["repo-a"], vec![]);
    w.lock().unwrap().logged_in = false;
    let f = fixture(w, &[("repo-a", "2.1.200")]);
    let report = f.control.nightly_preview().await;
    assert_eq!(report.blocked.as_deref(), Some("logged-out"));
    assert!(report.items.is_empty());
}

#[tokio::test]
async fn preview_route_answers_with_the_report() {
    let w = world(at(4, 30), &["repo-a"], vec![]);
    let f = fixture(w, &[("repo-a", "2.1.300")]);
    let control = f.control.clone();
    let fx_ = fx();
    let state = rebuild(&fx_, move |o| o.rc_control = Some(control));
    let (code, body) = call(
        &state,
        "GET",
        "/api/rc/nightly/preview",
        None,
        ReqOptions::default(),
    )
    .await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body["dryRun"], true);
    let remote = ReqOptions {
        peer: Some("100.64.0.1"),
        ..Default::default()
    };
    let (code, _) = call(&state, "GET", "/api/rc/nightly/preview", None, remote).await;
    assert_eq!(code, 403, "프로세스를 띄우는 읽기는 로컬 전용");
    assert_eq!(body["items"][0]["outcome"], "current");
    let (code, _) = call(
        &fx_.state,
        "GET",
        "/api/rc/nightly/preview",
        None,
        ReqOptions::default(),
    )
    .await;
    assert_eq!(code, 404, "rc 가 꺼진 기기");
}

// ── 야간 재시작 ──

#[tokio::test]
async fn restarts_stale_idle_servers_canary_first() {
    let w = world(
        at(4, 30),
        &["repo-b", "repo-a", "repo-c"],
        vec![READY, READY],
    );
    let f = fixture(
        w,
        &[
            ("repo-a", "2.1.200"),
            ("repo-b", "2.1.200"),
            ("repo-c", "2.1.300"),
        ],
    );
    let report = run(&f).await;
    assert_eq!(
        log(&f),
        vec![
            "stop repo-a",
            "spawn repo-a",
            "stop repo-b",
            "spawn repo-b --no-create-session-in-dir",
        ],
        "고정이 canary — 뜬 뒤에 나머지를 내린다"
    );
    assert_eq!(outcome(&report, "repo-a").0, NightlyOutcome::Restarted);
    assert_eq!(
        outcome(&report, "repo-b").1,
        "2.1.200 → 2.1.300 · 서버만(세션은 앱에서)"
    );
    assert_eq!(outcome(&report, "repo-c").0, NightlyOutcome::Current);
    let rc_dir = f.dir.path().join("rc");
    assert_eq!(
        std::fs::read_to_string(rc_dir.join("repo-b.version")).unwrap(),
        "2.1.300\n"
    );
    assert!(
        !rc_dir.join("repo-a.revive").exists() && !rc_dir.join("repo-b.revive").exists(),
        "뜬 것의 표식은 지운다"
    );
    assert!(
        f.banners.lock().unwrap().is_empty(),
        "다 떴으면 울리지 않는다"
    );
    let mut status = rocky_core::rc::RcStatus::unconfigured();
    f.control.decorate(&mut status);
    let nightly = status.nightly.expect("손으로 돌린 결과가 현황에 실린다");
    assert!(!nightly.running);
    assert_eq!(nightly.at, None, "일정은 켜지 않았다");
    assert_eq!(nightly.last.unwrap().count(NightlyOutcome::Restarted), 2);
}

#[tokio::test]
async fn servers_without_a_record_or_version_are_left_alone() {
    let w = world(at(4, 30), &["repo-a", "repo-b"], vec![]);
    let f = fixture(w, &[("repo-a", "2.1.300")]);
    let report = run(&f).await;
    assert_eq!(outcome(&report, "repo-b").0, NightlyOutcome::Skipped);
    assert!(log(&f).is_empty());

    let w = world(at(4, 30), &["repo-a"], vec![]);
    w.lock().unwrap().version = None;
    let f = fixture(w, &[("repo-a", "2.1.200")]);
    let report = run(&f).await;
    assert_eq!(
        outcome(&report, "repo-a"),
        (NightlyOutcome::Skipped, "설치 버전을 못 쟀다".into())
    );
    assert!(log(&f).is_empty());
}

#[tokio::test]
async fn failed_canary_leaves_the_rest_and_hands_itself_to_supervise() {
    let w = world(at(4, 30), &["repo-a", "repo-b"], vec![DIES]);
    let f = fixture(w, &[("repo-a", "2.1.200"), ("repo-b", "2.1.200")]);
    let report = run(&f).await;
    assert!(report.canary_failed);
    let (o, note) = outcome(&report, "repo-b");
    assert_eq!(o, NightlyOutcome::Skipped);
    assert!(note.contains("repo-a 가 안 돼서"), "{note}");
    assert!(
        !log(&f).contains(&"stop repo-b".to_string()),
        "나머지는 내리지 않는다"
    );
    assert_eq!(outcome(&report, "repo-a").0, NightlyOutcome::Down);
    assert!(
        f.dir.path().join("rc/repo-a.revive").exists(),
        "감시가 살릴 표식"
    );
    assert!(
        f.control.begin("repo-a", RcCommand::Start).is_ok(),
        "끝나면 잠금을 놓는다"
    );
    assert_eq!(f.banners.lock().unwrap().len(), 1, "canary 실패는 알린다");
}

#[tokio::test]
async fn offline_stops_nothing() {
    let w = world(at(4, 30), &["repo-a", "repo-b"], vec![]);
    w.lock().unwrap().online = false;
    let f = fixture(w, &[("repo-a", "2.1.200"), ("repo-b", "2.1.200")]);
    let report = run(&f).await;
    assert!(log(&f).is_empty(), "내리면 다시 등록하지 못한다");
    assert_eq!(report.count(NightlyOutcome::Skipped), 2);
    assert!(f.control.begin("repo-a", RcCommand::Start).is_ok());
}

#[tokio::test]
async fn busy_server_moves_to_tomorrow() {
    let w = world(at(4, 30), &["repo-b"], vec![]);
    let f = fixture(w, &[("repo-b", "2.1.200")]);
    talk(&f, "repo-b");
    let report = run(&f).await;
    let (o, note) = outcome(&report, "repo-b");
    assert_eq!(o, NightlyOutcome::Skipped);
    assert!(note.contains("다음 날로"), "{note}");
    assert!(log(&f).is_empty());
}

#[tokio::test]
async fn logged_out_daemon_context_blocks_the_whole_run() {
    let w = world(at(4, 30), &["repo-a"], vec![]);
    w.lock().unwrap().logged_in = false;
    let f = fixture(w, &[("repo-a", "2.1.200")]);
    let report = run(&f).await;
    assert_eq!(report.blocked.as_deref(), Some("logged-out"));
    assert!(report.items.is_empty() && log(&f).is_empty());
}

#[tokio::test]
async fn schedule_runs_once_a_day_and_not_on_the_first_afternoon() {
    let w = world(at(4, 30), &[], vec![]);
    let f = fixture(w, &[]);
    // 처음 켠 날 낮 — 이미 돈 것으로 친다.
    assert!(!f.control.claim_nightly(at(12, 0)));
    let next = |h, m| {
        chrono::NaiveDate::from_ymd_opt(2026, 10, 8)
            .unwrap()
            .and_hms_opt(h, m, 0)
            .unwrap()
    };
    assert!(!f.control.claim_nightly(next(4, 29)));
    assert!(
        f.control.claim_nightly(next(9, 0)),
        "자고 넘겼으면 깬 뒤 한 번"
    );
    assert!(
        !f.control.claim_nightly(next(9, 1)),
        "돌고 있는 동안 · 같은 날은 다시 안 돈다"
    );
    f.control.run_nightly(&f.notify).await;
    assert!(!f.control.claim_nightly(next(23, 0)));
    let raw = std::fs::read_to_string(f.dir.path().join("rc/nightly.json")).unwrap();
    assert!(raw.contains(r#""lastRun":"2026-10-08""#), "{raw}");
}

#[tokio::test]
async fn manual_run_route_is_local_only_and_one_at_a_time() {
    let w = world(at(4, 30), &["repo-a"], vec![]);
    let f = fixture(w, &[("repo-a", "2.1.300")]);
    let control = f.control.clone();
    let fx_ = fx();
    let state = rebuild(&fx_, move |o| o.rc_control = Some(control));
    let remote = ReqOptions {
        peer: Some("100.64.0.1"),
        ..Default::default()
    };
    let (code, _) = call(&state, "POST", "/api/rc/nightly", None, remote).await;
    assert_eq!(code, 403, "서버를 내리고 띄운다");
    // 일정이 돌고 있는 동안 손 실행은 409.
    f.control.begin_nightly().unwrap();
    let (code, body) = post(&state, "/api/rc/nightly", serde_json::json!({})).await;
    assert_eq!(code, 409, "{body}");
    let (code, _) = post(&fx_.state, "/api/rc/nightly", serde_json::json!({})).await;
    assert_eq!(code, 404, "rc 가 꺼진 기기");
}

#[tokio::test]
async fn a_restart_that_never_stopped_the_server_is_not_a_restart() {
    // 첫 ps 는 야간의 판정, 둘째는 기동기가 내리기 직전 — 그것이 실패하면 옛 서버는 그대로다.
    let w = world(at(4, 30), &["repo-a", "repo-b"], vec![]);
    w.lock().unwrap().fail_ps_call = Some(2);
    let f = fixture(w, &[("repo-a", "2.1.200"), ("repo-b", "2.1.200")]);
    let report = run(&f).await;
    let (o, note) = outcome(&report, "repo-a");
    assert_eq!(o, NightlyOutcome::Skipped);
    assert!(note.starts_with("손대지 못했다 — 현황을 못 읽어"), "{note}");
    assert!(
        report.canary_failed,
        "canary 가 안 됐으니 나머지는 건드리지 않는다"
    );
    assert_eq!(outcome(&report, "repo-b").0, NightlyOutcome::Skipped);
    assert!(log(&f).is_empty(), "아무것도 내리지 않았다");
    assert!(
        !f.dir.path().join("rc/repo-a.revive").exists(),
        "떠 있는 서버의 표식은 남기지 않는다"
    );
    assert!(f.control.begin("repo-a", RcCommand::Start).is_ok());
}

#[tokio::test]
async fn without_supervise_the_report_says_who_must_start_it() {
    let w = world(at(4, 30), &["repo-b"], vec![DIES]);
    let f = fixture_with(
        w,
        &[("repo-b", "2.1.200")],
        RcConfig {
            supervise: false,
            ..config()
        },
    );
    let report = run(&f).await;
    let (o, note) = outcome(&report, "repo-b");
    assert_eq!(o, NightlyOutcome::Down);
    assert!(note.contains("rocky rc start"), "{note}");
    assert!(f.banners.lock().unwrap()[0].contains("rocky rc start"));
}
