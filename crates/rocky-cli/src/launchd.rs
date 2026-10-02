//! launchd 상주 등록 — `rocky daemon install` 이 쓰는 macOS 전용 헬퍼.
//! TS 원본 `src/launchd.ts`.
//!
//! KeepAlive 로 데몬을 로그인 세션 동안 상시 유지한다. 미설치 상태여도 CLI 의
//! 온디맨드 자동 기동은 그대로 동작하므로 install 은 선택 사항이다.

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use rocky_core::config::{expand_tilde, launchd_label};

use crate::client::daemon_binary;

/// launchd job 라벨 — 기본값. 실제로 쓰는 값은 `launchd_label()`(개발용 `ROCKY_LAUNCHD_LABEL`).
pub use rocky_core::config::LAUNCHD_LABEL;

fn plist_path() -> PathBuf {
    expand_tilde(&format!("~/Library/LaunchAgents/{}.plist", launchd_label()))
}

/// plist 로그가 놓이는 기본 디렉터리 — TS 와 같이 **설정된 dir 이 아니라 기본 경로**를
/// 쓴다. plist 는 설치 시점에 구워지는 정적 파일이라 이후 설정 변경을 따라갈 수 없고,
/// 로그 위치가 설정 따라 흔들리는 것보다 한 자리에 고정되는 쪽이 찾기 쉽다.
fn default_todo_dir() -> PathBuf {
    expand_tilde("~/.config/rocky/todo")
}

/// launchd(KeepAlive) 상주 job 이 등록돼 있나 — plist 존재 여부로 판별한다 (macOS 전용).
///
/// 등록돼 있으면 데몬은 launchd 가 관리하므로, 구버전을 교체할 때 PID 만 죽여선 안 된다
/// (KeepAlive 가 같은 plist 경로의 구버전을 즉시 되살린다). `install_launchd` 로 job
/// 자체를 현재 설치 경로로 교체해야 한다.
pub fn is_launchd_registered() -> bool {
    cfg!(target_os = "macos") && plist_path().is_file()
}

/// launchd 가 잡에 물려주는 PATH 는 최소치(`/usr/bin:/bin:/usr/sbin:/sbin`)라 Homebrew 등
/// 사용자 설치 위치가 빠진다. 데몬이 이름만으로 spawn 하는 외부 CLI 가 둘 있다:
/// `gh`(이슈 생성 — 최소 PATH 아래서 "gh CLI 를 찾을 수 없다"는 잘못된 메시지가 뜬다)와
/// `claude`(못 찾으면 핸드오프 기능 전체가 `available:false` 로 죽는다). SessionStart
/// 훅이 띄운 데몬은 셸 PATH 를 상속해 잘 도는데 `daemon install` 로 상주시킨 데몬만
/// 안 되는 상태를 만드는 함정이다.
const PLIST_PATH_FALLBACK: &str = "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin";

/// plist 에 구울 PATH — **설치 시점의 `PATH`** 를 우선한다. 지금 이 셸에서 `gh`/`claude`
/// 가 보이면 launchd 데몬도 보게 만드는 게 가장 정확하다. 뒤에 흔한 설치 위치를 이어
/// 붙여, PATH 가 비었거나 비표준 셸에서 설치한 경우도 받친다(중복 항목은 무해하다).
fn path_for_plist() -> String {
    match std::env::var("PATH") {
        Ok(inherited) if !inherited.is_empty() => format!("{inherited}:{PLIST_PATH_FALLBACK}"),
        _ => PLIST_PATH_FALLBACK.to_string(),
    }
}

/// plist 는 XML 이다 — 보간되는 값(PATH, 실행 파일 경로, 로그 경로)에 `&`/`<`/`>` 가
/// 섞이면(예: `/Users/x/Tools & Scripts/bin`) 파싱 불가한 plist 가 만들어진다.
/// `install_launchd` 는 이 plist 를 쓰기 전에 기존 job 을 먼저 내리므로, 깨진 plist 로
/// 로드가 실패하면 상주 데몬이 롤백 없이 사라진다 — 여기서 막아야 하는 이유다.
/// `"` 는 전부 텍스트 노드 안이라 이스케이프 대상에서 뺐다.
fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// launchd 데몬의 로그 파일 — 개발용 라벨이면 따로(실제 상주 데몬의 daemon.log 에 섞이지 않게).
fn log_path() -> String {
    let label = launchd_label();
    let file = if label == LAUNCHD_LABEL {
        "daemon.log".to_string()
    } else {
        format!("{label}.log")
    };
    default_todo_dir().join(file).to_string_lossy().to_string()
}

