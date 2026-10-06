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
    /// 기동 버전 기록이 지금 설치 버전과 다르다 — 야간 재시작이 쉬는 때 다시 띄운다.
    #[serde(skip_serializing_if = "is_false")]
    pub stale: bool,
    /// 그 폴더의 최근 활동(git) — 부를 때만 잰다(`?activity=1`). 5초 폴링 현황에는 없다.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity: Option<Activity>,
}

/// 진행 중인 일.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RcAction {
    Starting,
    Restarting,
    /// `already served` 를 만나 쉬었다 다시 띄우는 중, 또는 이어받기가 안 떠 새로 띄우는 중.
    Retrying,
    /// 재시작하려는데 열린 세션이 막 대화하는 중 — 턴이 끝나길 기다린다.
    Waiting,
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
    /// 데몬이 띄운 핸드오프 서버(보드의 새 세션 띄우기) — 대상 밖 서버에서 가려낸 것(`split_handoffs`). 없으면 비운다.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub handoffs: Vec<HandoffServerRow>,
    pub auth: AuthState,
    /// `agy` 가 없으면 None. `rc` 블록과 상관없이 `agy` 설치 여부를 따른다(그 기기에서 agy 를 쓰면 보인다).
    pub antigravity: Option<AgyStatus>,
    /// 프로브 명령(`ps`·`lsof`)이 실패했으면 그 사유 — 이때 "꺼짐"은 모르는 것이다.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub probe_error: Option<String>,
    /// 감시(`rc.supervise`)가 켜져 있으면 그 상태 — 꺼져 있으면 None.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supervise: Option<SuperviseInfo>,
    /// 야간 재시작 — 일정이 켜져 있거나 손으로 돌린 결과가 있으면.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nightly: Option<NightlyInfo>,
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
            handoffs: Vec::new(),
            auth: AuthState::Unknown,
            antigravity: None,
            probe_error: None,
            supervise: None,
            nightly: None,
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

