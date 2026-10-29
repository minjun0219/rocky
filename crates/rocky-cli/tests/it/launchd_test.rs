//! launchd job 교체 — bootout 직후 bootstrap 이 튀는 레이스를 재시도로 넘기는지, 재시작이 그대로인 plist 를
//! 다시 쓰지 않고 `kickstart -k` 로 끝내는지.

use std::cell::RefCell;
use std::path::Path;
use std::time::{Duration, SystemTime};

use rocky_cli::launchd::{
    baked_path, exec_through_link, join_path, ownership_warning, parse_job, plan_restart,
    plist_content, register_job, restart_job, stalled_job_warning, JobRestart, LaunchdJob,
    PlistValues, LAUNCHD_LABEL,
};

const DOMAIN: &str = "gui/501";
const PLIST: &str = "/Users/u/Library/LaunchAgents/com.rocky.daemon.plist";

/// 가짜 launchctl — `print` 는 서비스가 "로드된 상태" 일 때만 성공하고, `bootstrap` 은
/// 정해진 횟수만큼 실패한 뒤 성공한다. 호출 순서를 남긴다.
struct Fake {
    calls: RefCell<Vec<String>>,
    loaded: RefCell<bool>,
    bootstrap_failures: RefCell<u32>,
    /// bootout 뒤에도 서비스가 `print` 에 몇 번 더 보이는가(비동기 해제 흉내).
    linger: RefCell<u32>,
    /// 로드된 job 이 띄우는 실행 파일 — 있으면 `print` 가 `program =` 줄을 같이 낸다.
    program: RefCell<Option<String>>,
    kickstart_ok: RefCell<bool>,
}

impl Fake {
    fn new(loaded: bool, bootstrap_failures: u32, linger: u32) -> Self {
        Fake {
            calls: RefCell::new(Vec::new()),
            loaded: RefCell::new(loaded),
            bootstrap_failures: RefCell::new(bootstrap_failures),
            linger: RefCell::new(linger),
            program: RefCell::new(None),
            kickstart_ok: RefCell::new(true),
        }
    }

    /// `program` 을 띄우는 job 이 로드돼 돌고 있다.
    fn serving(program: &str) -> Self {
        let fake = Fake::new(true, 0, 0);
        *fake.program.borrow_mut() = Some(program.into());
        fake
    }

    fn print_output(&self) -> String {
        match self.program.borrow().as_deref() {
            Some(program) => format!("state = running\nprogram = {program}\npid = 4321"),
            None => "state = running".into(),
        }
    }

    fn run(&self, args: &[&str]) -> (bool, String) {
        self.calls.borrow_mut().push(args.join(" "));
        match args[0] {
            "bootout" => {
                if *self.linger.borrow() == 0 {
                    *self.loaded.borrow_mut() = false;
                }
                (true, String::new())
            }
            "print" => {
                let mut linger = self.linger.borrow_mut();
                if *linger > 0 {
                    *linger -= 1;
                    if *linger == 0 {
                        *self.loaded.borrow_mut() = false;
                    }
                    return (true, self.print_output());
                }
                if *self.loaded.borrow() {
                    (true, self.print_output())
                } else {
                    (false, format!("Could not find service \"{LAUNCHD_LABEL}\""))
                }
            }
            "bootstrap" => {
                let mut left = self.bootstrap_failures.borrow_mut();
                if *left > 0 {
                    *left -= 1;
                    return (false, "Bootstrap failed: 5: Input/output error".into());
                }
                *self.loaded.borrow_mut() = true;
                (true, String::new())
            }
            "kickstart" => {
                let ok = *self.kickstart_ok.borrow();
                (
                    ok,
                    if ok {
                        String::new()
                    } else {
                        "Operation not permitted".into()
                    },
                )
            }
            other => panic!("unexpected launchctl {other}"),
        }
    }

    fn count(&self, verb: &str) -> usize {
        self.calls
            .borrow()
            .iter()
            .filter(|c| c.starts_with(verb))
            .count()
    }
}

#[test]
fn bootout_then_bootstrap_then_verify() {
    let fake = Fake::new(true, 0, 0);
    register_job(&|a| fake.run(a), DOMAIN, PLIST, Duration::ZERO).unwrap();
    let calls = fake.calls.borrow();
    assert_eq!(calls[0], format!("bootout {DOMAIN} {PLIST}"));
    assert_eq!(fake.count("bootstrap"), 1);
    // 마지막은 로드 확인.
    assert_eq!(
        calls.last().unwrap(),
        &format!("print {DOMAIN}/{LAUNCHD_LABEL}")
    );
}

