//! `claude rc` 서버 현황 프로브 — `ps` 한 번, `lsof` 한 번, `claude auth status`, `agy`. 판정은 `rocky_core::rc`.
//!
//! 이 조각은 보기만 한다(띄우기·내리기 없음). 짧은 명령뿐이라 기존 러너(timeout + kill_on_drop)로 충분하다.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rocky_core::config::RcConfig;
use rocky_core::rc::{self, LiveServer, RcStatus};

use crate::runner::{BoxFut, Runner};

const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
/// 화면 폴링이 `ps`·`lsof` 를 매번 부르지 않게.
pub const RC_CACHE_TTL: Duration = Duration::from_secs(5);

pub type RcProvider = Arc<dyn Fn() -> BoxFut<RcStatus> + Send + Sync>;

fn argv(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

/// 한 번 잰다. 설정이 없으면 아무것도 돌리지 않는다.
pub async fn probe(runner: &Runner, config: Option<&RcConfig>, home: &str) -> RcStatus {
    let Some(config) = config else {
        return RcStatus::unconfigured();
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
        runner(
            argv(&["agy", "remote-control", "status"]),
            String::new(),
            PROBE_TIMEOUT,
        ),
    );
    let mut probe_error = (!ps.ok()).then(|| format!("ps 실패: {}", ps.stderr.trim()));
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
            probe_error.get_or_insert_with(|| format!("lsof 실패: {}", lsof.stderr.trim()));
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
    // 실행 파일이 없으면 spawn 이 실패해 출력이 비고 실패 코드다 — 설치 안 됨으로 본다.
    // (설치돼 있어도 timeout 이면 같은 모양이라 구분하지 못한다.)
    let antigravity =
        (agy.ok() || !agy.stdout.is_empty()).then(|| rc::parse_agy_status(&agy.stdout));

    RcStatus {
        configured: true,
        servers,
        strays,
        auth: rc::parse_auth_status(&auth.stdout),
        antigravity,
        probe_error,
    }
}

/// TTL 캐시를 씌운 조회기.
pub fn cached_rc(
    runner: Runner,
    config: Option<RcConfig>,
    home: String,
    ttl: Duration,
) -> RcProvider {
    let cache: Arc<Mutex<Option<(Instant, RcStatus)>>> = Arc::new(Mutex::new(None));
    let config = Arc::new(config);
    let home = Arc::new(home);
    Arc::new(move || {
        let (runner, config, home, cache) =
            (runner.clone(), config.clone(), home.clone(), cache.clone());
        Box::pin(async move {
            if let Ok(slot) = cache.lock() {
                if let Some((at, status)) = slot.as_ref() {
                    if at.elapsed() < ttl {
                        return status.clone();
                    }
                }
            }
            let status = probe(&runner, config.as_ref().as_ref(), &home).await;
            if let Ok(mut slot) = cache.lock() {
                *slot = Some((Instant::now(), status.clone()));
            }
            status
        })
    })
}
