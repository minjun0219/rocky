//! `rocky-todo mcp worklog` — worklog_* 4 도구를 내는 **stdio** MCP 서버.
//! TS 원본 `src/index.ts`(플러그인 stdio 서버).
//!
//! 데몬(`/mcp`)이 아니라 CLI 가 여는 이유: 워크로그는 **프로젝트별**(레포 루트 키)인데
//! 데몬은 호출자의 cwd 를 모른다. 플러그인 stdio 서버는 세션의 프로젝트 디렉터리에서
//! 뜨므로 `process cwd` 가 곧 프로젝트다 — TS 판과 같은 앵커에 쌓인다.

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, ServerCapabilities, ServerInfo};
use rmcp::{tool, tool_handler, tool_router, ServerHandler, ServiceExt};
use rocky_todo_core::config::{load_worklog_config, user_config_path};
use rocky_todo_core::worklog::{
    Worklog, WorklogAppendInput, WorklogReadOptions, WorklogSearchOptions,
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

#[derive(Clone)]
pub struct WorklogMcp {
    worklog: Worklog,
    tool_router: ToolRouter<Self>,
}

impl WorklogMcp {
    pub fn new(worklog: Worklog) -> Self {
        WorklogMcp {
            worklog,
            tool_router: Self::tool_router(),
        }
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
    async fn worklog_append(&self, Parameters(args): Parameters<AppendArgs>) -> CallToolResult {
        outcome(
            self.worklog
                .append(&WorklogAppendInput {
                    content: args.content,
                    kind: args.kind,
                    tags: args.tags,
                    page_id: args.page_id,
                })
                .map(|entry| json_result(&entry)),
        )
    }

    #[tool(
        name = "worklog_read",
        description = "저널을 가장 최근 항목부터 필터 / limit 적용해 반환한다. 손상된 라인은 자동 skip. remote 호출 없음. (limit?: 기본 20, kind?: 정확 일치, tag?: 태그 포함, pageId?: 정규화 후 일치, since?: 해당 시각 이후 ISO8601)"
    )]
    async fn worklog_read(&self, Parameters(args): Parameters<ReadArgs>) -> CallToolResult {
        outcome(
            self.worklog
                .read(&WorklogReadOptions {
                    limit: args.limit,
                    kind: args.kind,
                    tag: args.tag,
                    page_id: args.page_id,
                    since: args.since,
                })
                .map(|entries| json_result(&entries)),
        )
    }

    #[tool(
        name = "worklog_search",
        description = "저널을 substring (case-insensitive) 으로 검색한다. content / kind / tags / pageId 를 매칭. remote 호출 없음. (query: 검색어, limit?: 기본 20, kind?: 풀 스코프 필터)"
    )]
    async fn worklog_search(&self, Parameters(args): Parameters<SearchArgs>) -> CallToolResult {
        outcome(
            self.worklog
                .search(
                    &args.query,
                    &WorklogSearchOptions {
                        limit: args.limit,
                        kind: args.kind,
                    },
                )
                .map(|entries| json_result(&entries)),
        )
    }

    #[tool(
        name = "worklog_status",
        description = "워크로그 메타(파일 경로, 존재 여부, 유효 항목 수 — 손상 라인 skip, 바이트 크기, 마지막 항목 시각) + 마지막 digest watermark(lastDigestAt) + 경로 출처(dirSource)를 조회한다. `/recall` 이 정리 시작 시 이걸로 증분 기준점을 확인한다. remote 호출 없음. 저장 위치는 `worklog.dir`(rocky.json) 또는 `ROCKY_WORKLOG_DIR`(env 우선)로 변경 가능하다."
    )]
    async fn worklog_status(&self, Parameters(_args): Parameters<StatusArgs>) -> CallToolResult {
        outcome(self.worklog.status().map(|status| json_result(&status)))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for WorklogMcp {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        info.server_info.name = "rocky".into();
        info.server_info.version = env!("CARGO_PKG_VERSION").into();
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info
    }
}

/// 프로세스 cwd 기준 워크로그 — env(`ROCKY_WORKLOG_DIR`) > rocky.json `worklog.dir` > 기본.
pub fn worklog_for_cwd() -> Worklog {
    let cwd = std::env::current_dir().ok();
    let config = load_worklog_config(
        &user_config_path(),
        cwd.as_deref().unwrap_or(std::path::Path::new(".")),
    );
    let env_dir = std::env::var("ROCKY_WORKLOG_DIR").ok();
    Worklog::from_env(env_dir.as_deref(), config.dir.as_deref(), cwd)
}

/// stdin/stdout 으로 MCP 를 서빙한다. 클라이언트가 stdin 을 닫으면 끝난다.
pub fn serve_stdio() -> Result<(), String> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("tokio runtime: {e}"))?;
    runtime.block_on(async {
        let service = WorklogMcp::new(worklog_for_cwd())
            .serve(rmcp::transport::stdio())
            .await
            .map_err(|e| format!("worklog mcp: {e}"))?;
        service
            .waiting()
            .await
            .map_err(|e| format!("worklog mcp: {e}"))?;
        Ok(())
    })
}
