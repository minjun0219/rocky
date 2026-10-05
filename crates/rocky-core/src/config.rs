//! 런타임 설정 해석 — TS 원본 `src/config.ts` + `src/rocky-config.ts`.
//!
//! 우선순위: env (`ROCKY_TODO_*`) > user `rocky.json` 의 `todo` 블록 > 기본값.
//! 데몬은 시스템 전역 단일 인스턴스라 project rocky.json 은 보지 않는다.

use std::path::{Path, PathBuf};

use crate::statusline::DEFAULT_STATUSLINE_TEMPLATE;

/// 기본 포트 — 키패드로 "todo" (8636).
pub const DEFAULT_TODO_PORT: u16 = 8636;

/// 노출 채널 — 빈 배열(기본)이면 루프백만.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExposeChannel {
    Lan,
    TailscaleServe,
}

impl ExposeChannel {
    /// 설정 파일에 적는 이름 — `parse` 의 역.
    pub fn as_str(&self) -> &'static str {
        match self {
            ExposeChannel::Lan => "lan",
            ExposeChannel::TailscaleServe => "tailscale-serve",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "lan" => Some(ExposeChannel::Lan),
            "tailscale-serve" => Some(ExposeChannel::TailscaleServe),
            _ => None,
        }
    }
}

/// user rocky.json 의 `todo` 블록 (경량 파싱 — todo 블록만, `enabled` 미read).
#[derive(Debug, Clone, Default)]
pub struct TodoConfig {
    pub port: Option<u16>,
    pub dir: Option<String>,
    /// 문자열 하나("lan")도 허용 — 배열로 정규화. "off"/null 은 미설정과 동일.
    pub expose: Option<ExposeValue>,
    pub watch: Option<bool>,
    pub statusline_template: Option<String>,
    /// 수집함 어댑터 — 모양이 틀린 항목은 건너뛴다(다른 필드와 같은 fail-open).
    pub inbox: Vec<InboxSource>,
    /// 보드 설정 화면에서 쓰는 수집함 어댑터(`todo.inboxAdapters[]`) — 그 자체로는 돌지 않고, 보드마다
    /// 등록한 값(`BoardInboxSource`)을 붙여 실행된다. 모양은 `inbox[]` 와 같다.
    pub inbox_adapters: Vec<InboxSource>,
    /// SessionStart 훅이 보드 요약 몇 줄을 세션 컨텍스트에 넣는가. 기본 true.
    pub session_summary: Option<bool>,
}

/// `todo.inbox[]` 항목 — 외부 투두 앱을 읽는 **명령** 하나. 규약은 `crate::inbox`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboxSource {
    /// `[a-z0-9-]+` — 응답의 소스 이름이자 올린 항목의 링크 제목 접두사.
    pub name: String,
    /// argv 배열. 셸을 거치지 않는다.
    pub command: Vec<String>,
    /// 기본 `inbox::DEFAULT_INBOX_TIMEOUT_MS`.
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone)]
pub enum ExposeValue {
    Off,
    Channels(Vec<ExposeChannel>),
}

#[derive(Debug, Clone)]
pub struct TodoRuntimeConfig {
    pub port: u16,
    pub dir: PathBuf,
    /// 바인딩 호스트 — expose 에서 유도 (lan 포함 → 0.0.0.0, 아니면 127.0.0.1).
    pub host: String,
    pub expose: Vec<ExposeChannel>,
    /// 항상 채워진다(기본값 폴백).
    pub statusline_template: String,
    /// env 오버라이드 없음 — 설정 파일에서만 온다.
    pub inbox: Vec<InboxSource>,
    /// 보드 설정 화면용 어댑터(`todo.inboxAdapters[]`). 설정 파일에서만 온다.
    pub inbox_adapters: Vec<InboxSource>,
}

/// launchd 상주 job 라벨 — CLI 의 plist 와 데몬의 "launchd 가 띄웠나" 판정이 같은 값을 본다.
pub const LAUNCHD_LABEL: &str = "com.rocky.daemon";