/// `start` 의 조상 pid(자기 자신 포함) — 같은 `ps` 표의 ppid 를 따라 올라간다. CLI 가 자기 서버를 재시작하려는지 가르는
/// 데 쓴다(옛 CLI 의 자가-살해 가드). 고리가 있어도 끝나게 64단까지만.
pub fn ancestors(rows: &[PsRow], start: u32) -> HashSet<u32> {
    let parent: HashMap<u32, u32> = rows.iter().map(|r| (r.pid, r.ppid)).collect();
    let mut out = HashSet::new();
    let mut pid = start;
    for _ in 0..64 {
        if !out.insert(pid) {
            break;
        }
        match parent.get(&pid) {
            Some(&p) if p > 1 => pid = p,
            _ => break,
        }
    }
    out
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
                stale: false,
                activity: None,
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
    /// `--session-id <id>` — 사람이 고른 세션으로 못 박아 이어받는다(`-c` 가 무는 "마지막 세션" 이 원하는 것이 아닐 때).
    Pin,
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

/// 안 뜰 수 있는 방식인가 — 그러면 내려간 것을 보고 `retry_mode` 로 한 번 더 띄운다. 못 박은 세션도 만료됐거나 틀린
/// id 면 뜨자마자 내려간다.
pub fn may_fail_to_start(mode: LaunchMode) -> bool {
    matches!(mode, LaunchMode::Resume | LaunchMode::Pin)
}

/// 못 박을 수 있는 세션 id 인가 — claude.ai 쪽 id(`cse_…` · `session_…`)다. 로컬 전사본 UUID 를 넣으면 서버가
/// 400 으로 뜨자마자 내려가니 미리 거른다.
pub fn valid_session_id(id: &str) -> bool {
    let rest = id
        .strip_prefix("cse_")
        .or_else(|| id.strip_prefix("session_"));
    rest.is_some_and(|r| {
        !r.is_empty()
            && r.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    })
}

/// 띄울 argv — 셸을 거치지 않는다. 라벨이 `--name` 이라 서버 판정(`is_server_argv`)에 걸린다. `Pin` 은 `session`
/// 이 있어야 뜻이 있다(없으면 이어받기와 같다).
pub fn server_argv(label: &str, mode: LaunchMode, session: Option<&str>) -> Vec<String> {
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
        LaunchMode::Pin => match session {
            Some(id) => argv.extend(["--session-id".to_string(), id.to_string()]),
            None => argv.push("-c".to_string()),
        },
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

/// `Served` 로 실패한 대상을 다시 띄우기 전에 쉬는 시간 — 합 3분. 남은 등록이 풀리는 데 2~3분인데, 45 · 90초(합 2분 15초)로는
/// 못 풀린 재시작이 있었다(2026-10-06 실측 — 내린 뒤 3분쯤에 풀렸다).
pub const REGISTRATION_BACKOFF: [Duration; 2] = [Duration::from_secs(60), Duration::from_secs(120)];
/// 내린 뒤 같은 폴더에 새로 띄우기 전에 쉬는 시간 — 곧바로 띄우면 `Served` 가 난다.
pub const RESTART_DELAY: Duration = Duration::from_secs(5);
/// SIGTERM 뒤 이만큼 기다리고도 살아 있으면 SIGKILL — SIGKILL 은 등록을 남겨 다음 기동이 `Served` 가 된다.
pub const STOP_GRACE: Duration = Duration::from_secs(20);
/// 등록 판정 — 띄우고 이만큼 뒤부터, 이 간격으로, 이만큼까지 본다. 끝까지 신호 없이 떠 있으면 "떴다(미확인)".
pub const REGISTRATION_FIRST: Duration = Duration::from_secs(3);
pub const REGISTRATION_POLL: Duration = Duration::from_secs(2);
pub const REGISTRATION_WAIT: Duration = Duration::from_secs(40);

/// 낮 재시작의 턴 대기 — 열린 세션이 `TURN_QUIET` 안에 대화했으면 막 답하는 중으로 보고 `TURN_POLL` 마다 다시 보며
/// `TURN_WAIT` 까지 기다린다. 야간의 "쉬는 서버"(60분)보다 훨씬 짧다 — 사람이 시킨 재시작이고, 대화 중인 세션은
/// 이어받기로 돌아오니 막 답하는 중만 피하면 된다.
pub const TURN_QUIET: Duration = Duration::from_secs(2 * 60);
pub const TURN_POLL: Duration = Duration::from_secs(15);
pub const TURN_WAIT: Duration = Duration::from_secs(10 * 60);

/// 턴이 도는 중인가 — 열린 세션이 있고 그 폴더 대화 기록(`last_write`, unix 초)이 `quiet` 안에 바뀌었다. 세션이 없거나
/// 기록이 없으면 아니다. 자기 서버를 재시작해도 요청한 턴이 끝난 뒤에 내려간다.
pub fn turn_in_progress(
    live_session: bool,
    last_write: Option<i64>,
    now: i64,
    quiet: Duration,
) -> bool {
    live_session && last_write.is_some_and(|t| now.saturating_sub(t) < quiet.as_secs() as i64)
}

/// 사람에게 보일 방식 설명 — CLI 출력과 웹 확인 창.
pub fn mode_note(mode: LaunchMode, fresh: bool) -> &'static str {
    match mode {
        LaunchMode::Resume => "열린 세션 이어받기(-c)",
        LaunchMode::Pin => "고른 세션 이어받기(--session-id)",
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
    /// 먼저 내린 하나가 안 떠서 나머지를 건드리지 않았다.
    #[serde(default, skip_serializing_if = "is_false")]
    pub canary_failed: bool,
    /// rocky 세 층의 버전과 최신 릴리스 태그 — 진짜 실행에만(리허설은 네트워크를 타지 않는다).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rocky: Option<RockyVersions>,
    /// agy 와 그 원격 제어 데몬 — 진짜 실행에만, agy 가 없으면 None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agy: Option<AgyRecord>,
    #[serde(default)]
    pub items: Vec<NightlyItem>,
}

impl NightlyReport {
    pub fn count(&self, outcome: NightlyOutcome) -> usize {
        self.items.iter().filter(|i| i.outcome == outcome).count()
    }
}

/// 현황의 야간 상태.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NightlyInfo {
    /// 매일 돌 시각(HH:MM). 일정이 꺼져 있으면 None(손으로 돌린 결과만 있다).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
    pub running: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last: Option<NightlyReport>,
}

// ── 최근 활동(옛 CLI `-s`) ──────────────────────────────────────────────────────
// 꺼진 대상 중 "열 만한 후보" 를 가른다 — 기동 조건이 아니라 사람이 고를 때 보는 표시다. git 은 데몬이 부를 때만 돈다.

/// 최근 활동으로 치는 마지막 커밋 나이(일).
pub const ACTIVE_DAYS: i64 = 14;

/// 대상 폴더의 git 사실.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Activity {
    /// git 저장소인가.
    pub repo: bool,
    /// 추적 중인 파일에 커밋 안 된 변경이 있다(`status --porcelain -uno` — 추적 안 하는 파일은 치지 않는다).
    #[serde(skip_serializing_if = "is_false")]
    pub dirty: bool,
    /// 지금 브랜치 — detached 면 None.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// `origin/HEAD` 가 가리키는 기본 브랜치(`origin/` 뗀 것) — 모르면 None.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_branch: Option<String>,
    /// 마지막 커밋 시각(unix 초) · 제목 — 커밋이 없으면 None.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    /// 활성인가(`is_active`) — 꺼진 대상이면 아니면 "정박" 이다.
    pub active: bool,
}