/// bootout 직후의 bootstrap 은 옛 서비스가 아직 남아 있어 `5: Input/output error` 로 튄다 —
/// 실제 사고의 모양. 몇 번 더 시도하면 붙는다.
#[test]
fn a_transient_bootstrap_failure_is_retried() {
    let fake = Fake::new(true, 2, 0);
    register_job(&|a| fake.run(a), DOMAIN, PLIST, Duration::ZERO).unwrap();
    assert_eq!(fake.count("bootstrap"), 3);
}

#[test]
fn waits_for_the_old_service_to_leave_before_bootstrapping() {
    let fake = Fake::new(true, 0, 3);
    register_job(&|a| fake.run(a), DOMAIN, PLIST, Duration::ZERO).unwrap();
    let calls = fake.calls.borrow();
    let first_bootstrap = calls
        .iter()
        .position(|c| c.starts_with("bootstrap"))
        .unwrap();
    let prints_before: usize = calls[..first_bootstrap]
        .iter()
        .filter(|c| c.starts_with("print"))
        .count();
    assert!(prints_before >= 3, "{calls:?}");
    assert_eq!(fake.count("bootstrap"), 1);
}

/// 끝까지 안 붙으면 사유를 돌려준다 — 호출자가 데몬이 사라졌다고 알아차릴 유일한 길이다.
#[test]
fn persistent_failure_is_an_error_with_the_launchctl_output() {
    let fake = Fake::new(true, 99, 0);
    let error = register_job(&|a| fake.run(a), DOMAIN, PLIST, Duration::ZERO).unwrap_err();
    assert!(error.contains("Input/output error"), "{error}");
    assert!(error.contains(LAUNCHD_LABEL), "{error}");
    assert_eq!(fake.count("bootstrap"), 5);
}

/// `launchctl print` 의 서비스 블록 — 돌고 있을 때와 KeepAlive 재기동 대기일 때.
const PRINT_RUNNING: &str = "gui/501/com.rocky.daemon = {
	active count = 1
	path = /Users/me/Library/LaunchAgents/com.rocky.daemon.plist
	type = LaunchAgent
	state = running

	program = /Users/me/.local/share/rocky/v0.38.0/rockyd
	pid = 4321
	endpoints = {
		state = active
	}
}";
const PRINT_SCHEDULED: &str = "gui/501/com.rocky.daemon = {
	active count = 0
	state = spawn scheduled
	program = /Users/me/.local/share/rocky/v0.38.0/rockyd
	last exit code = 0
}";

#[test]
fn print_output_gives_the_job_state_pid_and_program() {
    let program = Some("/Users/me/.local/share/rocky/v0.38.0/rockyd".to_string());
    assert_eq!(
        parse_job(PRINT_RUNNING),
        LaunchdJob {
            state: "running".into(),
            pid: Some(4321),
            program: program.clone(),
        }
    );
    assert_eq!(
        parse_job(PRINT_SCHEDULED),
        LaunchdJob {
            state: "spawn scheduled".into(),
            pid: None,
            program,
        }
    );
}

#[test]
fn a_port_holder_that_is_not_the_launchd_process_is_flagged() {
    let running = parse_job(PRINT_RUNNING);
    assert_eq!(ownership_warning(Some(4321), Some(&running)), None);

    let warning = ownership_warning(Some(32682), Some(&running)).expect("다른 pid");
    assert!(
        warning.contains("pid 32682") && warning.contains("pid 4321"),
        "{warning}"
    );
    assert!(warning.contains("rocky daemon restart"), "{warning}");

    // 실제 사고 모양: launchd 데몬은 already running 으로 끝나 재기동 대기 중
    let scheduled = parse_job(PRINT_SCHEDULED);
    let warning = ownership_warning(Some(32682), Some(&scheduled)).expect("job 이 안 돈다");
    assert!(warning.contains("state=spawn scheduled"), "{warning}");
}

#[test]
fn ownership_is_not_judged_without_a_loaded_job_or_a_pid() {
    assert_eq!(ownership_warning(Some(1), None), None);
    assert_eq!(
        ownership_warning(None, Some(&parse_job(PRINT_RUNNING))),
        None
    );
}

/// 포트가 비었는데 job 이 돌지 않는다 — 데몬이 뜨자마자 끝나는 루프. 막 뜨는 중(running)이면 말하지 않는다.
#[test]
fn a_job_that_keeps_dying_is_flagged_with_its_log() {
    let warning = stalled_job_warning(&parse_job(PRINT_SCHEDULED)).expect("spawn scheduled");
    assert!(
        warning.contains("state=spawn scheduled") && warning.contains(".log"),
        "{warning}"
    );
    assert!(warning.contains("rocky daemon restart"), "{warning}");
    assert_eq!(stalled_job_warning(&parse_job(PRINT_RUNNING)), None);
}

