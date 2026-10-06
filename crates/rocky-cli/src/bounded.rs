//! 마감 안에서만 도는 하위 프로세스 — `rocky statusline --full` 의 git 과 `extraCommands` 가 쓴다.
//!
//! statusline 은 1초마다 도는 자리라 하위 프로세스가 늦으면 그 세그먼트만 빼고 줄은 그대로 낸다(cc-usage 불변 조건 1).
//! 끊을 때는 **프로세스 그룹째** 끊는다 — 본 프로세스만 죽이면 그것이 띄운 자식이 고아로 남아 1초마다 쌓이고, 자식이
//! stdout 을 물려받았으면 파이프도 닫히지 않는다. 그래서 끊은 뒤에는 파이프를 기다리지 않는다.

use std::io::Read;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::{mpsc, Mutex};
use std::time::{Duration, Instant};

/// 본 프로세스가 끝난 뒤 파이프(stdout·stderr)가 닫히기를 기다리는 시간 — 백그라운드로 띄운 자식이 어느 쪽이든 붙잡고
/// 있으면 그 출력은 버린다(cc-usage 의 `WaitDelay` 와 같은 값·같은 범위).
const PIPE_GRACE: Duration = Duration::from_millis(100);

/// spawn 을 한 번에 하나씩 — macOS 에는 `pipe2` 가 없어 파이프를 만든 뒤 close-on-exec 를 따로 거는데, 그 사이에 다른
/// 스레드가 spawn 하면 이 파이프의 쓰기 끝이 남의 자식에게 상속돼 EOF 가 오지 않는다(Go 의 `syscall.ForkLock` 자리).
/// spawn 만 잠그므로 실행은 여전히 나란히 돈다.
static SPAWN: Mutex<()> = Mutex::new(());

/// spawn 잠금 — 이 프로세스에서 자식을 띄우는 곳(`run`, detached 갱신)은 모두 이것을 잡고 띄운다.
pub(crate) fn spawn_lock() -> std::sync::MutexGuard<'static, ()> {
    SPAWN.lock().unwrap_or_else(|e| e.into_inner())
}

/// `argv` 를 셸 없이 돌려 stdout 을 바이트 그대로 돌려준다. 실행 파일이 없거나, 0 이 아닌 코드로 끝났거나, 마감을
/// 넘겼거나, 끝난 뒤에도 자식이 파이프를 붙잡고 있으면 `None` — 어느 쪽이든 statusline 에는 아무것도 붙지 않는다.
/// stderr 는 읽어서 버린다(닫히는지만 본다).
pub fn run(argv: &[String], timeout: Duration) -> Option<Vec<u8>> {
    let (program, args) = argv.split_first()?;
    let mut child = {
        let _guard = spawn_lock();
        Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .spawn()
            .ok()?
    };
    // 출력이 파이프 버퍼를 넘치면 자식이 write 에서 멈춘다 — 기다리는 동안 따로 읽는다.
    let (tx, rx) = mpsc::channel();
    for (is_stdout, mut pipe) in [
        child
            .stdout
            .take()
            .map(|p| (true, Box::new(p) as Box<dyn Read + Send>)),
        child
            .stderr
            .take()
            .map(|p| (false, Box::new(p) as Box<dyn Read + Send>)),
    ]
    .into_iter()
    .flatten()
    {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = tx.send((is_stdout, pipe.read_to_end(&mut buf).map(|_| buf)));
        });
    }
    drop(tx);

    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let out = drain(&rx, Instant::now() + PIPE_GRACE);
                if out.is_none() {
                    kill_group(child.id());
                }
                return out.filter(|_| status.success());
            }
            Ok(None) if Instant::now() >= deadline => {
                kill_group(child.id());
                // 그룹을 떠난 프로세스면 그룹 kill 이 빗나간다 — 본 프로세스는 따로 끊어 wait 가 막히지 않게 한다.
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(2)),
            Err(_) => return None,
        }
    }
}

/// 두 파이프가 `until` 안에 모두 닫히면 stdout 을, 아니면(읽기 실패 포함) `None`.
fn drain(rx: &mpsc::Receiver<(bool, std::io::Result<Vec<u8>>)>, until: Instant) -> Option<Vec<u8>> {
    let mut stdout = None;
    for _ in 0..2 {
        let left = until.saturating_duration_since(Instant::now());
        let (is_stdout, read) = rx.recv_timeout(left).ok()?;
        let bytes = read.ok()?;
        if is_stdout {
            stdout = Some(bytes);
        }
    }
    stdout
}

fn kill_group(pid: u32) {
    // SAFETY: kill(2) 에 음수 pid 를 주면 그 프로세스 그룹 전체에 보낸다 — process_group(0) 으로 띄운 프로세스가 자기
    // 그룹의 리더라 그룹 id 가 곧 그 pid 다.
    unsafe {
        libc::kill(-(pid as libc::pid_t), libc::SIGKILL);
    }
}
