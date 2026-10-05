//! `claude rc`(Remote Control) 서버 현황 — 순수 판정. 프로브(`ps`·`lsof`·`pgrep`)는 `rockyd::rc` 가 돌리고
//! 여기는 그 출력을 읽어 합친다. 설계: `docs/design/specs/2026-10-05-rc-server-design.md`.
//!
//! 판정 규칙은 이 기능을 먼저 하던 별도 CLI 와 같다 — 그쪽에서 밟고 고친 함정을 테스트로 고정한다.

use std::collections::{BTreeMap, HashMap, HashSet};

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
    /// `rc` 블록이 있나 — 없으면 프로브를 돌리지 않고 나머지는 비어 있다.
    pub configured: bool,
    pub servers: Vec<ServerRow>,
    pub strays: Vec<StrayRow>,
    pub auth: AuthState,
    /// `agy` 가 없으면 None.
    pub antigravity: Option<AgyStatus>,
    /// 프로브 명령(`ps`·`lsof`)이 실패했으면 그 사유 — 이때 "꺼짐"은 모르는 것이다.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub probe_error: Option<String>,
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
