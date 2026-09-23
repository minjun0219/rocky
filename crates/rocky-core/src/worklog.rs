//! Append-only 에이전트 저널 — 기록(記錄) 레이어. TS 원본 `src/core/worklog.ts`.
//!
//! 한 turn 안에서 결정 / blocker / 사용자 답변 등 "다음 turn 에 인용하고 싶은 사실"을
//! 디스크에 한 줄(JSONL)씩 쌓는다. 저널은 그 자체가 source of truth 라 만료·무효화·
//! 덮어쓰기가 없다. 읽어서 지식으로 증류(整理)하는 것은 호스트 LLM(`/rocky:recall`)의 몫.
//!
//! 디스크 레이아웃: `<dir>/worklog.jsonl`, 각 줄이 하나의 [`WorklogEntry`].
//! `dir` 기본값은 `~/.config/rocky/worklog/<project-key>` — 프로젝트별 격리. 키는
//! 레포 루트 basename + 그 절대경로 sha1 앞 8자 ([`default_project_key`]) 이고, **TS 판과
//! 바이트 단위로 같아야** 기존 앵커 디렉터리가 그대로 이어진다. `ROCKY_WORKLOG_DIR` 로
//! 통째로 덮어쓴다.
//!
//! 동시 쓰기: append 는 O_APPEND 지만 라인 단위 비-interleaving 을 가정하지 않는다. 직전
//! 프로세스가 mid-write 로 죽어 마지막 줄이 `\n` 없이 끝나 있으면 append 가 leading `\n`
//! 을 붙이고, read 는 파싱되지 않는 줄을 graceful skip 한다.

use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use rand::RngCore;
use serde::{Deserialize, Serialize};

use crate::config::expand_tilde;

/// 저널 파일 이름 — 한 디렉터리에 단일 파일을 둔다.
pub const WORKLOG_FILE: &str = "worklog.jsonl";

const DEFAULT_LIMIT: usize = 20;

/// `pageId` 필드 정규화 — 입력(Notion page id 또는 URL)에서 32자 hex 를 뽑아 8-4-4-4-12
/// 소문자로 맞춘다. 기존 엔트리의 `pageId` 형태와 같아야 `read { pageId }` 필터가 과거
/// 기록에도 맞는다.
pub fn normalize_page_id(input: &str) -> Result<String, String> {
    let trimmed = input.trim();
    let raw = find_dashed_hex(trimmed)
        .map(|s| s.replace('-', ""))
        .or_else(|| find_hex_run(trimmed, 32));
    let Some(raw) = raw.filter(|r| r.len() == 32) else {
        return Err(format!(
            "worklog: cannot extract a Notion page id from pageId \"{input}\""
        ));
    };
    let lower = raw.to_ascii_lowercase();
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &lower[0..8],
        &lower[8..12],
        &lower[12..16],
        &lower[16..20],
        &lower[20..]
    ))
}

/// `8-4-4-4-12` 꼴 hex 첫 매치.
fn find_dashed_hex(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let groups = [8usize, 4, 4, 4, 12];
    let total = 36;
    if bytes.len() < total {
        return None;
    }
    'outer: for start in 0..=(bytes.len() - total) {
        let mut i = start;
        for (gi, len) in groups.iter().enumerate() {
            for _ in 0..*len {
                if !bytes[i].is_ascii_hexdigit() {
                    continue 'outer;
                }
                i += 1;
            }
            if gi < groups.len() - 1 {
                if bytes[i] != b'-' {
                    continue 'outer;
                }
                i += 1;
            }
        }
        return Some(s[start..start + total].to_string());
    }
    None
}

/// 연속 hex `n` 자 첫 매치 (JS `/[0-9a-fA-F]{32}/` 대응 — 더 긴 런의 앞 32자).
fn find_hex_run(s: &str, n: usize) -> Option<String> {
    let bytes = s.as_bytes();
    if bytes.len() < n {
        return None;
    }
    (0..=(bytes.len() - n))
        .find(|&start| bytes[start..start + n].iter().all(u8::is_ascii_hexdigit))
        .map(|start| s[start..start + n].to_string())
}

/// 프로젝트별 저널 디렉터리의 부모. `ROCKY_WORKLOG_DIR` 로 통째로 덮어쓴다.
pub fn default_worklog_root() -> PathBuf {
    expand_tilde("~/.config/rocky/worklog")
}