#[test]
fn rebaking_the_daemons_own_path_does_not_grow_it() {
    let fallback = "/opt/homebrew/bin:/usr/bin:/bin";
    let once = join_path("/Users/me/.cargo/bin:/usr/bin", fallback);
    assert_eq!(once, "/Users/me/.cargo/bin:/usr/bin:/opt/homebrew/bin:/bin");
    // 데몬 아래에서 부른 `daemon install` 은 이미 구운 PATH 를 물려받아 다시 굽는다.
    assert_eq!(join_path(&once, fallback), once);
    assert_eq!(join_path("", fallback), fallback);
    assert_eq!(join_path("/a::/b:", "/b"), "/a:/b");
}

const EXEC: &str = "/Users/u/.local/share/rocky/main/rockyd";

fn values(path: &str) -> PlistValues {
    PlistValues {
        exec_path: Some(EXEC.into()),
        log_path: Some("/Users/u/.config/rocky/todo/daemon.log".into()),
        path: Some(path.into()),
    }
}

/// 재시작은 구워 둔 PATH 를 그대로 다시 쓴다 — 부른 쪽의 PATH 로 다시 구우면 재시작마다 plist 가 바뀐다.
#[test]
fn the_baked_path_reads_back_what_the_plist_holds() {
    let path = "/Users/u/Tools & Scripts/bin:/usr/bin";
    assert_eq!(
        baked_path(&plist_content(&values(path))).as_deref(),
        Some(path)
    );
    // `&lt;` 라는 글자가 든 경로는 `<` 로 두 번 풀리지 않는다.
    assert_eq!(
        baked_path(&plist_content(&values("/a&lt;b"))).as_deref(),
        Some("/a&lt;b")
    );
    assert_eq!(baked_path("<plist/>"), None);
}

fn register(why: &str) -> JobRestart {
    JobRestart::Register(why.to_string())
}

#[test]
fn only_an_unchanged_plist_with_the_same_loaded_program_restarts_with_kickstart() {
    let desired = plist_content(&values("/usr/bin"));
    let job = parse_job(&format!("state = running\nprogram = {EXEC}\npid = 7"));
    assert_eq!(
        plan_restart(Some(&desired), &desired, Some(&job), EXEC),
        JobRestart::Kickstart
    );
    // plist 가 다르거나(실행 파일 경로가 바뀐 릴리스) 없다.
    let changed = register("plist 가 지금 구울 내용과 다르다");
    assert_eq!(
        plan_restart(Some("old"), &desired, Some(&job), EXEC),
        changed
    );
    assert_eq!(plan_restart(None, &desired, Some(&job), EXEC), changed);
    // job 이 로드되지 않았다(plist 만 남음) — kickstart 할 job 이 없다.
    assert_eq!(
        plan_restart(Some(&desired), &desired, None, EXEC),
        register("job 이 로드돼 있지 않다")
    );
    // launchd 가 쥔 정의가 다르다(plist 를 쓰고 등록하기 전에 끊긴 교체) — kickstart 는 옛 정의로 띄운다.
    let stale = parse_job("state = running\nprogram = /Users/u/.local/share/rocky/v0.51.0/rockyd");
    assert_eq!(
        plan_restart(Some(&desired), &desired, Some(&stale), EXEC),
        register("launchd 가 쥔 실행 파일이 다르다(/Users/u/.local/share/rocky/v0.51.0/rockyd)")
    );
    // `program =` 을 못 읽으면 다시 등록한다(예전과 같은 길).
    assert_eq!(
        plan_restart(
            Some(&desired),
            &desired,
            Some(&parse_job("state = running")),
            EXEC
        ),
        register("launchd 가 쥔 실행 파일을 읽지 못했다")
    );
}

/// plist 의 수정 시각을 옛날로 돌려 둔다 — 재시작이 파일을 다시 쓰면 바뀐다.
fn age(plist: &Path) -> SystemTime {
    let old = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
    std::fs::File::options()
        .write(true)
        .open(plist)
        .unwrap()
        .set_modified(old)
        .unwrap();
    old
}

fn modified(plist: &Path) -> SystemTime {
    std::fs::metadata(plist).unwrap().modified().unwrap()
}