impl Activity {
    /// 기본 브랜치가 아닌 곳에 있다. 둘 중 하나라도 모르면 아니다.
    pub fn on_side_branch(&self) -> bool {
        matches!((&self.branch, &self.default_branch), (Some(b), Some(d)) if b != d)
    }
}

/// git 출력을 사실로 — `rev-parse --git-dir` 의 성공 여부, `status --porcelain -uno`, `symbolic-ref --short HEAD`,
/// `symbolic-ref --short refs/remotes/origin/HEAD`, `log -1 --format=%ct%n%s` 의 stdout(실패면 빈 문자열).
pub fn parse_git_facts(
    repo: bool,
    status: &str,
    head: &str,
    origin_head: &str,
    log: &str,
    now: i64,
) -> Activity {
    if !repo {
        return Activity {
            active: true,
            ..Activity::default()
        };
    }
    let nonempty = |s: &str| Some(s.trim()).filter(|v| !v.is_empty()).map(str::to_string);
    let mut lines = log.trim_end_matches('\n').splitn(2, '\n');
    let commit_at = lines.next().and_then(|t| t.trim().parse::<i64>().ok());
    let mut a = Activity {
        repo: true,
        dirty: status.lines().any(|l| !l.trim().is_empty()),
        branch: nonempty(head),
        default_branch: nonempty(origin_head).map(|b| b.trim_start_matches("origin/").to_string()),
        commit_at,
        subject: commit_at.and(lines.next().map(str::to_string)),
        active: false,
    };
    a.active = is_active(&a, now, ACTIVE_DAYS);
    a
}

/// 활성인가 — 작업 트리가 더럽거나, 곁가지 브랜치에 있거나, 마지막 커밋이 `days` 일 안이다. 판정 못 하면(git 아님 ·
/// 커밋 없음) 활성이다 — 열 만한 후보에서 빼지 않는다. 미래 커밋(시계 어긋남)도 활성이다.
pub fn is_active(a: &Activity, now: i64, days: i64) -> bool {
    match a.commit_at {
        _ if !a.repo || a.dirty || a.on_side_branch() => true,
        None => true,
        Some(t) => now.saturating_sub(t) / 86_400 <= days,
    }
}

/// 커밋 나이 — 1시간 미만 `방금`, 하루 미만 `N시간 전`, 그 뒤 `N일 전`.
pub fn activity_age(now: i64, at: i64) -> String {
    let secs = now.saturating_sub(at).max(0);
    match (secs / 3600, secs / 86_400) {
        (0, _) => "방금".into(),
        (h, 0) => format!("{h}시간 전"),
        (_, d) => format!("{d}일 전"),
    }
}

/// 커밋 제목을 38 **글자**로 자르고 `…` — 바이트로 자르면 한글이 깨진다.
pub fn short_subject(subject: &str) -> String {
    let mut chars = subject.chars();
    let head: String = chars.by_ref().take(38).collect();
    if chars.next().is_some() {
        format!("{head}…")
    } else {
        head
    }
}

// ── 야간 보고의 rocky 버전 ──────────────────────────────────────────────────────
// 옛 CLI 의 야간이 남기던 것이다 — 설치는 하지 않는다(올리는 건 `rocky update`). 기록만 해서 사람이 밀린 것을 본다.

/// rocky 세 층의 버전과 최신 릴리스 태그. 못 잰 칸은 None.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RockyVersions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cli: Option<String>,
    pub daemon: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest: Option<String>,
}