/// `git rev-parse --git-common-dir` 실행기 — 테스트가 갈아끼운다. 레포가 아니면 `None`.
pub type GitCommonDir<'a> = &'a dyn Fn(&Path) -> Option<String>;

/// 실제 `git` 을 호출하는 기본 구현. git 이 없거나 레포가 아니면 `None` — 어떤 경우에도
/// 실패하지 않는다(워크로그 기록이 git 유무로 죽으면 안 된다).
pub fn git_common_dir(cwd: &Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--git-common-dir"])
        .current_dir(cwd)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

/// cwd 가 속한 git 레포의 **주 워크트리 루트**. git 레포가 아니면 cwd 자체.
///
/// `--git-common-dir` 는 linked worktree 안에서도 주 워크트리의 `.git` 을 가리켜 worktree
/// 와 원본이 같은 루트로 접힌다. `--separate-git-dir` / bare 처럼 이름이 `.git` 이 아니면
/// common dir 자체를 식별자로 쓴다.
pub fn resolve_repo_root(cwd: &Path, git: GitCommonDir<'_>) -> PathBuf {
    let Some(common) = git(cwd) else {
        return canonical(cwd);
    };
    let abs = cwd.join(common);
    let is_dot_git = abs.file_name().is_some_and(|n| n == ".git");
    let target = if is_dot_git {
        abs.parent().map(Path::to_path_buf).unwrap_or(abs)
    } else {
        abs
    };
    canonical(&target)
}

/// 심볼릭 링크를 푼 절대 경로. cwd 는 realpath 가 아닐 수 있어(macOS `/tmp` →
/// `/private/tmp`) 정규화하지 않으면 같은 레포가 다른 해시로 갈린다. 경로가 아직 없으면
/// 절대화만 한다.
fn canonical(p: &Path) -> PathBuf {
    let abs = std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
    std::fs::canonicalize(&abs).unwrap_or(abs)
}

/// 프로젝트 키 `<sanitized-basename>-<sha1(레포 루트 절대경로) 앞 8자>`.
/// 기준은 cwd 가 아니라 레포 루트 — worktree 에서 작업해도 원본과 같은 워크로그에 쌓인다.
pub fn default_project_key(cwd: &Path, git: GitCommonDir<'_>) -> String {
    let root = resolve_repo_root(cwd, git);
    let base = sanitize_basename(
        &root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default(),
    );
    let digest = ring::digest::digest(
        &ring::digest::SHA1_FOR_LEGACY_USE_ONLY,
        root.to_string_lossy().as_bytes(),
    );
    let hash: String = digest
        .as_ref()
        .iter()
        .take(4)
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("{base}-{hash}")
}

/// TS `basename.replace(/[^a-zA-Z0-9_-]+/g, '-').replace(/^-+|-+$/g, '') || 'project'`.
fn sanitize_basename(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut in_run = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            out.push(ch);
            in_run = false;
        } else if !in_run {
            out.push('-');
            in_run = true;
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        "project".to_string()
    } else {
        trimmed.to_string()
    }
}

/// 저널 한 줄. 필드 순서는 TS `JSON.stringify` 와 같다 — 파일에 그대로 찍힌다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorklogEntry {
    /// `<ms epoch>-<6 hex>` — 같은 ms 안에 두 번 append 해도 충돌 안 나게.
    pub id: String,
    /// append 시각 ISO8601 (UTC, 밀리초).
    pub timestamp: String,
    pub kind: String,
    pub content: String,
    pub tags: Vec<String>,
    /// 옵셔널 Notion page id 연결고리 — 디스크에는 정규화된 `8-4-4-4-12`.
    #[serde(rename = "pageId", skip_serializing_if = "Option::is_none", default)]
    pub page_id: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct WorklogAppendInput {
    pub content: String,
    pub kind: Option<String>,
    pub tags: Option<Vec<String>>,
    pub page_id: Option<String>,
}

