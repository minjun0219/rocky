//! `claude rc`(Remote Control) 서버 현황 — 순수 판정. 프로브(`ps`·`lsof`·`pgrep`)는 `rockyd::rc` 가 돌리고
//! 여기는 그 출력을 읽어 합친다. 설계: `docs/design/specs/2026-10-05-rc-server-design.md`.
//!
//! 판정 규칙은 이 기능을 먼저 하던 별도 CLI 와 같다 — 그쪽에서 밟고 고친 함정을 테스트로 고정한다.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Duration;

use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use serde::Serialize;

use crate::config::RcConfig;

/// 기본 작업 폴더 — 상대 이름은 이 아래로 푼다(`rc.root` 로 바꾼다).
pub const DEFAULT_ROOT: &str = "~/dev/workspaces";

/// 설정의 이름 하나를 푼 결과. 라벨은 디렉터리 basename 이다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub label: String,
    pub dir: String,
    pub pinned: bool,
}

/// `ps -axww -o pid=,ppid=,etime=,args=` 한 줄.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PsRow {
    pub pid: u32,
    pub ppid: u32,
    /// 떠 있은 시간(초). etime 을 못 읽으면 None.
    pub uptime_secs: Option<u64>,
    pub args: String,
}

/// 떠 있는 서버 하나 — 프로브가 모은 사실.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveServer {
    pub pid: u32,
    pub dir: String,
    pub uptime_secs: Option<u64>,
    /// 열린 세션 수(자식 중 `--sdk-url …/code/session` 이 붙은 것).
    pub sessions: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthState {
    In,
    Out,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgyStatus {
    pub state: Option<String>,
    pub pid: Option<u32>,
    pub instance: Option<String>,
}

/// `agy remote-control` 손잡이 — `start`(등록 + 기동)와 `stop`(정지 + 등록 해제). 데몬은 agy 가 launchd 잡으로
/// 올리므로 rockyd 의 자식이 아니다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgyAction {
    Start,
    Stop,
}