/// plist 보간값 — 기본은 실제 install 시점 값. 테스트에서 특수문자 이스케이프를
/// 검증할 수 있도록 override 가능한 seam 을 열어뒀다.
#[derive(Debug, Clone, Default)]
pub struct PlistValues {
    pub exec_path: Option<String>,
    pub log_path: Option<String>,
    pub path: Option<String>,
}

/// plist 본문 — install 시점에 캡처한 PATH 를 EnvironmentVariables 로 굽는다.
///
/// TS 판과 달리 ProgramArguments 가 `bun run daemon.ts` 가 아니라 `rockyd`
/// 바이너리 하나다. WorkingDirectory 고정도 없다 — 그건 bunfig.toml(Tailwind serve
/// 플러그인)이 시작 cwd 에서 읽히던 TS 시절의 제약이고, Rust 데몬은 미리 번들된
/// dist 를 서빙한다.
pub fn plist_content(overrides: &PlistValues) -> String {
    let exec_path = overrides
        .exec_path
        .clone()
        .unwrap_or_else(|| daemon_binary().to_string_lossy().to_string());
    let log_path = overrides.log_path.clone().unwrap_or_else(log_path);
    let path = overrides.path.clone().unwrap_or_else(path_for_plist);
    let label = launchd_label();
    // 개발용 라벨로 등록할 때만 그 라벨과 설정 파일을 데몬에 물려준다 — 데몬이 자기 job 을 알아보고
    // (`launched_by_launchd`) 실제 상주 데몬과 다른 포트·dir 로 뜨게. 평소 plist 에는 넣지 않는다.
    let mut dev_env = String::new();
    if label != LAUNCHD_LABEL {
        for key in ["ROCKY_LAUNCHD_LABEL", "ROCKY_CONFIG"] {
            if let Ok(value) = std::env::var(key) {
                dev_env.push_str(&format!(
                    "\n    <key>{key}</key><string>{}</string>",
                    escape_xml(&value)
                ));
            }
        }
    }
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{label}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{exec}</string>
  </array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>EnvironmentVariables</key>
  <dict>
    <key>PATH</key><string>{path}</string>{dev_env}
  </dict>
  <key>StandardOutPath</key><string>{log}</string>
  <key>StandardErrorPath</key><string>{log}</string>
</dict>
</plist>
"#,
        label = escape_xml(&label),
        exec = escape_xml(&exec_path),
        path = escape_xml(&path),
        log = escape_xml(&log_path),
    )
}

/// `launchctl` 실행기 — `register_job` 의 주입점. 테스트는 가짜로 bootout/bootstrap/print 의
/// 응답 순서를 꾸민다.
pub type Launchctl<'a> = &'a dyn Fn(&[&str]) -> (bool, String);

fn launchctl(args: &[&str]) -> (bool, String) {
    let output = Command::new("launchctl").args(args).output();
    match output {
        Ok(out) => {
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            (out.status.success(), text.trim().to_string())
        }
        Err(error) => (false, error.to_string()),
    }
}

fn gui_domain() -> String {
    // SAFETY: getuid 는 인자 없는 순수 조회다 — 실패 경로가 없다.
    let uid = unsafe { libc::getuid() };
    format!("gui/{uid}")
}

/// bootout 뒤 서비스가 도메인에서 사라지길 기다리는 횟수 / bootstrap 재시도 횟수.
/// 실제 사고: 0.27→0.28 업그레이드에서 bootout 직후의 bootstrap 이 실패했는데 훅이 그 결과를
/// 버려, plist 는 새 경로인데 launchd 에 서비스가 없고 데몬도 없는 상태로 남았다.
const SETTLE_ATTEMPTS: u32 = 10;
const BOOTSTRAP_ATTEMPTS: u32 = 5;