/// read 필터. 모두 옵셔널 — 다 비우면 가장 최근 `limit`(기본 20)개.
#[derive(Debug, Clone, Default)]
pub struct WorklogReadOptions {
    pub limit: Option<usize>,
    pub kind: Option<String>,
    pub tag: Option<String>,
    pub page_id: Option<String>,
    pub since: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct WorklogSearchOptions {
    pub limit: Option<usize>,
    pub kind: Option<String>,
}

/// 저널 dir 의 해석 출처 — env(`ROCKY_WORKLOG_DIR`) / config(`worklog.dir`) / 계산된 기본.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorklogDirSource {
    Env,
    Config,
    Default,
}

/// `worklog_status` 응답. 필드 순서는 TS 와 같다.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorklogStatus {
    pub path: String,
    pub exists: bool,
    /// 파싱 / 정규화에 성공한 유효 항목 수. 손상된 라인은 세지 않는다.
    pub total_entries: usize,
    pub size_bytes: u64,
    /// 프로젝트 식별 키 (`<basename>-<hash8>`).
    pub project_key: String,
    pub dir_source: WorklogDirSource,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub last_entry_at: Option<String>,
    /// 마지막 `kind:"digest"` watermark 의 timestamp. `/recall` 증분 정리 기준점.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub last_digest_at: Option<String>,
}

/// 파일시스템 기반 append-only 저널. 모든 read 경로는 손상 라인을 건너뛴다.
#[derive(Debug, Clone)]
pub struct Worklog {
    dir: PathBuf,
    file: PathBuf,
    project_key: String,
    dir_source: WorklogDirSource,
}

/// [`Worklog::new`] 옵션. `base_dir` 가 없으면 프로젝트별 기본 경로.
#[derive(Debug, Clone, Default)]
pub struct WorklogOptions {
    pub base_dir: Option<String>,
    /// 프로젝트 키 override (기본 `default_project_key(cwd)`).
    pub project_key: Option<String>,
    /// 출처 힌트. 불변식 `Default ⟺ base_dir 미제공` 을 생성자가 강제한다 —
    /// base_dir 있는데 미지정/`Default` 면 `Config` 로 교정.
    pub dir_source: Option<WorklogDirSource>,
    /// 기본 경로·키 계산의 기준 cwd. 미지정이면 프로세스 cwd.
    pub cwd: Option<PathBuf>,
}