impl AgyAction {
    /// 라우트·CLI 의 동작 이름. 모르는 이름은 None — 임의의 하위 명령을 agy 에 넘기지 않는다.
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "start" => Some(AgyAction::Start),
            "stop" => Some(AgyAction::Stop),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            AgyAction::Start => "start",
            AgyAction::Stop => "stop",
        }
    }

    /// 돌릴 명령. `--name`·`--session` 은 넘기지 않는다 — 기기 이름·등록 범위는 agy 쪽에서 정한 값을 그대로 쓴다.
    pub fn argv(self) -> Vec<String> {
        ["agy", "remote-control", self.as_str()]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    /// 이 동작 뒤의 상태가 자리 잡았나. 끈 직후의 `status` 는 launchd 의 중간값(`Daemon state = SIGTERMed`)을
    /// 내다가 잠시 뒤 `Daemon status: not running`(state 줄 없음)이 된다 — 실측 agy 1.2.14.
    pub fn settled(self, status: Option<&AgyStatus>) -> bool {
        let state = status.and_then(|s| s.state.as_deref());
        match self {
            AgyAction::Start => state == Some("running"),
            AgyAction::Stop => state.is_none(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerRow {
    pub label: String,
    pub dir: String,
    pub pinned: bool,
    pub running: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uptime_secs: Option<u64>,
    pub sessions: usize,
    /// 데몬이 이 대상에 지금 하고 있는 일 — 없으면 쉬는 중.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<RcAction>,
    /// 마지막으로 띄우거나 다시 띄운 결과(데몬이 다시 뜨면 사라진다).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_result: Option<RcResult>,
    /// 자격이 끊겼다 돌아오기 전에 뜬 서버 — 죽은 토큰을 들고 있을 수 있어 다시 띄우기를 권한다(감시가 켜져 있을 때만 잰다).
    #[serde(skip_serializing_if = "is_false")]
    pub auth_suspect: bool,
}

/// 진행 중인 일.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RcAction {
    Starting,
    Restarting,
    /// `already served` 를 만나 쉬었다 다시 띄우는 중, 또는 이어받기가 안 떠 새로 띄우는 중.
    Retrying,
}

/// 띄우기 · 재시작 한 번의 결과.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RcResult {
    pub ok: bool,
    /// 사람이 읽을 한 줄 — 무엇으로 띄웠고 어떻게 끝났나.
    pub message: String,
    /// RFC 3339.
    pub at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StrayRow {
    pub label: String,
    pub dir: String,
    pub pid: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uptime_secs: Option<u64>,
    pub sessions: usize,
}

/// `GET /api/rc/servers` 응답.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RcStatus {
    /// `rc` 블록이 있나 — 없으면 `claude rc` 프로브를 돌리지 않고 servers·strays·auth 는 비어 있다.
    pub configured: bool,
    pub servers: Vec<ServerRow>,
    pub strays: Vec<StrayRow>,
    pub auth: AuthState,
    /// `agy` 가 없으면 None. `rc` 블록과 상관없이 `agy` 설치 여부를 따른다(그 기기에서 agy 를 쓰면 보인다).
    pub antigravity: Option<AgyStatus>,
    /// 프로브 명령(`ps`·`lsof`)이 실패했으면 그 사유 — 이때 "꺼짐"은 모르는 것이다.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub probe_error: Option<String>,
    /// 감시(`rc.supervise`)가 켜져 있으면 그 상태 — 꺼져 있으면 None.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supervise: Option<SuperviseInfo>,
}

/// 감시 상태 — 마지막 바퀴 시각과, 데몬 맥락이 지금 로그아웃으로 보이는지(그동안은 되살리지 못한다).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SuperviseInfo {
    /// RFC 3339. 아직 한 바퀴도 안 돌았으면 None.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_tick: Option<String>,
    pub logged_out: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl RcStatus {
    pub fn unconfigured() -> Self {
        RcStatus {
            configured: false,
            servers: Vec::new(),
            strays: Vec::new(),
            auth: AuthState::Unknown,
            antigravity: None,
            probe_error: None,
            supervise: None,
        }
    }
}

/// 명령줄 하나가 상주 rc 서버인지를 **구조로** 판정한다: argv[0] basename 이 `claude`, argv[1] 이 `rc` /
/// `remote-control`, 뒤에 `--name` 이 있다. 글자만 찾으면 `zsh -c "… claude rc --name …"` 처럼 명령을 인자로
/// 품은 셸까지 잡히고, `--name` 을 안 보면 일회성 `claude rc --help` 가 잡힌다.
pub fn is_server_argv(args: &str) -> bool {
    let f: Vec<&str> = args.split_whitespace().collect();
    if f.len() < 3 {
        return false;
    }
    let base = f[0].rsplit('/').next().unwrap_or(f[0]);
    if base != "claude" || (f[1] != "rc" && f[1] != "remote-control") {
        return false;
    }
    f[2..]
        .iter()
        .any(|a| *a == "--name" || a.starts_with("--name="))
}

/// `ps` etime(`[[dd-]hh:]mm:ss`) → 초. 로케일과 무관한 형식이라 기동 시각 대신 이걸 쓴다.
pub fn parse_etime(s: &str) -> Option<u64> {
    let (days, rest) = match s.split_once('-') {
        Some((d, rest)) => (d.parse::<u64>().ok()?, rest),
        None => (0, s),
    };
    let parts: Vec<&str> = rest.split(':').collect();
    if !(2..=3).contains(&parts.len()) {
        return None;
    }
    let mut secs = 0u64;
    for p in parts {
        secs = secs * 60 + p.parse::<u64>().ok()?;
    }
    Some(days * 86_400 + secs)
}

/// `ps -axww -o pid=,ppid=,etime=,args=` 출력 → 줄들. 서버와 그 자식(세션)을 한 번의 `ps` 로 다 본다 —
/// 서버마다 `pgrep -P` 를 부르지 않는다(macOS `pgrep` 는 호출자의 조상을 빼서 결과가 호출 위치에 따라 달라진다).
pub fn parse_ps(out: &str) -> Vec<PsRow> {
    out.lines()
        .filter_map(|line| {
            let mut f = line.split_whitespace();
            let pid = f.next()?.parse::<u32>().ok()?;
            let ppid = f.next()?.parse::<u32>().ok()?;
            let etime = f.next()?;
            Some(PsRow {
                pid,
                ppid,
                uptime_secs: parse_etime(etime),
                args: f.collect::<Vec<_>>().join(" "),
            })
        })
        .collect()
}

/// 표에서 rc 서버만(pid 순).
pub fn servers(rows: &[PsRow]) -> Vec<&PsRow> {
    let mut out: Vec<&PsRow> = rows.iter().filter(|r| is_server_argv(&r.args)).collect();
    out.sort_by_key(|r| r.pid);
    out
}

/// 서버 하나에 열린 세션 수 — 직계 자식 중 세션 명령줄인 것.
pub fn session_count(rows: &[PsRow], server_pid: u32) -> usize {
    rows.iter()
        .filter(|r| r.ppid == server_pid && is_session_command(&r.args))
        .count()
}

/// `lsof -a -d cwd -p <pids> -F pn` 출력 → pid → cwd. `p<pid>` 다음 `n<path>` 를 짝짓는다.
pub fn parse_lsof_cwd(out: &str) -> HashMap<u32, String> {
    let mut map = HashMap::new();
    let mut pid = None;
    for line in out.lines() {
        if let Some(p) = line.strip_prefix('p') {
            pid = p.parse::<u32>().ok();
        } else if let (Some(n), Some(p)) = (line.strip_prefix('n'), pid) {
            map.insert(p, n.to_string());
        }
    }
    map
}

/// 자식 명령줄이 열린 세션인지 — 세션은 `--sdk-url …/code/session(s)/…` 이 붙은 claude 자식으로 달린다.
/// 앱에서 세션을 닫으면 그 자식이 사라지므로 닫은 세션은 세지 않는다.
pub fn is_session_command(cmd: &str) -> bool {
    cmd.find("--sdk-url")
        .is_some_and(|i| cmd[i + "--sdk-url".len()..].contains("/code/session"))
}

/// 설정의 이름 하나 → 디렉터리. `/` 는 그대로, `~` · `~/…` 는 홈 기준, 나머지는 `root` 아래.
/// 끝의 `/` 만 뗀다 — `lsof` 의 cwd 에는 붙지 않아 남기면 영영 맞지 않는다. 그 밖은 정규화하지 않는다
/// (`lsof` 의 cwd 와 문자열로 맞대는 자리라 같은 문자열이어야 한다).
pub fn resolve_dir(name: &str, home: &str, root: &str) -> String {
    let dir = if name.starts_with('/') {
        name.to_string()
    } else if name == "~" {
        home.to_string()
    } else if let Some(rest) = name.strip_prefix("~/") {
        format!("{}/{rest}", home.trim_end_matches('/'))
    } else {
        format!("{}/{name}", root.trim_end_matches('/'))
    };
    match dir.trim_end_matches('/') {
        "" => "/".to_string(),
        trimmed => trimmed.to_string(),
    }
}

/// 설정 → 대상 목록. 고정이 먼저, 같은 디렉터리는 한 번(고정이 이긴다), 빈 이름은 버린다.
pub fn resolve_targets(config: &RcConfig, home: &str) -> Vec<Target> {
    let root_raw = config.root.as_deref().unwrap_or(DEFAULT_ROOT);
    // 상대 경로 root 는 홈 기준으로 읽는다(`"ws"` → `~/ws`).
    let root = resolve_dir(root_raw, home, home);
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let named = config
        .pinned
        .iter()
        .map(|n| (n, true))
        .chain(config.targets.iter().map(|n| (n, false)));
    for (name, pinned) in named {
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        let dir = resolve_dir(name, home, &root);
        if !seen.insert(dir.clone()) {
            continue;
        }
        out.push(Target {
            label: label_of(&dir),
            dir,
            pinned,
        });
    }
    out
}

fn label_of(dir: &str) -> String {
    let trimmed = dir.trim_end_matches('/');
    trimmed.rsplit('/').next().unwrap_or(trimmed).to_string()
}

/// `claude auth status --json` → 로그인 여부. 확실히 `"loggedIn": false` 일 때만 Out — 형식을 모르면
/// Unknown 이다(모르는 것을 로그아웃으로 읽어 막으면 멀쩡한 서버까지 못 띄운다).
pub fn parse_auth_status(out: &str) -> AuthState {
    match serde_json::from_str::<serde_json::Value>(out)
        .ok()
        .and_then(|v| v.get("loggedIn").and_then(|b| b.as_bool()))
    {
        Some(true) => AuthState::In,
        Some(false) => AuthState::Out,
        None => AuthState::Unknown,
    }
}

/// `agy remote-control status` → 첫 `Daemon state = `, `Daemon pid = `, `Instance name: ` 의 첫 단어.
pub fn parse_agy_status(out: &str) -> AgyStatus {
    let first = |prefix: &str| {
        out.lines()
            .find_map(|l| l.trim().strip_prefix(prefix).map(str::trim))
            .filter(|v| !v.is_empty())
    };
    AgyStatus {
        state: first("Daemon state = ").map(str::to_string),
        pid: first("Daemon pid = ").and_then(|p| p.parse().ok()),
        instance: first("Instance name:")
            .and_then(|v| v.split_whitespace().next())
            .map(str::to_string),
    }
}

/// 대상과 떠 있는 서버를 디렉터리 문자열로 맞댄다. 대상에 없는 서버는 `strays`(폴더를 개명하면 옛 서버가
/// 옛 경로를 물고 여기 남는다 — 정리는 사람). 한 디렉터리에 서버가 둘이면 먼저 뜬 쪽(pid 작은 쪽)을 쓴다.
pub fn build_rows(targets: &[Target], live: &[LiveServer]) -> (Vec<ServerRow>, Vec<StrayRow>) {
    let mut by_dir: BTreeMap<&str, &LiveServer> = BTreeMap::new();
    let mut sorted: Vec<&LiveServer> = live.iter().collect();
    sorted.sort_by_key(|s| s.pid);
    for s in &sorted {
        by_dir.entry(s.dir.as_str()).or_insert(s);
    }
    let rows = targets
        .iter()
        .map(|t| {
            let hit = by_dir.get(t.dir.as_str());
            ServerRow {
                label: t.label.clone(),
                dir: t.dir.clone(),
                pinned: t.pinned,
                running: hit.is_some(),
                pid: hit.map(|s| s.pid),
                uptime_secs: hit.and_then(|s| s.uptime_secs),
                sessions: hit.map_or(0, |s| s.sessions),
                action: None,
                last_result: None,
                auth_suspect: false,
            }
        })
        .collect();
    // 행에 붙은 서버만 뺀다 — 대상 폴더에 서버가 둘이면 나중 것도 여기 보여야 한다.
    let attached: HashSet<u32> = targets
        .iter()
        .filter_map(|t| by_dir.get(t.dir.as_str()).map(|s| s.pid))
        .collect();
    let strays = sorted
        .into_iter()
        .filter(|s| !attached.contains(&s.pid))
        .map(|s| StrayRow {
            label: label_of(&s.dir),
            dir: s.dir.clone(),
            pid: s.pid,
            uptime_secs: s.uptime_secs,
            sessions: s.sessions,
        })
        .collect();
    (rows, strays)
}

// ── 띄우기 · 재시작 판정 ────────────────────────────────────────────────────────
// 배선(프로세스 기동 · 정지 · 대기)은 `rockyd::rc` 다. 여기는 무엇을 어떤 인자로 띄우고 결과를 어떻게 읽는지만.

/// 서버를 띄우는 방식 — `claude rc` 의 세션 인자.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LaunchMode {
    /// `-c` — 마지막 세션을 이어받는다. 단일 세션 모드라 그 세션이 끝나면 서버도 내려간다.
    Resume,
    /// 세션 하나를 만들어 둔 채 뜬다.
    Session,
    /// `--no-create-session-in-dir` — 서버만 띄우고 세션은 앱에서 연다.
    Server,
}

/// 꺼진 대상을 이름으로 띄울 때의 방식 — 고정이든 아니든 세션을 만들어 둔다(사람이 지금 쓰려고 부른 것이다).
/// `Server` 는 되살림처럼 사람이 부르지 않은 기동에 쓴다.
pub const START_MODE: LaunchMode = LaunchMode::Session;

/// 재시작 방식. 열린 세션이 있으면 이어받는다 — `-c` 는 세션이 없을 때 쓰면 뜨자마자 할 일이 없어 내려간다.
/// 없거나 `fresh`(이어받지 않음)면 고정은 세션까지, 그 밖은 서버만.
pub fn restart_mode(pinned: bool, live_session: bool, fresh: bool) -> LaunchMode {
    if live_session && !fresh {
        LaunchMode::Resume
    } else {
        fresh_mode(pinned)
    }
}

/// 이어받기가 안 떴을 때(`-c` 기록은 약 4시간 뒤 만료된다) 한 번 더 띄울 방식.
pub fn retry_mode(pinned: bool) -> LaunchMode {
    fresh_mode(pinned)
}

fn fresh_mode(pinned: bool) -> LaunchMode {
    if pinned {
        LaunchMode::Session
    } else {
        LaunchMode::Server
    }
}

/// 안 뜰 수 있는 방식인가 — 그러면 내려간 것을 보고 `retry_mode` 로 한 번 더 띄운다.
pub fn may_fail_to_start(mode: LaunchMode) -> bool {
    mode == LaunchMode::Resume
}

/// 띄울 argv — 셸을 거치지 않는다. 라벨이 `--name` 이라 서버 판정(`is_server_argv`)에 걸린다.
pub fn server_argv(label: &str, mode: LaunchMode) -> Vec<String> {
    let mut argv = vec![
        "claude".to_string(),
        "rc".to_string(),
        "--name".to_string(),
        label.to_string(),
    ];
    match mode {
        LaunchMode::Resume => argv.push("-c".to_string()),
        LaunchMode::Session => {}
        LaunchMode::Server => argv.push("--no-create-session-in-dir".to_string()),
    }
    argv
}

/// 기동 로그로 읽은 등록 결과.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Registration {
    /// 아직 모른다.
    Pending,
    /// `.out` 에 `· Connected ·`(세션 모드) 나 `· Ready ·`(서버만 모드) — 등록됐다. `Connecting` 은 아직이다.
    Connected,
    /// `.err` 에 `already served` — 내린 서버의 등록이 claude.ai 쪽에 남아 있다. 새 서버는 45초쯤 떠 있다 스스로 내려간다.
    Served,
}

