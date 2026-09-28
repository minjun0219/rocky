//! 사용 로그 — rocky 의 표면(REST 라우트 · MCP 도구 · CLI 명령 · 훅 · 웹 UI 이벤트)이
//! 실제로 얼마나 쓰이는지를 한 줄씩 남기고, 그걸로 "뺄 것 / 손볼 것" 을 판단한다.
//!
//! v0.23 의 도구 정리는 39개 레포의 워크로그를 손으로 뒤져 "0건" 을 셌다. 이 모듈은 그 셈을
//! 상시로 만든다. 내용(제목·본문)은 싣지 않는다 — 이름과 모양만이다.
//!
//! 저장은 `<dir>/YYYY-MM.jsonl`(월별 append-only). 읽기·집계는 순수 함수라 데몬 없이도
//! `rocky usage` 가 돈다.

use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Datelike, Local, Timelike, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UsageSource {
    /// 데몬 REST — 웹·TUI·CLI 가 다 지나간다.
    Rest,
    /// MCP 도구 — 보드 5개(데몬) + worklog 4개(CLI stdio).
    Mcp,
    /// `rocky <cmd>` 직접 실행.
    Cli,
    /// Claude Code 훅 엔트리.
    Hook,
    /// 웹 UI 에서 서버를 안 거치는 조작 — 이름 붙인 이벤트만.
    Web,
}

impl UsageSource {
    pub fn as_str(self) -> &'static str {
        match self {
            UsageSource::Rest => "rest",
            UsageSource::Mcp => "mcp",
            UsageSource::Cli => "cli",
            UsageSource::Hook => "hook",
            UsageSource::Web => "web",
        }
    }
}

/// 한 줄 — 무엇을(name) 누가(actor · client) 언제(ts) 얼마나(ms) 잘(ok) 썼나.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageEvent {
    pub ts: String,
    pub source: UsageSource,
    /// `GET /api/todos/:ref` · `todo_write` · `rocky board path` · `hook notify-todo` · `web:now-row`.
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<String>,
    /// `web` · `tui` · `cli` · `mcp` — 누가 불렀나(사람 이름이 아니라 표면).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client: Option<String>,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ms: Option<u64>,
    /// 작은 부가 정보(예: 훅이 컨텍스트를 주입했는가). 내용은 싣지 않는다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

impl UsageEvent {
    pub fn new(source: UsageSource, name: impl Into<String>, ok: bool) -> Self {
        UsageEvent {
            ts: now_iso(),
            source,
            name: name.into(),
            actor: None,
            client: None,
            ok,
            ms: None,
            meta: None,
        }
    }
}

pub fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// 기록하지 않는 라우트 — 1초마다 도는 것과 스트림, 그리고 로그 자신.
const SKIPPED_ROUTES: &[&str] = &[
    "/api/health",
    "/api/statusline",
    "/api/events",
    "/api/usage",
];

/// REST 이름 — `GET /api/todos/abc123` → `GET /api/todos/:ref`. 셋째 세그먼트(id·key)만
/// 접고 그 뒤 동작 이름(`handoff`·`archive`·`comments`)은 남긴다. 기록 안 할 라우트는 None.
pub fn normalize_route(method: &str, path: &str) -> Option<String> {
    let path = path.split('?').next().unwrap_or(path);
    if SKIPPED_ROUTES.contains(&path) {
        return None;
    }
    let mut segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if segs.len() >= 3 && segs[0] == "api" {
        segs[2] = ":ref";
    }
    Some(format!("{} /{}", method.to_uppercase(), segs.join("/")))
}

/// 클라이언트 종류 — `x-rocky-client` 헤더가 있으면 그것, 없으면 User-Agent 로 추정.
pub fn client_of(x_rocky_client: Option<&str>, user_agent: Option<&str>) -> String {
    if let Some(c) = x_rocky_client.map(str::trim).filter(|c| !c.is_empty()) {
        return c.to_string();
    }
    let ua = user_agent.unwrap_or("");
    if ua.contains("rocky-tui") {
        "tui".into()
    } else if ua.contains("rocky") || ua.starts_with("ureq") {
        "cli".into()
    } else if ua.contains("Mozilla") {
        "web".into()
    } else {
        "other".into()
    }
}