/// 실제로 쓰는 라벨 — `ROCKY_LAUNCHD_LABEL` 이 있으면 그것(개발용: 실제 상주 job 을 건드리지 않고
/// 다른 라벨·포트로 launchd 동작을 재현할 때), 없으면 `LAUNCHD_LABEL`. 개발용 라벨은 **전용
/// `ROCKY_CONFIG` 가 같이 있을 때만** 쓴다 — 라벨만 바꾸고 설정을 빠뜨리면 개발용 job 이 실제 포트·DB 를
/// 쥐고, 교체 확인이 실제 상주 데몬을 "launchd 밖의 고아" 로 보고 내린다.
pub fn launchd_label() -> String {
    let config_set = std::env::var("ROCKY_CONFIG").is_ok_and(|c| !c.trim().is_empty());
    std::env::var("ROCKY_LAUNCHD_LABEL")
        .ok()
        .filter(|l| config_set && !l.trim().is_empty())
        .unwrap_or_else(|| LAUNCHD_LABEL.to_string())
}

/// 이 프로세스를 그 라벨의 launchd job 이 띄웠나 — launchd 는 job 의 환경에 `XPC_SERVICE_NAME=<라벨>` 을
/// 넣고(셸에서 띄운 프로세스는 `0` 이거나 없다) 부모는 launchd(pid 1)다. 부모까지 보는 이유: launchd 데몬이
/// 띄운 자식(세션·어댑터)은 이 변수를 물려받아, 그 아래에서 따로 뜬 데몬이 자기를 launchd 의 것으로 착각한다.
pub fn launched_by_launchd(xpc_service_name: Option<&str>, label: &str, parent_pid: u32) -> bool {
    xpc_service_name == Some(label) && parent_pid == 1
}

/// `~/...` 를 홈으로 확장한다.
pub fn expand_tilde(input: &str) -> PathBuf {
    let home = || std::env::var("HOME").map(PathBuf::from).unwrap_or_default();
    if input == "~" {
        return home();
    }
    if let Some(rest) = input.strip_prefix("~/") {
        return home().join(rest);
    }
    PathBuf::from(input)
}

/// user-level config 기본 경로 — `~/.config/rocky/rocky.json`. env `ROCKY_CONFIG` 우선.
pub fn user_config_path() -> PathBuf {
    if let Ok(p) = std::env::var("ROCKY_CONFIG") {
        return PathBuf::from(p);
    }
    expand_tilde("~/.config/rocky/rocky.json")
}

/// user rocky.json 의 `todo` 블록만 읽는다. 파일 없음/파싱 실패/모양 오류는 전부
/// 기본값(fail-open — 데몬은 기본값으로 뜬다).
pub fn load_todo_config(config_path: &Path) -> TodoConfig {
    let Ok(raw) = std::fs::read_to_string(config_path) else {
        return TodoConfig::default();
    };
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return TodoConfig::default();
    };
    let Some(todo) = parsed.get("todo").and_then(|v| v.as_object()) else {
        return TodoConfig::default();
    };
    let mut out = TodoConfig::default();
    if let Some(port) = todo.get("port").and_then(|v| v.as_u64()) {
        out.port = u16::try_from(port).ok();
    }
    if let Some(dir) = todo.get("dir").and_then(|v| v.as_str()) {
        out.dir = Some(dir.to_string());
    }
    if let Some(expose) = todo.get("expose") {
        out.expose = parse_expose_value(expose);
    }
    if let Some(watch) = todo.get("watch").and_then(|v| v.as_bool()) {
        out.watch = Some(watch);
    }
    // 모양이 어긋나면 통째로 무시 — 다른 필드와 같은 fail-open 규칙.
    if let Some(template) = todo
        .get("statusline")
        .and_then(|v| v.as_object())
        .and_then(|s| s.get("template"))
        .and_then(|v| v.as_str())
    {
        out.statusline_template = Some(template.to_string());
    }
    if let Some(inbox) = todo.get("inbox").and_then(|v| v.as_array()) {
        out.inbox = inbox.iter().filter_map(parse_inbox_source).collect();
    }
    if let Some(adapters) = todo.get("inboxAdapters").and_then(|v| v.as_array()) {
        out.inbox_adapters = adapters.iter().filter_map(parse_inbox_source).collect();
    }
    if let Some(flag) = todo.get("sessionSummary").and_then(|v| v.as_bool()) {
        out.session_summary = Some(flag);
    }
    out
}

