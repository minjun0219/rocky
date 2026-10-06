//! rc 야간 재시작 — 가짜 프로세스 세계와 가짜 시계로 판정 · 순서를 고정한다(실제 `claude` 는 띄우지 않는다).

use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex};

use chrono::NaiveDateTime;
use rocky_core::config::{NightlyConfig, RcConfig};
use rocky_core::rc::{NightlyOutcome, NightlyReport};
use rockyd::rc::{RcController, RcOps};
use rockyd::runner::{CmdOutput, Runner};

use crate::common::*;

struct World {
    alive: HashSet<u32>,
    /// 라벨 → 지금 그 폴더의 서버 pid.
    servers: BTreeMap<String, u32>,
    /// 열린 세션이 붙은 라벨.
    sessions: HashSet<String>,
    /// 일어난 일 — `stop repo-a` · `spawn repo-a <방식 인자>`.
    log: Vec<String>,
    clock: NaiveDateTime,
    version: Option<&'static str>,
    logged_in: bool,
}

fn at(h: u32, m: u32) -> NaiveDateTime {
    chrono::NaiveDate::from_ymd_opt(2026, 10, 7)
        .unwrap()
        .and_hms_opt(h, m, 0)
        .unwrap()
}

fn world(clock: NaiveDateTime, running: &[&str]) -> Arc<Mutex<World>> {
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
        log: Vec::new(),
        clock,
        version: Some("2.1.300 (Claude Code)"),
        logged_in: true,
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
        let w = w.lock().unwrap();
        let out = match (argv[0].as_str(), argv.get(1).map(String::as_str)) {
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
            ("claude", Some("auth")) => ok(format!(r#"{{"loggedIn": {}}}"#, w.logged_in)),
            _ => CmdOutput::failure("없음"),
        };
        Box::pin(async move { out })
    })
}

fn ops(world: Arc<Mutex<World>>) -> RcOps {
    let (w1, w2, w3, w4) = (world.clone(), world.clone(), world.clone(), world);
    RcOps {
        spawn: Arc::new(
            move |argv: &[String], _dir: &Path, _out: &Path, _err: &Path| {
                w1.lock()
                    .unwrap()
                    .log
                    .push(format!("spawn {}", argv[3..].join(" ")));
                Err(std::io::Error::other("이 테스트는 띄우지 않는다"))
            },
        ),
        signal: Arc::new(move |pid, sig| {
            let mut w = w2.lock().unwrap();
            if sig != 0 {
                w.log.push(format!("signal {pid} {sig}"));
            }
            w.alive.contains(&pid)
        }),
        sleep: Arc::new(move |d| {
            w3.lock().unwrap().clock += d;
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
}

/// repo-a 고정, 나머지 비고정. `records` 는 라벨별 기동 버전 기록.
fn fixture(world: Arc<Mutex<World>>, records: &[(&str, &str)]) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let rc_dir = dir.path().join("rc");
    std::fs::create_dir_all(&rc_dir).unwrap();
    for (label, v) in records {
        std::fs::write(rc_dir.join(format!("{label}.version")), format!("{v}\n")).unwrap();
    }
    let control = Arc::new(RcController::new(
        Some(config()),
        dir.path().join("home").to_string_lossy().into_owned(),
        rc_dir,
        runner(world.clone()),
        ops(world.clone()),
    ));
    Fixture {
        dir,
        world,
        control,
    }
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
    let w = world(at(4, 30), &["repo-a", "repo-b", "repo-c", "repo-d"]);
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
    let w = world(at(9, 0), &["repo-b"]);
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
    let w = world(at(4, 30), &["repo-a"]);
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
    let w = world(at(4, 30), &["repo-a"]);
    w.lock().unwrap().logged_in = false;
    let f = fixture(w, &[("repo-a", "2.1.200")]);
    let report = f.control.nightly_preview().await;
    assert_eq!(report.blocked.as_deref(), Some("logged-out"));
    assert!(report.items.is_empty());
}

#[tokio::test]
async fn preview_route_answers_with_the_report() {
    let w = world(at(4, 30), &["repo-a"]);
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