impl Worklog {
    pub fn new(options: WorklogOptions) -> Self {
        let cwd = options
            .cwd
            .clone()
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));
        let project_key = options
            .project_key
            .clone()
            .unwrap_or_else(|| default_project_key(&cwd, &git_common_dir));
        let dir = match &options.base_dir {
            Some(base) => {
                std::path::absolute(expand_tilde(base)).unwrap_or_else(|_| expand_tilde(base))
            }
            None => default_worklog_root().join(&project_key),
        };
        let dir_source = if options.base_dir.is_some() {
            match options.dir_source {
                Some(WorklogDirSource::Env) => WorklogDirSource::Env,
                _ => WorklogDirSource::Config,
            }
        } else {
            WorklogDirSource::Default
        };
        let file = dir.join(WORKLOG_FILE);
        Worklog {
            dir,
            file,
            project_key,
            dir_source,
        }
    }

    /// env(`ROCKY_WORKLOG_DIR`) → config(`worklog.dir`) → 프로젝트별 기본값.
    pub fn from_env(env_dir: Option<&str>, config_dir: Option<&str>, cwd: Option<PathBuf>) -> Self {
        let env_dir = first_non_empty(env_dir);
        let config_dir = first_non_empty(config_dir);
        let (base_dir, dir_source) = match (env_dir, config_dir) {
            (Some(d), _) => (Some(d), WorklogDirSource::Env),
            (None, Some(d)) => (Some(d), WorklogDirSource::Config),
            (None, None) => (None, WorklogDirSource::Default),
        };
        Worklog::new(WorklogOptions {
            base_dir,
            project_key: None,
            dir_source: Some(dir_source),
            cwd,
        })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn path(&self) -> &Path {
        &self.file
    }

    pub fn project_key(&self) -> &str {
        &self.project_key
    }

    /// 저널에 한 줄 append. `content` 는 trim 후 비어 있으면 에러. `page_id` 는 정규화 후 저장.
    pub fn append(&self, input: &WorklogAppendInput) -> Result<WorklogEntry, String> {
        let content = input.content.trim();
        if content.is_empty() {
            return Err("Worklog.append: content must be a non-empty string after trim".into());
        }
        let kind = input
            .kind
            .as_deref()
            .map(str::trim)
            .filter(|k| !k.is_empty())
            .unwrap_or("note")
            .to_string();
        let tags = input
            .tags
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .map(|t| t.trim())
            .filter(|t| !t.is_empty())
            .map(str::to_string)
            .collect();
        let page_id = match input.page_id.as_deref().map(str::trim) {
            Some(p) if !p.is_empty() => Some(normalize_page_id(p)?),
            _ => None,
        };
        let entry = WorklogEntry {
            id: new_entry_id(),
            timestamp: now_iso(),
            kind,
            content: content.to_string(),
            tags,
            page_id,
        };
        std::fs::create_dir_all(&self.dir)
            .map_err(|e| format!("mkdir {}: {e}", self.dir.display()))?;
        let prefix = if self.ends_with_newline() { "" } else { "\n" };
        let line = serde_json::to_string(&entry).map_err(|e| e.to_string())?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.file)
            .map_err(|e| format!("open {}: {e}", self.file.display()))?;
        file.write_all(format!("{prefix}{line}\n").as_bytes())
            .map_err(|e| format!("append {}: {e}", self.file.display()))?;
        Ok(entry)
    }

    /// 파일 끝 바이트가 `\n` 인지 한 바이트만 peek. 파일이 없거나 비어 있으면 true.
    /// 읽기 실패는 best-effort 로 true — append 자체가 막히지 않도록.
    fn ends_with_newline(&self) -> bool {
        let Ok(meta) = std::fs::metadata(&self.file) else {
            return true;
        };
        if meta.len() == 0 {
            return true;
        }
        let Ok(mut f) = std::fs::File::open(&self.file) else {
            return true;
        };
        if f.seek(SeekFrom::End(-1)).is_err() {
            return true;
        }
        let mut buf = [0u8; 1];
        match f.read(&mut buf) {
            Ok(1) => buf[0] == b'\n',
            _ => true,
        }
    }

    /// 가장 최근 항목부터 `limit` 개. 필터는 AND — kind 정확 일치, tag 포함, pageId 정규화
    /// 후 일치, since *이후*(파싱 실패 시 무시).
    pub fn read(&self, options: &WorklogReadOptions) -> Result<Vec<WorklogEntry>, String> {
        let mut filtered = self.read_all()?;
        if let Some(k) = options
            .kind
            .as_deref()
            .map(str::trim)
            .filter(|k| !k.is_empty())
        {
            filtered.retain(|e| e.kind == k);
        }
        if let Some(t) = options
            .tag
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
        {
            filtered.retain(|e| e.tags.iter().any(|x| x == t));
        }
        if let Some(p) = options
            .page_id
            .as_deref()
            .map(str::trim)
            .filter(|p| !p.is_empty())
        {
            let pid = normalize_page_id(p)?;
            filtered.retain(|e| e.page_id.as_deref() == Some(pid.as_str()));
        }
        if let Some(since) = options
            .since
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            if let Some(since_ms) = parse_timestamp_ms(since) {
                filtered
                    .retain(|e| parse_timestamp_ms(&e.timestamp).is_some_and(|ms| ms > since_ms));
            }
        }
        filtered.reverse();
        filtered.truncate(cap_or_default(options.limit));
        Ok(filtered)
    }

    /// substring 검색 (case-insensitive) — content / kind / tags / pageId. 빈 query 는
    /// 전체(kind 필터만)를 최근부터.
    pub fn search(
        &self,
        query: &str,
        options: &WorklogSearchOptions,
    ) -> Result<Vec<WorklogEntry>, String> {
        let mut pool = self.read_all()?;
        if let Some(k) = options
            .kind
            .as_deref()
            .map(str::trim)
            .filter(|k| !k.is_empty())
        {
            pool.retain(|e| e.kind == k);
        }
        let needle = query.trim().to_lowercase();
        if !needle.is_empty() {
            pool.retain(|e| entry_matches(e, &needle));
        }
        pool.reverse();
        pool.truncate(cap_or_default(options.limit));
        Ok(pool)
    }

    /// 저널 메타 + 마지막 digest watermark. `total_entries` 는 유효 entry 만.
    pub fn status(&self) -> Result<WorklogStatus, String> {
        let path = self.file.to_string_lossy().to_string();
        if !self.file.exists() {
            return Ok(WorklogStatus {
                path,
                exists: false,
                total_entries: 0,
                size_bytes: 0,
                project_key: self.project_key.clone(),
                dir_source: self.dir_source,
                last_entry_at: None,
                last_digest_at: None,
            });
        }
        let size_bytes = std::fs::metadata(&self.file).map(|m| m.len()).unwrap_or(0);
        let all = self.read_all()?;
        let last_entry_at = all.last().map(|e| e.timestamp.clone());
        let last_digest_at = all
            .iter()
            .rev()
            .find(|e| e.kind == "digest")
            .map(|e| e.timestamp.clone());
        Ok(WorklogStatus {
            path,
            exists: true,
            total_entries: all.len(),
            size_bytes,
            project_key: self.project_key.clone(),
            dir_source: self.dir_source,
            last_entry_at,
            last_digest_at,
        })
    }

    /// 모든 valid entry 를 append 순으로. 손상 라인은 skip, 파일 부재는 빈 목록, 그 외 IO
    /// 오류는 에러 — `[]` 로 삼키면 저널이 사라진 것처럼 보이고 `/recall` 의 증분 기준이
    /// 오산된다.
    fn read_all(&self) -> Result<Vec<WorklogEntry>, String> {
        let raw = match std::fs::read_to_string(&self.file) {
            Ok(raw) => raw,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(format!("read {}: {e}", self.file.display())),
        };
        let mut out = Vec::new();
        for line in raw.split('\n') {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) else {
                continue;
            };
            if let Some(entry) = normalize_entry(&value) {
                out.push(entry);
            }
        }
        Ok(out)
    }
}

