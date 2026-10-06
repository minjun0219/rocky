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
        self.read_account().map(|a| a.email)
    }

    /// 계정 파일 — 후보를 차례로 보고 처음 **읽힌** 파일. 아무것도 못 읽으면 `None`.
    pub fn read_account(&self) -> Option<claude_account::AccountFile> {
        self.account_paths.iter().find_map(|p| {
            let raw = std::fs::read_to_string(p).ok()?;
            claude_account::parse_account_file(&raw)
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
    if let Err(e) = file.write_all(&body) {
        drop(file);
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    drop(file);
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// 이 렌더의 판정 결과.
#[derive(Debug, Clone)]
pub struct Observed {
    pub limits: rocky_core::limits::Limits,
    pub usage: rocky_core::limits::UsageCache,
    pub alert: rocky_core::limits::Alert,
    /// 로그인된 계정의 이메일(계정 캐시) — 배지를 고른다. 모르면 `None`.
    pub email: Option<String>,
    pub credits: rocky_core::limits::CreditView,
    /// 크레딧 금액 색의 진하기(`limits::credit_glow`).
    pub credit_glow: Option<f64>,
}

/// 이 렌더의 한도·usage 캐시·경보·계정 — `None` 이면 한도를 다루지 않는다(`source: none`, 캐시를 읽지도 쓰지도 않는다).
///
/// 계정 캐시에서 이메일을 꺼내 그 계정의 캐시를 읽고 한도를 고른다. 계정 파일은 한도 숫자가 바뀌었거나 1분이 지났을
/// 때만 다시 읽고, 이메일이 바뀌었으면(같은 폴더 안의 전환 — claude-swap · `/login`) 그 계정의 캐시로 갈아탄다.
/// 못 읽으면 계정 캐시를 덮지 않는다. stdin 에 한도가 있으면 그 관측을 `state.json` 에 남긴다(6시간 폴백·`auto` 판단).
/// usage API 응답이 필요하면(`need_refresh`) `rocky statusline refresh` 를 detached 로 띄운다.
pub fn observe(
    cfg: &rocky_core::limits::LimitsConfig,
    configured_dir: Option<&str>,
    input: &rocky_core::limits::Input,
    now: chrono::DateTime<chrono::Utc>,
) -> Option<Observed> {
    use rocky_core::limits::{
        account_cached, alert, alerts, credit_glow, credits, need_account_check, need_refresh,
        select, use_stdin, AccountCache, Source, StateFile, UsageCache,
    };

    if cfg.source == Source::None {
        return None;
    }
    let Some(slot) = Slot::from_env(configured_dir) else {
        // 홈을 모르면 캐시도 계정도 없이 — 경보는 시각을 적을 데가 없어 배지로 고정하고, 계정 배지도 없다.
        // (cc-usage 는 HOME 이 없어도 CLAUDE_CONFIG_DIR 의 계정 파일·XDG_CACHE_HOME 을 쓴다 — 의도된 차이, 스펙 참고.)
        let usage = UsageCache::default();
        let lim = select(cfg, input, &StateFile::default(), &usage, now)?;
        // 쓰기 시작한 시각을 적을 데도 없어 페이드 없이 — 쓰는 중이면 원래 색, 아니면 옅은 색.
        let cv = credits(cfg, &lim, &usage, now);
        let credit_glow = (cfg.credit_fade() && cv.show && cv.enabled).then_some(if cv.spending {
            1.0
        } else {
            0.0
        });
        return Some(Observed {
            alert: alert(cfg, &lim),
            limits: lim,
            usage,
            email: None,
            credits: cv,
            credit_glow,
        });
    };
    let load = |email: Option<&str>| {
        let bucket = slot.bucket(email);
        let state: StateFile = read(&state_file(&bucket));
        let usage: UsageCache = read(&usage_file(&bucket));
        (bucket, state, usage)
    };

    let source = slot.account_source();
    let account: AccountCache = read(&slot.account_file());
    let mut email = account_cached(&source, &account).map(str::to_owned);
    let (mut bucket, mut state, mut usage) = load(email.as_deref());
    let mut lim = select(cfg, input, &state, &usage, now)?;

    if need_account_check(&source, &lim, &account, now) {
        if let Some(current) = slot.read_email() {
            if email.as_deref() != Some(current.as_str()) {
                (bucket, state, usage) = load(Some(&current));
                lim = select(cfg, input, &state, &usage, now)?;
            }
            email = Some(current.clone());
            let updated = AccountCache {
                source,
                email: current,
                at: lim.key(),
                checked_at: Some(now),
            };
            let _ = write(&slot.account_file(), &updated);
        }
    }

    let stdin_present = input.five_hour.is_some() || input.seven_day.is_some();
    let stdin_side = use_stdin(cfg, &state, stdin_present, now);
    let mut dirty = false;
    if stdin_present {
        state.observed_at = Some(now);
        state.stdin_limits_seen = Some(now);
        state.five_hour = input.five_hour;
        state.seven_day = input.seven_day;
        dirty = true;
    }
    // 갱신은 띄우고 기다리지 않는다 — 결과는 다음 렌더가 usage.json 에서 읽는다.
    if need_refresh(cfg, stdin_side, &lim, &state, &usage, now)
        && crate::statusline_refresh::spawn_detached(cfg.source)
    {
        state.spawned_at = Some(now);
        dirty = true;
    }
    // 단계가 오른 시각을 적어야 다음 렌더가 깜빡임 구간인지 안다.
    let (alert, alert_dirty) = alerts(cfg, &lim, &mut state, now);
    // 크레딧을 쓰기 시작한 시각도 적어야 다음 렌더가 페이드를 이어 그린다.
    let cv = credits(cfg, &lim, &usage, now);
    let (credit_glow, glow_dirty) = credit_glow(cfg, &cv, &mut state, now);
    if dirty || alert_dirty || glow_dirty {
        let _ = write(&state_file(&bucket), &state);
    }
    Some(Observed {
        limits: lim,
        usage,
        alert,
        email,
        credits: cv,
        credit_glow,
    })
}