/// 보안 솔루션(EDR)이 악성코드로 본 흐름 — 재시작마다 plist 를 쓰고 bootout/bootstrap — 을 타지 않는다.
#[test]
fn an_unchanged_restart_kickstarts_without_touching_the_plist() {
    let dir = tempfile::tempdir().unwrap();
    let plist = dir.path().join("com.rocky.daemon.plist");
    let desired = plist_content(&values("/usr/bin"));
    std::fs::write(&plist, &desired).unwrap();
    let old = age(&plist);
    let fake = Fake::serving(EXEC);
    let done = restart_job(
        &|a| fake.run(a),
        DOMAIN,
        &plist,
        Some(&desired),
        &desired,
        EXEC,
        Duration::ZERO,
    )
    .unwrap();
    assert_eq!(done, JobRestart::Kickstart);
    assert_eq!(
        *fake.calls.borrow(),
        vec![
            format!("print {DOMAIN}/{LAUNCHD_LABEL}"),
            format!("kickstart -k {DOMAIN}/{LAUNCHD_LABEL}"),
        ]
    );
    assert_eq!(modified(&plist), old);
}

#[test]
fn a_changed_plist_is_written_and_the_job_registered_again() {
    let dir = tempfile::tempdir().unwrap();
    let plist = dir.path().join("com.rocky.daemon.plist");
    std::fs::write(&plist, "old plist").unwrap();
    let desired = plist_content(&values("/usr/bin"));
    let fake = Fake::serving("/Users/u/.local/share/rocky/v0.51.0/rockyd");
    let done = restart_job(
        &|a| fake.run(a),
        DOMAIN,
        &plist,
        Some("old plist"),
        &desired,
        EXEC,
        Duration::ZERO,
    )
    .unwrap();
    assert_eq!(done, register("plist 가 지금 구울 내용과 다르다"));
    assert_eq!(std::fs::read_to_string(&plist).unwrap(), desired);
    assert_eq!(fake.count("kickstart"), 0);
    assert_eq!(fake.count("bootout"), 1);
    assert_eq!(fake.count("bootstrap"), 1);
}

/// kickstart 가 실패하면 예전 길(다시 등록)로 간다 — plist 는 그대로라 쓰지 않는다.
#[test]
fn a_failed_kickstart_falls_back_to_registering_again() {
    let dir = tempfile::tempdir().unwrap();
    let plist = dir.path().join("com.rocky.daemon.plist");
    let desired = plist_content(&values("/usr/bin"));
    std::fs::write(&plist, &desired).unwrap();
    let old = age(&plist);
    let fake = Fake::serving(EXEC);
    *fake.kickstart_ok.borrow_mut() = false;
    let done = restart_job(
        &|a| fake.run(a),
        DOMAIN,
        &plist,
        Some(&desired),
        &desired,
        EXEC,
        Duration::ZERO,
    )
    .unwrap();
    assert_eq!(
        done,
        register("kickstart -k 가 실패했다 — Operation not permitted")
    );
    assert_eq!(fake.count("bootstrap"), 1);
    assert_eq!(modified(&plist), old);
}

#[test]
fn the_plist_points_at_the_current_link_only_for_its_own_release() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for folder in ["v0.52.0", "v0.53.0", "main", "nightly"] {
        std::fs::create_dir(root.join(folder)).unwrap();
        std::fs::write(root.join(folder).join("rockyd"), "").unwrap();
    }
    let link = root.join("current");
    let relink = |target: &str| {
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink(target, &link).unwrap();
    };
    // 링크가 이 설치본을 가리키면 링크를 거친다 — 릴리스가 바뀌어도 plist 가 그대로다.
    relink("v0.53.0");
    assert_eq!(
        exec_through_link(&root.join("v0.53.0/rockyd"), &link),
        link.join("rockyd")
    );
    // 다른 설치본(번갈아 세션을 여는 옛 설치본)이면 자기 옆 그대로 — 링크로 띄우면 다른 버전이 뜬다.
    assert_eq!(
        exec_through_link(&root.join("v0.52.0/rockyd"), &link),
        root.join("v0.52.0/rockyd")
    );
    // main 빌드는 링크를 거치지 않는다 — `main/` 은 이미 바뀌지 않는 경로다.
    relink("main");
    assert_eq!(
        exec_through_link(&root.join("main/rockyd"), &link),
        root.join("main/rockyd")
    );
    // nightly 도 같다 — `nightly/` 는 받을 때마다 같은 폴더다.
    relink("nightly");
    assert_eq!(
        exec_through_link(&root.join("nightly/rockyd"), &link),
        root.join("nightly/rockyd")
    );
    // 링크가 없으면(개발 빌드·ROCKY_BIN) 자기 옆.
    std::fs::remove_file(&link).unwrap();
    assert_eq!(
        exec_through_link(&root.join("v0.53.0/rockyd"), &link),
        root.join("v0.53.0/rockyd")
    );
}
