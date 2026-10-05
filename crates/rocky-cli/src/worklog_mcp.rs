//! `rocky mcp worklog` — worklog_* 4 도구를 내는 **stdio** MCP 서버.
//! TS 원본 `src/index.ts`(플러그인 stdio 서버).
//!
//! 데몬(`/mcp`)이 아니라 CLI 가 여는 이유: 워크로그는 **프로젝트별**(레포 루트 키)인데
//! 데몬은 호출자의 cwd 를 모른다. 플러그인 stdio 서버는 세션의 프로젝트 디렉터리에서
//! 뜨므로 `process cwd` 가 곧 프로젝트다 — TS 판과 같은 앵커에 쌓인다.
//!
//! `--roots` 는 그렇지 않은 호스트(Antigravity — 플러그인 폴더에서 띄운다)용이다: 도구를 부를 때마다
//! 클라이언트에 MCP `roots/list` 를 물어 그 폴더를 프로젝트로 쓴다. 답을 못 받으면 cwd 로 물러서지 않고
//! 에러를 낸다 — 플러그인 폴더 칸에 쌓이는 것보다 낫다.

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, ServerCapabilities, ServerInfo};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{tool, tool_handler, tool_router, ServerHandler, ServiceExt};
use rocky_core::config::{load_worklog_config, user_config_path};
use rocky_core::worklog::{
    root_uri_path, Worklog, WorklogAppendInput, WorklogReadOptions, WorklogSearchOptions,
};
use schemars::JsonSchema;
use serde::Deserialize;

/// TS `jsonResult` — `JSON.stringify(value, null, 2)` 한 블록.
fn json_result(value: &impl serde::Serialize) -> CallToolResult {
    match serde_json::to_string_pretty(value) {
        Ok(text) => CallToolResult::success(vec![ContentBlock::text(text)]),
        Err(error) => CallToolResult::error(vec![ContentBlock::text(error.to_string())]),
    }
}

fn outcome(result: Result<CallToolResult, String>) -> CallToolResult {
    match result {
        Ok(out) => out,
        Err(message) => CallToolResult::error(vec![ContentBlock::text(message)]),
    }
}

/// 도구 한 번을 사용 로그에 남긴다 — 이 서버는 데몬 밖(stdio)이라 데몬 싱크가 못 본다.
fn recorded(name: &str, run: impl FnOnce() -> Result<CallToolResult, String>) -> CallToolResult {
    let started = std::time::Instant::now();
    let result = run();
    let ok = result.is_ok();
    crate::usage_cmd::record(
        rocky_core::usage::UsageSource::Mcp,
        name,
        ok,
        Some(started),
        None,
    );
    outcome(result)
}

