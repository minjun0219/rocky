//! `rocky statusline refresh` 와 `--full` 의 detached 갱신 — 가짜 usage API 서버로 끝까지 돌린다. 실제 keychain·API 에는
//! 닿지 않는다: keychain 항목은 없는 이름, API 주소는 이 테스트의 로컬 서버다.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

const NOW: &str = "2026-09-16T07:40:00Z";
const EMAIL: &str = "a@example.com";

/// 요청마다 같은 응답을 돌려주는 로컬 HTTP 서버 — 받은 요청 머리는 `requests` 에 남는다.
struct FakeApi {
    url: String,
    requests: Arc<Mutex<Vec<String>>>,
}

impl FakeApi {
    fn start(status: &'static str, headers: &'static str, body: &'static str) -> FakeApi {
        FakeApi::start_with(status, headers, body, || {})
    }

    /// 요청을 받을 때마다 `on_request` 를 먼저 부른다 — 응답 사이에 일어나는 일(계정 전환 등)을 흉내 낸다.
    fn start_with(
        status: &'static str,
        headers: &'static str,
        body: &'static str,
        on_request: impl Fn() + Send + 'static,
    ) -> FakeApi {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/api/oauth/usage", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = requests.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut buf = [0u8; 8192];
                let mut head = Vec::new();
                while !head.windows(4).any(|w| w == b"\r\n\r\n") {
                    match stream.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => head.extend_from_slice(&buf[..n]),
                    }
                }
                seen.lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&head).into_owned());
                on_request();
                let reply = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(reply.as_bytes());
            }
        });
        FakeApi { url, requests }
    }
}

const OK_BODY: &str = r#"{"five_hour":{"utilization":42,"resets_at":"2026-09-16T09:00:00Z"},"seven_day":{"utilization":10},"extra_usage":{"is_enabled":true,"used_credits":1160,"monthly_limit":5000}}"#;

/// 임시 HOME — 계정 파일(이메일)과 rocky.json 을 둔다.
struct Home {
    _dir: tempfile::TempDir,
    path: PathBuf,
}

impl Home {
    fn new(email: Option<&str>, statusline: serde_json::Value) -> Home {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_path_buf();
        if let Some(email) = email {
            std::fs::create_dir_all(path.join(".claude")).unwrap();
            std::fs::write(
                path.join(".claude/.claude.json"),
                format!(r#"{{"oauthAccount":{{"emailAddress":"{email}"}}}}"#),
            )
            .unwrap();
        }
        let mut statusline = statusline;
        statusline["keychainService"] = "rocky-test-absent".into();
        std::fs::write(
            path.join("rocky.json"),
            serde_json::json!({
                "todo": {"port": 1, "dir": "/nonexistent", "expose": "off"},
                "statusline": statusline,
            })
            .to_string(),
        )
        .unwrap();
        Home { _dir: dir, path }
    }

    fn account_file(&self) -> PathBuf {
        self.path.join(".claude/.claude.json")
    }

    fn bucket(&self, email: Option<&str>) -> PathBuf {
        rocky_core::claude_account::cache_bucket(
            &rocky_core::claude_account::cache_slot(
                &self.path.join(".cache"),
                &self.path.join(".claude"),
            ),
            email,
        )
    }

    fn rocky(&self, api: &str, env: &[(&str, &str)]) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_rocky"));
        cmd.env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("HOME", &self.path)
            .env("ROCKY_USAGE_DIR", self.path.join("usage"))
            .env("ROCKY_CONFIG", self.path.join("rocky.json"))
            .env("ROCKY_STATUSLINE_USAGE_URL", api)
            .env("ROCKY_STATUSLINE_NOW", NOW)
            .env("NO_COLOR", "1")
            .env("TZ", "UTC");
        for (k, v) in env {
            cmd.env(k, v);
        }
        cmd
    }