/// 프로세스가 떠 있는 것만으로 "떴다" 고 하지 않는다 — `Served` 서버도 한동안 떠 있다(옛 CLI 에서 재시작 넷 중 셋이 그랬다).
pub fn read_registration(out: &str, err: &str) -> Registration {
    if err.contains("already served") {
        Registration::Served
    } else if out.contains("· Connected ·") || out.contains("· Ready ·") {
        Registration::Connected
    } else {
        Registration::Pending
    }
}

/// `Served` 로 실패한 대상을 다시 띄우기 전에 쉬는 시간. 남은 등록이 풀리는 데 2~3분이라 합이 3분을 넘지 않게 둘.
pub const REGISTRATION_BACKOFF: [Duration; 2] = [Duration::from_secs(45), Duration::from_secs(90)];
/// 내린 뒤 같은 폴더에 새로 띄우기 전에 쉬는 시간 — 곧바로 띄우면 `Served` 가 난다.
pub const RESTART_DELAY: Duration = Duration::from_secs(5);
/// SIGTERM 뒤 이만큼 기다리고도 살아 있으면 SIGKILL — SIGKILL 은 등록을 남겨 다음 기동이 `Served` 가 된다.
pub const STOP_GRACE: Duration = Duration::from_secs(20);
/// 등록 판정 — 띄우고 이만큼 뒤부터, 이 간격으로, 이만큼까지 본다. 끝까지 신호 없이 떠 있으면 "떴다(미확인)".
pub const REGISTRATION_FIRST: Duration = Duration::from_secs(3);
pub const REGISTRATION_POLL: Duration = Duration::from_secs(2);
pub const REGISTRATION_WAIT: Duration = Duration::from_secs(40);