fn first_non_empty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

fn cap_or_default(limit: Option<usize>) -> usize {
    match limit {
        Some(n) if n > 0 => n,
        _ => DEFAULT_LIMIT,
    }
}

fn entry_matches(entry: &WorklogEntry, needle: &str) -> bool {
    entry.content.to_lowercase().contains(needle)
        || entry.kind.to_lowercase().contains(needle)
        || entry
            .page_id
            .as_deref()
            .is_some_and(|p| p.to_lowercase().contains(needle))
        || entry.tags.iter().any(|t| t.to_lowercase().contains(needle))
}

/// raw JSON 한 줄 → entry. 필수 필드가 비거나 timestamp 가 파싱 안 되면 `None`(skip).
fn normalize_entry(value: &serde_json::Value) -> Option<WorklogEntry> {
    let obj = value.as_object()?;
    let text = |key: &str| obj.get(key)?.as_str().map(|s| s.trim().to_string());
    let id = text("id")?;
    let timestamp = text("timestamp")?;
    let kind = text("kind")?;
    let content = text("content")?;
    if id.is_empty() || timestamp.is_empty() || kind.is_empty() || content.is_empty() {
        return None;
    }
    parse_timestamp_ms(&timestamp)?;
    let tags = obj
        .get("tags")
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|t| t.as_str())
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let page_id = obj
        .get("pageId")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(str::to_string);
    Some(WorklogEntry {
        id,
        timestamp,
        kind,
        content,
        tags,
        page_id,
    })
}

/// JS `Date.parse` 의 실용 부분집합 — RFC 3339 / ISO8601(밀리초·Z·오프셋) / 날짜만.
/// 못 읽으면 `None`.
pub fn parse_timestamp_ms(s: &str) -> Option<i64> {
    let s = s.trim();
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Some(dt.timestamp_millis());
    }
    for fmt in [
        "%Y-%m-%dT%H:%M:%S%.fZ",
        "%Y-%m-%dT%H:%M:%SZ",
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%dT%H:%M",
    ] {
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(s, fmt) {
            return Some(naive.and_utc().timestamp_millis());
        }
    }
    if let Ok(date) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return Some(date.and_hms_opt(0, 0, 0)?.and_utc().timestamp_millis());
    }
    None
}

/// JS `toISOString()` 재현 — 밀리초 3자리 + Z.
pub fn now_iso() -> String {
    chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string()
}

fn new_entry_id() -> String {
    let ms = chrono::Utc::now().timestamp_millis();
    let mut bytes = [0u8; 3];
    rand::rng().fill_bytes(&mut bytes);
    format!("{ms}-{:02x}{:02x}{:02x}", bytes[0], bytes[1], bytes[2])
}
