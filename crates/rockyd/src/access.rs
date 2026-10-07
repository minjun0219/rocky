//! Cloudflare Access 로 들어온 사람 확인 — 공개키(JWKS)를 받아 두고 `rocky_core::access::verify_token` 에 넘긴다.
//!
//! 공개키는 한 시간 동안 쓰고, 모르는 `kid` 가 오면(Cloudflare 가 키를 돌렸다) 다시 받는다. 다시 받기는 1분에
//! 한 번까지 — 아무 `kid` 나 지어 보내는 요청이 매번 Cloudflare 를 두드리게 하지 않는다.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rocky_core::access::{parse_jwks, token_kid, verify_token, AccessConfig, AccessIdentity, Jwk};

/// 공개키 문서를 받아 오는 함수(주소 → 본문). 테스트가 갈아 끼운다.
pub type CertsFetcher = Arc<
    dyn Fn(String) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send>> + Send + Sync,
>;

const KEYS_TTL: Duration = Duration::from_secs(3600);
const MIN_REFETCH: Duration = Duration::from_secs(60);

#[derive(Default)]
struct KeyCache {
    keys: Vec<Jwk>,
    fetched_at: Option<Instant>,
    attempted_at: Option<Instant>,
}

pub struct AccessGate {
    config: AccessConfig,
    fetch: CertsFetcher,
    cache: tokio::sync::Mutex<KeyCache>,
    last_failure: std::sync::Mutex<Option<String>>,
}

impl AccessGate {
    pub fn new(config: AccessConfig, fetch: CertsFetcher) -> Self {
        AccessGate {
            config,
            fetch,
            cache: tokio::sync::Mutex::new(KeyCache::default()),
            last_failure: std::sync::Mutex::new(None),
        }
    }

    /// `access.remoteControl` — 원격 제어 탭을 Access 로 들어온 허용 이메일에게 여는지.
    pub fn remote_control(&self) -> bool {
        self.config.remote_control
    }

    /// 토큰을 검증한다. 실패 이유는 문자열로(토큰 내용은 담지 않는다).
    pub async fn identify(&self, token: &str) -> Result<AccessIdentity, String> {
        let kid = token_kid(token).ok_or_else(|| "토큰 모양이 JWT 가 아니다".to_string())?;
        let mut cache = self.cache.lock().await;
        let stale = cache.fetched_at.is_none_or(|t| t.elapsed() > KEYS_TTL);
        let unknown = !cache.keys.iter().any(|k| k.kid == kid);
        let may_try = cache.attempted_at.is_none_or(|t| t.elapsed() > MIN_REFETCH);
        if (stale || unknown) && may_try {
            cache.attempted_at = Some(Instant::now());
            match (self.fetch)(self.config.certs_url()).await {
                Ok(body) => {
                    let keys = parse_jwks(&body);
                    if keys.is_empty() {
                        eprintln!(
                            "rocky: Access 공개키 문서에 RSA 키가 없다 — {}",
                            self.config.certs_url()
                        );
                    } else {
                        cache.keys = keys;
                        cache.fetched_at = Some(Instant::now());
                    }
                }
                Err(error) => {
                    eprintln!(
                        "rocky: Access 공개키를 못 받았다 — {}: {error}",
                        self.config.certs_url()
                    );
                }
            }
        }
        // 받기는 잠금 안에서 한 번만(동시에 온 요청이 같이 받으러 가지 않게), 검증은 잠금 밖에서.
        let keys = cache.keys.clone();
        drop(cache);
        let now = chrono::Utc::now().timestamp();
        let result = verify_token(token, &keys, &self.config, now).map_err(|e| e.to_string());
        if result.is_ok() {
            *self.last_failure.lock().unwrap_or_else(|e| e.into_inner()) = None;
        }
        result
    }

    /// 실패 이유를 적고, 직전과 다르면 true — 로그를 이유가 바뀔 때만 남긴다.
    pub fn note_failure(&self, reason: &str) -> bool {
        let mut last = self.last_failure.lock().unwrap_or_else(|e| e.into_inner());
        if last.as_deref() == Some(reason) {
            return false;
        }
        *last = Some(reason.to_string());
        true
    }
}

/// 기본 공개키 받기 — `ureq`(블로킹)를 블로킹 스레드에서, 5초 제한.
pub fn default_certs_fetcher() -> CertsFetcher {
    Arc::new(|url: String| {
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                let agent: ureq::Agent = ureq::Agent::config_builder()
                    .timeout_global(Some(Duration::from_secs(5)))
                    .build()
                    .into();
                let mut response = agent.get(&url).call().map_err(|e| e.to_string())?;
                response
                    .body_mut()
                    .read_to_string()
                    .map_err(|e| e.to_string())
            })
            .await
            .map_err(|e| e.to_string())?
        })
    })
}