/// 사람에게 보일 방식 설명 — CLI 출력과 웹 확인 창.
pub fn mode_note(mode: LaunchMode, fresh: bool) -> &'static str {
    match mode {
        LaunchMode::Resume => "열린 세션 이어받기(-c)",
        _ if fresh => "이어받지 않고 새로",
        LaunchMode::Session => "새 세션과 함께",
        LaunchMode::Server => "서버만(세션은 앱에서)",
    }
}

// ── 감시(되살리기) 판정 ─────────────────────────────────────────────────────────
// 배선(주기 루프 · 알림 · 기록 파일)은 `rockyd::rc` 다. 옛 CLI 의 주기 실행(`-q`)을 옮겼다.

/// 감시 한 바퀴 간격과 기동 뒤 첫 바퀴까지의 시간.
pub const SUPERVISE_INTERVAL: Duration = Duration::from_secs(120);
pub const SUPERVISE_FIRST: Duration = Duration::from_secs(120);
/// 연속 실패 쉬기의 상한.
pub const SUPERVISE_BACKOFF_MAX: Duration = Duration::from_secs(30 * 60);

/// 감시가 띄울 대상 하나와 그 방식.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Revive {
    pub label: String,
    pub mode: LaunchMode,
}

/// 이번 바퀴에 되살릴 대상 — 꺼졌고 진행 중이 아닌 것 중, 고정(새 세션과 함께)과 야간이 내리고 못 띄워 되살림 표식이
/// 남은 비고정(`marked`, 서버만 — 사람이 부른 기동이 아니다). 프로브가 실패했거나(꺼짐이 모름) 자격이 확실히
/// 로그아웃이면(띄워도 곧 내려간다) 아무것도 고르지 않는다. 표식 없는 비고정은 고르지 않는다 — 옛 CLI 에서 비고정 자동
/// 기동이 같은 폴더를 계속 띄우던 함정이 있었다.
pub fn revive_candidates(status: &RcStatus, marked: &HashSet<String>) -> Vec<Revive> {
    if !status.configured || status.probe_error.is_some() || status.auth == AuthState::Out {
        return Vec::new();
    }
    status
        .servers
        .iter()
        .filter(|s| !s.running && s.action.is_none())
        .filter_map(|s| {
            let mode = if s.pinned {
                START_MODE
            } else if marked.contains(&s.label) {
                LaunchMode::Server
            } else {
                return None;
            };
            Some(Revive {
                label: s.label.clone(),
                mode,
            })
        })
        .collect()
}

