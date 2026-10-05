//! `rocky statusline --full` 의 캐시 파일 — 계정 캐시(`account.json`)와 계정별 `state.json` · `usage.json`. 판정은
//! `rocky_core::limits`, 자리 규칙은 `rocky_core::claude_account`, 여기는 읽고 쓰기만 한다.
//!
//! 읽기 실패(없음·깨짐)는 빈 값이다 — statusline 이 캐시 때문에 비면 안 된다. 쓰기는 같은 폴더의 임시 파일에 쓰고
//! rename 해서(0600) 반쯤 쓴 파일을 남이 읽지 않게 한다. writer 는 파일마다 하나다: `state.json`·`account.json` 은
//! statusline, `usage.json` 은 갱신 프로세스.

use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use rocky_core::claude_account;
use serde::de::DeserializeOwned;
use serde::Serialize;

/// 이 세션의 계정 자리 — 환경(`CLAUDE_CONFIG_DIR`·`XDG_CACHE_HOME`·`HOME`)과 설정의 `configDir` 로 정한다.
#[derive(Debug, Clone)]
pub struct Slot {
    pub config_dir: PathBuf,
    /// 계정 파일 후보(앞이 이 세션의 자리).
    pub account_paths: Vec<PathBuf>,
    /// `<cache>/rocky/statusline/<config_dir 해시>`.
    pub dir: PathBuf,
}

impl Slot {
    pub fn from_env(configured_dir: Option<&str>) -> Option<Slot> {
        let home = PathBuf::from(std::env::var("HOME").ok().filter(|h| !h.is_empty())?);
        let env_dir = std::env::var("CLAUDE_CONFIG_DIR").ok();
        let config_dir = claude_account::config_dir(env_dir.as_deref(), configured_dir, &home);
        let account_paths = claude_account::account_paths(env_dir.as_deref(), &config_dir, &home);
        let root =
            claude_account::cache_root(std::env::var("XDG_CACHE_HOME").ok().as_deref(), &home);
        let dir = claude_account::cache_slot(&root, &config_dir);
        Some(Slot {
            config_dir,
            account_paths,
            dir,
        })
    }

    /// 계정 캐시가 "어느 계정 파일에서 읽었나" 를 적는 키 — 첫 후보 경로.
    pub fn account_source(&self) -> String {
        self.account_paths
            .first()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    pub fn account_file(&self) -> PathBuf {
        self.dir.join("account.json")
    }

    /// 계정 하나의 캐시 폴더.
    pub fn bucket(&self, email: Option<&str>) -> PathBuf {
        claude_account::cache_bucket(&self.dir, email)
    }

    /// 지금 로그인된 이메일 — 후보를 차례로 보고 처음 **읽힌** 파일의 값. 아무것도 못 읽으면 `None`.
    pub fn read_email(&self) -> Option<String> {
        self.account_paths.iter().find_map(|p| {
            let raw = std::fs::read_to_string(p).ok()?;
            claude_account::email_from_account_file(&raw)
        })
    }
}

pub fn state_file(bucket: &Path) -> PathBuf {
    bucket.join("state.json")
}

pub fn usage_file(bucket: &Path) -> PathBuf {
    bucket.join("usage.json")
}

/// 없거나 깨졌으면 기본값.
pub fn read<T: DeserializeOwned + Default>(path: &Path) -> T {
    std::fs::read(path)
        .ok()
        .and_then(|raw| serde_json::from_slice(&raw).ok())
        .unwrap_or_default()
}

/// 원자적으로 쓴다 — 같은 폴더의 임시 파일(0600)에 쓰고 rename. 폴더는 0700 으로 만든다.
pub fn write<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| std::io::Error::other(format!("no parent: {}", path.display())))?;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    let body = serde_json::to_vec_pretty(value).map_err(std::io::Error::other)?;
    let tmp = dir.join(format!(".tmp-{}", std::process::id()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)?;
    file.write_all(&body)?;
    drop(file);
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// 이 렌더의 한도와 usage 캐시 — `None` 이면 한도를 다루지 않는다(`source: none`, 캐시를 읽지도 쓰지도 않는다).
///
/// 계정 캐시에서 이메일을 꺼내 그 계정의 캐시를 읽고 한도를 고른다. 계정 파일은 한도 숫자가 바뀌었거나 1분이 지났을
/// 때만 다시 읽고, 이메일이 바뀌었으면(같은 폴더 안의 전환 — claude-swap · `/login`) 그 계정의 캐시로 갈아탄다.
/// 못 읽으면 계정 캐시를 덮지 않는다. stdin 에 한도가 있으면 그 관측을 `state.json` 에 남긴다(6시간 폴백·`auto` 판단).
pub fn observe(
    cfg: &rocky_core::limits::LimitsConfig,
    configured_dir: Option<&str>,
    input: &rocky_core::limits::Input,
    now: chrono::DateTime<chrono::Utc>,
) -> Option<(rocky_core::limits::Limits, rocky_core::limits::UsageCache)> {
    use rocky_core::limits::{
        account_cached, need_account_check, select, AccountCache, Source, StateFile, UsageCache,
    };

    if cfg.source == Source::None {
        return None;
    }
    let Some(slot) = Slot::from_env(configured_dir) else {
        // 홈을 모르면 캐시 없이 — cc-usage 의 첫 렌더와 같다.
        let usage = UsageCache::default();
        let lim = select(cfg, input, &StateFile::default(), &usage, now)?;
        return Some((lim, usage));
    };
    let load = |email: Option<&str>| {
        let bucket = slot.bucket(email);
        let state: StateFile = read(&state_file(&bucket));
        let usage: UsageCache = read(&usage_file(&bucket));
        (bucket, state, usage)
    };

    let source = slot.account_source();
    let account: AccountCache = read(&slot.account_file());
    let email = account_cached(&source, &account).map(str::to_owned);
    let (mut bucket, mut state, mut usage) = load(email.as_deref());
    let mut lim = select(cfg, input, &state, &usage, now)?;

    if need_account_check(&source, &lim, &account, now) {
        if let Some(current) = slot.read_email() {
            if email.as_deref() != Some(current.as_str()) {
                (bucket, state, usage) = load(Some(&current));
                lim = select(cfg, input, &state, &usage, now)?;
            }
            let updated = AccountCache {
                source,
                email: current,
                at: lim.key(),
                checked_at: Some(now),
            };
            let _ = write(&slot.account_file(), &updated);
        }
    }

    if input.five_hour.is_some() || input.seven_day.is_some() {
        state.observed_at = Some(now);
        state.stdin_limits_seen = Some(now);
        state.five_hour = input.five_hour;
        state.seven_day = input.seven_day;
        let _ = write(&state_file(&bucket), &state);
    }
    Some((lim, usage))
}