#[derive(Deserialize, JsonSchema)]
pub struct AppendArgs {
    /// 필수 본문
    pub content: String,
    /// decision / blocker / answer / note 등. 기본 note
    pub kind: Option<String>,
    pub tags: Option<Vec<String>>,
    /// 연결할 Notion page id 또는 URL
    #[serde(rename = "pageId")]
    #[schemars(rename = "pageId")]
    pub page_id: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ReadArgs {
    /// 기본 20
    pub limit: Option<usize>,
    /// 정확 일치
    pub kind: Option<String>,
    /// 태그 포함
    pub tag: Option<String>,
    /// 정규화 후 일치
    #[serde(rename = "pageId")]
    #[schemars(rename = "pageId")]
    pub page_id: Option<String>,
    /// 해당 시각 이후 (ISO8601)
    pub since: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct SearchArgs {
    /// 검색어
    pub query: String,
    /// 기본 20
    pub limit: Option<usize>,
    /// 풀 스코프 필터
    pub kind: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct StatusArgs {}

/// 워크로그의 프로젝트를 어디서 정하나.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectSource {
    /// 서버 프로세스의 cwd — Claude Code·Codex·opencode 처럼 작업 폴더에서 띄우는 호스트.
    Cwd,
    /// 도구를 부를 때마다 클라이언트의 `roots/list` 첫 `file://` 폴더.
    Roots,
}

/// `roots/list` 를 기다리는 한도 — 답하지 않는 클라이언트에 도구 호출이 묶이지 않게.
const ROOTS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

#[derive(Clone)]
pub struct WorklogMcp {
    worklog: Worklog,
    source: ProjectSource,
    tool_router: ToolRouter<Self>,
}

impl WorklogMcp {
    pub fn new(worklog: Worklog) -> Self {
        Self::with_source(worklog, ProjectSource::Cwd)
    }

    pub fn with_source(worklog: Worklog, source: ProjectSource) -> Self {
        WorklogMcp {
            worklog,
            source,
            tool_router: Self::tool_router(),
        }
    }

    /// 이번 호출이 쓸 워크로그 — `Roots` 면 클라이언트에 작업 폴더를 묻는다.
    /// roots 는 SEP-2577 로 폐기 예정이지만(폐기 뒤 1년은 동작) 대체 수단이 아직 없다.
    #[allow(deprecated)]
    async fn worklog_for_call(&self, ctx: &RequestContext<RoleServer>) -> Result<Worklog, String> {
        if self.source == ProjectSource::Cwd {
            return Ok(self.worklog.clone());
        }
        // capability 는 initialize 로 한 번(옛 수명주기) 오거나, 요청마다 `_meta` 로 온다(2026-07-28 —
        // agy 는 `server/discover` 로 연다).
        let supports_roots = ctx
            .meta
            .client_capabilities()
            .is_some_and(|caps| caps.roots.is_some())
            || ctx
                .peer
                .peer_info()
                .is_some_and(|info| info.capabilities.roots.is_some());
        if !supports_roots {
            return Err(
                "--roots: 클라이언트가 roots capability 를 선언하지 않아 작업 폴더를 알 수 없다"
                    .into(),
            );
        }
        let listed = tokio::time::timeout(ROOTS_TIMEOUT, ctx.peer.list_roots())
            .await
            .map_err(|_| {
                format!(
                    "--roots: roots/list 가 {}초 안에 답하지 않았다",
                    ROOTS_TIMEOUT.as_secs()
                )
            })?
            .map_err(|e| format!("--roots: roots/list 실패: {e}"))?;
        let uris: Vec<&str> = listed.roots.iter().map(|r| r.uri.as_str()).collect();
        let dir = uris
            .iter()
            .find_map(|uri| root_uri_path(uri))
            .ok_or_else(|| format!("--roots: roots 에 file:// 폴더가 없다: {uris:?}"))?;
        Ok(worklog_for_dir(Some(dir)))
    }

    /// 도구 이름 목록 — 표면 가드 테스트용.
    pub fn tool_names() -> Vec<String> {
        Self::tool_router()
            .list_all()
            .into_iter()
            .map(|t| t.name.to_string())
            .collect()
    }
}

#[tool_router(router = tool_router)]
impl WorklogMcp {
    #[tool(
        name = "worklog_append",
        description = "워크로그에 한 줄을 append-only 로 기록한다. 다음 turn 에 인용할 결정 / blocker / 사용자 답변 / 메모를 남길 때 사용. remote 호출 없음. 저장 위치는 `worklog.dir`(rocky.json) 또는 `ROCKY_WORKLOG_DIR`(env 우선)로 변경 가능(worklog_status 로 확인). (content: 필수 본문, kind?: decision/blocker/answer/note 등 기본 note, tags?: 문자열 배열, pageId?: 연결할 Notion page id 또는 URL)"
    )]
    async fn worklog_append(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<AppendArgs>,
    ) -> CallToolResult {
        let worklog = self.worklog_for_call(&ctx).await;
        recorded("worklog_append", || {
            worklog?
                .append(&WorklogAppendInput {
                    content: args.content,
                    kind: args.kind,
                    tags: args.tags,
                    page_id: args.page_id,
                })
                .map(|entry| json_result(&entry))
        })
    }