    fn refresh(&self, api: &str, env: &[(&str, &str)]) {
        let status = self
            .rocky(api, env)
            .args(["statusline", "refresh"])
            .stdin(Stdio::null())
            .status()
            .unwrap();
        assert!(status.success());
    }

    fn full(&self, api: &str, env: &[(&str, &str)], stdin: &str) -> String {
        let mut child = self
            .rocky(api, env)
            .args(["statusline", "--full"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(stdin.as_bytes())
            .unwrap();
        String::from_utf8(child.wait_with_output().unwrap().stdout).unwrap()
    }
}

fn usage_json(bucket: &Path) -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(bucket.join("usage.json")).unwrap()).unwrap()
}

const TOKEN_ENV: [(&str, &str); 1] = [("ROCKY_TEST_TOKEN", "tok-123")];
const MODEL_ONLY: &str = r#"{"model":{"display_name":"M"}}"#;

#[test]
fn refresh_fetches_into_the_logged_in_accounts_cache_and_full_draws_it() {
    let api = FakeApi::start("200 OK", "", OK_BODY);
    let home = Home::new(
        Some("a@example.com"),
        serde_json::json!({"source": "api", "tokenEnv": "ROCKY_TEST_TOKEN"}),
    );
    home.refresh(&api.url, &TOKEN_ENV);

    let req = api.requests.lock().unwrap().join("\n");
    assert!(
        req.contains("authorization: Bearer tok-123")
            || req.contains("Authorization: Bearer tok-123"),
        "{req}"
    );
    assert!(
        req.to_lowercase()
            .contains("anthropic-beta: oauth-2025-04-20"),
        "{req}"
    );
    // 토큰의 계정(이메일) 캐시에 쓴다.
    let usage = usage_json(&home.bucket(Some(EMAIL)));
    assert_eq!(usage["usage"]["five_hour"]["percent"], 42.0);
    assert_eq!(usage["usage"]["extra"]["used_credits"], 1160.0);
    // --full 은 갱신을 기다리지 않고 캐시를 그린다 — 크레딧은 남은 금액(5000-1160 cent).
    assert_eq!(
        home.full(&api.url, &TOKEN_ENV, MODEL_ONLY),
        "M · 5h 58% (↻09:00) · $38.40 ($50)\n"
    );
}

#[test]
fn refresh_records_rate_limits_with_retry_after_and_full_shows_the_reason() {
    let api = FakeApi::start("429 Too Many Requests", "Retry-After: 900\r\n", "{}");
    let home = Home::new(
        Some(EMAIL),
        serde_json::json!({"source": "api", "tokenEnv": "ROCKY_TEST_TOKEN"}),
    );
    home.refresh(&api.url, &TOKEN_ENV);
    let usage = usage_json(&home.bucket(Some(EMAIL)));
    assert_eq!(usage["last_error"], "rate limited (429)");
    assert_eq!(usage["failures"], 1);
    // backoff 는 1분이지만 서버가 15분을 말했다.
    assert_eq!(usage["backoff_until"], "2026-09-16T07:55:00Z");
    assert_eq!(
        home.full(&api.url, &TOKEN_ENV, MODEL_ONLY),
        "M · usage: rate limited (429)\n"
    );
    // backoff 중에는 다시 부르지 않는다.
    home.refresh(&api.url, &TOKEN_ENV);
    assert_eq!(api.requests.lock().unwrap().len(), 1);
}

#[test]
fn refresh_reports_auth_and_server_errors_without_calling_out_twice() {
    let api = FakeApi::start("401 Unauthorized", "", "{}");
    let home = Home::new(
        Some(EMAIL),
        serde_json::json!({"source": "api", "tokenEnv": "ROCKY_TEST_TOKEN"}),
    );
    home.refresh(&api.url, &TOKEN_ENV);
    assert_eq!(
        usage_json(&home.bucket(Some(EMAIL)))["last_error"],
        "unauthorized (401/403)"
    );

    let api = FakeApi::start("500 Internal Server Error", "", "upstream unavailable");
    let home = Home::new(
        Some(EMAIL),
        serde_json::json!({"source": "api", "tokenEnv": "ROCKY_TEST_TOKEN"}),
    );
    home.refresh(&api.url, &TOKEN_ENV);
    assert_eq!(
        usage_json(&home.bucket(Some(EMAIL)))["last_error"],
        "http 500: upstream unavailable"
    );
}

