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

/// 데몬을 기동해 리슨한다 — TS `startDaemon` 대응. 반환하지 않는다(서버 수명).
pub async fn run_daemon(
    runtime: TodoRuntimeConfig,
    ui_dist: Option<PathBuf>,
    usage: Option<crate::usage_sink::UsageSink>,
    pr_watch: rocky_core::config::PrWatchConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    // 단일 인스턴스 가드 — 포트 자체가 락.
    let base_url = format!("http://127.0.0.1:{}", runtime.port);
    let already = tokio::task::spawn_blocking({
        let base_url = base_url.clone();
        move || daemon_health(&base_url).is_some()
    })
    .await?;
    if already {
        println!(
            "rocky daemon already running on port {} — exiting",
            runtime.port
        );
        return Ok(());
    }

    std::fs::create_dir_all(&runtime.dir)?;
    let store = Arc::new(TodoStore::open(&runtime.dir.join("todo.db"))?);
    let state = build_server(ServerOptions {
        statusline_template: Some(runtime.statusline_template.clone()),
        inbox_sources: runtime.inbox.clone(),
        inbox_adapters: runtime.inbox_adapters.clone(),
        usage,
        ..ServerOptions::new(store)
    });
    // 죽은 세션이 쥔 doing 자동 해제 — 기동 1분 뒤부터 10분마다.
    crate::sweep::spawn_sweeper(
        state.clone(),
        std::time::Duration::from_secs(60),
        std::time::Duration::from_secs(600),
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
    let router = build_router(state, ui_dist.as_deref());

    let addr: SocketAddr = format!("{}:{}", runtime.host, runtime.port).parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;

    let pid_path = runtime.dir.join("daemon.pid");
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
    let usage = rocky_core::config::resolve_usage_dir(
        &env,
        &rocky_core::config::load_usage_block(&config_path),
    )
    .map(crate::usage_sink::file_sink);
    let pr_watch = rocky_core::config::load_pr_block(&config_path);
    run_daemon(runtime, ui_dist, usage, pr_watch).await
}
