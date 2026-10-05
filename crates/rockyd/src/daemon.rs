//! 데몬 배선 — TS 원본 `src/daemon.ts`. lib 에 두는 이유: Tauri 앱이 같은 프로세스에
//! 마운트한다(단독 실행은 main.rs).
//!
//! 네 표면: `/` 웹 UI(dist 정적 서빙 + SPA fallback) · `/api/*` REST · `/api/events` SSE
//! · `/mcp` MCP streamable HTTP. 단일성 보장: 기동 시 같은 포트의 기존 인스턴스 health
//! 를 확인하고 있으면 즉시 종료한다(포트 자체가 락).

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{ConnectInfo, State};
use axum::http::{Request, StatusCode};
use axum::response::Response;
use axum::routing::any;
use axum::Router;
use rocky_core::local_request::{is_cross_site_request, is_local_request, CROSS_SITE_MESSAGE};
use rocky_core::TodoStore;
use tower::Service;
use tower_http::services::{ServeDir, ServeFile};

use crate::mcp::{mcp_service, TodoMcp};
use crate::runner::default_runner;
use crate::server::{build_server, handle_api, ServerOptions, ServerState};
use rocky_core::config::{resolve_runtime_config, ExposeChannel, TodoRuntimeConfig};

type McpSvc = rmcp::transport::streamable_http_server::StreamableHttpService<
    TodoMcp,
    rmcp::transport::streamable_http_server::session::never::NeverSessionManager,
>;

#[derive(Clone)]
pub struct AppState {
    pub server: Arc<ServerState>,
    /// allowIssueCreate=true/false 두 벌 — 요청의 isLocalRequest 판정으로 고른다.
    mcp_local: McpSvc,
    mcp_remote: McpSvc,
}

/// 같은 포트의 살아 있는 rocky 인스턴스 확인 — **신원 검증** 포함(무관한 서비스의
/// 2xx JSON 을 데몬으로 오인하지 않는다).
pub fn daemon_health(base_url: &str) -> Option<serde_json::Value> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_millis(700)))
        .build()
        .into();
    let mut response = agent.get(format!("{base_url}/api/health")).call().ok()?;
    let body: serde_json::Value = response.body_mut().read_json().ok()?;
    if body.get("ok") != Some(&serde_json::Value::Bool(true))
        || body.get("name").and_then(|n| n.as_str()) != Some("rocky")
    {
        return None;
    }
    Some(body)
}

/// axum Router 를 만든다 — 서빙 바인딩은 호출자(run_daemon / Tauri) 몫.
pub fn build_router(state: Arc<ServerState>, ui_dist: Option<&Path>) -> Router {
    let app_state = AppState {
        mcp_local: mcp_service(state.clone(), true),
        mcp_remote: mcp_service(state.clone(), false),
        server: state,
    };
    let mut router = Router::new()
        .route("/mcp", any(mcp_handler))
        // 노트 동시 편집 소켓 — `/api/{*rest}` 보다 먼저(구체 경로가 이긴다).
        .route("/api/ws", axum::routing::get(ws_handler))
        .route("/api/{*rest}", any(api_handler))
        .route("/api", any(api_handler));
    // 웹 UI — 퍼머링크(`/rocky/12`) 새로고침은 index.html fallback 으로 돌아온다.
    if let Some(dist) = ui_dist {
        let serve = ServeDir::new(dist).fallback(ServeFile::new(dist.join("index.html")));
        router = router.fallback_service(serve);
    }
    router.with_state(app_state)
}

async fn api_handler(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    req: Request<Body>,
) -> Response {
    handle_api(&state.server, req, Some(peer.ip().to_string())).await
}

/// 노트 소켓 핸드셰이크 — REST 변경과 같은 cross-site 가드를 **업그레이드 전에** 건다(웹소켓은 CORS 밖이다).
/// 브라우저는 소켓에 헤더를 못 붙이므로 actor 는 쿼리(`?actor=`)로 받는다.
async fn ws_handler(
    State(state): State<AppState>,
    upgrade: axum::extract::ws::WebSocketUpgrade,
    headers: axum::http::HeaderMap,
    axum::extract::Query(query): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Response {
    let get_header = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
    };
    let host = get_header("host").unwrap_or_else(|| "localhost".to_string());
    if is_cross_site_request(get_header, &format!("http://{host}/api/ws")) {
        return Response::builder()
            .status(StatusCode::FORBIDDEN)
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({ "error": CROSS_SITE_MESSAGE }).to_string(),
            ))
            .unwrap();
    }
    let actor = query
        .get("actor")
        .map(|a| a.trim().to_string())
        .filter(|a| !a.is_empty())
        .unwrap_or_else(|| "unknown".to_string());
    let server = state.server.clone();
    upgrade.on_upgrade(move |socket| crate::ws::serve_socket(socket, server, actor))
}