/// 지울 되살림 표식 — 이미 떠 있는 대상(누가 띄웠다)과 더는 대상이 아닌 라벨(설정에서 뺐다). 프로브가 실패했으면
/// 아무것도 지우지 않는다 — 꺼짐이 모름이라 지우면 되살릴 기회를 잃는다. 진행 중인 대상도 남긴다 — 야간은 표식을 찍고
/// 내린 뒤 띄우는 동안(정지 유예 · 등록 판정) 서버가 떠 있는 것으로 보이고, 그때 지우면 끝내 못 뜬 서버를 아무도
/// 되살리지 않는다. 라벨 순.
pub fn stale_revive_marks(status: &RcStatus, marked: &HashSet<String>) -> Vec<String> {
    if !status.configured || status.probe_error.is_some() {
        return Vec::new();
    }
    let mut out: Vec<String> = marked
        .iter()
        .filter(|label| {
            status
                .servers
                .iter()
                .find(|s| &s.label == *label)
                .is_none_or(|s| s.running && s.action.is_none())
        })
        .cloned()
        .collect();
    out.sort();
    out
}

/// 연달아 `failures` 번 못 띄운 대상이 다음에 시도하기까지 쉬는 시간 — 2 · 4 · 8 · 16 · 30분(상한). 폴더가 사라졌거나
/// trust 가 풀린 대상을 2분마다 두드리지 않게. 0 이면 쉬지 않는다.
pub fn failure_backoff(failures: u32) -> Duration {
    if failures == 0 {
        return Duration::ZERO;
    }
    let secs = 120u64.saturating_mul(1u64 << (failures - 1).min(10));
    Duration::from_secs(secs).min(SUPERVISE_BACKOFF_MAX)
}

/// 감시 맥락(데몬)이 본 자격의 기록 — 언제 로그아웃을 봤고, 그 뒤 언제 다시 로그인을 봤나(unix 초).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthMark {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_out: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_in: Option<i64>,
}

