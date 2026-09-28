//! launchd job 교체 — bootout 직후 bootstrap 이 튀는 레이스를 재시도로 넘기는지.

use std::cell::RefCell;
use std::time::Duration;

use rocky_cli::launchd::{register_job, LAUNCHD_LABEL};

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
}

impl Fake {
    fn new(loaded: bool, bootstrap_failures: u32, linger: u32) -> Self {
        Fake {
            calls: RefCell::new(Vec::new()),
            loaded: RefCell::new(loaded),
            bootstrap_failures: RefCell::new(bootstrap_failures),
            linger: RefCell::new(linger),
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
                    return (true, "state = running".into());
                }
                if *self.loaded.borrow() {
                    (true, "state = running".into())
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