/// 소스 이름 규칙 — 보드 key 와 같은 `[a-z0-9-]+`. 링크 제목·응답 키에 그대로 들어간다.
fn is_inbox_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn parse_inbox_source(value: &serde_json::Value) -> Option<InboxSource> {
    let obj = value.as_object()?;
    let name = obj.get("name")?.as_str()?.to_string();
    if !is_inbox_name(&name) {
        return None;
    }
    let command: Vec<String> = obj
        .get("command")?
        .as_array()?
        .iter()
        .map(|v| v.as_str().map(str::to_string))
        .collect::<Option<Vec<_>>>()?;
    if command.is_empty() {
        return None;
    }
    let timeout_ms = obj
        .get("timeoutMs")
        .and_then(|v| v.as_u64())
        .filter(|ms| *ms > 0);
    Some(InboxSource {
        name,
        command,
        timeout_ms,
    })
}

fn parse_expose_value(value: &serde_json::Value) -> Option<ExposeValue> {
    match value {
        serde_json::Value::Null => Some(ExposeValue::Off),
        serde_json::Value::String(s) => {
            if s == "off" {
                Some(ExposeValue::Off)
            } else {
                ExposeChannel::parse(s).map(|c| ExposeValue::Channels(vec![c]))
            }
        }
        serde_json::Value::Array(items) => Some(ExposeValue::Channels(
            items
                .iter()
                .filter_map(|v| v.as_str())
                .filter_map(ExposeChannel::parse)
                .collect(),
        )),
        _ => None,
    }
}

/// env 스냅샷 — 테스트 주입용.
pub type EnvMap = std::collections::HashMap<String, String>;

pub fn env_snapshot() -> EnvMap {
    std::env::vars().collect()
}

fn parse_port(raw: Option<&String>) -> Option<u16> {
    raw?.trim().parse::<u16>().ok().filter(|p| *p >= 1)
}

/// env > config > 기본값. TS `resolveTodoRuntimeConfig` 와 동일 규칙.
pub fn resolve_runtime_config(env: &EnvMap, todo: &TodoConfig) -> TodoRuntimeConfig {
    let port = parse_port(env.get("ROCKY_TODO_PORT"))
        .or(todo.port)
        .unwrap_or(DEFAULT_TODO_PORT);
    let raw_dir = env
        .get("ROCKY_TODO_DIR")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| todo.dir.clone())
        .unwrap_or_else(|| "~/.config/rocky/todo".to_string());
    // env 가 설정돼 있으면 (유효 채널이 없어도) config 를 통째로 덮어쓴다 — "off" 강제 차단 가능.
    let expose: Vec<ExposeChannel> = match env.get("ROCKY_TODO_EXPOSE") {
        Some(raw) => raw
            .split(',')
            .map(|t| t.trim().to_lowercase())
            .filter_map(|t| ExposeChannel::parse(&t))
            .collect(),
        None => match &todo.expose {
            None | Some(ExposeValue::Off) => Vec::new(),
            Some(ExposeValue::Channels(channels)) => channels.clone(),
        },
    };
    let host = if expose.contains(&ExposeChannel::Lan) {
        "0.0.0.0"
    } else {
        "127.0.0.1"
    };
    // 빈 문자열은 "출력 안 함" 이 아니라 오설정 — 기본 템플릿으로 폴백한다.
    let statusline_template = env
        .get("ROCKY_TODO_STATUSLINE")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            todo.statusline_template
                .as_ref()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| DEFAULT_STATUSLINE_TEMPLATE.to_string());

    TodoRuntimeConfig {
        port,
        dir: expand_tilde(&raw_dir),
        host: host.to_string(),
        expose,
        statusline_template,
        inbox: todo.inbox.clone(),
        inbox_adapters: todo.inbox_adapters.clone(),
    }
}

/// `rocky.json` 의 `worklog` 블록 — TS `rocky-config.ts` 의 `WorklogConfig`.
/// user(`~/.config/rocky/rocky.json`) 위에 project(`<cwd>/rocky.json`)가 필드 단위로 덮인다.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorklogConfig {
    /// 저널 JSONL 디렉터리. 미지정 시 프로젝트별 기본 경로.
    pub dir: Option<String>,
    /// Stop 훅 자동 기록 on/off. 기본 true. env `ROCKY_WORKLOG_AUTO_CAPTURE` 우선.
    pub auto_capture: Option<bool>,
    /// turn 엔트리 req/did 최대 글자 수. 기본 800.
    pub capture_max_chars: Option<usize>,
    /// `/rocky:recall` 의 Haiku↔Sonnet 임계. 기본 40.
    pub digest_threshold: Option<usize>,
}