impl AuthMark {
    /// 마지막 로그아웃 뒤 로그인을 다시 봤나.
    pub fn recovered(&self) -> bool {
        matches!((self.last_out, self.last_in), (Some(out), Some(inn)) if inn > out)
    }

    /// 마지막으로 본 것이 로그아웃인가.
    pub fn still_out(&self) -> bool {
        self.last_out.is_some() && !self.recovered()
    }
}

/// 이번 관찰로 바뀐 것 — 알림은 바뀐 바퀴에 한 번만.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthTransition {
    None,
    /// 로그인 → 로그아웃(또는 처음 본 것이 로그아웃).
    LoggedOut,
    /// 로그아웃 뒤 처음 로그인.
    Recovered,
}

/// 자격 관찰을 기록에 반영한다. `Unknown`(시간 초과 · 형식 모름)은 판정이 아니라 기록을 건드리지 않는다.
pub fn next_auth_mark(state: AuthState, mark: &AuthMark, now: i64) -> (AuthMark, AuthTransition) {
    match state {
        AuthState::Out => {
            let transition = if mark.still_out() {
                AuthTransition::None
            } else {
                AuthTransition::LoggedOut
            };
            (
                AuthMark {
                    last_out: Some(now),
                    last_in: mark.last_in,
                },
                transition,
            )
        }
        AuthState::In if mark.still_out() => (
            AuthMark {
                last_out: mark.last_out,
                last_in: Some(now),
            },
            AuthTransition::Recovered,
        ),
        _ => (mark.clone(), AuthTransition::None),
    }
}

/// 자격이 끊겼다 돌아온 뒤, 그 로그아웃보다 먼저 뜬 서버 — 갱신 못 한 토큰을 들고 있어 세션을 열면 죽는다(옛 CLI 가
/// 2026-10-04 에 겪었다). 아직 로그아웃 중이면 의심하지 않는다 — 그건 "데몬 맥락이 로그인을 못 읽는다" 는 다른 신호다.
/// 다시 띄우면 기동 시각이 그 뒤라 저절로 풀린다.
pub fn auth_suspect(uptime_secs: Option<u64>, mark: &AuthMark, now: i64) -> bool {
    let (Some(up), Some(out)) = (uptime_secs, mark.last_out) else {
        return false;
    };
    mark.recovered() && now.saturating_sub(up as i64) < out
}

// ── 야간 재시작 판정 ────────────────────────────────────────────────────────────
// 배선(`claude update` · 네트워크 확인 · 내리고 띄우기 · 바쁨 대기 · 회복 · 기록 파일)은 `rockyd::rc` 다. 옛 CLI 의
// 야간 모드(`--nightly`)를 옮겼다 — 새벽에 `claude update` 뒤 구버전이면서 쉬는 서버만 다시 띄운다.

/// 야간의 `already served` 재시도 간격 — 낮(45초 · 90초)으로는 안 풀린 적이 있고(옛 CLI, 5분쯤 뒤 풀렸다), 새벽에
/// 못 띄우면 아침까지 꺼져 있다. 기다리는 비용은 새벽이라 싸다 — 합 13분.
pub const NIGHTLY_REGISTRATION_BACKOFF: [Duration; 4] = [
    Duration::from_secs(60),
    Duration::from_secs(120),
    Duration::from_secs(240),
    Duration::from_secs(360),
];
/// 바쁜 서버를 다시 보는 간격.
pub const NIGHTLY_BUSY_POLL: Duration = Duration::from_secs(5 * 60);

/// `"04:30"` · `"4:30"` → 시각. 모양이 틀리면 None.
pub fn parse_hhmm(s: &str) -> Option<NaiveTime> {
    let (h, m) = s.trim().split_once(':')?;
    let digits = |p: &str, max_len: usize| {
        !p.is_empty() && p.len() <= max_len && p.bytes().all(|b| b.is_ascii_digit())
    };
    if !digits(h, 2) || !digits(m, 2) || m.len() != 2 {
        return None;
    }
    NaiveTime::from_hms_opt(h.parse().ok()?, m.parse().ok()?, 0)
}