/// job 교체의 본체 — bootout → 서비스가 내려갈 때까지 대기 → bootstrap(재시도) → 로드 확인.
///
/// `launchctl bootout` 은 비동기다: 옛 서비스가 아직 도메인에 남아 있는 동안 같은 라벨을
/// bootstrap 하면 `Bootstrap failed: 5: Input/output error` / `36: Operation now in progress`
/// 로 튄다. 그래서 `print` 가 실패(=서비스 없음)할 때까지 잠깐 기다리고, 그래도 실패하면
/// `pause` 간격으로 몇 번 더 시도한다. bootstrap 이 성공해도 `print` 로 실제 로드를 확인한
/// 뒤에야 성공이다 — 여기서 `Err` 면 호출자는 데몬이 **없어졌다**고 봐야 한다(bootout 은
/// 이미 됐다).
pub fn register_job(
    run: Launchctl,
    domain: &str,
    plist: &str,
    pause: Duration,
) -> Result<(), String> {
    let target = format!("{domain}/{}", launchd_label());
    let _ = run(&["bootout", domain, plist]);
    for _ in 0..SETTLE_ATTEMPTS {
        if !run(&["print", &target]).0 {
            break;
        }
        std::thread::sleep(pause);
    }
    let mut last_error = String::new();
    for attempt in 1..=BOOTSTRAP_ATTEMPTS {
        let (ok, out) = run(&["bootstrap", domain, plist]);
        if ok {
            for _ in 0..SETTLE_ATTEMPTS {
                if run(&["print", &target]).0 {
                    return Ok(());
                }
                std::thread::sleep(pause);
            }
            return Err(format!(
                "launchctl bootstrap 은 성공했으나 {target} 이 로드되지 않았다"
            ));
        }
        last_error = out;
        if attempt < BOOTSTRAP_ATTEMPTS {
            std::thread::sleep(pause);
        }
    }
    Err(format!(
        "launchctl bootstrap {target} 이 {BOOTSTRAP_ATTEMPTS}회 실패했다: {last_error}"
    ))
}

/// `daemon install` — plist 를 굽고 launchd job 을 (재)등록한다. 멱등.
///
/// # Errors
/// plist 를 못 쓰거나 `register_job` 이 실패하면 사유. **실패했으면 옛 job 은 이미 내려간
/// 뒤일 수 있다** — 호출자(훅)는 데몬 유무를 다시 확인해 launchd 밖에서라도 띄운다.
pub fn install_launchd() -> Result<String, String> {
    let plist = plist_path();
    if let Some(parent) = plist.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::create_dir_all(default_todo_dir());
    std::fs::write(&plist, plist_content(&PlistValues::default()))
        .map_err(|error| format!("launchd 등록 실패: plist 를 쓰지 못했다 — {error}"))?;
    let plist_str = plist.to_string_lossy().to_string();
    register_job(
        &launchctl,
        &gui_domain(),
        &plist_str,
        Duration::from_millis(200),
    )
    .map_err(|error| format!("launchd 등록 실패: {error}\nplist: {plist_str}"))?;
    Ok(format!(
        "✓ launchd 등록 완료 ({}) — 로그인 시 자동 기동 + KeepAlive\n  plist: {plist_str}",
        launchd_label()
    ))
}

/// plist 가 있고 **실제로 launchd 에 로드돼** 있나. `is_launchd_registered` 는 파일만 본다 —
/// 둘이 갈리는 상태(plist 만 남음)가 "재부팅 뒤 데몬이 안 뜨는" 상태다.
pub fn launchd_loaded() -> bool {
    if !is_launchd_registered() {
        return false;
    }
    let target = job_target();
    launchctl(&["print", &target]).0
}

/// `daemon uninstall` — job 을 내리고 plist 를 지운다.
pub fn uninstall_launchd() -> String {
    let plist = plist_path();
    let plist_str = plist.to_string_lossy().to_string();
    let (ok, _) = launchctl(&["bootout", &gui_domain(), &plist_str]);
    if plist.exists() {
        let _ = std::fs::remove_file(&plist);
    }
    if ok {
        format!("✓ launchd 해제 완료 ({})", launchd_label())
    } else {
        "launchd 해제: 등록되어 있지 않았다 (plist 는 정리됨)".to_string()
    }
}

fn job_target() -> String {
    format!("{}/{}", gui_domain(), launchd_label())
}