impl WorklogConfig {
    /// 필드 단위 덮어쓰기 — `other` 에 있는 값만 이긴다.
    pub fn merged_with(&self, other: &WorklogConfig) -> WorklogConfig {
        WorklogConfig {
            dir: other.dir.clone().or_else(|| self.dir.clone()),
            auto_capture: other.auto_capture.or(self.auto_capture),
            capture_max_chars: other.capture_max_chars.or(self.capture_max_chars),
            digest_threshold: other.digest_threshold.or(self.digest_threshold),
        }
    }
}

/// 한 파일의 `worklog` 블록. 파일 없음 / 파싱 실패 / 블록 없음은 기본값(fail-open).
pub fn load_worklog_block(config_path: &Path) -> WorklogConfig {
    let Ok(raw) = std::fs::read_to_string(config_path) else {
        return WorklogConfig::default();
    };
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return WorklogConfig::default();
    };
    let Some(block) = parsed.get("worklog").and_then(|v| v.as_object()) else {
        return WorklogConfig::default();
    };
    let positive = |key: &str| {
        block
            .get(key)
            .and_then(|v| v.as_u64())
            .filter(|n| *n >= 1)
            .and_then(|n| usize::try_from(n).ok())
    };
    WorklogConfig {
        dir: block
            .get("dir")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        auto_capture: block.get("autoCapture").and_then(|v| v.as_bool()),
        capture_max_chars: positive("captureMaxChars"),
        digest_threshold: positive("digestThreshold"),
    }
}

/// user + project 를 병합한 `worklog` 설정. project 는 `<project_root>/rocky.json`.
pub fn load_worklog_config(user_path: &Path, project_root: &Path) -> WorklogConfig {
    let user = load_worklog_block(user_path);
    let project = load_worklog_block(&project_root.join("rocky.json"));
    user.merged_with(&project)
}

/// `rocky.json` 의 `pr` 블록 — PR 감시(`rocky_core::prwatch`). 기본: repo 가 설정된 보드가 있으면
/// 켜짐, 3분 간격, macOS 알림 켬.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PrWatchConfig {
    pub enabled: Option<bool>,
    pub interval_minutes: Option<u64>,
    pub notify: Option<bool>,
    /// 알림 브릿지 — ready·conflict 마다 실행하는 명령들(`pr.notifiers[]`). 환경마다 고른다
    /// (텔레그램·ntfy·…은 `bridges/<name>/`). `notify`(macOS 배너)와 독립.
    pub notifiers: Vec<CommandBridge>,
    /// 세션 알림 — ready·conflict 를 그 레포 보드에서 일하는 Claude Code 세션의 받은편지함 소켓에
    /// 밀어 넣는다(`rocky_core::peer_inbox`). 기본 켬.
    pub session_notify: Option<bool>,
}

/// 명령 하나로 된 브릿지 — `todo.inbox[]`(읽기)와 `pr.notifiers[]`(알림)가 같은 모양이다.
/// 셸을 거치지 않고 argv 그대로 실행하며, 토큰은 명령이 스스로 읽는다(`op read`).
pub type CommandBridge = InboxSource;

impl PrWatchConfig {
    pub const DEFAULT_INTERVAL_MINUTES: u64 = 3;

    pub fn interval_minutes(&self) -> u64 {
        self.interval_minutes
            .filter(|m| *m > 0)
            .unwrap_or(Self::DEFAULT_INTERVAL_MINUTES)
    }
}

/// 파일 없음 / 파싱 실패 / 블록 없음은 기본값(fail-open).
pub fn load_pr_block(config_path: &Path) -> PrWatchConfig {
    let Ok(raw) = std::fs::read_to_string(config_path) else {
        return PrWatchConfig::default();
    };
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return PrWatchConfig::default();
    };
    let Some(block) = parsed.get("pr").and_then(|v| v.as_object()) else {
        return PrWatchConfig::default();
    };
    PrWatchConfig {
        enabled: block.get("enabled").and_then(|v| v.as_bool()),
        interval_minutes: block.get("intervalMinutes").and_then(|v| v.as_u64()),
        notify: block.get("notify").and_then(|v| v.as_bool()),
        notifiers: block
            .get("notifiers")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(parse_inbox_source).collect())
            .unwrap_or_default(),
        session_notify: block.get("sessionNotify").and_then(|v| v.as_bool()),
    }
}