impl RockyVersions {
    /// `current`(셋 다 최신) · `behind`(하나라도 밀림) · `unknown`(하나라도 못 잼).
    pub fn status(&self) -> &'static str {
        let layers = [
            self.plugin.as_deref(),
            self.cli.as_deref(),
            Some(self.daemon.as_str()),
        ];
        match self.latest.as_deref() {
            Some(latest) if layers.iter().all(|v| v.is_some()) => {
                if layers.iter().all(|v| *v == Some(latest)) {
                    "current"
                } else {
                    "behind"
                }
            }
            _ => "unknown",
        }
    }
}

/// `claude plugin list --json` 에서 그 id 의 version.
pub fn parse_plugin_version(json: &str, id: &str) -> Option<String> {
    serde_json::from_str::<Vec<serde_json::Value>>(json.trim())
        .ok()?
        .into_iter()
        .find(|p| p.get("id").and_then(|v| v.as_str()) == Some(id))?
        .get("version")?
        .as_str()
        .map(str::to_string)
}

/// `rocky 0.42.0` 같은 출력의 첫 줄 마지막 칸(`v` 는 뗀다).
pub fn parse_tool_version(out: &str) -> Option<String> {
    let last = out.trim().lines().next()?.split_whitespace().last()?;
    Some(last.trim_start_matches('v').to_string())
}

/// `git ls-remote --tags --refs <repo> 'v*'` 에서 가장 높은 정식 릴리스(`vX.Y.Z`, `v` 뗀 것). 사전 릴리스(`-rc.1`)는 뺀다 —
/// 설치되는 것은 정식 릴리스다. 문자열이 아니라 숫자로 비교한다.
pub fn latest_tag(ls_remote: &str) -> Option<String> {
    ls_remote
        .lines()
        .filter_map(|l| l.split_whitespace().nth(1)?.strip_prefix("refs/tags/v"))
        .filter_map(|v| semver(v).map(|n| (n, v)))
        .max_by_key(|(n, _)| *n)
        .map(|(_, v)| v.to_string())
}

// ── 야간 보고의 agy ─────────────────────────────────────────────────────────────
// 옛 CLI 의 야간이 남기던 것이다 — 설치 · 재시작은 하지 않는다(agy 데몬은 agy 가 올린 launchd 잡이 살린다). 기동 시각과
// 실행 파일 mtime 을 같이 남겨, agy 업데이트 뒤 데몬이 새 바이너리로 다시 떴는지를 본다.

/// agy 버전과 원격 제어 데몬의 상태. 못 잰 칸은 None.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgyRecord {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// `agy remote-control status` 의 첫 `Daemon state`. 데몬이 없으면 None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance: Option<String>,
    /// 데몬 프로세스가 뜬 시각(unix 초).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started: Option<i64>,
    /// agy 실행 파일의 mtime(unix 초) — 업데이트는 이 파일을 제자리에서 바꾼다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binary_mtime: Option<i64>,
    /// 데몬이 지금 설치된 agy 보다 먼저 떴다 — 업데이트 뒤 옛 바이너리로 돈다.
    #[serde(default)]
    pub old_binary: bool,
}

/// 데몬이 옛 바이너리로 도나 — 실행 파일이 데몬 기동보다 나중에 바뀌었다. 둘 중 하나라도 모르면 false.
pub fn agy_old_binary(started: Option<i64>, binary_mtime: Option<i64>) -> bool {
    matches!((started, binary_mtime), (Some(s), Some(m)) if m > s)
}

// ── 핸드오프 서버(spawn 의 rc 갈래) ─────────────────────────────────────────────
// 보드의 "새 세션 띄우기" 가 할 일의 워크트리에서 단일 세션 rc 서버를 띄우고, 그 세션에 핸드오프를 넣는다. `claude --bg`
// 세션은 로그인 세션 밖이라 ssh · 자격이 끊긴다. 대상(`rc.targets`)이 아니라 감시 · 야간은 건드리지 않는다.

/// 서버 이름의 요약 칸 — 할 일 제목을 이만큼 **글자**로 자른다(폰 · 웹 세션 목록 한 줄).
pub const HANDOFF_SUMMARY_CHARS: usize = 24;