/// `X.Y.Z` → 숫자 셋. 문자열로 비교하면 `2.1.29` 가 `2.1.287` 보다 크다.
fn semver(v: &str) -> Option<(u64, u64, u64)> {
    let mut parts = v.split('.');
    let mut next = || -> Option<u64> {
        let p = parts.next()?;
        (!p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
            .then(|| p.parse().ok())
            .flatten()
    };
    let n = (next()?, next()?, next()?);
    parts.next().is_none().then_some(n)
}

/// `claude --version` 출력(`2.1.288 (Claude Code)`)의 첫 줄 첫 단어가 X.Y.Z 면 그것. 시간 초과로 비었거나 모양이
/// 다르면 None — 모르는 값을 기동 버전으로 남기면 다음 야간이 멀쩡한 서버를 구버전으로 본다.
pub fn parse_claude_version(out: &str) -> Option<String> {
    let first = out.lines().find_map(|l| l.split_whitespace().next())?;
    semver(first).map(|_| first.to_string())
}

/// 설치 경로(`~/.local/bin/claude` 링크의 대상 `…/claude/versions/2.1.287`)의 마지막 이름이 X.Y.Z 면 그것.
/// 새 바이너리의 첫 실행이 Gatekeeper 검사로 멎어 `--version` 을 못 잴 때 쓴다.
pub fn version_from_path(path: &str) -> Option<String> {
    let name = path.trim_end_matches('/').rsplit('/').next()?;
    semver(name).map(|_| name.to_string())
}

/// 이름들(`versions/` 목록) 중 가장 높은 X.Y.Z.
pub fn newest_version<'a>(names: impl IntoIterator<Item = &'a str>) -> Option<String> {
    names
        .into_iter()
        .filter_map(|n| semver(n).map(|v| (v, n)))
        .max_by_key(|(v, _)| *v)
        .map(|(_, n)| n.to_string())
}

/// Claude Code 가 폴더의 대화 기록을 두는 `~/.claude/projects/` 아래 이름 — ASCII 영숫자가 아닌 글자를 전부 `-` 로
/// (`/a/b.c/.d` → `-a-b-c--d`).
pub fn project_dir_name(dir: &str) -> String {
    dir.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// 야간 판정의 이유 — 이벤트 기록의 `reason` 값.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum NightlyReason {
    /// 구버전이고 쉰다 — 다시 띄운다.
    Restart,
    /// 이미 설치 버전으로 떠 있다.
    Current,
    /// 설치 버전을 못 쟀다.
    VersionUnknown,
    /// 기동 버전 기록이 없거나 비었다 — 구버전인지 모른다(이 기능 전에 뜬 서버, 기동 때 버전을 못 잰 서버).
    NoRecord,
    /// 열린 세션이 최근에 대화했다 — 마감까지 다시 본다.
    Busy,
    /// 내릴 차례인데 네트워크가 닿지 않는다 — 내리면 다시 등록하지 못한다.
    NoNetwork,
}

/// 서버 하나에 대해 잰 사실.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NightlyState {
    /// 설치 버전(`claude update` 뒤). None 이면 모른다.
    pub current: Option<String>,
    /// 기동 때 남긴 `<라벨>.version`. 없거나 못 읽었으면 None.
    pub recorded: Option<String>,
    pub live_session: bool,
    /// 그 폴더 대화 기록(`*.jsonl`) 중 가장 최근 mtime(unix 초). 기록이 하나도 없으면 None.
    pub last_write: Option<i64>,
    pub pinned: bool,
    /// 내리기 직전 네트워크 확인이 끝내 실패했다.
    pub offline: bool,
}

/// 서버 하나에 대한 판정.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NightlyDecision {
    pub reason: NightlyReason,
    /// 기동 버전(기록의 앞뒤 공백을 뗀 것).
    pub from: Option<String>,
    /// 다시 띄울 방식 — `Restart` 일 때만. 사람이 부르는 재시작과 같은 판정(`restart_mode`)이다.
    pub mode: Option<LaunchMode>,
    /// 마지막 대화 뒤 지난 초 — 열린 세션이 있고 대화 기록도 있을 때만.
    pub idle_secs: Option<u64>,
}

/// 판정한다. 순서: 버전 모름 → 기록 없음 → 최신 → 바쁨 → 네트워크 없음 → 재시작.
///
/// **쉬는 서버** — 열린 세션이 없으면(서버만) 쉰다. 있으면 그 폴더 대화 기록이 `quiet` 넘게 안 바뀌었을 때. 세션이
/// 있는데 기록이 하나도 없으면(아직 한 마디도 안 한 세션) 끊겨도 잃을 대화가 없으니 쉬는 것으로 본다. 세션이 없으면
/// 같은 폴더의 로컬 터미널이 최근 기록을 남겼어도 서버는 쉬는 것이다.
///
/// 기록이 없는 서버는 구버전으로 몰지 않고 건드리지도 않는다 — `claude update` 직후라 지금 버전을 적어 넣으면 옛
/// 바이너리로 떠 있는 서버를 최신으로 속인다.
pub fn decide_nightly(s: &NightlyState, now: i64, quiet: Duration) -> NightlyDecision {
    let from = s
        .recorded
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string);
    let idle_secs = s
        .last_write
        .filter(|_| s.live_session)
        .map(|t| now.saturating_sub(t).max(0) as u64);
    let reason = match (&s.current, &from) {
        (None, _) => NightlyReason::VersionUnknown,
        (_, None) => NightlyReason::NoRecord,
        (Some(cur), Some(rec)) if cur == rec => NightlyReason::Current,
        _ if idle_secs.is_some_and(|idle| idle < quiet.as_secs()) => NightlyReason::Busy,
        _ if s.offline => NightlyReason::NoNetwork,
        _ => NightlyReason::Restart,
    };
    let mode =
        (reason == NightlyReason::Restart).then(|| restart_mode(s.pinned, s.live_session, false));
    NightlyDecision {
        reason,
        from,
        mode,
        idle_secs,
    }
}

