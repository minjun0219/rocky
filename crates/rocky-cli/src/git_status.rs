//! `rocky statusline --full` 의 git 세그먼트 재료 — `git status --porcelain=v2` 를 한 번 부른다. 파싱은
//! `rocky_core::statusline::git`, 여기는 실행만 한다.
//!
//! statusline 은 1초마다 도는 자리라 마감을 넘기면 세그먼트만 빼고 줄은 그대로 낸다(cc-usage 불변 조건 1).
//! 마감을 넘기면 **프로세스 그룹째** 끊는다 — git 만 죽이면 git 이 띄운 자식(훅·fsmonitor 등)이 고아로 남아
//! 1초마다 쌓인다. 끊은 뒤에는 파이프를 기다리지 않는다(남은 자식이 파이프를 쥐고 있어도 매달리지 않게).

use std::io::Read;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use rocky_core::statusline::git::GitStatus;

/// cc-usage 와 같은 마감.
pub const GIT_TIMEOUT: Duration = Duration::from_millis(500);

/// `dir` 의 브랜치·변경 수. git repo 가 아니거나 실패하거나 마감을 넘기면 `None`. untracked 는 스캔 비용 때문에 세지
/// 않는다(`--untracked-files=no`), 인덱스 잠금을 잡지 않는다(`--no-optional-locks`).
pub fn read(dir: &str, timeout: Duration) -> Option<GitStatus> {
    if dir.is_empty() {
        return None;
    }
    let mut child = Command::new("git")
        .args(["-C", dir, "--no-optional-locks", "status"])
        .args(["--porcelain=v2", "--branch", "--untracked-files=no"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .ok()?;
    // 출력이 파이프 버퍼를 넘치면 자식이 write 에서 멈춘다 — 기다리는 동안 따로 읽는다.
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut out = String::new();
        stdout.read_to_string(&mut out).map(|_| out)
    });
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let out = reader.join().ok()?.ok()?;
                return status.success().then(|| GitStatus::parse(&out));
            }
            Ok(None) if Instant::now() >= deadline => {
                // SAFETY: kill(2) 에 음수 pid 를 주면 그 프로세스 그룹 전체에 보낸다 — process_group(0) 으로 git 이
                // 자기 그룹의 리더라 그룹 id 가 곧 git 의 pid 다.
                unsafe {
                    libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
                }
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(2)),
            Err(_) => return None,
        }
    }
}
