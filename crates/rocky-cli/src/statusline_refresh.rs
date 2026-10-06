//! `rocky statusline refresh` — usage API 를 한 번 불러 그 계정의 `usage.json` 을 고친다. cc-usage `refresh` 의 이식.
//!
//! statusline 은 네트워크를 기다리지 않는다. 갱신이 필요하면 이 명령을 **detached** 로 띄우고 바로 끝난다 — 자식이 세션
//! 환경(`CLAUDE_CONFIG_DIR` 등)을 그대로 물려받아 그 세션 계정의 토큰을 쓴다. 데몬이 아닌 이유가 이것이다(데몬은 세션
//! 환경을 볼 수 없다). 같은 계정의 갱신은 lock 으로 한 번에 하나만 돈다.
//!
//! 토큰은 읽기만 한다(갱신하지 않는다): `tokenEnv` → keychain(기본 설정 폴더만) → `.credentials.json`.

use std::os::unix::process::CommandExt;
use std::path::Path;
use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use rocky_core::claude_account::{self, OauthToken};
use rocky_core::config::StatuslineConfig;
use rocky_core::limits::{
    apply_failure, apply_fetch, parse_usage_response, select, Input, Source, StateFile, UsageCache,
};

use crate::statusline_cache::{read, state_file, usage_file, write, Slot};

/// usage API — 비공식이라 응답 구조가 바뀔 수 있고 rate limit 이 낮다.
const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const BETA_HEADER: &str = "oauth-2025-04-20";
/// 테스트 전용 — 가짜 서버 주소. 사용자 표면이 아니다(cc-usage 의 `CC_USAGE_API_URL` 자리).
const USAGE_URL_ENV: &str = "ROCKY_STATUSLINE_USAGE_URL";
const FETCH_TIMEOUT: Duration = Duration::from_secs(10);
const KEYCHAIN_TIMEOUT: Duration = Duration::from_secs(3);

/// 갱신 한 번. 결과는 `usage.json` 에만 남는다 — 실패도 이유·backoff 로 기록해 statusline 이 그린다.
pub fn run(cfg: &StatuslineConfig, now: DateTime<Utc>) {
    if cfg.limits.source == Source::None {
        return;
    }
    let Some(slot) = Slot::from_env(cfg.config_dir.as_deref()) else {
        return;
    };
    // 홈이 없으면(지워진 임시 폴더 등) 아무것도 만들지 않는다.
    if !std::env::var("HOME").is_ok_and(|h| Path::new(&h).is_dir()) {
        return;
    }
    // 계정 파일을 직접 읽는다 — 토큰과 같은 계정의 캐시에 써야 한다. 못 읽으면(원자적 재작성 중 등) 이 계정이 누구인지
    // 모르는 것이라 쓰지 않는다 — `_` 에 쓰면 그 숫자가 나중에 다른 계정 줄에 나온다.
    let Some(email) = slot.read_email() else {
        return;
    };
    let bucket = slot.bucket(Some(&email));
    let Some(_lock) = Lock::try_acquire(&bucket.join("refresh.lock")) else {
        return; // 다른 갱신이 도는 중
    };
    let mut usage: UsageCache = read(&usage_file(&bucket));
    let state: StateFile = read(&state_file(&bucket));
    if usage.backoff_until.is_some_and(|t| now < t) {
        return;
    }
    let result = load_token(cfg, &slot, now).and_then(|token| fetch(&token.access_token, now));
    // 그 사이 계정이 바뀌었으면(claude-swap 이 keychain 을 먼저 바꾸는 등) 이 응답이 누구 것인지 확신할 수 없다 — 버린다.
    if slot.read_email().as_deref() != Some(email.as_str()) {
        return;
    }
    match result {
        Ok(fetched) => {
            // 새 응답 기준으로 소진된 창 — 크레딧 기준선이 어느 창의 것인지 정한다.
            let mut next = usage.clone();
            next.usage = Some(fetched.clone());
            let hit_key = select(&cfg.limits, &Input::default(), &state, &next, now)
                .and_then(|lim| lim.exhausted_key());
            apply_fetch(&mut usage, fetched, hit_key, now);
        }
        Err(Failure {
            message,
            retry_after,
        }) => apply_failure(&mut usage, &message, retry_after, now),
    }
    let _ = write(&usage_file(&bucket), &usage);
}