/// 야간을 돌 차례인가 — 오늘 `at` 이 지났고 오늘 아직 안 돌았다(`last_run` 은 마지막으로 돈 날). 맥이 그 시각에 자고
/// 있었으면 깬 뒤 처음 보는 때가 차례다 — 옛 CLI 의 launchd 달력 잡이 놓친 실행을 깬 뒤 한 번 돌리던 것과 같다.
pub fn nightly_due(now: NaiveDateTime, at: NaiveTime, last_run: NaiveDate) -> bool {
    now.time() >= at && last_run < now.date()
}

/// 기록이 없을 때(야간을 처음 켠 날) 이미 돈 것으로 칠 날 — 오늘 `at` 이 지났으면 오늘, 아니면 어제. 켜자마자 낮에
/// 서버를 내리지 않고 다음 `at` 부터 돈다.
pub fn nightly_first_mark(now: NaiveDateTime, at: NaiveTime) -> NaiveDate {
    if now.time() >= at {
        now.date()
    } else {
        now.date().pred_opt().unwrap_or(now.date())
    }
}

/// 바쁜 서버를 기다리고 못 뜬 서버를 다시 띄우는 마감 — 오늘 `until`. 이미 지났으면 now(기다리지 않는다): 04:30 을
/// 자고 넘겨 09:00 에 도는 실행은 바쁜 서버를 다음 날로 넘긴다.
pub fn busy_deadline(now: NaiveDateTime, until: NaiveTime) -> NaiveDateTime {
    let deadline = now.date().and_time(until);
    deadline.max(now)
}

/// 내리고 못 띄운 서버를 n 번째(1부터) 다시 띄우기 전 대기 — 1 · 2 · 4 · 8 · 16분, 그 뒤 16분. 마감까지 되풀이하고,
/// 그래도 안 뜨면 되살림 표식을 남긴 채 감시에 넘긴다.
pub fn recovery_wait(n: u32) -> Duration {
    Duration::from_secs(60 << (n.clamp(1, 5) - 1))
}

/// 먼저 내려 볼 하나를 맨 앞에 — 고정을 고른다. 고정은 실패해도 감시가 새 세션으로 되살리니 시험대로 가장 싸다.
/// 고정이 없으면 순서 그대로. 이것이 안 뜨면 나머지는 건드리지 않는다 — 옛 CLI 가 새벽에 다섯을 한꺼번에 내렸다가
/// 전부 못 띄운 일이 있었다(2026-10-03, 네트워크가 아직 안 붙었다).
pub fn canary_first<T>(mut items: Vec<T>, pinned: impl Fn(&T) -> bool) -> Vec<T> {
    if let Some(i) = items.iter().position(&pinned) {
        let canary = items.remove(i);
        items.insert(0, canary);
    }
    items
}

/// 야간 전체를 건너뛸 이유 — 이벤트 기록의 `reason` 값. 프로브가 실패했으면 떠 있는 것을 모르고, 자격이 확실히
/// 로그아웃이면 내린 서버를 다시 못 띄운다.
pub fn nightly_blocked(status: &RcStatus) -> Option<&'static str> {
    if !status.configured {
        Some("unconfigured")
    } else if status.probe_error.is_some() {
        Some("probe-failed")
    } else if status.auth == AuthState::Out {
        Some("logged-out")
    } else {
        None
    }
}

/// 야간 한 대상의 결과.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NightlyOutcome {
    /// 다시 띄웠다(기다린 끝에 · 회복 재시도 포함).
    Restarted,
    /// 이미 설치 버전이다.
    Current,
    /// 손대지 않았다 — 사유는 `note`(기록 없음 · 네트워크 없음 · canary 실패 · 마감까지 바쁨 …).
    Skipped,
    /// 내렸지만 마감까지 못 띄웠다 — 되살림 표식이 남아 감시가 띄운다.
    Down,
    /// 리허설 — 다시 띄울 것.
    WouldRestart,
    /// 리허설 — 바빠서 마감까지 기다릴 것.
    WouldWait,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NightlyItem {
    pub label: String,
    pub outcome: NightlyOutcome,
    /// 사람이 읽을 한 줄 — 버전(`2.1.283 → 2.1.288`)과 사유.
    pub note: String,
}

/// 야간 한 번의 결과 — 현황과 `rc/nightly.json` 에 싣는다.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NightlyReport {
    /// RFC 3339.
    pub started_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub dry_run: bool,
    /// `claude update` 한 줄 요약.
    pub update: String,
    /// 판정에 쓴 설치 버전.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// 전체를 건너뛴 이유(`nightly_blocked`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked: Option<String>,
    #[serde(default)]
    pub items: Vec<NightlyItem>,
}

impl NightlyReport {
    pub fn count(&self, outcome: NightlyOutcome) -> usize {
        self.items.iter().filter(|i| i.outcome == outcome).count()
    }
}