async fn mcp_handler(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    req: Request<Body>,
) -> Response {
    // REST 와 같은 cross-site 가드 — "변경은 라우트 전에 끊는다" 규칙의 예외를 남기지 않는다.
    let headers = req.headers().clone();
    let get_header = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
    };
    if req.method() != axum::http::Method::GET {
        let host = get_header("host").unwrap_or_else(|| "localhost".to_string());
        let url = format!("http://{host}{}", req.uri().path());
        if is_cross_site_request(get_header, &url) {
            return Response::builder()
                .status(StatusCode::FORBIDDEN)
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "error": CROSS_SITE_MESSAGE }).to_string(),
                ))
                .unwrap();
        }
    }
    let peer_ip = peer.ip().to_string();
    let local = is_local_request(Some(&peer_ip), |name| req.headers().contains_key(name));
    let mut service = if local {
        state.mcp_local.clone()
    } else {
        state.mcp_remote.clone()
    };
    match service.call(req).await {
        Ok(response) => response.map(Body::new),
        Err(never) => match never {},
    }
}

/// 포트를 잡는다 — 이미 쓰이고 있으면(종료 중인 옛 데몬) `wait` 동안 다시 시도한다.
async fn bind_when_free(
    addr: SocketAddr,
    wait: std::time::Duration,
) -> std::io::Result<tokio::net::TcpListener> {
    let deadline = std::time::Instant::now() + wait;
    loop {
        match tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => return Ok(listener),
            Err(e)
                if e.kind() == std::io::ErrorKind::AddrInUse
                    && std::time::Instant::now() < deadline =>
            {
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            }
            Err(e) => return Err(e),
        }
    }
}

/// `daemon.pid` 의 옛 데몬이 아직 살아 있나 — 내 pid 가 아니고, 그 pid 가 rockyd 일 때만(남의 프로세스가 같은
/// 번호를 받았으면 무시한다).
pub fn previous_daemon_alive(
    pid_file: &Path,
    own: u32,
    is_rockyd: impl Fn(u32) -> bool,
) -> Option<u32> {
    let pid: u32 = std::fs::read_to_string(pid_file)
        .ok()?
        .trim()
        .parse()
        .ok()?;
    (pid != own && is_rockyd(pid)).then_some(pid)
}

/// 그 pid 가 지금 살아 있는 rockyd 인가 — `ps` 로 본다(새 의존성 없이).
fn pid_is_rockyd(pid: u32) -> bool {
    std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "comm="])
        .output()
        .map(|out| {
            String::from_utf8_lossy(&out.stdout)
                .trim()
                .ends_with("rockyd")
        })
        .unwrap_or(false)
}