/// `launchctl print` 에서 읽은 job 의 지금 상태 — `state = running` 이면 `pid = N` 이 같이 나온다.
/// `spawn scheduled` 는 프로세스가 끝나 KeepAlive 가 다시 띄우려고 기다리는 중이다(pid 없음).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchdJob {
    pub state: String,
    pub pid: Option<u32>,
}

/// `launchctl print gui/<uid>/<label>` 출력 → `LaunchdJob`. 서비스 블록의 첫 `state =`·`pid =` 만 본다
/// (뒤쪽 하위 블록에도 같은 이름의 줄이 있다).
pub fn parse_job(print_output: &str) -> LaunchdJob {
    let field = |name: &str| {
        print_output
            .lines()
            .find_map(|line| line.trim().strip_prefix(name).map(str::trim))
            .map(str::to_string)
    };
    LaunchdJob {
        state: field("state = ").unwrap_or_else(|| "unknown".into()),
        pid: field("pid = ").and_then(|p| p.parse().ok()),
    }
}

/// 로드된 job 의 지금 상태. plist 가 없거나 로드되지 않았으면 `None`.
pub fn launchd_job() -> Option<LaunchdJob> {
    if !is_launchd_registered() {
        return None;
    }
    let (ok, out) = launchctl(&["print", &job_target()]);
    ok.then(|| parse_job(&out))
}

/// launchd 에게 job 을 지금 띄우라고 한다(이미 돌면 아무 일 없음, KeepAlive 대기 중이면 바로 띄운다).
/// 데몬이 없을 때 CLI 가 launchd 밖에서 따로 띄우지 않게 하는 길이다 — 따로 띄운 것은 포트를 쥔 채
/// launchd 의 다음 데몬을 "already running" 으로 계속 돌려보낸다.
pub fn kickstart() -> bool {
    launchctl(&["kickstart", &job_target()]).0
}

/// 포트를 쥔 데몬이 launchd job 의 프로세스가 아니면 경고 — 그 데몬은 업데이트·재시작이 내려 주지
/// 못하고(bootout 은 launchd 자기 프로세스만 내린다), launchd 의 새 데몬은 포트 충돌로 바로 끝난다.
/// `health_pid` 는 포트에서 응답한 데몬의 pid, `job` 은 로드된 launchd job(없으면 판정하지 않는다).
pub fn ownership_warning(health_pid: Option<u32>, job: Option<&LaunchdJob>) -> Option<String> {
    let job = job?;
    let holder = health_pid?;
    if job.pid == Some(holder) {
        return None;
    }
    let launchd = match job.pid {
        Some(pid) => format!("launchd 의 데몬은 pid {pid}"),
        None => format!("launchd job 은 state={}", job.state),
    };
    Some(format!(
        "⚠ 포트를 쥔 데몬(pid {holder})은 launchd 가 띄운 것이 아니다 — {launchd}. 업데이트가 이 데몬을 바꾸지 못한다 → rocky daemon restart"
    ))
}

/// 포트에 데몬이 없는데 launchd job 도 돌지 않는다(`spawn scheduled` — 데몬이 뜨자마자 죽어 KeepAlive 가
/// 다시 띄우려는 중 등) — 로그를 보라는 경고. 돌고 있으면(막 뜨는 중) 판정하지 않는다.
pub fn stalled_job_warning(job: &LaunchdJob) -> Option<String> {
    if job.state == "running" {
        return None;
    }
    Some(format!(
        "⚠ launchd job 이 state={} 인데 포트에 데몬이 없다 — 데몬이 뜨자마자 끝나고 있을 수 있다. 로그 {} 를 보고 rocky daemon restart",
        job.state,
        log_path()
    ))
}

/// `daemon status` 한 줄 — 미등록 / plist 만 존재 / 로드됨(state 포함)을 가른다.
pub fn launchd_status() -> String {
    let plist = plist_path();
    if !plist.is_file() {
        return "launchd: 미등록 (온디맨드 자동 기동만 사용중)".to_string();
    }
    match launchd_job() {
        None => format!(
            "launchd: plist 는 있으나 로드되지 않음 ({}) — 재부팅·크래시 뒤 데몬이 살아나지 않는다 → rocky daemon install 로 다시 등록",
            plist.display()
        ),
        Some(job) => format!(
            "launchd: 등록됨, state={}{}",
            job.state,
            job.pid.map(|p| format!(", pid {p}")).unwrap_or_default()
        ),
    }
}