    #[tool(
        name = "worklog_read",
        description = "저널을 가장 최근 항목부터 필터 / limit 적용해 반환한다. 손상된 라인은 자동 skip. remote 호출 없음. (limit?: 기본 20, kind?: 정확 일치, tag?: 태그 포함, pageId?: 정규화 후 일치, since?: 해당 시각 이후 ISO8601)"
    )]
    async fn worklog_read(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<ReadArgs>,
    ) -> CallToolResult {
        let worklog = self.worklog_for_call(&ctx).await;
        recorded("worklog_read", || {
            worklog?
                .read(&WorklogReadOptions {
                    limit: args.limit,
                    kind: args.kind,
                    tag: args.tag,
                    page_id: args.page_id,
                    since: args.since,
                })
                .map(|entries| json_result(&entries))
        })
    }

    #[tool(
        name = "worklog_search",
        description = "저널을 substring (case-insensitive) 으로 검색한다. content / kind / tags / pageId 를 매칭. remote 호출 없음. (query: 검색어, limit?: 기본 20, kind?: 풀 스코프 필터)"
    )]
    async fn worklog_search(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<SearchArgs>,
    ) -> CallToolResult {
        let worklog = self.worklog_for_call(&ctx).await;
        recorded("worklog_search", || {
            worklog?
                .search(
                    &args.query,
                    &WorklogSearchOptions {
                        limit: args.limit,
                        kind: args.kind,
                    },
                )
                .map(|entries| json_result(&entries))
        })
    }

    #[tool(
        name = "worklog_status",
        description = "워크로그 메타(파일 경로, 존재 여부, 유효 항목 수 — 손상 라인 skip, 바이트 크기, 마지막 항목 시각) + 마지막 digest watermark(lastDigestAt) + 경로 출처(dirSource)를 조회한다. `/recall` 이 정리 시작 시 이걸로 증분 기준점을 확인한다. remote 호출 없음. 저장 위치는 `worklog.dir`(rocky.json) 또는 `ROCKY_WORKLOG_DIR`(env 우선)로 변경 가능하다."
    )]
    async fn worklog_status(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(_args): Parameters<StatusArgs>,
    ) -> CallToolResult {
        let worklog = self.worklog_for_call(&ctx).await;
        recorded("worklog_status", || {
            worklog?.status().map(|status| json_result(&status))
        })
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for WorklogMcp {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        info.server_info.name = "rocky".into();
        info.server_info.version = env!("CARGO_PKG_VERSION").into();
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        // rocky 채널 — 데몬의 PR 전이를 세션에 밀어 넣는다(`crate::channel`). 선언은 늘 하고,
        // 배달 여부는 Claude Code 가 세션 플래그로 정한다.
        info.capabilities.experimental = Some(crate::channel::channel_capabilities());
        info.instructions = Some(crate::channel::INSTRUCTIONS.to_string());
        info
    }
}

/// 프로세스 cwd 기준 워크로그 — env(`ROCKY_WORKLOG_DIR`) > rocky.json `worklog.dir` > 기본.
pub fn worklog_for_cwd() -> Worklog {
    worklog_for_dir(std::env::current_dir().ok())
}

/// 그 폴더를 프로젝트로 보는 워크로그 — 우선순위는 `worklog_for_cwd` 와 같다.
pub fn worklog_for_dir(dir: Option<std::path::PathBuf>) -> Worklog {
    let config = load_worklog_config(
        &user_config_path(),
        dir.as_deref().unwrap_or(std::path::Path::new(".")),
    );
    let env_dir = std::env::var("ROCKY_WORKLOG_DIR").ok();
    Worklog::from_env(env_dir.as_deref(), config.dir.as_deref(), dir)
}

/// stdin/stdout 으로 MCP 를 서빙한다. 클라이언트가 stdin 을 닫으면 끝난다.
pub fn serve_stdio(source: ProjectSource) -> Result<(), String> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("tokio runtime: {e}"))?;
    runtime.block_on(async {
        let service = WorklogMcp::with_source(worklog_for_cwd(), source)
            .serve(rmcp::transport::stdio())
            .await
            .map_err(|e| format!("worklog mcp: {e}"))?;
        // 채널 전달 스레드 — 데몬 주소는 CLI 와 같은 규칙(env > user rocky.json > 기본 포트).
        let todo = rocky_core::config::load_todo_config(&user_config_path());
        let runtime_config =
            rocky_core::config::resolve_runtime_config(&rocky_core::config::env_snapshot(), &todo);
        crate::channel::spawn_forwarder(
            service.peer().clone(),
            format!("http://127.0.0.1:{}", runtime_config.port),
            tokio::runtime::Handle::current(),
        );
        service
            .waiting()
            .await
            .map_err(|e| format!("worklog mcp: {e}"))?;
        Ok(())
    })
}