/// `<dir>/YYYY-MM.jsonl` — ts 의 앞 7자.
pub fn month_file(dir: &Path, ts: &str) -> PathBuf {
    let month = ts.get(..7).unwrap_or("unknown");
    dir.join(format!("{month}.jsonl"))
}

/// 한 줄 append. 디렉터리가 없으면 만든다. 실패는 호출자가 무시한다(로그가 본업을 막지 않는다).
pub fn append_event(dir: &Path, event: &UsageEvent) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let line = serde_json::to_string(event).map_err(std::io::Error::other)?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(month_file(dir, &event.ts))?;
    file.write_all(line.as_bytes())?;
    file.write_all(b"\n")
}

/// `since`(ISO) 이후의 이벤트 전부 — 그 달부터의 파일만 연다. 깨진 줄은 건너뛴다.
pub fn read_events(dir: &Path, since: &str) -> Vec<UsageEvent> {
    let since_month = since.get(..7).unwrap_or("");
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension().is_some_and(|x| x == "jsonl")
                && p.file_stem()
                    .and_then(|s| s.to_str())
                    .is_some_and(|stem| stem >= since_month)
        })
        .collect();
    files.sort();
    let mut out = Vec::new();
    for file in files {
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        for line in text.lines() {
            if let Ok(ev) = serde_json::from_str::<UsageEvent>(line) {
                if ev.ts.as_str() >= since {
                    out.push(ev);
                }
            }
        }
    }
    out
}