#[test]
fn refresh_without_a_token_records_why_and_never_calls_the_api() {
    let api = FakeApi::start("200 OK", "", OK_BODY);
    let home = Home::new(Some(EMAIL), serde_json::json!({"source": "api"}));
    home.refresh(&api.url, &[]);
    let error = usage_json(&home.bucket(Some(EMAIL)))["last_error"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(error.starts_with("token not found ("), "{error}");
    assert!(api.requests.lock().unwrap().is_empty());
}

#[test]
fn refresh_reads_the_credentials_file_and_refuses_an_expired_token() {
    let api = FakeApi::start("200 OK", "", OK_BODY);
    let home = Home::new(Some(EMAIL), serde_json::json!({"source": "api"}));
    let creds = home.path.join(".claude/.credentials.json");
    std::fs::create_dir_all(creds.parent().unwrap()).unwrap();
    // 2026-09-16T07:40:00Z 보다 한 시간 전에 만료.
    std::fs::write(
        &creds,
        r#"{"claudeAiOauth":{"accessToken":"old","expiresAt":1757997600000}}"#,
    )
    .unwrap();
    home.refresh(&api.url, &[]);
    assert_eq!(
        usage_json(&home.bucket(Some(EMAIL)))["last_error"],
        rocky_core::claude_account::TOKEN_EXPIRED
    );
    assert!(api.requests.lock().unwrap().is_empty());
}

#[test]
fn refresh_leaves_source_none_alone() {
    let api = FakeApi::start("200 OK", "", OK_BODY);
    let home = Home::new(
        Some(EMAIL),
        serde_json::json!({"source": "none", "tokenEnv": "ROCKY_TEST_TOKEN"}),
    );
    home.refresh(&api.url, &TOKEN_ENV);
    assert!(api.requests.lock().unwrap().is_empty());
    assert!(!home.bucket(Some(EMAIL)).join("usage.json").exists());
}

/// `--full` 은 응답이 필요하면 갱신을 detached 로 띄우고 바로 끝난다 — 다음 렌더가 그 결과를 그린다.
#[test]
fn full_spawns_a_detached_refresh_and_the_next_render_draws_it() {
    let api = FakeApi::start("200 OK", "", OK_BODY);
    let home = Home::new(
        Some(EMAIL),
        serde_json::json!({"source": "api", "tokenEnv": "ROCKY_TEST_TOKEN"}),
    );
    assert_eq!(home.full(&api.url, &TOKEN_ENV, MODEL_ONLY), "M · usage …\n");
    let usage = home.bucket(Some(EMAIL)).join("usage.json");
    for _ in 0..100 {
        if usage.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(
        usage.exists(),
        "detached 갱신이 5초 안에 캐시를 쓰지 않았다"
    );
    // 같은 시각이면 30초 안에 다시 띄우지 않는다(spawned_at).
    let state: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(home.bucket(Some(EMAIL)).join("state.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(state["spawned_at"], NOW);
    assert_eq!(
        home.full(&api.url, &TOKEN_ENV, MODEL_ONLY),
        "M · 5h 58% (↻09:00) · $38.40 ($50)\n"
    );
}

/// 계정 파일을 못 읽으면(원자적 재작성 중 등) 이 토큰이 누구 것인지 모른다 — 부르지도 쓰지도 않는다.
#[test]
fn refresh_without_a_readable_account_file_neither_calls_nor_writes() {
    let api = FakeApi::start("200 OK", "", OK_BODY);
    let home = Home::new(
        Some(EMAIL),
        serde_json::json!({"source": "api", "tokenEnv": "ROCKY_TEST_TOKEN"}),
    );
    std::fs::write(home.account_file(), "{ half written").unwrap();
    home.refresh(&api.url, &TOKEN_ENV);
    assert!(api.requests.lock().unwrap().is_empty());
    assert!(!home.bucket(None).join("usage.json").exists());
    assert!(!home.bucket(Some(EMAIL)).join("usage.json").exists());
}

/// 응답을 기다리는 사이 계정이 바뀌면(claude-swap 이 keychain 을 먼저 바꾸는 등) 그 응답은 버린다.
#[test]
fn refresh_drops_a_response_when_the_account_switched_mid_flight() {
    let target = Arc::new(Mutex::new(None::<PathBuf>));
    let swap = target.clone();
    let api = FakeApi::start_with("200 OK", "", OK_BODY, move || {
        if let Some(path) = swap.lock().unwrap().as_ref() {
            std::fs::write(path, r#"{"oauthAccount":{"emailAddress":"b@example.com"}}"#).unwrap();
        }
    });
    let home = Home::new(
        Some(EMAIL),
        serde_json::json!({"source": "api", "tokenEnv": "ROCKY_TEST_TOKEN"}),
    );
    *target.lock().unwrap() = Some(home.account_file());
    home.refresh(&api.url, &TOKEN_ENV);
    assert_eq!(api.requests.lock().unwrap().len(), 1);
    assert!(!home.bucket(Some(EMAIL)).join("usage.json").exists());
    assert!(!home
        .bucket(Some("b@example.com"))
        .join("usage.json")
        .exists());
}

#[test]
fn refresh_caps_an_absurd_retry_after_instead_of_crashing() {
    let api = FakeApi::start(
        "429 Too Many Requests",
        "Retry-After: 1000000000000000\r\n",
        "{}",
    );
    let home = Home::new(
        Some(EMAIL),
        serde_json::json!({"source": "api", "tokenEnv": "ROCKY_TEST_TOKEN"}),
    );
    home.refresh(&api.url, &TOKEN_ENV);
    let usage = usage_json(&home.bucket(Some(EMAIL)));
    assert_eq!(usage["last_error"], "rate limited (429)");
    assert_eq!(
        usage["backoff_until"], "2026-09-17T07:40:00Z",
        "하루로 자른다"
    );
}

/// `keychainService` 는 그것이 가리키는 폴더(configDir, 없으면 ~/.claude)의 세션에만 — 다른 계정 세션은 기본 규칙
/// (비기본 폴더면 keychain 을 건너뛴다)으로 간다. 안 그러면 그 토큰으로 남의 숫자를 그린다.
#[test]
fn keychain_setting_applies_only_to_its_own_config_dir() {
    let api = FakeApi::start("200 OK", "", OK_BODY);
    let home = Home::new(Some(EMAIL), serde_json::json!({"source": "api"}));
    let work = home.path.join("work");
    std::fs::create_dir_all(&work).unwrap();
    std::fs::write(
        work.join(".claude.json"),
        r#"{"oauthAccount":{"emailAddress":"w@example.com"}}"#,
    )
    .unwrap();
    home.refresh(&api.url, &[("CLAUDE_CONFIG_DIR", work.to_str().unwrap())]);
    let slot = rocky_core::claude_account::cache_slot(&home.path.join(".cache"), &work);
    let usage = usage_json(&rocky_core::claude_account::cache_bucket(
        &slot,
        Some("w@example.com"),
    ));
    let error = usage["last_error"].as_str().unwrap();
    // 설정의 keychain 항목(rocky-test-absent)은 기본 폴더 것이라 work 세션에서는 묻지도 않는다.
    assert!(
        error.contains("keychain: 건너뜀 (비기본 config_dir)"),
        "{error}"
    );
    assert!(!error.contains("rocky-test-absent"), "{error}");
    assert!(api.requests.lock().unwrap().is_empty());
}