/// `<보드>-<n>: <요약>` — claude.ai 세션 목록에 보이는 이름. 요약은 제목 앞부분(글자 단위). 넘치면 마지막 띄어쓰기까지
/// 물러나 `…` 를 붙인다 — 띄어쓰기가 너무 앞(절반 전)이면 그냥 자른다. 제목이 비면 번호만.
pub fn handoff_server_name(board_key: &str, number: i64, title: &str) -> String {
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = title.chars();
    let head: String = chars.by_ref().take(HANDOFF_SUMMARY_CHARS).collect();
    let summary = match chars.next() {
        None => head,
        // 단어가 끝난 자리에서 잘렸다.
        Some(' ') => format!("{head}…"),
        Some(_) => {
            let cut = head
                .rfind(' ')
                .filter(|&i| head[..i].chars().count() >= HANDOFF_SUMMARY_CHARS / 2)
                .map_or(head.as_str(), |i| &head[..i]);
            format!("{}…", cut.trim_end())
        }
    };
    if summary.is_empty() {
        format!("{board_key}-{number}")
    } else {
        format!("{board_key}-{number}: {summary}")
    }
}

/// 핸드오프 서버 argv — 단일 세션 모드(`--spawn session`: 세션 하나만, 다른 접속은 거절). 이름은 `--name=<이름>` 한 인자로 —
/// 셸을 거치지 않으니 공백 · `:` 가 있어도 되고, 보드 key 가 `-` 로 시작해도 플래그로 읽히지 않는다. 서버 판정(`is_server_argv`)
/// 에 걸린다 — 현황에는 대상 밖 서버로 보인다.
pub fn handoff_server_argv(name: &str) -> Vec<String> {
    vec![
        "claude".to_string(),
        "rc".to_string(),
        "--spawn".to_string(),
        "session".to_string(),
        format!("--name={name}"),
    ]
}

/// 라벨에 넣는 보드 key 의 최대 글자 수 — `<label>.out` 이 파일 이름 한도(255바이트) 안에 머물게.
const HANDOFF_LABEL_KEY_MAX: usize = 48;

/// 핸드오프 서버의 기동 로그 · 이벤트 라벨 — `handoff-<보드>-<n>`. 보드 key 는 원격에서도 바꿀 수 있고 길이 · `/` · `..` 를
/// 막지 않으니, 파일 이름에 쓰기 전에 `[A-Za-z0-9_-]` 밖의 글자를 `_` 로 바꾸고 48글자로 자른다. 바꾸거나 잘랐으면 원래 key 의
/// SHA-1 앞 8자를 붙인다 — 다른 key 가 같은 라벨(같은 로그 파일)로 겹치지 않게.
pub fn handoff_log_label(board_key: &str, number: i64) -> String {
    let safe: String = board_key
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(HANDOFF_LABEL_KEY_MAX)
        .collect();
    if safe == board_key {
        return format!("handoff-{safe}-{number}");
    }
    let digest = ring::digest::digest(
        &ring::digest::SHA1_FOR_LEGACY_USE_ONLY,
        board_key.as_bytes(),
    );
    let hash: String = digest
        .as_ref()
        .iter()
        .take(4)
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("handoff-{safe}-{hash}-{number}")
}

/// 받은편지함 소켓 경로의 pid — `/tmp/cc-socks/<pid>.sock`. 모양이 아니면 None.
pub fn socket_pid(socket: &str) -> Option<u32> {
    if !crate::peer_inbox::is_inbox_socket_path(socket) {
        return None;
    }
    socket
        .rsplit('/')
        .next()?
        .strip_suffix(".sock")?
        .parse()
        .ok()
}

/// 핸드오프 서버가 만든 세션의 받은편지함 등록 — **소켓 pid 의 부모가 그 서버**이고 서버를 띄운 뒤(`since`, 유닉스 초)에
/// 등록한 것. 소켓 이름은 pid 라 재사용되면 옛 세션의 등록이 같은 경로로 남아 있을 수 있다 — 새 세션이 SessionStart 에서
/// 등록하기 전(3~7초)에 그것을 고르지 않게 시각으로 거른다. cwd 는 보지 않는다(부모 pid 로 충분하고, 경로 표기가 갈리면
/// 놓친다). 여럿이면 마지막에 등록한 것.
pub fn handoff_session<'a>(
    registrations: &'a [crate::peer_inbox::InboxRegistration],
    rows: &[PsRow],
    server_pid: u32,
    since: i64,
) -> Option<&'a crate::peer_inbox::InboxRegistration> {
    let parent: HashMap<u32, u32> = rows.iter().map(|r| (r.pid, r.ppid)).collect();
    registrations
        .iter()
        .filter(|r| r.seen_at >= since)
        .filter(|r| {
            socket_pid(&r.socket).and_then(|pid| parent.get(&pid).copied()) == Some(server_pid)
        })
        .max_by_key(|r| r.seen_at)
}