/// `30d` · `12h` · `2w` → 그만큼 전의 ISO. 숫자만 오면 일.
pub fn parse_since(spec: &str, now: DateTime<Utc>) -> Option<String> {
    let spec = spec.trim();
    let (num, unit) = match spec.chars().last() {
        Some(c) if c.is_ascii_alphabetic() => (&spec[..spec.len() - 1], c),
        _ => (spec, 'd'),
    };
    let n: i64 = num.parse().ok()?;
    let dur = match unit {
        'h' => chrono::Duration::hours(n),
        'd' => chrono::Duration::days(n),
        'w' => chrono::Duration::weeks(n),
        _ => return None,
    };
    Some((now - dur).to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}

/// 표면 전수 — "한 번도 안 쓰인 것" 을 세려면 무엇이 있는지 알아야 한다. CLI 는 `rocky <cmd>`
/// 접두사로 맞춘다(`rocky board path` 는 `rocky board` 에 속한다).
pub const KNOWN_SURFACES: &[(UsageSource, &str)] = &[
    (UsageSource::Mcp, "todo_list"),
    (UsageSource::Mcp, "todo_write"),
    (UsageSource::Mcp, "todo_status"),
    (UsageSource::Mcp, "note_list"),
    (UsageSource::Mcp, "note_write"),
    (UsageSource::Mcp, "worklog_append"),
    (UsageSource::Mcp, "worklog_read"),
    (UsageSource::Mcp, "worklog_search"),
    (UsageSource::Mcp, "worklog_status"),
    (UsageSource::Hook, "hook ensure-daemon"),
    (UsageSource::Hook, "hook notify-todo"),
    (UsageSource::Hook, "hook handoff-stop"),
    (UsageSource::Hook, "hook log-turn"),
    (UsageSource::Cli, "rocky ls"),
    (UsageSource::Cli, "rocky next"),
    (UsageSource::Cli, "rocky today"),
    (UsageSource::Cli, "rocky add"),
    (UsageSource::Cli, "rocky show"),
    (UsageSource::Cli, "rocky update"),
    (UsageSource::Cli, "rocky comment"),
    (UsageSource::Cli, "rocky issue"),
    (UsageSource::Cli, "rocky handoff"),
    (UsageSource::Cli, "rocky spawn"),
    (UsageSource::Cli, "rocky sessions"),
    (UsageSource::Cli, "rocky move"),
    (UsageSource::Cli, "rocky start"),
    (UsageSource::Cli, "rocky stop"),
    (UsageSource::Cli, "rocky done"),
    (UsageSource::Cli, "rocky reopen"),
    (UsageSource::Cli, "rocky archive"),
    (UsageSource::Cli, "rocky unarchive"),
    (UsageSource::Cli, "rocky section"),
    (UsageSource::Cli, "rocky note"),
    (UsageSource::Cli, "rocky history"),
    (UsageSource::Cli, "rocky board"),
    (UsageSource::Cli, "rocky tui"),
    (UsageSource::Cli, "rocky open"),
    (UsageSource::Cli, "rocky daemon"),
    (UsageSource::Cli, "rocky mcp"),
    (UsageSource::Cli, "rocky tailscale"),
    (UsageSource::Cli, "rocky config"),
    (UsageSource::Cli, "rocky usage"),
    (UsageSource::Web, "web:now-row"),
    (UsageSource::Web, "web:board-tab"),
    (UsageSource::Web, "web:todo-open"),
    (UsageSource::Web, "web:notes-toggle"),
    (UsageSource::Web, "web:theme"),
    (UsageSource::Web, "web:archived-toggle"),
    (UsageSource::Web, "web:quick-add"),
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SurfaceStat {
    pub source: UsageSource,
    pub name: String,
    pub count: u64,
    pub errors: u64,
    pub last_ts: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p50_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p95_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageReport {
    pub since: String,
    pub until: String,
    pub total: u64,
    /// 많이 쓴 순.
    pub surfaces: Vec<SurfaceStat>,
    /// 알려진 표면 중 기간 안에 한 번도 안 나온 것.
    pub unused: Vec<(UsageSource, String)>,
    /// `YYYY-MM-DD` → 건수, 날짜순.
    pub by_day: Vec<(String, u64)>,
    /// 현지 시각 0~23시.
    pub by_hour: Vec<u64>,
    /// client → 건수, 많이 쓴 순.
    pub clients: Vec<(String, u64)>,
}

fn percentile(sorted: &[u64], p: f64) -> Option<u64> {
    if sorted.is_empty() {
        return None;
    }
    let idx = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted.get(idx).copied()
}

/// 집계. `known` 은 보통 `KNOWN_SURFACES`.
pub fn build_report(
    events: &[UsageEvent],
    since: &str,
    until: &str,
    known: &[(UsageSource, &str)],
) -> UsageReport {
    #[derive(Default)]
    struct Acc {
        count: u64,
        errors: u64,
        last_ts: String,
        ms: Vec<u64>,
    }
    let mut by_surface: HashMap<(UsageSource, String), Acc> = HashMap::new();
    let mut by_day: BTreeMap<String, u64> = BTreeMap::new();
    let mut by_hour = vec![0u64; 24];
    let mut clients: HashMap<String, u64> = HashMap::new();
    for ev in events {
        let entry = by_surface.entry((ev.source, ev.name.clone())).or_default();
        entry.count += 1;
        if !ev.ok {
            entry.errors += 1;
        }
        if ev.ts > entry.last_ts {
            entry.last_ts = ev.ts.clone();
        }
        if let Some(ms) = ev.ms {
            entry.ms.push(ms);
        }
        if let Some(day) = ev.ts.get(..10) {
            *by_day.entry(day.to_string()).or_default() += 1;
        }
        if let Ok(t) = DateTime::parse_from_rfc3339(&ev.ts) {
            let hour = t.with_timezone(&Local).hour() as usize;
            by_hour[hour] += 1;
        }
        if let Some(c) = &ev.client {
            *clients.entry(c.clone()).or_default() += 1;
        }
    }
    let mut surfaces: Vec<SurfaceStat> = by_surface
        .into_iter()
        .map(|((source, name), mut acc)| {
            acc.ms.sort_unstable();
            SurfaceStat {
                source,
                name,
                count: acc.count,
                errors: acc.errors,
                last_ts: acc.last_ts,
                p50_ms: percentile(&acc.ms, 0.5),
                p95_ms: percentile(&acc.ms, 0.95),
            }
        })
        .collect();
    surfaces.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
    let unused: Vec<(UsageSource, String)> = known
        .iter()
        .filter(|(source, name)| {
            !surfaces.iter().any(|s| {
                s.source == *source
                    && (s.name == *name
                        || (*source == UsageSource::Cli && s.name.starts_with(&format!("{name} "))))
            })
        })
        .map(|(source, name)| (*source, (*name).to_string()))
        .collect();
    let mut clients: Vec<(String, u64)> = clients.into_iter().collect();
    clients.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    UsageReport {
        since: since.to_string(),
        until: until.to_string(),
        total: events.len() as u64,
        surfaces,
        unused,
        by_day: by_day.into_iter().collect(),
        by_hour,
        clients,
    }
}

/// 사람이 읽는 보고 — 많이 쓴 표면, 에러, 안 쓴 표면, 시간 분포.
pub fn render_report(r: &UsageReport) -> String {
    let mut lines: Vec<String> = Vec::new();
    lines.push(format!(
        "rocky 사용 로그 — {} ~ {} · {}건",
        r.since.get(..10).unwrap_or(&r.since),
        r.until.get(..10).unwrap_or(&r.until),
        r.total
    ));
    if r.total == 0 {
        lines.push("  기록 없음 — 데몬·CLI 가 이 버전으로 돈 뒤부터 쌓인다.".into());
        return lines.join("\n");
    }
    if !r.clients.is_empty() {
        lines.push(format!(
            "  클라이언트: {}",
            r.clients
                .iter()
                .map(|(c, n)| format!("{c} {n}"))
                .collect::<Vec<_>>()
                .join(" · ")
        ));
    }
    lines.push(String::new());
    lines.push("많이 쓴 표면".into());
    for s in r.surfaces.iter().take(20) {
        let err = if s.errors > 0 {
            format!("  ✗{}", s.errors)
        } else {
            String::new()
        };
        let lat = match (s.p50_ms, s.p95_ms) {
            (Some(a), Some(b)) => format!("  {a}/{b}ms"),
            _ => String::new(),
        };
        lines.push(format!(
            "  {:>6}  {:<5} {}{}{}  (마지막 {})",
            s.count,
            s.source.as_str(),
            s.name,
            err,
            lat,
            s.last_ts.get(..10).unwrap_or(&s.last_ts)
        ));
    }
    if r.surfaces.len() > 20 {
        lines.push(format!("  … {}개 더 (--json)", r.surfaces.len() - 20));
    }
    if !r.unused.is_empty() {
        lines.push(String::new());
        lines.push(format!("안 쓴 표면 {}개", r.unused.len()));
        let mut by_source: BTreeMap<UsageSource, Vec<&str>> = BTreeMap::new();
        for (source, name) in &r.unused {
            by_source.entry(*source).or_default().push(name);
        }
        for (source, names) in by_source {
            lines.push(format!("  {:<5} {}", source.as_str(), names.join(" · ")));
        }
    }
    lines.push(String::new());
    let busiest = r
        .by_hour
        .iter()
        .enumerate()
        .max_by_key(|(_, n)| **n)
        .map(|(h, n)| format!("{h:02}시({n}건)"))
        .unwrap_or_default();
    let days = r.by_day.len();
    lines.push(format!(
        "활동 {days}일 · 하루 최대 {}건 · 가장 바쁜 시각 {busiest}",
        r.by_day.iter().map(|(_, n)| *n).max().unwrap_or(0)
    ));
    lines.join("\n")
}

/// 현지 오늘 날짜 — 보고 머리용.
pub fn today_local() -> String {
    let d = Local::now();
    format!("{:04}-{:02}-{:02}", d.year(), d.month(), d.day())
}