/// statusline 이 갱신을 띄운다 — 새 세션(`setsid`)으로 떼어 내 statusline 이 끝나거나 끊겨도 살아남게 하고, 입출력은
/// 모두 버린다. 띄우지 못하면 `false`(다음 렌더가 다시 시도한다).
pub fn spawn_detached() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let mut cmd = std::process::Command::new(exe);
    cmd.args(["statusline", "refresh"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    // SAFETY: fork 와 exec 사이에서 setsid(2) 하나만 부른다 — async-signal-safe 하다.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    // git·extra 를 띄우는 스레드와 겹치면 그 파이프 끝을 이 자식이 물려받는다(`bounded` 의 주석) — 같은 잠금 아래서.
    let _guard = crate::bounded::spawn_lock();
    cmd.spawn().is_ok()
}

/// 실패 — 기록할 문구와, 서버가 말한 대기 시간(429 의 `Retry-After`).
#[derive(Debug)]
struct Failure {
    message: String,
    retry_after: TimeDelta,
}

impl From<String> for Failure {
    fn from(message: String) -> Failure {
        Failure {
            message,
            retry_after: TimeDelta::zero(),
        }
    }
}

fn load_token(
    cfg: &StatuslineConfig,
    slot: &Slot,
    now: DateTime<Utc>,
) -> Result<OauthToken, Failure> {
    if let Some(var) = cfg.token_env.as_deref() {
        if let Some(v) = std::env::var(var)
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
        {
            return Ok(OauthToken {
                access_token: v,
                expires_at: None,
            });
        }
    }
    let home = std::env::var("HOME").unwrap_or_default();
    let home = Path::new(&home);
    // keychainService·credentialsFile 이 가리키는 폴더 — 설정의 configDir(세션 env 는 보지 않는다).
    let settings_dir = claude_account::config_dir(None, cfg.config_dir.as_deref(), home);
    let mut errors = Vec::new();
    if cfg!(target_os = "macos") {
        match claude_account::keychain_service(
            cfg.keychain_service.as_deref(),
            &settings_dir,
            &slot.config_dir,
            home,
        ) {
            Some(service) => match from_keychain(&service) {
                // 찾았으면 만료여도 그 결과다 — 파일로 넘어가지 않는다(cc-usage 와 같다).
                Ok(token) => return claude_account::check_token(token, now).map_err(Failure::from),
                Err(e) => errors.push(format!("keychain: service {service:?}: {e}")),
            },
            None => errors.push("keychain: 건너뜀 (비기본 config_dir)".to_string()),
        }
    }
    let file = claude_account::credentials_file(
        cfg.credentials_file.as_deref(),
        &settings_dir,
        &slot.config_dir,
        home,
    );
    match std::fs::read_to_string(&file) {
        Ok(raw) => match claude_account::parse_token(&raw) {
            Ok(token) => return claude_account::check_token(token, now).map_err(Failure::from),
            Err(e) => errors.push(format!("file: {}: {e}", file.display())),
        },
        Err(e) => errors.push(format!("file: {}: {e}", file.display())),
    }
    Err(format!("token not found ({})", errors.join("; ")).into())
}

fn from_keychain(service: &str) -> Result<OauthToken, String> {
    let argv = [
        "/usr/bin/security",
        "find-generic-password",
        "-s",
        service,
        "-w",
    ]
    .map(String::from);
    let out = crate::bounded::run(&argv, KEYCHAIN_TIMEOUT).ok_or("not found")?;
    claude_account::parse_token(&String::from_utf8_lossy(&out))
}

fn fetch(token: &str, now: DateTime<Utc>) -> Result<rocky_core::limits::CachedUsage, Failure> {
    let url = std::env::var(USAGE_URL_ENV).unwrap_or_else(|_| USAGE_URL.to_string());
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(FETCH_TIMEOUT))
        // 상태 코드는 직접 본다 — 에러로 바꾸면 429 의 Retry-After 와 본문을 잃는다.
        .http_status_as_error(false)
        .build()
        .into();
    let mut response = agent
        .get(&url)
        .header("Authorization", &format!("Bearer {token}"))
        .header("anthropic-beta", BETA_HEADER)
        .header("Accept", "application/json")
        .header("User-Agent", "rocky")
        .call()
        .map_err(|e| Failure::from(format!("Get {url:?}: {e}")))?;
    let status = response.status().as_u16();
    let retry_after = response
        .headers()
        .get("Retry-After")
        .and_then(|v| v.to_str().ok())
        .map(|v| retry_after(v, now))
        .unwrap_or_default();
    let body = response
        .body_mut()
        .with_config()
        .limit(1 << 20)
        .read_to_vec()
        .map_err(|e| Failure::from(format!("read body: {e} (GET {url} → http {status})")))?;
    match status {
        429 => Err(Failure {
            message: "rate limited (429)".into(),
            retry_after,
        }),
        401 | 403 => Err("unauthorized (401/403)".to_string().into()),
        // 원인을 앞에, 요청 맥락은 뒤 괄호에 — statusline 은 에러를 40바이트에서 자르므로 앞이 원인이어야 읽힌다.
        200 => parse_usage_response(&body, now)
            .map_err(|e| Failure::from(format!("{e} (GET {url} → http {status})"))),
        _ => Err(format!(
            "http {status}: {}",
            truncate(&String::from_utf8_lossy(&body), 120)
        )
        .into()),
    }
}

/// `Retry-After` — 초, 또는 HTTP 날짜. 읽을 수 없으면 0.
fn retry_after(v: &str, now: DateTime<Utc>) -> TimeDelta {
    let v = v.trim();
    let wait = match v.parse::<i64>() {
        Ok(secs) => TimeDelta::try_seconds(secs).unwrap_or(MAX_RETRY_AFTER),
        Err(_) => DateTime::parse_from_rfc2822(v)
            .map(|t| t.with_timezone(&Utc) - now)
            .unwrap_or_default(),
    };
    // 터무니없는 값에 갇히지 않게 — 하루를 넘기면 하루로.
    wait.clamp(TimeDelta::zero(), MAX_RETRY_AFTER)
}

const MAX_RETRY_AFTER: TimeDelta = TimeDelta::hours(24);

/// 120바이트에서 자른다(글자 중간에서는 자르지 않는다).
fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        return s.to_string();
    }
    format!("{}…", &s[..s.floor_char_boundary(n)])
}

/// `refresh.lock` 의 flock — 잡혀 있으면 기다리지 않고 포기한다. 놓는 것은 drop(파일을 닫으면 풀린다).
struct Lock {
    _file: std::fs::File,
}

impl Lock {
    fn try_acquire(path: &Path) -> Option<Lock> {
        use std::os::fd::AsRawFd;
        use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path.parent()?)
            .ok()?;
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .mode(0o600)
            .open(path)
            .ok()?;
        // SAFETY: 열어 둔 파일의 fd 에 flock(2) 만 건다.
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        (rc == 0).then_some(Lock { _file: file })
    }
}