/// 옛 데몬이 끝날 때까지 기다린다. `wait` 안에 안 끝나면 DB 를 열지 않고 실패한다 — 두 데몬이 같은 DB 에 동시에
/// 스키마를 바꾸는 것보다 기동 실패가 낫다(훅이 다음 턴에 다시 띄운다).
async fn wait_for_previous_daemon(
    pid_file: &Path,
    wait: std::time::Duration,
) -> Result<(), Box<dyn std::error::Error>> {
    let own = std::process::id();
    let deadline = std::time::Instant::now() + wait;
    let mut announced = false;
    while let Some(pid) = previous_daemon_alive(pid_file, own, pid_is_rockyd) {
        if !announced {
            eprintln!("rocky: 옛 데몬(pid {pid})이 끝나기를 기다린 뒤 DB 를 연다");
            announced = true;
        }
        if std::time::Instant::now() >= deadline {
            return Err(format!(
                "옛 데몬(pid {pid})이 {}초 안에 끝나지 않았다 — DB 를 열지 않고 멈춘다",
                wait.as_secs()
            )
            .into());
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    if announced {
        eprintln!("rocky: 옛 데몬이 끝났다 — DB 를 연다");
    }
    Ok(())
}

/// 데몬을 기동해 리슨한다 — TS `startDaemon` 대응. 반환하지 않는다(서버 수명).
pub async fn run_daemon(
    runtime: TodoRuntimeConfig,
    ui_dist: Option<PathBuf>,
    usage: Option<crate::usage_sink::UsageSink>,
    usage_dir: Option<PathBuf>,
    pr_watch: rocky_core::config::PrWatchConfig,
    tokens: TokensRuntime,
    verify: rocky_core::verify::VerifyConfig,
    rc: Option<rocky_core::config::RcConfig>,
) -> Result<(), Box<dyn std::error::Error>> {
    // 단일 인스턴스 가드 — 포트 자체가 락.
    let base_url = format!("http://127.0.0.1:{}", runtime.port);
    let already = tokio::task::spawn_blocking({
        let base_url = base_url.clone();
        move || daemon_health(&base_url).is_some()
    })
    .await?;
    if already {
        let label = rocky_core::config::launchd_label();
        let xpc = std::env::var("XPC_SERVICE_NAME").ok();
        let parent = std::os::unix::process::parent_id();
        if !rocky_core::config::launched_by_launchd(xpc.as_deref(), &label, parent) {
            println!(
                "rocky daemon already running on port {} — exiting",
                runtime.port
            );
            return Ok(());
        }
        // launchd 가 띄웠는데 포트를 다른 데몬(대개 launchd 밖에서 뜬 고아)이 쥐고 있다. 바로 끝나면 KeepAlive 가
        // 10초마다 다시 띄워 로그만 쌓이고(`spawn scheduled` 루프) 고아가 내려가도 다음 재기동까지 비게 된다 —
        // 그 데몬이 내려갈 때까지 기다렸다가 이어받는다.
        eprintln!(
            "rocky: 다른 데몬이 포트 {} 를 쥐고 있다 — launchd({label})의 데몬으로서 그게 내려가면 이어받는다 (rocky daemon status)",
            runtime.port
        );
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            let still = tokio::task::spawn_blocking({
                let base_url = base_url.clone();
                move || daemon_health(&base_url).is_some()
            })
            .await?;
            if !still {
                eprintln!("rocky: 포트가 비었다 — 이어받는다");
                break;
            }
        }
    }

    std::fs::create_dir_all(&runtime.dir)?;
    // 포트를 **DB 를 열기 전에** 잡는다 — 포트가 단일 인스턴스 락이고, 마이그레이션은 락을 쥔 뒤에만 돈다.
    // 예전엔 DB 를 열어 마이그레이션까지 한 뒤에 bind 했다: 업그레이드 때 종료 중인 옛 데몬이 아직 쓰는 사이 새
    // 데몬이 스키마를 바꿨고, 2026-09-30 실제 DB 가 손상됐다(history 와 새 표가 같은 페이지를 가리킴).
    let addr: SocketAddr = format!("{}:{}", runtime.host, runtime.port).parse()?;
    let listener = bind_when_free(addr, std::time::Duration::from_secs(10)).await?;
    // 포트를 놓은 뒤에도 옛 프로세스는 하던 쓰기를 몇 초 마무리한다 — 끝날 때까지 기다린다.
    let pid_path = runtime.dir.join("daemon.pid");
    wait_for_previous_daemon(&pid_path, std::time::Duration::from_secs(15)).await?;
    let store = Arc::new(TodoStore::open(&runtime.dir.join("todo.db"))?);
    // 기동 때 한 번 — 깨진 DB 를 모르고 열흘 넘게 쓰지 않게 health 에 싣는다.
    let integrity = store
        .quick_check()
        .unwrap_or_else(|e| format!("quick_check 실패: {e}"));
    if integrity != "ok" {
        eprintln!("rocky: ⚠ DB 무결성 이상 — {integrity}");
    }
    let state = build_server(ServerOptions {
        statusline_template: Some(runtime.statusline_template.clone()),
        inbox_sources: runtime.inbox.clone(),
        inbox_adapters: runtime.inbox_adapters.clone(),
        usage,
        logs_db: Some(runtime.dir.join("logs.db")),
        token_recommend: tokens.recommend.clone(),
        rc: Some(crate::rc::cached_rc(
            crate::runner::default_runner(),
            rc,
            std::env::var("HOME").unwrap_or_default(),
            crate::rc::RC_CACHE_TTL,
        )),
        ..ServerOptions::new(store)
    });
    // 로그 색인 — 작업로그·사용 로그·Claude Code 트랜스크립트(JSONL)를 logs.db 로. 전용 OS 스레드라 보드 DB 잠금도 tokio 워커도 쓰지 않는다.
    crate::logindex::spawn_indexer(
        runtime.dir.join("logs.db"),
        rocky_core::worklog::default_worklog_root(),
        usage_dir,
        tokens.transcripts_dir.clone(),
        Some(crate::logindex::RecommendationFeed::new(
            state.token_events.clone(),
            tokens.recommend.clone(),
        )),
        std::time::Duration::from_secs(60),
    );
    state.set_db_integrity(integrity);
    // 세션 목록 예열 — 비어 있으면 첫 요청이 `claude agents --json`(콜드 수 초)을 기다린다.
    {
        let state = state.clone();
        tokio::spawn(async move {
            state.sessions().await;
        });
    }
    // 죽은 세션이 쥔 doing 자동 해제 — 기동 1분 뒤부터 10분마다.
    crate::sweep::spawn_sweeper(
        state.clone(),
        std::time::Duration::from_secs(60),
        std::time::Duration::from_secs(600),
    );
    // 수집함 구독 감시 — 세션이 구독한 소스만 5분마다 읽어 새 항목을 그 세션에 알린다(기동 1분 뒤부터).
    crate::inbox_watch::spawn_inbox_watcher(
        state.clone(),
        std::time::Duration::from_secs(60),
        std::time::Duration::from_secs(300),
    );
    // PR 감시 — repo 가 설정된 보드의 PR 을 주기적으로 보고 ready·충돌을 알린다. 기동 90초 뒤 처음.
    if pr_watch.enabled != Some(false) {
        let runner = crate::runner::default_runner();
        // 알리는 채널들 — macOS 배너(`notify`)·세션 받은편지함(`sessionNotify`)·알림 브릿지(`notifiers[]`)는 서로 독립이다.
        let mut notifiers = Vec::new();
        if pr_watch.notify.unwrap_or(true) {
            notifiers.push(crate::prwatch::osascript_notifier(runner.clone()));
        }
        if pr_watch.session_notify.unwrap_or(true) {
            notifiers.push(crate::prwatch::session_notifier(state.clone()));
        }
        for bridge in pr_watch.notifiers.clone() {
            notifiers.push(crate::prwatch::bridge_notifier(runner.clone(), bridge));
        }
        let notify = !notifiers.is_empty();
        crate::prwatch::spawn_pr_watcher(
            state.clone(),
            runner,
            crate::prwatch::compose_notifiers(notifiers),
            notify,
            std::time::Duration::from_secs(90),
            std::time::Duration::from_secs(pr_watch.interval_minutes() * 60),
        );
    }
    // 기본 브랜치 검증 — `verify.targets[]` 가 있을 때만. 기동 2분 뒤 처음(업데이트 직후 몰리지 않게), 바퀴가 끝나면 주기만큼 쉰다.
    let verify_runner = crate::runner::default_runner();
    crate::verify::spawn_verifier(
        state.clone(),
        verify_runner.clone(),
        crate::verify::osascript_verify_notifier(verify_runner),
        runtime.dir.join("verify"),
        verify,
        std::time::Duration::from_secs(120),
    );
    let router = build_router(state, ui_dist.as_deref());

    std::fs::write(&pid_path, std::process::id().to_string())?;

    println!(
        "rocky daemon listening on http://{}:{} (db: {})",
        runtime.host,
        runtime.port,
        runtime.dir.display()
    );
    if runtime.host != "127.0.0.1" {
        println!("주의: 루프백 외 바인딩 — 같은 네트워크의 기기가 인증 없이 보드에 접근할 수 있다");
        println!("      (GitHub 이슈 생성은 예외 — 로컬 요청만 허용된다)");
    }

    // 옵션: expose 에 tailscale 채널이 있을 때만 serve 보장 — 남의 노출은 빼앗지 않는다.
    if runtime.expose.contains(&ExposeChannel::TailscaleServe) {
        let runner = default_runner();
        let port = runtime.port;
        let message = crate::tailscale::ensure_tailscale_serve(&runner, port, move |target| {
            Box::pin(async move {
                tokio::task::spawn_blocking(move || {
                    daemon_health(&format!("http://127.0.0.1:{target}")).is_some()
                })
                .await
                .unwrap_or(false)
            })
        })
        .await;
        println!("{message}");
    }

    let shutdown_pid = pid_path.clone();
    let (signaled_tx, signaled_rx) = tokio::sync::watch::channel(false);
    let server = axum::serve(
        listener,
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        shutdown_signal().await;
        let _ = signaled_tx.send(true);
        let _ = std::fs::remove_file(&shutdown_pid);
    });
    serve_with_grace(
        std::future::IntoFuture::into_future(server),
        signaled_rx,
        SHUTDOWN_GRACE,
    )
    .await?;
    let _ = std::fs::remove_file(&pid_path);
    Ok(())
}

