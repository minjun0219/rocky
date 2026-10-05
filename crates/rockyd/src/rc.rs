//! `claude rc` 서버 현황 프로브 — `ps` 한 번, `lsof` 한 번, `claude auth status`, `agy`. 판정은 `rocky_core::rc`.
//!
//! `claude rc` 는 아직 보기만 한다. `agy remote-control` 은 켜고 끌 수 있다(`agy_control`) — agy 가 데몬을 launchd
//! 잡으로 올리고 명령 자체는 곧 끝나므로 기존 러너(timeout + kill_on_drop)로 충분하다.

use std::sync::Arc;
use std::time::{Duration, Instant};

use rocky_core::config::RcConfig;
use rocky_core::rc::{self, AgyAction, AgyStatus, LiveServer, RcStatus};

use crate::runner::{BoxFut, CmdOutput, Runner};

const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
/// 화면 폴링이 `ps`·`lsof` 를 매번 부르지 않게.
pub const RC_CACHE_TTL: Duration = Duration::from_secs(5);

/// `agy remote-control` 켜기·끄기 명령 한도 — launchd 등록이 끼어 상태 조회보다 오래 걸린다.
const AGY_CONTROL_TIMEOUT: Duration = Duration::from_secs(60);

pub type RcProvider = Arc<dyn Fn() -> BoxFut<RcStatus> + Send + Sync>;
/// agy 를 켜고 끈 뒤 새로 잰 현황. 명령이 실패하면 그 사유.
pub type AgyControl = Arc<dyn Fn(AgyAction) -> BoxFut<Result<RcStatus, String>> + Send + Sync>;

/// 프로브 명령 실패를 진단할 수 있게 — 무엇을 물었고(`what`) 어떻게 끝났나(종료 코드·stderr, 비었으면 그렇다고).
fn probe_failure(what: &str, out: &CmdOutput) -> String {
    let stderr = out.stderr.trim();
    let stderr = if stderr.is_empty() {
        "stderr 없음"
    } else {
        stderr
    };
    format!("{what} 실패(종료 코드 {}): {stderr}", out.code)
}