/// `rocky.json` 의 `usage` 블록 — 사용 로그(`rocky_core::usage`). 기본 켜짐, `~/.config/rocky/usage`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UsageConfig {
    pub dir: Option<String>,
    pub enabled: Option<bool>,
}

/// 파일 없음 / 파싱 실패 / 블록 없음은 기본값(fail-open).
pub fn load_usage_block(config_path: &Path) -> UsageConfig {
    let Ok(raw) = std::fs::read_to_string(config_path) else {
        return UsageConfig::default();
    };
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return UsageConfig::default();
    };
    let Some(block) = parsed.get("usage").and_then(|v| v.as_object()) else {
        return UsageConfig::default();
    };
    UsageConfig {
        dir: block
            .get("dir")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        enabled: block.get("enabled").and_then(|v| v.as_bool()),
    }
}

/// `rocky.json` 의 `tokens` 블록 — Claude Code 토큰 색인(`rocky_core::tokens`). 기본 켜짐.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TokensConfig {
    pub enabled: Option<bool>,
    /// 트랜스크립트 루트. 기본 `$CLAUDE_CONFIG_DIR/projects`, 없으면 `~/.claude/projects`.
    pub dir: Option<String>,
    /// 추천 규칙(`tokens.recommend`) — 빠진 칸은 기본값.
    pub recommend: crate::tokens::RecommendConfig,
}

/// 파일 없음 / 파싱 실패 / 블록 없음은 기본값(fail-open).
pub fn load_tokens_block(config_path: &Path) -> TokensConfig {
    let Ok(raw) = std::fs::read_to_string(config_path) else {
        return TokensConfig::default();
    };
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return TokensConfig::default();
    };
    let Some(block) = parsed.get("tokens").and_then(|v| v.as_object()) else {
        return TokensConfig::default();
    };
    let mut recommend = crate::tokens::RecommendConfig::default();
    if let Some(r) = block.get("recommend").and_then(|v| v.as_object()) {
        let num = |k: &str| r.get(k).and_then(|v| v.as_u64()).filter(|n| *n > 0);
        let flag = |k: &str, d: bool| r.get(k).and_then(|v| v.as_bool()).unwrap_or(d);
        recommend.window = num("window").map_or(recommend.window, |n| n as usize);
        recommend.min_turns = num("minTurns").map_or(recommend.min_turns, |n| n as usize);
        recommend.low_output_tokens = num("lowOutputTokens").unwrap_or(recommend.low_output_tokens);
        recommend.lower_effort = flag("lowerEffort", recommend.lower_effort);
        recommend.hold_after_raise = flag("holdAfterRaise", recommend.hold_after_raise);
        recommend.switch_to_sonnet = flag("switchToSonnet", recommend.switch_to_sonnet);
    }
    TokensConfig {
        enabled: block.get("enabled").and_then(|v| v.as_bool()),
        dir: block
            .get("dir")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        recommend,
    }
}

/// 트랜스크립트 루트 — None 이면 끔. `tokens.dir` > `$CLAUDE_CONFIG_DIR/projects` > `~/.claude/projects`.
pub fn resolve_transcripts_dir(env: &EnvMap, tokens: &TokensConfig) -> Option<PathBuf> {
    if tokens.enabled == Some(false) {
        return None;
    }
    if let Some(dir) = &tokens.dir {
        return Some(expand_tilde(dir));
    }
    let base = env
        .get("CLAUDE_CONFIG_DIR")
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(expand_tilde)
        .unwrap_or_else(|| expand_tilde("~/.claude"));
    Some(base.join("projects"))
}

/// 사용 로그 디렉터리 — None 이면 끔. env `ROCKY_USAGE`(0/false/off/no 로 끔) ·
/// `ROCKY_USAGE_DIR` 이 설정 파일보다 우선. 기본 `~/.config/rocky/usage`.
pub fn resolve_usage_dir(env: &EnvMap, usage: &UsageConfig) -> Option<PathBuf> {
    if let Some(flag) = env.get("ROCKY_USAGE") {
        let value = flag.trim().to_lowercase();
        if matches!(value.as_str(), "0" | "false" | "off" | "no") {
            return None;
        }
    } else if usage.enabled == Some(false) {
        return None;
    }
    let raw = env
        .get("ROCKY_USAGE_DIR")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| usage.dir.clone())
        .unwrap_or_else(|| "~/.config/rocky/usage".to_string());
    Some(expand_tilde(&raw))
}