/// 종료 신호 뒤 열린 연결이 끝나기를 기다리는 한도. SSE 스트림(`/api/events`, 채널 전달기)은 스스로
/// 끝나지 않아서, 기다림에 한도가 없으면 옛 데몬이 포트만 놓은 채 계속 산다 — 그 안의 PR 감시·sweep 도 같이
/// 돌아 새 데몬과 겹친다(2026-09-30 실측: 교체된 v0.32.3 이 옛 세션 두 곳의 SSE 를 물고 몇 시간 남았다).
pub const SHUTDOWN_GRACE: std::time::Duration = std::time::Duration::from_secs(3);

/// 서버를 돌리되, 종료 신호(`signaled` 가 true)를 받은 뒤 `grace` 가 지나도 안 끝나면 그대로 돌아온다 —
/// 남은 연결은 프로세스가 끝나면서 끊기고, 클라이언트(SSE)는 새 데몬에 다시 붙는다.
pub async fn serve_with_grace<F, E>(
    server: F,
    mut signaled: tokio::sync::watch::Receiver<bool>,
    grace: std::time::Duration,
) -> Result<(), E>
where
    F: std::future::Future<Output = Result<(), E>>,
{
    tokio::select! {
        result = server => result,
        () = async {
            // 보내는 쪽이 없어지면(서버가 먼저 끝남) 기다릴 이유가 없다 — 위 분기가 끝난다.
            if signaled.wait_for(|v| *v).await.is_err() {
                std::future::pending::<()>().await;
            }
            tokio::time::sleep(grace).await;
        } => {
            eprintln!("rocky: 종료 유예 {grace:?} 초과 — 남은 연결을 끊고 나간다");
            Ok(())
        }
    }
}