fn argv(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

/// `agy remote-control status` 결과 → 현황. 실행 파일이 없으면 spawn 이 실패해 출력이 비고 실패 코드다 — 설치 안 됨.
/// (설치돼 있어도 timeout 이면 같은 모양이라 구분하지 못한다.)
fn agy_status(out: &CmdOutput) -> Option<AgyStatus> {
    (out.ok() || !out.stdout.is_empty()).then(|| rc::parse_agy_status(&out.stdout))
}

fn agy_status_argv() -> Vec<String> {
    argv(&["agy", "remote-control", "status"])
}

/// 한 번 잰다. `rc` 블록이 없으면 `claude rc` 쪽은 돌리지 않고 agy 만 본다 — agy 줄은 설치 여부를 따른다.
pub async fn probe(runner: &Runner, config: Option<&RcConfig>, home: &str) -> RcStatus {
    let Some(config) = config else {
        let agy = runner(agy_status_argv(), String::new(), PROBE_TIMEOUT).await;
        return RcStatus {
            antigravity: agy_status(&agy),
            ..RcStatus::unconfigured()
        };
    };
    let targets = rc::resolve_targets(config, home);

    // 서로 기다릴 이유가 없는 셋은 같이 돌린다 — 하나가 timeout 까지 걸려도 요청이 그만큼만 늦다.
    let (ps, auth, agy) = tokio::join!(
        runner(
            argv(&["ps", "-axww", "-o", "pid=,ppid=,etime=,args="]),
            String::new(),
            PROBE_TIMEOUT,
        ),
        runner(
            argv(&["claude", "auth", "status", "--json"]),
            String::new(),
            PROBE_TIMEOUT,
        ),
        runner(agy_status_argv(), String::new(), PROBE_TIMEOUT),
    );
    let mut probe_error = (!ps.ok()).then(|| probe_failure("ps", &ps));
    let rows = rc::parse_ps(&ps.stdout);
    let servers = rc::servers(&rows);
    let live = if servers.is_empty() {
        Vec::new()
    } else {
        let pids = servers
            .iter()
            .map(|s| s.pid.to_string())
            .collect::<Vec<_>>()
            .join(",");
        // pid 마다 부르지 않는다 — 서버가 열 개면 lsof 열 번이다.
        let lsof = runner(
            argv(&["lsof", "-a", "-d", "cwd", "-p", &pids, "-F", "pn"]),
            String::new(),
            PROBE_TIMEOUT,
        )
        .await;
        // 그 사이 끝난 pid 가 하나라도 있으면 lsof 는 1 로 끝나되 나머지는 낸다 — 출력이 비었을 때만 실패다.
        if !lsof.ok() && lsof.stdout.trim().is_empty() {
            probe_error.get_or_insert_with(|| probe_failure(&format!("lsof -p {pids}"), &lsof));
        }
        let cwd = rc::parse_lsof_cwd(&lsof.stdout);
        servers
            .iter()
            .filter_map(|s| {
                Some(LiveServer {
                    pid: s.pid,
                    dir: cwd.get(&s.pid)?.clone(),
                    uptime_secs: s.uptime_secs,
                    sessions: rc::session_count(&rows, s.pid),
                })
            })
            .collect()
    };
    let (servers, strays) = rc::build_rows(&targets, &live);
    let antigravity = agy_status(&agy);

    RcStatus {
        configured: true,
        servers,
        strays,
        auth: rc::parse_auth_status(&auth.stdout),
        antigravity,
        probe_error,
    }
}

/// `agy remote-control start|stop` 을 한 번 돌린다. 실패면 무엇을 돌렸고 어떻게 끝났는지를 낸다.
pub async fn agy_control(runner: &Runner, action: AgyAction) -> Result<(), String> {
    let argv = action.argv();
    let out = runner(argv.clone(), String::new(), AGY_CONTROL_TIMEOUT).await;
    if out.ok() {
        Ok(())
    } else {
        Err(probe_failure(&argv.join(" "), &out))
    }
}

/// TTL 캐시를 씌운 조회기. 잠금을 프로브가 끝날 때까지 쥔다 — 만료 직후 겹친 요청은 진행 중인 프로브를
/// 기다렸다 그 결과를 받는다(요청마다 `ps`·`lsof` 를 새로 띄우지 않고, 늦게 끝난 옛 프로브가 새 결과를 덮지 않는다).
pub fn cached_rc(
    runner: Runner,
    config: Option<RcConfig>,
    home: String,
    ttl: Duration,
) -> RcProvider {
    rc_handles(runner, config, home, ttl).0
}

/// 조회기와 agy 손잡이를 **같은 캐시**로 묶는다. 손잡이는 캐시 잠금을 쥔 채 명령을 돌리고 다시 재서 넣는다 —
/// 그 사이 조회는 기다렸다 새 값을 받고, 켜기 직전의 옛 값이 5초 동안 남지 않는다.
pub fn rc_handles(
    runner: Runner,
    config: Option<RcConfig>,
    home: String,
    ttl: Duration,
) -> (RcProvider, AgyControl) {
    let cache: Arc<tokio::sync::Mutex<Option<(Instant, RcStatus)>>> =
        Arc::new(tokio::sync::Mutex::new(None));
    let config = Arc::new(config);
    let home = Arc::new(home);
    let provider: RcProvider = {
        let (runner, config, home, cache) =
            (runner.clone(), config.clone(), home.clone(), cache.clone());
        Arc::new(move || {
            let (runner, config, home, cache) =
                (runner.clone(), config.clone(), home.clone(), cache.clone());
            Box::pin(async move {
                let mut slot = cache.lock().await;
                if let Some((at, status)) = slot.as_ref() {
                    if at.elapsed() < ttl {
                        return status.clone();
                    }
                }
                let status = probe(&runner, config.as_ref().as_ref(), &home).await;
                *slot = Some((Instant::now(), status.clone()));
                status
            })
        })
    };
    let control: AgyControl = Arc::new(move |action| {
        let (runner, config, home, cache) =
            (runner.clone(), config.clone(), home.clone(), cache.clone());
        Box::pin(async move {
            let mut slot = cache.lock().await;
            let done = agy_control(&runner, action).await;
            // 실패해도 다시 잰다 — 반쯤 바뀐 상태(등록만 됨 등)가 화면에 남지 않게.
            let status = probe(&runner, config.as_ref().as_ref(), &home).await;
            *slot = Some((Instant::now(), status.clone()));
            done.map(|()| status)
        })
    });
    (provider, control)
}