/// `git symbolic-ref --short refs/remotes/origin/HEAD` 출력(`origin/main`) → 워크트리를 딸 기준. 비면 None — 그때 데몬은
/// `origin/main` · `origin/master` 순으로 짐작하고, 그것도 없으면 메인 체크아웃의 HEAD 에서 딴다.
pub fn worktree_base(origin_head: &str) -> Option<String> {
    let base = origin_head.trim();
    (base.len() > "origin/".len() && base.starts_with("origin/")).then(|| base.to_string())
}

/// 워크트리 브랜치 — Claude Code `--worktree <이름>` 과 같은 이름(`worktree-<이름>`)이라 예전 spawn 의 워크트리와 이어진다.
pub fn worktree_branch(worktree_name: &str) -> String {
    format!("worktree-{worktree_name}")
}

/// 데몬이 띄운 핸드오프 서버의 기록(`<todo dir>/rc/handoff/<label>.json`) — 현황이 대상 밖 서버에서 가려내고, 닫기가 pid 를
/// 고른다. 서버가 내려가면(닫기 · 사라짐) 지운다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HandoffServerRecord {
    /// 기동 로그 라벨(`handoff_log_label`) — 파일 이름이기도 하다.
    pub label: String,
    /// claude.ai 에 보이는 이름(`<보드>-<n>: <요약>`).
    pub name: String,
    pub pid: u32,
    pub dir: String,
    /// 할 일 참조(`rocky-41`) — 닫기를 이걸로도 부른다.
    pub todo_ref: String,
    /// RFC 3339.
    pub started_at: String,
}

/// 현황의 핸드오프 서버 한 줄.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HandoffServerRow {
    pub label: String,
    pub name: String,
    pub todo_ref: String,
    pub dir: String,
    pub pid: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uptime_secs: Option<u64>,
    pub sessions: usize,
}

/// 대상 밖 서버 중 기록과 **pid · 폴더가 둘 다 맞는** 것을 핸드오프 행으로 가린다 — 나머지는 그대로 대상 밖. pid 만 보면
/// 재사용된 pid 를 잡는다. 셋째 값은 떠 있지 않은 기록(지워도 되는 것)의 라벨 — 프로브가 실패했으면 호출자가 지우지 않는다.
pub fn split_handoffs(
    strays: Vec<StrayRow>,
    records: &[HandoffServerRecord],
) -> (Vec<HandoffServerRow>, Vec<StrayRow>, Vec<String>) {
    let mut handoffs = Vec::new();
    let mut rest = Vec::new();
    let mut seen = HashSet::new();
    for stray in strays {
        let record = records.iter().find(|r| handoff_is_live(r, &stray));
        match record {
            Some(r) => {
                seen.insert(r.label.clone());
                handoffs.push(HandoffServerRow {
                    label: r.label.clone(),
                    name: r.name.clone(),
                    todo_ref: r.todo_ref.clone(),
                    dir: stray.dir.clone(),
                    pid: stray.pid,
                    uptime_secs: stray.uptime_secs,
                    sessions: stray.sessions,
                });
            }
            None => rest.push(stray),
        }
    }
    let gone = records
        .iter()
        .filter(|r| !seen.contains(&r.label))
        .map(|r| r.label.clone())
        .collect();
    (handoffs, rest, gone)
}

/// 그 대상 밖 서버가 이 기록의 서버인가 — pid · 폴더가 **둘 다** 맞아야 한다(pid 만 보면 재사용된 pid 를 잡는다). 현황의
/// 가려내기와 닫기가 같은 판정을 쓴다.
pub fn handoff_is_live(record: &HandoffServerRecord, stray: &StrayRow) -> bool {
    record.pid == stray.pid && record.dir.trim_end_matches('/') == stray.dir
}

/// 닫을 핸드오프 서버를 고른다 — 라벨이나 할 일 참조(`rocky-41`, 대소문자 무시)로.
pub fn find_handoff<'a>(
    records: &'a [HandoffServerRecord],
    key: &str,
) -> Option<&'a HandoffServerRecord> {
    records
        .iter()
        .find(|r| r.label == key || r.todo_ref.eq_ignore_ascii_case(key))
}