async fn shutdown_signal() {
    use tokio::signal::unix::{signal, SignalKind};
    let mut sigterm = signal(SignalKind::terminate()).expect("sigterm handler");
    let mut sigint = signal(SignalKind::interrupt()).expect("sigint handler");
    tokio::select! {
        _ = sigterm.recv() => {}
        _ = sigint.recv() => {}
    }
}

/// 설정 로드까지 포함한 진입 — main.rs 와 Tauri 가 공유.
pub async fn start_daemon(ui_dist: Option<PathBuf>) -> Result<(), Box<dyn std::error::Error>> {
    let config_path = rocky_core::config::user_config_path();
    let todo = rocky_core::config::load_todo_config(&config_path);
    let env = rocky_core::config::env_snapshot();
    let runtime = resolve_runtime_config(&env, &todo);
    // 사용 로그 — 켜져 있으면 파일 싱크, 아니면 안 남긴다.
    let usage_dir = rocky_core::config::resolve_usage_dir(
        &env,
        &rocky_core::config::load_usage_block(&config_path),
    );
    let usage = usage_dir.clone().map(crate::usage_sink::file_sink);
    let pr_watch = rocky_core::config::load_pr_block(&config_path);
    let tokens_block = rocky_core::config::load_tokens_block(&config_path);
    let tokens = TokensRuntime {
        transcripts_dir: rocky_core::config::resolve_transcripts_dir(&env, &tokens_block),
        recommend: tokens_block.recommend,
    };
    let verify = rocky_core::verify::load_verify_block(&config_path);
    // rc 서버 현황 — `rc` 블록이 있을 때만 프로브가 돈다.
    let rc = rocky_core::config::load_rc_block(&config_path);
    run_daemon(runtime, ui_dist, usage, usage_dir, pr_watch, tokens, verify, rc).await
}

/// 토큰 색인 — 트랜스크립트 루트(None 이면 끔)와 추천 규칙.
#[derive(Debug, Clone, Default)]
pub struct TokensRuntime {
    pub transcripts_dir: Option<PathBuf>,
    pub recommend: rocky_core::tokens::RecommendConfig,
}
