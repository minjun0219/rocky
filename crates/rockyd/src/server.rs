//! rocky REST + SSE 표면 — TS 원본 `src/server.ts`.
//!
//! TS 처럼 단일 fetch 핸들러(수동 매칭)로 둔다 — 라우팅 순서·에러 매핑·경로 디코딩까지
//! 계약이라, 프레임워크 라우터로 흩으면 동작 동일성을 검증하기 어렵다. actor 는
//! `x-rocky-actor` 헤더로 전달된다.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Body;
use axum::http::{header, HeaderMap, Method, Request, StatusCode};
use axum::response::Response;
use rocky_core::config::InboxSource;
use rocky_core::doing::{handoff_phase, is_unstarted, resolve_doing_state, HandoffPhase};
use rocky_core::handoff::{
    build_handoff_poke, build_handoff_prompt_from, HandoffPokeInput, HandoffPromptInput,
};
use rocky_core::inbox::INBOX_CACHE_TTL_SECS;
use rocky_core::inbox::{mark_promoted, InboxResponse};
use rocky_core::local_request::{
    is_cross_site_request, is_local_request, CROSS_SITE_MESSAGE, NON_LOCAL_BOARD_META_MESSAGE,
    NON_LOCAL_INBOX_SOURCE_MESSAGE, NON_LOCAL_ISSUE_MESSAGE, NON_LOCAL_PR_SUBSCRIPTION_MESSAGE,
    NON_LOCAL_SPAWN_MESSAGE,
};
use rocky_core::refs::{
    ref_needs_board_context, ref_of, with_ref_note, with_ref_todo, NoteView, TodoView,
};
use rocky_core::sessions::{match_board, AgentSession, SessionsResult};
use rocky_core::statusline::{
    board_key_for_cwd, render_statusline, BoardLocation, StatuslineData, StatuslineMine,
    DEFAULT_STATUSLINE_TEMPLATE, STATUSLINE_TITLE_MAX,
};
use rocky_core::store::{StoreError, StoreResult, TodoStore};
use rocky_core::summary::{build_summary, count_unpromoted, due_bucket, Summary};
use rocky_core::types::*;
use rocky_core::usage::{client_of, normalize_route, UsageEvent, UsageSource};
use serde_json::{json, Value};
use tokio::sync::broadcast;

use crate::github::{
    create_issue_for_todo, find_issue_link, is_repo_slug, IssueForTodoError, IssueForTodoOptions,
};
use crate::inbox_exec::{cached_inbox_dynamic, InboxFetch, InboxProvider, SourcesFn};
use crate::runner::{default_runner, Runner};
use crate::sessions_exec::{cached_sessions, uncached_sessions, SessionsProvider};
use crate::spawnctl::{
    default_spawn_fn, find_live_session_at, worktree_name_for, worktree_path_for, RecentSpawns,
    SpawnFn, SpawnInput, RECENT_SPAWN_TTL,
};
use crate::usage_sink::{noop_sink, UsageSink};

/// 상태를 바꾸는 메서드 — cross-site 가드가 적용되는 범위.
fn is_mutating(method: &Method) -> bool {
    matches!(
        *method,
        Method::POST | Method::PATCH | Method::PUT | Method::DELETE
    )
}

pub type PathExists = Arc<dyn Fn(&str) -> bool + Send + Sync>;
pub type RealPath = Arc<dyn Fn(&str) -> std::io::Result<String> + Send + Sync>;

/// 서버 옵션 — 테스트가 fake 를 넣는 주입점 (TS `TodoServerOptions` 대응).
pub struct ServerOptions {
    pub store: Arc<TodoStore>,
    pub statusline_template: Option<String>,
    pub sessions: Option<SessionsProvider>,
    /// spawn 라우트 전용 — 기본은 **캐시 없는** 조회기 (spawn 이전 스냅샷 금지).
    pub spawn_sessions: Option<SessionsProvider>,
    /// statusline 라우트 전용 — 기본 TTL 15초 (초당 도는 유일한 경로).
    pub statusline_sessions: Option<SessionsProvider>,
    pub gh_runner: Option<Runner>,
    pub spawn: Option<SpawnFn>,
    pub path_exists: Option<PathExists>,
    pub real_path: Option<RealPath>,
    pub recent_spawns: Option<Arc<RecentSpawns>>,
    /// 수집함 소스(`todo.inbox[]`). `inbox` 주입이 없을 때 기본 조회기가 이걸로 만들어진다.
    pub inbox_sources: Vec<InboxSource>,
    /// 보드 설정 화면용 어댑터(`todo.inboxAdapters[]`) — 보드마다 등록한 값을 붙여 실행한다.
    pub inbox_adapters: Vec<InboxSource>,
    /// 수집함 조회기 — 테스트가 fake 를 넣는다.
    pub inbox: Option<InboxProvider>,
    /// 사용 로그 싱크 — 없으면 안 남긴다(테스트·끈 설정).
    pub usage: Option<UsageSink>,
}

impl ServerOptions {
    pub fn new(store: Arc<TodoStore>) -> Self {
        ServerOptions {
            store,
            statusline_template: None,
            sessions: None,
            spawn_sessions: None,
            statusline_sessions: None,
            gh_runner: None,
            spawn: None,
            path_exists: None,
            real_path: None,
            recent_spawns: None,
            inbox_sources: Vec::new(),
            inbox_adapters: Vec::new(),
            inbox: None,
            usage: None,
        }
    }
}

pub struct ServerState {
    pub store: Arc<TodoStore>,
    statusline_template: String,
    sessions: SessionsProvider,
    spawn_sessions: SessionsProvider,
    statusline_sessions: SessionsProvider,
    gh_runner: Runner,
    spawn: SpawnFn,
    path_exists: PathExists,
    real_path: RealPath,
    recent_spawns: Arc<RecentSpawns>,
    inbox: InboxProvider,
    inbox_adapters: Arc<Vec<InboxSource>>,
    /// 지금 조회할 수집함 소스 목록(설정 + 보드 등록) — 구독 감시가 소스 하나만 골라 돌릴 때 쓴다.
    pub inbox_sources: SourcesFn,
    /// 설정 파일의 소스 이름 — 보드 수집함 이름이 겹치지 않게.
    inbox_config_names: Vec<String>,
    usage: UsageSink,
    /// SSE 팬아웃 — 스토어 리스너가 밀어 넣는다.
    pub events: broadcast::Sender<String>,
    /// 노트별 문서 스트림(`GET /api/notes/:ref/doc/events`) — CRDT update 와 프레즌스만.
    /// 전역 `events` 에 싣지 않는 이유: 그 채널의 구독자는 전부 refetch 하므로 글자마다
    /// 보드 전체를 다시 읽게 된다. 구독자가 0 이 된 노트의 채널은 다음 방송 때 걷는다.
    note_streams: Mutex<HashMap<String, broadcast::Sender<String>>>,
    /// PR 감시 잡의 마지막 결과 — health 가 낸다.
    pr_watch: Mutex<crate::prwatch::PrWatchStatus>,
    /// PR 감시가 마지막으로 본 gh 로그인 계정 — 보드 `prAuthors` 의 `@me`.
    gh_viewer: Mutex<Option<String>>,
    /// 세션 받은편지함 등록부 — 훅이 `session_id → 소켓` 을 알려 준다(`rocky_core::peer_inbox`).
    /// 데몬 수명 상태다: 훅이 턴마다 다시 등록하므로 재기동 뒤 첫 턴에 다시 채워진다.
    inboxes: Mutex<HashMap<String, rocky_core::peer_inbox::InboxRegistration>>,
    /// 세션에 보낸 기록 — 최근 `DELIVERY_LOG_MAX` 건, 메모리에만(웹 "세션 전달" 현황).
    deliveries: Mutex<std::collections::VecDeque<rocky_core::peer_inbox::Delivery>>,
    /// "보내지 않기" 를 켠 세션 — PR·수집함 알림을 이 세션에는 보내지 않는다. 메모리에만(데몬을 다시
    /// 띄우면 풀린다 — 세션은 오래 살지 않는다).
    muted: Mutex<std::collections::HashSet<String>>,
    /// 스토어 구독 해제용.
    _subscription: u64,
    _doc_subscription: u64,
}

impl ServerState {
    /// MCP 도구가 이슈 생성에 쓰는 러너 — REST 와 같은 주입을 공유한다.
    pub fn mcp_gh_runner(&self) -> Runner {
        self.gh_runner.clone()
    }

    /// 일반 라우트와 같은(TTL 캐시) 세션 목록 — 스윕이 쓴다.
    pub async fn sessions(&self) -> SessionsResult {
        (self.sessions)().await
    }

    /// 사용 로그 한 건 — REST 입구와 MCP 도구가 부른다.
    pub fn record_usage(&self, event: UsageEvent) {
        (self.usage)(event);
    }

    /// 노트의 문서 스트림 — 없으면 만든다. 테스트가 구독해 방송을 본다.
    /// PR 감시 상태 — 잡이 tick 마다 갱신한다.
    pub fn set_gh_viewer(&self, viewer: Option<String>) {
        if viewer.is_some() {
            *self.gh_viewer.lock().expect("gh_viewer poisoned") = viewer;
        }
    }

    pub fn gh_viewer(&self) -> Option<String> {
        self.gh_viewer.lock().expect("gh_viewer poisoned").clone()
    }

    pub fn set_pr_watch(&self, status: crate::prwatch::PrWatchStatus) {
        *self.pr_watch.lock().expect("pr_watch poisoned") = status;
    }

    pub fn pr_watch(&self) -> crate::prwatch::PrWatchStatus {
        self.pr_watch.lock().expect("pr_watch poisoned").clone()
    }

    /// 세션 받은편지함 등록 — 같은 세션이면 덮어쓴다(cwd·소켓이 바뀌었을 수 있다).
    pub fn register_inbox(&self, registration: rocky_core::peer_inbox::InboxRegistration) {
        let mut inboxes = self.inboxes.lock().expect("inboxes poisoned");
        let now = registration.seen_at;
        inboxes.retain(|_, r| now - r.seen_at <= rocky_core::peer_inbox::REGISTRATION_TTL_SECS);
        inboxes.insert(registration.session_id.clone(), registration);
    }

    /// 지금 등록된 세션들(사본).
    pub fn inboxes(&self) -> Vec<rocky_core::peer_inbox::InboxRegistration> {
        self.inboxes
            .lock()
            .expect("inboxes poisoned")
            .values()
            .cloned()
            .collect()
    }

    /// 세션에 보낸 한 건을 적는다(오래된 것부터 버린다).
    pub fn record_delivery(&self, delivery: rocky_core::peer_inbox::Delivery) {
        let mut log = self.deliveries.lock().expect("deliveries poisoned");
        log.push_front(delivery);
        log.truncate(DELIVERY_LOG_MAX);
    }

    /// 최근 보낸 기록 — 새 것부터.
    pub fn deliveries(&self) -> Vec<rocky_core::peer_inbox::Delivery> {
        self.deliveries
            .lock()
            .expect("deliveries poisoned")
            .iter()
            .cloned()
            .collect()
    }

    /// 이 세션에 보내지 않기를 켜거나 끈다.
    pub fn set_muted(&self, session_id: &str, muted: bool) {
        let mut set = self.muted.lock().expect("muted poisoned");
        if muted {
            set.insert(session_id.to_string());
        } else {
            set.remove(session_id);
        }
    }

    pub fn is_muted(&self, session_id: &str) -> bool {
        self.muted
            .lock()
            .expect("muted poisoned")
            .contains(session_id)
    }

    /// 외부 명령 실행기 — 수집함 어댑터·gh 가 같은 것을 쓴다(테스트는 하나를 갈아 끼운다).
    pub fn runner(&self) -> Runner {
        self.gh_runner.clone()
    }

    /// 보내다 실패한 등록을 걷는다 — 세션이 끝나 소켓이 없거나 아무도 안 듣는다.
    pub fn forget_inbox(&self, session_id: &str) {
        self.inboxes
            .lock()
            .expect("inboxes poisoned")
            .remove(session_id);
    }

    /// 노트의 문서 스트림을 **구독한다** — 채널이 없으면 만든다. 구독을 락 안에서 끝내는 이유:
    /// 보내는 쪽(`broadcast_note`)이 "듣는 이 0" 인 채널을 걷어 내므로, 채널을 꺼내 온 뒤
    /// 구독하기 전에 방송이 끼면 걷힌 채널을 구독해 그 뒤로 아무것도 못 받는다.
    pub fn subscribe_note(&self, note_id: &str) -> broadcast::Receiver<String> {
        let mut streams = self.note_streams.lock().expect("note_streams poisoned");
        streams
            .entry(note_id.to_string())
            .or_insert_with(|| broadcast::channel::<String>(NOTE_STREAM_CAPACITY).0)
            .subscribe()
    }

    /// 지금 그 노트를 듣는 연결 수 — 테스트용.
    pub fn note_receivers(&self, note_id: &str) -> usize {
        let streams = self.note_streams.lock().expect("note_streams poisoned");
        streams.get(note_id).map_or(0, |s| s.receiver_count())
    }

    /// 노트 스트림에 한 건 방송. 듣는 이가 없으면 채널을 걷는다(노트 수만큼 채널이 남지 않게).
    /// 테스트가 직접 부르기도 한다(밀린 연결을 끊는지 보려고).
    pub fn broadcast_note(&self, note_id: &str, payload: &serde_json::Value) {
        let mut streams = self.note_streams.lock().expect("note_streams poisoned");
        let Some(sender) = streams.get(note_id) else {
            return;
        };
        if sender.receiver_count() == 0 || sender.send(payload.to_string()).is_err() {
            streams.remove(note_id);
        }
    }
}

/// 노트 스트림 채널 크기 — 이만큼 밀린 연결은 끊는다(`sse_from`, `on_lag`).
const NOTE_STREAM_CAPACITY: usize = 256;

/// 서버 상태를 만든다 — 스토어 change 이벤트를 SSE 브로드캐스트로 잇는다.
pub fn build_server(options: ServerOptions) -> Arc<ServerState> {
    let (events, _) = broadcast::channel::<String>(256);
    let sender = events.clone();
    let subscription = options.store.subscribe(move |event| {
        if let Ok(payload) = serde_json::to_string(event) {
            let _ = sender.send(payload); // 수신자 없음은 정상 (send 는 sync)
        }
    });
    // 노트 문서 갱신은 어느 경로(웹 update·MCP/CLI set/append)든 스토어가 한 건씩 내고, 여기서
    // 그 노트의 스트림에만 방송한다 — 열린 웹 편집기가 에이전트의 편집을 즉시 본다.
    let doc_state: Arc<Mutex<Option<std::sync::Weak<ServerState>>>> = Arc::new(Mutex::new(None));
    let doc_state_for_listener = doc_state.clone();
    let doc_subscription = options.store.subscribe_note_docs(move |event| {
        let Some(state) = doc_state_for_listener
            .lock()
            .ok()
            .and_then(|slot| slot.as_ref().and_then(std::sync::Weak::upgrade))
        else {
            return;
        };
        state.broadcast_note(
            &event.note_id,
            &json!({
                "kind": "update",
                "update": encode_b64(&event.update),
                "client": event.client,
                "actor": event.actor,
            }),
        );
    });
    let default_gh = default_runner();
    let inbox_adapters = Arc::new(options.inbox_adapters.clone());
    let inbox_store = options.store.clone();
    // `--describe` 와 수집함 조회가 같은 러너를 쓴다 — 테스트가 하나만 갈아 끼우면 둘 다 가짜가 된다.
    let gh_runner = options.gh_runner.clone().unwrap_or(default_gh.clone());
    let sources_fn = board_sources_fn(
        inbox_store,
        options.inbox_sources.clone(),
        inbox_adapters.clone(),
    );
    // 주입된 sessions 는 spawn/statusline 조회기의 **폴백**이기도 하다 — 테스트가
    // sessions 하나만 넣었을 때 세 라우트가 같은 결정론적 목록을 보게 한다
    // (TS `resolveSpawnSessions` / statuslineSessions 배선과 동일).
    let injected = options.sessions.clone();
    let sessions = injected
        .clone()
        .unwrap_or_else(|| cached_sessions(default_gh.clone(), Duration::from_secs(3)));
    // spawn 라우트만 기본이 **캐시 없는** 조회기 — 가드가 spawn 이전 스냅샷을 보면 안 된다.
    let spawn_sessions = options
        .spawn_sessions
        .or_else(|| injected.clone())
        .unwrap_or_else(|| uncached_sessions(default_gh.clone()));
    let statusline_sessions = options
        .statusline_sessions
        .or(injected)
        .unwrap_or_else(|| cached_sessions(default_gh.clone(), Duration::from_secs(15)));
    let state = Arc::new(ServerState {
        store: options.store,
        statusline_template: options
            .statusline_template
            .unwrap_or_else(|| DEFAULT_STATUSLINE_TEMPLATE.to_string()),
        sessions,
        spawn_sessions,
        statusline_sessions,
        gh_runner: gh_runner.clone(),
        spawn: options.spawn.unwrap_or_else(default_spawn_fn),
        path_exists: options
            .path_exists
            .unwrap_or_else(|| Arc::new(|path| std::path::Path::new(path).exists())),
        real_path: options.real_path.unwrap_or_else(|| {
            Arc::new(|path| std::fs::canonicalize(path).map(|p| p.to_string_lossy().to_string()))
        }),
        recent_spawns: options
            .recent_spawns
            .unwrap_or_else(|| Arc::new(RecentSpawns::new(RECENT_SPAWN_TTL))),
        inbox: options.inbox.unwrap_or_else(|| {
            cached_inbox_dynamic(
                gh_runner.clone(),
                sources_fn.clone(),
                Duration::from_secs(INBOX_CACHE_TTL_SECS),
            )
        }),
        inbox_adapters: inbox_adapters.clone(),
        inbox_sources: sources_fn.clone(),
        inbox_config_names: options
            .inbox_sources
            .iter()
            .map(|s| s.name.clone())
            .collect(),
        usage: options.usage.unwrap_or_else(noop_sink),
        events,
        note_streams: Mutex::new(HashMap::new()),
        pr_watch: Mutex::new(crate::prwatch::PrWatchStatus::default()),
        gh_viewer: Mutex::new(None),
        inboxes: Mutex::new(HashMap::new()),
        deliveries: Mutex::new(std::collections::VecDeque::new()),
        muted: Mutex::new(std::collections::HashSet::new()),
        _subscription: subscription,
        _doc_subscription: doc_subscription,
    });
    // 문서 이벤트 리스너는 상태를 약하게 잡는다 — 강하게 잡으면 서로를 물고 영영 안 죽는다.
    if let Ok(mut slot) = doc_state.lock() {
        *slot = Some(Arc::downgrade(&state));
    }
    state
}

// ── 응답 헬퍼 ───────────────────────────────────────────────────────────────

fn json_response(body: &impl serde::Serialize, status: StatusCode) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::to_string(body).unwrap_or_else(|_| "null".into()),
        ))
        .unwrap()
}

fn ok_json(body: &impl serde::Serialize) -> Response {
    json_response(body, StatusCode::OK)
}

fn error_response(message: &str, status: StatusCode) -> Response {
    json_response(&json!({ "error": message }), status)
}

/// 이슈 중복 응답 — 사전 검사와 orchestrator 경유가 **같은 본문**을 내도록 한 곳에.
fn already_has_issue(url: &str) -> Response {
    json_response(
        &json!({ "error": format!("todo already has a GitHub issue: {url}"), "url": url }),
        StatusCode::CONFLICT,
    )
}

/// not found 류 스토어 에러를 HTTP status 로 번역한다.
fn to_http_error(error: &StoreError) -> Response {
    let message = error.to_string();
    let status = if message.to_lowercase().contains("not found") {
        StatusCode::NOT_FOUND
    } else {
        StatusCode::BAD_REQUEST
    };
    error_response(&message, status)
}

fn plain(body: &str) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(Body::from(body.to_string()))
        .unwrap()
}

// ── 요청 파싱 ───────────────────────────────────────────────────────────────

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = &input[i + 1..i + 3];
            if let Ok(byte) = u8::from_str_radix(hex, 16) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

/// 쿼리스트링 → 첫 값 우선 맵. `+` 는 공백.
fn query_params(query: Option<&str>) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let Some(query) = query else {
        return out;
    };
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let key = percent_decode(&key.replace('+', " "));
        let value = percent_decode(&value.replace('+', " "));
        out.entry(key).or_insert(value);
    }
    out
}

fn header_of(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
}

/// 변경 본문은 `application/json` 만 받는다 — `<form enctype="text/plain">` 의
/// preflight 없는 cross-site 쓰기에 대한 심층 방어.
fn assert_json_content_type(headers: &HeaderMap) -> Result<(), StoreError> {
    let content_type = header_of(headers, "content-type").unwrap_or_default();
    if !content_type.to_lowercase().contains("application/json") {
        return Err(StoreError::new(format!(
            "content-type must be application/json (got: {})",
            if content_type.is_empty() {
                "(없음)"
            } else {
                &content_type
            }
        )));
    }
    Ok(())
}

async fn read_raw_body(body: Body) -> Result<String, StoreError> {
    let bytes = axum::body::to_bytes(body, 16 * 1024 * 1024)
        .await
        .map_err(|e| StoreError::new(e.to_string()))?;
    Ok(String::from_utf8_lossy(&bytes).to_string())
}

fn parse_json_object(text: &str) -> Result<serde_json::Map<String, Value>, StoreError> {
    let parsed: Value =
        serde_json::from_str(text).map_err(|_| StoreError::new("invalid JSON body"))?;
    match parsed {
        Value::Object(map) => Ok(map),
        // 배열도 object 다 — 필드 접근이 조용히 흘러가기 전에 막는다.
        _ => Err(StoreError::new("body must be a JSON object")),
    }
}

async fn read_body(
    headers: &HeaderMap,
    body: Body,
) -> Result<serde_json::Map<String, Value>, StoreError> {
    assert_json_content_type(headers)?;
    let text = read_raw_body(body).await?;
    parse_json_object(&text)
}

/// 몸통이 아예 없어도 되는 라우트용(issue/spawn). 빈 본문 + content-type **있음**이면
/// JSON 타입 강제(폼의 마지막 우회로 차단), 헤더도 본문도 없으면 무검사 통과.
async fn read_optional_body(
    headers: &HeaderMap,
    body: Body,
) -> Result<Option<serde_json::Map<String, Value>>, StoreError> {
    let text = read_raw_body(body).await?;
    if text.trim().is_empty() {
        if headers.contains_key("content-type") {
            assert_json_content_type(headers)?;
        }
        return Ok(None);
    }
    assert_json_content_type(headers)?;
    let parsed: Value =
        serde_json::from_str(&text).map_err(|_| StoreError::new("invalid JSON body"))?;
    match parsed {
        Value::Object(map) => Ok(Some(map)),
        _ => Err(StoreError::new("body must be a JSON object")),
    }
}

fn str_field<'a>(body: &'a serde_json::Map<String, Value>, name: &str) -> Option<&'a str> {
    body.get(name).and_then(|v| v.as_str())
}

// ── view 조립 ───────────────────────────────────────────────────────────────

/// 응답용 todo 에 doingState 를 얹는다 — doing 인 항목에만, 세션 조회를 했을 때만.
/// 필드 부재 = "판정하지 않았다".
fn with_doing_state(
    store: &TodoStore,
    todo: Todo,
    sessions: Option<&SessionsResult>,
) -> StoreResult<TodoView> {
    let is_doing = todo.status == TodoStatus::Doing;
    let board_id = todo.board_id.clone();
    let mut view = with_ref_todo(store, todo)?;
    if let (Some(sessions), true) = (sessions, is_doing) {
        let board_key = store.board_key_of(&board_id)?.unwrap_or_default();
        view.doing_state = Some(resolve_doing_state(&view.todo, &board_key, sessions));
    }
    Ok(view)
}

/// 이 세션을 가리키는 식별자 전부 — full UUID 와 spawn 의 짧은 8자 id.
fn session_aliases(session: &str, sessions: &SessionsResult) -> Vec<String> {
    let mut aliases = vec![session.to_string()];
    if let Some(found) = sessions
        .sessions
        .iter()
        .find(|s| s.session_id == session || s.id.as_deref() == Some(session))
    {
        aliases.push(found.session_id.clone());
        if let Some(id) = &found.id {
            aliases.push(id.clone());
        }
    }
    aliases
}

/// 응답 전용 핸드오프 — 저장 모델 + phase/unstarted/stale (TS `HandoffView`).
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct HandoffViewOut {
    #[serde(flatten)]
    handoff: Handoff,
    phase: HandoffPhase,
    unstarted: bool,
    stale: bool,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionOut {
    #[serde(flatten)]
    session: AgentSession,
    matched: bool,
}

// ── 메인 핸들러 ─────────────────────────────────────────────────────────────

/// `/api/*` 요청 하나를 처리한다.
///
/// @param peer_address 요청 소켓의 주소 — 생략하면 루프백이 아닌 것으로 취급(fail-closed).
pub async fn handle_api(
    state: &Arc<ServerState>,
    req: Request<Body>,
    peer_address: Option<String>,
) -> Response {
    let (parts, body) = req.into_parts();
    let method = parts.method;
    let path = parts.uri.path().to_string();
    let query = query_params(parts.uri.query());
    let headers = parts.headers;
    let actor = header_of(&headers, "x-rocky-actor").unwrap_or_else(|| "unknown".to_string());
    let local = is_local_request(peer_address.as_deref(), |name| headers.contains_key(name));

    // 다른 사이트가 시킨 변경은 라우트를 보기도 전에 끊는다. 읽기는 통과.
    let host = header_of(&headers, "host").unwrap_or_else(|| "localhost".to_string());
    let req_url = format!("http://{host}{path}");
    if is_mutating(&method) && is_cross_site_request(|name| header_of(&headers, name), &req_url) {
        return error_response(CROSS_SITE_MESSAGE, StatusCode::FORBIDDEN);
    }

    let started = std::time::Instant::now();
    let response =
        match dispatch(state, &method, &path, &query, &headers, body, &actor, local).await {
            Ok(response) => response,
            Err(error) => to_http_error(&error),
        };
    // 사용 로그 — 이름은 모양만(`GET /api/todos/:ref`), 1초마다 도는 라우트는 빠진다.
    if let Some(name) = normalize_route(method.as_str(), &path) {
        let mut event = UsageEvent::new(UsageSource::Rest, name, response.status().as_u16() < 400);
        event.actor = Some(actor.clone());
        event.client = Some(client_of(
            header_of(&headers, "x-rocky-client").as_deref(),
            header_of(&headers, "user-agent").as_deref(),
        ));
        event.ms = Some(started.elapsed().as_millis() as u64);
        state.record_usage(event);
    }
    response
}

/// `?board=` 쿼리(보드 key) → boardId. 없으면 None. 있는데 안 풀리면 — ref 가 맨숫자
/// 꼴일 때만 에러(400), 아니면 무시(CLI 가 cwd 유추 키를 무조건 붙이는 것 대응).
fn current_board_id_of(
    store: &TodoStore,
    query: &HashMap<String, String>,
    r: &str,
) -> StoreResult<Option<String>> {
    let Some(key) = query.get("board").filter(|k| !k.is_empty()) else {
        return Ok(None);
    };
    match store.board_id_of(key)? {
        Some(board_id) => Ok(Some(board_id)),
        None => {
            if ref_needs_board_context(r) {
                Err(StoreError::new(format!("unknown board: {key}")))
            } else {
                Ok(None)
            }
        }
    }
}

fn seg_match(path: &str, prefix: &str, suffix: &str) -> Option<String> {
    let rest = path.strip_prefix(prefix)?;
    let rest = rest.strip_suffix(suffix)?;
    if rest.is_empty() || rest.contains('/') {
        return None;
    }
    Some(percent_decode(rest))
}

/// `/api/notes/:ref/(archive|unarchive)` 류 — (ref, 마지막 세그먼트).
fn seg2_match<'a>(path: &str, prefix: &str, tails: &[&'a str]) -> Option<(String, &'a str)> {
    let rest = path.strip_prefix(prefix)?;
    let (first, second) = rest.split_once('/')?;
    if first.is_empty() || second.contains('/') {
        return None;
    }
    let tail = tails.iter().find(|t| **t == second)?;
    Some((percent_decode(first), tail))
}

#[allow(clippy::too_many_arguments)]
async fn dispatch(
    state: &Arc<ServerState>,
    method: &Method,
    path: &str,
    query: &HashMap<String, String>,
    headers: &HeaderMap,
    body: Body,
    actor: &str,
    local: bool,
) -> StoreResult<Response> {
    let store = &state.store;
    let flag = |name: &str| query.get(name).map(String::as_str) == Some("true");

    // ── usage — 웹 UI 가 서버를 안 거치는 조작을 이름으로 남긴다 ──
    if *method == Method::POST && path == "/api/usage" {
        let body = read_body(headers, body).await?;
        let name = body
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|n| n.starts_with("web:") && n.len() <= 64 && !n.contains(char::is_whitespace))
            .ok_or_else(|| StoreError::new("name must be `web:<event>` (≤64 chars)"))?;
        let mut event = UsageEvent::new(UsageSource::Web, name, true);
        event.actor = Some(actor.to_string());
        event.client = Some("web".into());
        // meta 는 작은 객체만 — 내용을 실어 오는 통로가 되지 않게 256자에서 자른다.
        event.meta = body
            .get("meta")
            .filter(|m| m.is_object() && m.to_string().len() <= 256)
            .cloned();
        state.record_usage(event);
        return Ok(ok_json(&json!({ "ok": true })));
    }

    // ── health ──
    if *method == Method::GET && path == "/api/health" {
        return Ok(ok_json(&json!({
            "ok": true,
            "name": "rocky",
            "version": env!("CARGO_PKG_VERSION"),
            "pid": std::process::id(),
            "issueCreateAllowed": local,
            "spawnAllowed": local,
            "prWatch": state.pr_watch(),
        })));
    }

    // ── statusline ──
    if *method == Method::GET && path == "/api/statusline" {
        return Ok(statusline_of(state, query).await);
    }

    // ── inbox (수집함 — 외부 투두 앱 읽기 전용) ──
    if *method == Method::GET && path == "/api/inbox" {
        let mode = if flag("refresh") {
            InboxFetch::Refresh
        } else if flag("cached") {
            InboxFetch::CachedOnly
        } else {
            InboxFetch::Normal
        };
        let scope = match query.get("board") {
            Some(board) => InboxScope::Board(Some(board.as_str())),
            None => InboxScope::All,
        };
        let response = marked_inbox(state, mode, scope).await?;
        // 실패 사유의 stderr·출력 조각은 로컬 요청에만 — 원격에는 exit code 만.
        let response = if local { response } else { response.redacted() };
        return Ok(ok_json(&response));
    }

    // ── 보드 수집함 설정 — 어댑터 칸 목록 · 등록 · 삭제는 로컬 전용(값이 실행 인자가 된다) ──
    if path == "/api/inbox/adapters" && *method == Method::GET {
        if !local {
            return Ok(error_response(
                NON_LOCAL_INBOX_SOURCE_MESSAGE,
                StatusCode::FORBIDDEN,
            ));
        }
        let mut out = Vec::new();
        for adapter in state.inbox_adapters.iter() {
            out.push(match describe_adapter(state, adapter).await {
                Ok(d) => json!({ "name": adapter.name, "title": d.title, "params": d.params }),
                Err(error) => json!({ "name": adapter.name, "error": error }),
            });
        }
        return Ok(ok_json(&out));
    }
    if path == "/api/inbox/sources" && *method == Method::GET {
        let board = query.get("board").map(String::as_str);
        let list: Vec<Value> = state
            .store
            .list_board_inbox_sources(board)?
            .into_iter()
            .map(|s| {
                let missing = !state.inbox_adapters.iter().any(|a| a.name == s.adapter);
                let clash = state.inbox_config_names.contains(&s.name);
                let mut v = serde_json::to_value(&s).unwrap_or(Value::Null);
                if missing {
                    v["adapterMissing"] = json!(true);
                }
                if clash {
                    v["nameClash"] = json!(true);
                }
                v
            })
            .collect();
        return Ok(ok_json(&list));
    }
    if path == "/api/inbox/sources" && *method == Method::POST {
        if !local {
            return Ok(error_response(
                NON_LOCAL_INBOX_SOURCE_MESSAGE,
                StatusCode::FORBIDDEN,
            ));
        }
        let body = read_body(headers, body).await?;
        let board = str_field(&body, "board").unwrap_or("").trim().to_string();
        let name = str_field(&body, "name").unwrap_or("").trim().to_string();
        let adapter_name = str_field(&body, "adapter").unwrap_or("").trim().to_string();
        if board.is_empty() || name.is_empty() || adapter_name.is_empty() {
            return Ok(error_response(
                "board, name, adapter are required",
                StatusCode::BAD_REQUEST,
            ));
        }
        if state.inbox_config_names.contains(&name) {
            return Ok(error_response(
                &format!("설정 파일의 수집함과 이름이 겹친다: {name}"),
                StatusCode::BAD_REQUEST,
            ));
        }
        let Some(adapter) = state.inbox_adapters.iter().find(|a| a.name == adapter_name) else {
            return Ok(error_response(
                &format!("rocky.json 의 todo.inboxAdapters[] 에 없는 어댑터: {adapter_name}"),
                StatusCode::BAD_REQUEST,
            ));
        };
        let mut values = std::collections::BTreeMap::new();
        let params_value = body.get("params").cloned().unwrap_or(json!({}));
        let Some(obj) = params_value.as_object() else {
            return Ok(error_response(
                "params 는 {플래그: 값} 객체여야 한다",
                StatusCode::BAD_REQUEST,
            ));
        };
        {
            for (flag, value) in obj {
                let Some(text) = value.as_str() else {
                    return Ok(error_response(
                        &format!("params.{flag} 는 문자열이어야 한다"),
                        StatusCode::BAD_REQUEST,
                    ));
                };
                values.insert(flag.clone(), text.to_string());
            }
        }
        let describe = match describe_adapter(state, adapter).await {
            Ok(d) => d,
            Err(error) => {
                return Ok(error_response(
                    &format!("{adapter_name} 어댑터의 --describe 실패: {error}"),
                    StatusCode::BAD_GATEWAY,
                ))
            }
        };
        let params = match rocky_core::inbox::validate_params(&describe, &values) {
            Ok(p) => p,
            Err(error) => return Ok(error_response(&error, StatusCode::BAD_REQUEST)),
        };
        let created =
            state
                .store
                .create_board_inbox_source(&board, &name, &adapter_name, &params, actor)?;
        return Ok(ok_json(&created));
    }
    if *method == Method::DELETE {
        if let Some(id) = seg_match(path, "/api/inbox/sources/", "") {
            if !local {
                return Ok(error_response(
                    NON_LOCAL_INBOX_SOURCE_MESSAGE,
                    StatusCode::FORBIDDEN,
                ));
            }
            state.store.delete_board_inbox_source(&id, actor)?;
            return Ok(ok_json(&json!({ "ok": true })));
        }
    }

    // ── 수집함 구독 — 세션이 소스를 구독하면 새 항목을 그 세션 받은편지함으로 보낸다(로컬 전용) ──
    if path == "/api/inbox/subscriptions" && *method == Method::GET {
        // 세션 id 는 로컬에만 — 노출된 화면에는 소스별 구독 세션 수만 내린다(웹 머리줄이 쓰는 것도 그것뿐).
        let list: Vec<Value> = state
            .store
            .inbox_subscriptions()?
            .into_iter()
            .map(|s| {
                if local {
                    json!({ "source": s.source, "sessionId": s.session_id })
                } else {
                    json!({ "source": s.source })
                }
            })
            .collect();
        return Ok(ok_json(&list));
    }
    if path == "/api/inbox/subscriptions" && *method == Method::POST {
        if !local {
            return Ok(error_response(
                NON_LOCAL_INBOX_SOURCE_MESSAGE,
                StatusCode::FORBIDDEN,
            ));
        }
        let body = read_body(headers, body).await?;
        let source_name = str_field(&body, "source").unwrap_or("").trim().to_string();
        let session_id = str_field(&body, "sessionId")
            .unwrap_or("")
            .trim()
            .to_string();
        let socket = str_field(&body, "socket").unwrap_or("").trim().to_string();
        if source_name.is_empty() || session_id.is_empty() {
            return Ok(error_response(
                "source, sessionId are required",
                StatusCode::BAD_REQUEST,
            ));
        }
        // 데몬이 이 경로에 쓴다 — Claude Code 받은편지함 모양만 받는다(세션 등록과 같은 가드).
        if !rocky_core::peer_inbox::is_inbox_socket_path(&socket) {
            return Ok(error_response(
                "socket 은 Claude Code 받은편지함 소켓이어야 한다(CLAUDE_CODE_MESSAGING_SOCKET) — 이 세션은 받은편지함이 없다",
                StatusCode::BAD_REQUEST,
            ));
        }
        let Some(source) = (state.inbox_sources)()
            .into_iter()
            .find(|s| s.name == source_name)
        else {
            return Ok(error_response(
                &format!("없는 수집함 소스: {source_name} — `rocky inbox` 로 이름을 본다"),
                StatusCode::NOT_FOUND,
            ));
        };
        // 구독 시점에 이미 있던 항목은 "본 것" — 켜자마자 옛 항목이 몰려오지 않게. 못 읽으면 기준선을
        // 잡을 수 없으니 구독하지 않는다(다음 조회에 전부가 새 항목으로 쏟아진다).
        // CLI 요청 한도(30초)보다 먼저 끝나게 자른다 — 넘기면 CLI 는 실패라는데 구독은 저장되는 어긋남이 난다.
        let mut bounded = source.clone();
        bounded.timeout_ms = Some(bounded.timeout_ms.unwrap_or(10_000).min(20_000));
        let result = crate::inbox_exec::fetch_source(&state.gh_runner, &bounded).await;
        if !result.available {
            return Ok(error_response(
                &format!(
                    "{source_name} 을 지금 읽지 못해 구독하지 않았다 — {}",
                    result.reason.as_deref().unwrap_or("사유 없음")
                ),
                StatusCode::BAD_GATEWAY,
            ));
        }
        let baseline: Vec<String> = result.items.iter().map(|i| i.id.clone()).collect();
        state.store.subscribe_inbox(
            &source_name,
            &session_id,
            &socket,
            &crate::inbox_exec::cache_key(&source),
            &baseline,
        )?;
        // 감시는 살아 있는 등록으로만 보낸다 — 구독한 지금부터 보낼 수 있게 등록도 갱신한다(훅과 같은 등록).
        state.register_inbox(rocky_core::peer_inbox::InboxRegistration {
            session_id: session_id.clone(),
            socket: socket.clone(),
            cwd: str_field(&body, "cwd").unwrap_or("").to_string(),
            seen_at: chrono::Utc::now().timestamp(),
        });
        return Ok(ok_json(&json!({
            "source": source_name,
            "sessionId": session_id,
            "baseline": baseline.len(),
        })));
    }
    if path == "/api/inbox/subscriptions" && *method == Method::DELETE {
        if !local {
            return Ok(error_response(
                NON_LOCAL_INBOX_SOURCE_MESSAGE,
                StatusCode::FORBIDDEN,
            ));
        }
        let Some(session_id) = query.get("sessionId").filter(|s| !s.is_empty()) else {
            return Ok(error_response(
                "sessionId is required",
                StatusCode::BAD_REQUEST,
            ));
        };
        let removed = state
            .store
            .unsubscribe_inbox(query.get("source").map(String::as_str), session_id)?;
        return Ok(ok_json(&json!({ "removed": removed })));
    }

    // ── 세션 전달 현황 — 어느 세션이 PR·수집함 알림을 받나, 최근 보낸 기록, 보내지 않기(로컬 전용) ──
    if path == "/api/deliveries" && *method == Method::GET {
        if !local {
            return Ok(error_response(
                NON_LOCAL_INBOX_SOURCE_MESSAGE,
                StatusCode::FORBIDDEN,
            ));
        }
        let boards = state.store.list_boards(false)?;
        let locations: Vec<BoardLocation> = boards
            .iter()
            .map(|b| BoardLocation {
                key: b.key.clone(),
                path: b.path.clone(),
            })
            .collect();
        let now = chrono::Utc::now().timestamp();
        let registrations = state.inboxes();
        // PR 알림을 받는 세션 — 그 세션이 구독한 PR 들(`repo#N`). 알림기와 같은 규칙: 구독한 세션에만 간다.
        let mut receives: HashMap<String, Vec<String>> = HashMap::new();
        for sub in state.store.pr_subscriptions()? {
            if let Some(id) = sub.session_id {
                receives
                    .entry(id)
                    .or_default()
                    .push(format!("{}#{}", sub.repo, sub.number));
            }
        }
        let mut sessions: Vec<Value> = registrations
            .iter()
            .filter(|r| now - r.seen_at <= rocky_core::peer_inbox::REGISTRATION_TTL_SECS)
            .map(|r| {
                json!({
                    "sessionId": r.session_id,
                    "cwd": r.cwd,
                    "board": board_key_for_cwd(&locations, Some(&r.cwd)),
                    "seenAt": chrono::DateTime::from_timestamp(r.seen_at, 0)
                        .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
                    "muted": state.is_muted(&r.session_id),
                    "receivesPrFor": receives.get(&r.session_id).cloned().unwrap_or_default(),
                })
            })
            .collect();
        sessions.sort_by(|a, b| b["seenAt"].as_str().cmp(&a["seenAt"].as_str()));
        let subscriptions: Vec<Value> = state
            .store
            .inbox_subscriptions()?
            .into_iter()
            .map(|s| json!({ "source": s.source, "sessionId": s.session_id }))
            .collect();
        return Ok(ok_json(&json!({
            "sessions": sessions,
            "subscriptions": subscriptions,
            "recent": state.deliveries(),
        })));
    }
    if path == "/api/deliveries/mute" && *method == Method::POST {
        if !local {
            return Ok(error_response(
                NON_LOCAL_INBOX_SOURCE_MESSAGE,
                StatusCode::FORBIDDEN,
            ));
        }
        let body = read_body(headers, body).await?;
        let session_id = str_field(&body, "sessionId")
            .unwrap_or("")
            .trim()
            .to_string();
        let Some(muted) = body.get("muted").and_then(Value::as_bool) else {
            return Ok(error_response(
                "sessionId 와 muted(true|false) 가 필요하다",
                StatusCode::BAD_REQUEST,
            ));
        };
        if session_id.is_empty() {
            return Ok(error_response(
                "sessionId is required",
                StatusCode::BAD_REQUEST,
            ));
        }
        state.set_muted(&session_id, muted);
        return Ok(ok_json(&json!({ "sessionId": session_id, "muted": muted })));
    }

    // ── summary (rocky today · SessionStart 요약) ──
    if *method == Method::GET && path == "/api/summary" {
        let mode = if flag("cached") {
            InboxFetch::CachedOnly
        } else {
            InboxFetch::Normal
        };
        return Ok(ok_json(&summary_of(state, query, mode).await?));
    }

    // ── SSE ──
    if *method == Method::GET && path == "/api/events" {
        return Ok(sse_response(state));
    }

    // ── boards ──
    if *method == Method::GET && path == "/api/boards" {
        return Ok(ok_json(&store.list_boards(flag("includeArchived"))?));
    }
    if *method == Method::POST && path == "/api/boards" {
        let body = read_body(headers, body).await?;
        let Some(key) = str_field(&body, "key").filter(|k| !k.is_empty()) else {
            return Ok(error_response("key is required", StatusCode::BAD_REQUEST));
        };
        let board = store.ensure_board(key, str_field(&body, "title"), actor)?;
        return Ok(json_response(&board, StatusCode::CREATED));
    }
    if *method == Method::PATCH {
        if let Some(key) = seg_match(path, "/api/boards/", "") {
            let body = read_body(headers, body).await?;
            // path/repo 는 소비 지점이 로컬 전용인 값 — 변경도 로컬 전용이다. autoResolve 도 같다:
            // 켜면 데몬이 세션에 일을 시키므로 보드 쓰기가 세션 조종으로 넓어지는 지점이다.
            if (body.contains_key("path")
                || body.contains_key("repo")
                || body.contains_key("autoResolve")
                || body.contains_key("prAuthors"))
                && !local
            {
                return Ok(error_response(
                    NON_LOCAL_BOARD_META_MESSAGE,
                    StatusCode::FORBIDDEN,
                ));
            }
            // 어느 필드를 고치려던 요청인지는 **키 존재 여부**로 가른다.
            let mut patch = BoardPatch::default();
            let mut any = false;
            if let Some(value) = body.get("autoResolve") {
                let Some(on) = value.as_bool() else {
                    return Ok(error_response(
                        "autoResolve must be true or false",
                        StatusCode::BAD_REQUEST,
                    ));
                };
                patch.auto_resolve = Some(on);
                any = true;
            }
            // 알릴 PR 작성자 — 배열(`@me`·login), `null`·빈 배열은 지우기(전부 알림). 모양은 스토어가 검증한다.
            if let Some(value) = body.get("prAuthors") {
                let authors = if value.is_null() {
                    Some(Vec::new())
                } else {
                    value.as_array().and_then(|a| {
                        a.iter()
                            .map(|v| v.as_str().map(|s| s.trim().to_string()))
                            .collect::<Option<Vec<_>>>()
                    })
                };
                let Some(authors) = authors else {
                    return Ok(error_response(
                        "prAuthors 는 문자열 배열(@me 또는 GitHub login) 또는 null 이어야 한다",
                        StatusCode::BAD_REQUEST,
                    ));
                };
                patch.pr_authors = Some(authors);
                any = true;
            }
            for name in ["key", "title", "description", "repo", "path"] {
                let Some(value) = body.get(name) else {
                    continue;
                };
                any = true;
                // 지우기는 `null` 로만 — 빈 문자열은 400 (폼 실수 방어). key/title 은 null 도 거절.
                let clearable = matches!(name, "description" | "repo" | "path");
                if value.is_null() && clearable {
                    match name {
                        "description" => patch.description = Some(None),
                        "repo" => patch.repo = Some(None),
                        _ => patch.path = Some(None),
                    }
                    continue;
                }
                let Some(text) = value.as_str().map(str::trim).filter(|t| !t.is_empty()) else {
                    let message = if clearable {
                        format!("{name} must be a non-empty string or null")
                    } else {
                        format!("{name} must be a non-empty string")
                    };
                    return Ok(error_response(&message, StatusCode::BAD_REQUEST));
                };
                if name == "repo" && !is_repo_slug(text) {
                    return Ok(error_response(
                        "repo must look like OWNER/NAME",
                        StatusCode::BAD_REQUEST,
                    ));
                }
                match name {
                    "key" => patch.key = Some(text.to_string()),
                    "title" => patch.title = Some(text.to_string()),
                    "description" => patch.description = Some(Some(text.to_string())),
                    "repo" => patch.repo = Some(Some(text.to_string())),
                    _ => patch.path = Some(Some(text.to_string())),
                }
            }
            if !any {
                return Ok(error_response(
                    "key, title, description, repo, path, autoResolve or prAuthors is required",
                    StatusCode::BAD_REQUEST,
                ));
            }
            return Ok(ok_json(&store.update_board(&key, &patch, actor)?));
        }
    }

    // ── sections ──
    if *method == Method::GET && path == "/api/sections" {
        let Some(board_key) = query.get("board").filter(|k| !k.is_empty()) else {
            return Ok(error_response(
                "board query parameter is required",
                StatusCode::BAD_REQUEST,
            ));
        };
        let Some(board_id) = store.board_id_of(board_key)? else {
            return Ok(ok_json(&Vec::<Section>::new()));
        };
        return Ok(ok_json(&store.list_sections(&board_id, false)?));
    }
    if *method == Method::POST && path == "/api/sections" {
        let body = read_body(headers, body).await?;
        let Some(board_key) = str_field(&body, "board").filter(|b| !b.is_empty()) else {
            return Ok(error_response("board is required", StatusCode::BAD_REQUEST));
        };
        let title = str_field(&body, "title").unwrap_or("").trim().to_string();
        if title.is_empty() {
            return Ok(error_response("title is required", StatusCode::BAD_REQUEST));
        }
        // 없는 보드를 자동 생성하지 않는다 — 오타난 key 로 빈 보드가 생기는 편이 조용한 사고.
        let Some(board_id) = store.board_id_of(board_key)? else {
            return Ok(error_response(
                &format!("board not found: {board_key}"),
                StatusCode::NOT_FOUND,
            ));
        };
        return Ok(json_response(
            &store.ensure_section(&board_id, &title, actor)?,
            StatusCode::CREATED,
        ));
    }
    if *method == Method::POST {
        if let Some((id, _)) = seg2_match(path, "/api/sections/", &["archive"]) {
            store.archive_section(&id, actor)?;
            return Ok(ok_json(&json!({ "ok": true })));
        }
    }

    // ── todos ──
    if *method == Method::GET && path == "/api/todos" {
        let filter = ListTodosFilter {
            board: query.get("board").cloned(),
            status: query.get("status").and_then(|s| TodoStatus::parse(s)),
            label: query.get("label").cloned(),
            include_archived: flag("includeArchived"),
        };
        let todos = store.list_todos(&filter)?;
        // doing 이 하나도 없으면 세션 조회(동기 spawn ~220ms)를 아예 건너뛴다.
        let sessions = if todos.iter().any(|t| t.status == TodoStatus::Doing) {
            Some((state.sessions)().await)
        } else {
            None
        };
        let views = todos
            .into_iter()
            .map(|todo| with_doing_state(store, todo, sessions.as_ref()))
            .collect::<StoreResult<Vec<_>>>()?;
        return Ok(ok_json(&views));
    }
    if *method == Method::POST && path == "/api/todos" {
        let body = read_body(headers, body).await?;
        let Some(title) = str_field(&body, "title").filter(|t| !t.is_empty()) else {
            return Ok(error_response("title is required", StatusCode::BAD_REQUEST));
        };
        let Some(board) = str_field(&body, "board").filter(|b| !b.is_empty()) else {
            return Ok(error_response("board is required", StatusCode::BAD_REQUEST));
        };
        let input = CreateTodoInput {
            board: board.to_string(),
            title: title.to_string(),
            description: str_field(&body, "description").map(str::to_string),
            section: str_field(&body, "section").map(str::to_string),
            parent_id: str_field(&body, "parentId").map(str::to_string),
            priority: str_field(&body, "priority").and_then(TodoPriority::parse),
            due: str_field(&body, "due").map(str::to_string),
            labels: body
                .get("labels")
                .and_then(|v| serde_json::from_value::<Vec<String>>(v.clone()).ok()),
            links: body
                .get("links")
                .and_then(|v| serde_json::from_value::<Vec<TodoLink>>(v.clone()).ok()),
        };
        let todo = store.create_todo(&input, actor)?;
        return Ok(json_response(
            &with_ref_todo(store, todo)?,
            StatusCode::CREATED,
        ));
    }

    // /api/todos/:ref — GET/PATCH
    if let Some(r) = seg_match(path, "/api/todos/", "") {
        let current_board_id = current_board_id_of(store, query, &r)?;
        if *method == Method::GET {
            let Some(todo) = store.get_todo(&r, current_board_id.as_deref())? else {
                return Ok(error_response(
                    &format!("todo not found: {r}"),
                    StatusCode::NOT_FOUND,
                ));
            };
            let sessions = if todo.status == TodoStatus::Doing {
                Some((state.sessions)().await)
            } else {
                None
            };
            let todo_id = todo.id.clone();
            let view = with_doing_state(store, todo, sessions.as_ref())?;
            let history = store.list_history(&ListHistoryFilter {
                entity_id: Some(todo_id.clone()),
                exclude_actions: DETAIL_HISTORY_EXCLUDED
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
                ..Default::default()
            })?;
            let comments = store.list_comments(&todo_id, flag("includeArchived"))?;
            return Ok(ok_json(
                &json!({ "todo": view, "history": history, "comments": comments }),
            ));
        }
        if *method == Method::PATCH {
            let body = read_body(headers, body).await?;
            let patch = todo_patch_from(&body);
            let updated = store.update_todo(&r, &patch, actor, current_board_id.as_deref())?;
            return Ok(ok_json(&with_ref_todo(store, updated)?));
        }
    }

    // /api/todos/:ref/status
    if *method == Method::POST {
        if let Some((r, _)) = seg2_match(path, "/api/todos/", &["status"]) {
            let current_board_id = current_board_id_of(store, query, &r)?;
            let body = read_body(headers, body).await?;
            let action = str_field(&body, "action").and_then(StatusAction::parse);
            let Some(action) = action else {
                let raw = body
                    .get("action")
                    .map(value_display)
                    .unwrap_or_else(|| "undefined".into());
                return Ok(error_response(
                    &format!("invalid action: {raw}"),
                    StatusCode::BAD_REQUEST,
                ));
            };
            let updated = store.set_todo_status(&r, action, actor, current_board_id.as_deref())?;
            return Ok(ok_json(&with_ref_todo(store, updated)?));
        }
        if let Some((r, _)) = seg2_match(path, "/api/todos/", &["issue"]) {
            return issue_route(state, &r, query, headers, body, actor, local).await;
        }
        if let Some((r, _)) = seg2_match(path, "/api/todos/", &["board"]) {
            let body = read_body(headers, body).await?;
            let Some(target) = str_field(&body, "board").filter(|b| !b.is_empty()) else {
                return Ok(error_response(
                    "board is required (target board key)",
                    StatusCode::BAD_REQUEST,
                ));
            };
            let current_board_id = current_board_id_of(store, query, &r)?;
            let moved = store.move_todo_to_board(&r, target, actor, current_board_id.as_deref())?;
            return Ok(ok_json(&with_ref_todo(store, moved)?));
        }
        if let Some((r, _)) = seg2_match(path, "/api/todos/", &["move"]) {
            let body = read_body(headers, body).await?;
            // before 키는 **명시**해야 한다 — null(맨 끝)과 "빠뜨림"을 구분.
            let Some(before_value) = body.get("before") else {
                return Ok(error_response(
                    "before is required (todo ref, or null for end)",
                    StatusCode::BAD_REQUEST,
                ));
            };
            let before = match before_value {
                Value::Null => None,
                Value::String(s) => Some(s.clone()),
                _ => {
                    return Ok(error_response(
                        "before must be a todo ref or null",
                        StatusCode::BAD_REQUEST,
                    ))
                }
            };
            let current_board_id = current_board_id_of(store, query, &r)?;
            let moved =
                store.move_todo(&r, before.as_deref(), actor, current_board_id.as_deref())?;
            return Ok(ok_json(&with_ref_todo(store, moved)?));
        }
        if let Some((r, _)) = seg2_match(path, "/api/todos/", &["handoff"]) {
            return handoff_route(state, &r, query, headers, body, actor).await;
        }
        if let Some((r, _)) = seg2_match(path, "/api/todos/", &["spawn"]) {
            return spawn_route(state, &r, query, headers, body, actor, local).await;
        }
        if let Some((r, _)) = seg2_match(path, "/api/todos/", &["comments"]) {
            let current_board_id = current_board_id_of(store, query, &r)?;
            let body = read_body(headers, body).await?;
            let Some(comment_body) = str_field(&body, "body") else {
                return Ok(error_response("body is required", StatusCode::BAD_REQUEST));
            };
            let comment =
                store.add_comment(&r, comment_body, actor, current_board_id.as_deref())?;
            return Ok(json_response(&comment, StatusCode::CREATED));
        }
    }

    // ── comments ──
    if *method == Method::PATCH {
        if let Some(id) = seg_match(path, "/api/comments/", "") {
            let body = read_body(headers, body).await?;
            let Some(comment_body) = str_field(&body, "body") else {
                return Ok(error_response("body is required", StatusCode::BAD_REQUEST));
            };
            return Ok(ok_json(&store.update_comment(&id, comment_body, actor)?));
        }
    }
    if *method == Method::POST {
        if let Some((id, tail)) = seg2_match(path, "/api/comments/", &["archive", "unarchive"]) {
            return Ok(ok_json(&store.set_comment_archived(
                &id,
                tail == "archive",
                actor,
            )?));
        }
    }

    // ── notes ──
    if *method == Method::GET && path == "/api/notes" {
        let notes = store.list_notes(&ListNotesFilter {
            board: query.get("board").cloned(),
            global: flag("global"),
            include_archived: flag("includeArchived"),
        })?;
        let views = notes
            .into_iter()
            .map(|note| with_ref_note(store, note))
            .collect::<StoreResult<Vec<NoteView>>>()?;
        return Ok(ok_json(&views));
    }
    if *method == Method::POST && path == "/api/notes" {
        let body = read_body(headers, body).await?;
        let Some(title) = str_field(&body, "title").filter(|t| !t.is_empty()) else {
            return Ok(error_response("title is required", StatusCode::BAD_REQUEST));
        };
        let note = store.create_note(
            &CreateNoteInput {
                board: str_field(&body, "board").map(str::to_string),
                title: title.to_string(),
                content: str_field(&body, "content").map(str::to_string),
            },
            actor,
        )?;
        return Ok(json_response(
            &with_ref_note(store, note)?,
            StatusCode::CREATED,
        ));
    }
    // ── notes: CRDT 문서 (docs/design/specs/2026-09-28-note-crdt-design.md) ──
    if let Some((r, tail)) = seg2_match(path, "/api/notes/", &["doc", "presence"]) {
        let current_board_id = current_board_id_of(store, query, &r)?;
        if tail == "doc" && *method == Method::GET {
            let since = match query.get("sv") {
                Some(sv) => Some(decode_b64(sv).map_err(StoreError::new)?),
                None => None,
            };
            let doc = store.note_doc_state(&r, since.as_deref(), current_board_id.as_deref())?;
            return Ok(ok_json(&json!({
                "noteId": doc.note_id,
                "update": encode_b64(&doc.update),
                "sv": encode_b64(&doc.state_vector),
            })));
        }
        if tail == "doc" && *method == Method::POST {
            let body = read_body(headers, body).await?;
            let Some(update) = str_field(&body, "update") else {
                return Ok(error_response(
                    "update is required",
                    StatusCode::BAD_REQUEST,
                ));
            };
            let bytes = match decode_b64(update) {
                Ok(bytes) => bytes,
                Err(error) => return Ok(error_response(&error, StatusCode::BAD_REQUEST)),
            };
            // 방송은 스토어의 문서 이벤트가 한다(에이전트 경로와 같은 길) — 여기서 따로 하지 않는다.
            let applied = match store.apply_note_update(
                &r,
                &bytes,
                actor,
                str_field(&body, "client"),
                current_board_id.as_deref(),
            ) {
                Ok(applied) => applied,
                Err(error) if error.to_string().starts_with("bad update") => {
                    return Ok(error_response(&error.to_string(), StatusCode::BAD_REQUEST));
                }
                Err(error) => return Err(error),
            };
            return Ok(ok_json(&json!({
                "ok": true,
                "changed": applied.changed,
                "updatedAt": applied.note.updated_at,
            })));
        }
        if tail == "presence" && *method == Method::POST {
            let body = read_body(headers, body).await?;
            let Some(note) = store.get_note(&r, current_board_id.as_deref())? else {
                return Ok(error_response(
                    &format!("note not found: {r}"),
                    StatusCode::NOT_FOUND,
                ));
            };
            // 저장하지 않는다 — 지금 누가 보고 있는지는 지금만 의미가 있다.
            state.broadcast_note(
                &note.id,
                &json!({
                    "kind": "presence",
                    "client": str_field(&body, "client"),
                    "actor": actor,
                    "state": body.get("state").cloned().unwrap_or(Value::Null),
                    "at": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                }),
            );
            return Ok(ok_json(&json!({ "ok": true })));
        }
    }
    if *method == Method::GET {
        if let Some(rest) = path.strip_prefix("/api/notes/") {
            if let Some((r, "doc/events")) = rest.split_once('/') {
                let r = percent_decode(r);
                let current_board_id = current_board_id_of(store, query, &r)?;
                let Some(note) = store.get_note(&r, current_board_id.as_deref())? else {
                    return Ok(error_response(
                        &format!("note not found: {r}"),
                        StatusCode::NOT_FOUND,
                    ));
                };
                // 밀리면 끊는다 — 이 구독자는 refetch 가 아니라 update 를 하나씩 적용하므로 한 건이
                // 빠지면 그 연결이 사는 동안 문서가 낡은 채(뒤 update 는 pending) 남는다. 끊기면
                // 브라우저가 다시 붙고 `GET doc?sv=` 로 빠진 것을 받는다.
                return Ok(sse_from(state.subscribe_note(&note.id), OnLag::Close));
            }
        }
    }

    if let Some(r) = seg_match(path, "/api/notes/", "") {
        let current_board_id = current_board_id_of(store, query, &r)?;
        if *method == Method::GET {
            let Some(note) = store.get_note(&r, current_board_id.as_deref())? else {
                return Ok(error_response(
                    &format!("note not found: {r}"),
                    StatusCode::NOT_FOUND,
                ));
            };
            let history = store.list_history(&ListHistoryFilter {
                entity_id: Some(note.id.clone()),
                ..Default::default()
            })?;
            return Ok(ok_json(
                &json!({ "note": with_ref_note(store, note)?, "history": history }),
            ));
        }
        if *method == Method::PATCH {
            let body = read_body(headers, body).await?;
            let patch = UpdateNotePatch {
                title: str_field(&body, "title").map(str::to_string),
                content: str_field(&body, "content").map(str::to_string),
                mode: if str_field(&body, "mode") == Some("append") {
                    NoteContentMode::Append
                } else {
                    NoteContentMode::Set
                },
            };
            let updated = store.update_note(&r, &patch, actor, current_board_id.as_deref())?;
            return Ok(ok_json(&with_ref_note(store, updated)?));
        }
    }
    if *method == Method::POST {
        if let Some((r, tail)) = seg2_match(
            path,
            "/api/notes/",
            &["archive", "unarchive", "pin", "unpin"],
        ) {
            let current_board_id = current_board_id_of(store, query, &r)?;
            let board_id = current_board_id.as_deref();
            let note = match tail {
                "archive" => store.archive_note(&r, actor, board_id)?,
                "unarchive" => store.unarchive_note(&r, actor, board_id)?,
                _ => store.set_note_pinned(&r, tail == "pin", actor, board_id)?,
            };
            return Ok(ok_json(&with_ref_note(store, note)?));
        }
    }

    // ── sessions ──
    if *method == Method::GET && path == "/api/sessions" {
        let result = (state.sessions)().await;
        let matched: Option<std::collections::HashSet<String>> =
            query.get("board").map(|board_key| {
                match_board(&result.sessions, board_key)
                    .into_iter()
                    .map(|s| s.session_id.clone())
                    .collect()
            });
        let sessions: Vec<SessionOut> = result
            .sessions
            .iter()
            .map(|session| SessionOut {
                session: session.clone(),
                matched: matched
                    .as_ref()
                    .is_some_and(|m| m.contains(&session.session_id)),
            })
            .collect();
        return Ok(ok_json(&json!({
            "available": result.available,
            "reason": result.reason,
            "sessions": sessions,
        })));
    }

    // ── 세션 받은편지함 등록 — 훅만 부른다 ──
    if *method == Method::POST && path == "/api/sessions/inbox" {
        // 데몬이 이 경로에 **쓰게** 되므로 원격에는 존재 자체를 드러내지 않고(404 위장),
        // 경로는 Claude Code 받은편지함 모양만 받는다.
        if !local {
            return Ok(error_response(
                &format!("not found: {method} {path}"),
                StatusCode::NOT_FOUND,
            ));
        }
        let body = read_body(headers, body).await?;
        let session_id = str_field(&body, "sessionId").unwrap_or("");
        let socket = str_field(&body, "socket").unwrap_or("");
        let cwd = str_field(&body, "cwd").unwrap_or("");
        if session_id.is_empty() || cwd.is_empty() {
            return Ok(error_response(
                "sessionId and cwd are required",
                StatusCode::BAD_REQUEST,
            ));
        }
        if !rocky_core::peer_inbox::is_inbox_socket_path(socket) {
            return Ok(error_response(
                &format!("not a Claude Code inbox socket path: {socket}"),
                StatusCode::BAD_REQUEST,
            ));
        }
        state.register_inbox(rocky_core::peer_inbox::InboxRegistration {
            session_id: session_id.to_string(),
            socket: socket.to_string(),
            cwd: cwd.to_string(),
            seen_at: chrono::Utc::now().timestamp(),
        });
        return Ok(Response::builder()
            .status(StatusCode::NO_CONTENT)
            .body(Body::empty())
            .unwrap());
    }

    // ── handoffs ──
    if *method == Method::POST && path == "/api/handoffs/claim" {
        // 훅만 부르는 라우트 — 원격에는 존재 자체를 드러내지 않는다(404 위장).
        if !local {
            return Ok(error_response(
                &format!("not found: {method} {path}"),
                StatusCode::NOT_FOUND,
            ));
        }
        let body = read_body(headers, body).await?;
        let session_id = str_field(&body, "sessionId").unwrap_or("");
        let via = if str_field(&body, "via") == Some("prompt") {
            HandoffVia::Prompt
        } else {
            HandoffVia::Stop
        };
        if session_id.is_empty() {
            return Ok(error_response(
                "sessionId is required",
                StatusCode::BAD_REQUEST,
            ));
        }
        return Ok(match store.claim_handoff(session_id, via)? {
            Some(claimed) => ok_json(&claimed),
            None => Response::builder()
                .status(StatusCode::NO_CONTENT)
                .body(Body::empty())
                .unwrap(),
        });
    }

    // ── PR 구독 — 데몬은 구독한 PR 만 보고 그 세션에만 알린다(docs/design/specs/2026-10-01-pr-subscriptions-design.md) ──
    if *method == Method::GET && path == "/api/prs/subscriptions" {
        return Ok(ok_json(&store.pr_subscriptions()?));
    }
    if path == "/api/prs/subscriptions" && (*method == Method::POST || *method == Method::DELETE) {
        // 세션을 깨울 곳을 정한다 — 세션을 조종하는 다른 동작처럼 로컬 전용.
        if !local {
            return Ok(error_response(
                NON_LOCAL_PR_SUBSCRIPTION_MESSAGE,
                StatusCode::FORBIDDEN,
            ));
        }
        if *method == Method::DELETE {
            let repo = query.get("repo").map(String::as_str).unwrap_or("");
            let Some(number) = query.get("number").and_then(|n| n.parse::<i64>().ok()) else {
                return Ok(error_response(
                    "repo 와 number 가 필요하다",
                    StatusCode::BAD_REQUEST,
                ));
            };
            let removed = store.unsubscribe_pr(repo, number)?;
            return Ok(ok_json(&json!({ "removed": removed })));
        }
        let body = read_body(headers, body).await?;
        let repo = str_field(&body, "repo").unwrap_or("").trim().to_string();
        let number = body.get("number").and_then(Value::as_i64).unwrap_or(0);
        if !rocky_core::prwatch::is_repo_slug(&repo) || number <= 0 {
            return Ok(error_response(
                &format!("repo(owner/name)와 양수 number 가 필요하다: {repo}#{number}"),
                StatusCode::BAD_REQUEST,
            ));
        }
        let session_id = str_field(&body, "sessionId")
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let sub = store.subscribe_pr(&repo, number, session_id)?;
        return Ok(json_response(&sub, StatusCode::CREATED));
    }

    // ── 필터 구독 — GitHub 검색 조건에 걸린 열린 PR 을 그 세션의 구독으로 넣는다 ──
    if *method == Method::GET && path == "/api/prs/filters" {
        return Ok(ok_json(&store.pr_filter_subscriptions()?));
    }
    if path == "/api/prs/filters" && (*method == Method::POST || *method == Method::DELETE) {
        if !local {
            return Ok(error_response(
                NON_LOCAL_PR_SUBSCRIPTION_MESSAGE,
                StatusCode::FORBIDDEN,
            ));
        }
        if *method == Method::DELETE {
            let Some(id) = query.get("id").filter(|s| !s.is_empty()) else {
                return Ok(error_response("id 가 필요하다", StatusCode::BAD_REQUEST));
            };
            return match store.unsubscribe_pr_filter(id)? {
                Some(prs) => Ok(ok_json(&json!({ "removed": true, "prs": prs }))),
                None => Ok(error_response(
                    &format!("필터 구독이 없다: {id}"),
                    StatusCode::NOT_FOUND,
                )),
            };
        }
        let body = read_body(headers, body).await?;
        let query_text = str_field(&body, "query").unwrap_or("").to_string();
        if !rocky_core::prwatch::is_filter_query(&query_text) {
            return Ok(error_response(
                &format!("query(한 줄, 256자 안의 GitHub 검색 조건)가 필요하다: {query_text:?}"),
                StatusCode::BAD_REQUEST,
            ));
        }
        let session_id = str_field(&body, "sessionId")
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let filter = store.subscribe_pr_filter(&query_text, session_id)?;
        return Ok(json_response(&filter, StatusCode::CREATED));
    }

    // ── PR 감시 — 기억하고 있는 스냅숏 (읽기 전용) ──
    if *method == Method::GET && path == "/api/prs" {
        let repo = match query.get("board").filter(|k| !k.is_empty()) {
            Some(key) => match store.board_id_of(key)? {
                Some(id) => store.board_by_id(&id)?.and_then(|b| b.repo),
                None => {
                    return Ok(error_response(
                        &format!("unknown board: {key}"),
                        StatusCode::BAD_REQUEST,
                    ))
                }
            },
            None => None,
        };
        if query.contains_key("board") && repo.is_none() {
            return Ok(ok_json(&Vec::<rocky_core::prwatch::PrSnapshot>::new()));
        }
        let mut prs = store.list_prs(repo.as_deref(), flag("open"))?;
        if repo.is_none() {
            // 전역 목록은 지금 보드에 설정된 레포만 — 떼어 낸 레포의 옛 스냅숏이 새 tick 전에 보이지 않게.
            let watched: std::collections::HashSet<String> = store
                .list_boards(false)?
                .into_iter()
                .filter_map(|b| b.repo)
                .collect();
            prs.retain(|p| watched.contains(&p.repo));
        }
        return Ok(ok_json(&prs));
    }

    if *method == Method::GET && path == "/api/handoffs" {
        return handoffs_list_route(state, query).await;
    }

    if *method == Method::POST {
        if let Some((id, _)) = seg2_match(path, "/api/handoffs/", &["cancel"]) {
            return Ok(ok_json(&store.cancel_handoff(&id, actor)?));
        }
    }

    // ── changes feed (훅 주입용) ──
    if *method == Method::GET && path == "/api/changes" {
        let raw = query.get("sinceId").map(String::as_str).unwrap_or("0");
        let Ok(since_id) = raw.parse::<i64>() else {
            return Ok(error_response(
                "sinceId must be a non-negative integer",
                StatusCode::BAD_REQUEST,
            ));
        };
        if since_id < 0 {
            return Ok(error_response(
                "sinceId must be a non-negative integer",
                StatusCode::BAD_REQUEST,
            ));
        }
        let limit = query.get("limit").and_then(|l| l.parse::<i64>().ok());
        return Ok(ok_json(&store.list_changes_since(since_id, limit)?));
    }

    // ── history ──
    if *method == Method::GET && path == "/api/history" {
        return Ok(ok_json(&store.list_history(&ListHistoryFilter {
            entity_id: query.get("entityId").cloned(),
            entity: query.get("entity").and_then(|e| HistoryEntity::parse(e)),
            limit: query.get("limit").and_then(|l| l.parse::<i64>().ok()),
            exclude_actions: Vec::new(),
        })?));
    }

    Ok(error_response(
        &format!("not found: {method} {path}"),
        StatusCode::NOT_FOUND,
    ))
}

fn value_display(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// PATCH /api/todos/:ref 본문 → UpdateTodoPatch. TS 는 body 를 그대로 updateTodo 에
/// 넘겼다 — 같은 규칙(키 존재 여부 = 고치려는 필드, null = 지움)으로 옮긴다.
fn todo_patch_from(body: &serde_json::Map<String, Value>) -> UpdateTodoPatch {
    let mut patch = UpdateTodoPatch::default();
    if let Some(title) = str_field(body, "title") {
        patch.title = Some(title.to_string());
    }
    if let Some(description) = str_field(body, "description") {
        patch.description = Some(description.to_string());
    }
    if let Some(priority) = str_field(body, "priority").and_then(TodoPriority::parse) {
        patch.priority = Some(priority);
    }
    if let Some(due) = body.get("due") {
        patch.due = Some(due.as_str().map(str::to_string));
    }
    if let Some(labels) = body.get("labels") {
        patch.labels = serde_json::from_value(labels.clone()).ok();
    }
    if let Some(links) = body.get("links") {
        patch.links = serde_json::from_value(links.clone()).ok();
    }
    if let Some(section) = body.get("section") {
        patch.section = Some(section.as_str().map(str::to_string));
    }
    if let Some(parent) = body.get("parentId") {
        patch.parent_id = Some(parent.as_str().map(str::to_string));
    }
    patch
}

// ── 서브 라우트 ─────────────────────────────────────────────────────────────

/// POST /api/todos/:ref/issue — 출처 검사가 **가장 먼저**(todo 존재보다 앞: ref 존재도
/// 흘리지 않는다). 중복은 사전·사후 모두 409, 그 외 실패는 항상 400.
async fn issue_route(
    state: &Arc<ServerState>,
    r: &str,
    query: &HashMap<String, String>,
    headers: &HeaderMap,
    body: Body,
    actor: &str,
    local: bool,
) -> StoreResult<Response> {
    let store = &state.store;
    if !local {
        return Ok(error_response(
            NON_LOCAL_ISSUE_MESSAGE,
            StatusCode::FORBIDDEN,
        ));
    }
    let current_board_id = current_board_id_of(store, query, r)?;
    let Some(todo) = store.get_todo(r, current_board_id.as_deref())? else {
        return Ok(error_response(
            &format!("todo not found: {r}"),
            StatusCode::NOT_FOUND,
        ));
    };
    if let Some(existing) = find_issue_link(&todo.links) {
        return Ok(already_has_issue(existing));
    }
    let body = read_optional_body(headers, body).await?;
    let mut repo: Option<String> = None;
    if let Some(body) = &body {
        if let Some(value) = body.get("repo") {
            let Some(text) = value.as_str().filter(|v| is_repo_slug(v)) else {
                return Ok(error_response(
                    "repo must look like OWNER/NAME",
                    StatusCode::BAD_REQUEST,
                ));
            };
            repo = Some(text.trim().to_string());
        }
    }
    match create_issue_for_todo(
        store,
        r,
        IssueForTodoOptions {
            actor,
            current_board_id: current_board_id.as_deref(),
            repo: repo.as_deref(),
        },
        &state.gh_runner,
    )
    .await
    {
        Ok((url, todo)) => Ok(json_response(
            &json!({ "url": url, "todo": with_ref_todo(store, todo)? }),
            StatusCode::CREATED,
        )),
        // 사전 검사와 재검사 사이의 await 창 — 같은 "이미 있음"이 타이밍에 따라
        // 409/400 으로 갈리지 않게 여기서도 409.
        Err(IssueForTodoError::AlreadyExists(error)) => Ok(already_has_issue(&error.url)),
        // 그 밖의 실패는 항상 400 — `gh` 의 "HTTP 404: Not Found" 가 404 로 새면
        // "todo not found" 계약이 깨진다.
        Err(IssueForTodoError::Other(error)) => {
            Ok(error_response(&error.to_string(), StatusCode::BAD_REQUEST))
        }
    }
}

/// POST /api/todos/:ref/handoff — 자동 매칭은 후보 정확히 1개일 때만.
async fn handoff_route(
    state: &Arc<ServerState>,
    r: &str,
    query: &HashMap<String, String>,
    headers: &HeaderMap,
    body: Body,
    actor: &str,
) -> StoreResult<Response> {
    let store = &state.store;
    let body = read_body(headers, body).await?;
    let note = str_field(&body, "note").map(str::to_string);
    let current_board_id = current_board_id_of(store, query, r)?;
    let Some(todo) = store.get_todo(r, current_board_id.as_deref())? else {
        return Ok(error_response(
            &format!("todo not found: {r}"),
            StatusCode::NOT_FOUND,
        ));
    };
    if store.pending_handoff_of(&todo.id)?.is_some() {
        return Ok(error_response(
            &format!("이 항목은 이미 다른 세션 앞에 대기 중이다: {r}"),
            StatusCode::CONFLICT,
        ));
    }

    let result = (state.sessions)().await;
    if !result.available {
        let reason = result
            .reason
            .as_deref()
            .unwrap_or("활성 세션 목록을 가져올 수 없다");
        return Ok(error_response(reason, StatusCode::CONFLICT));
    }

    // sessionId 타입 오류는 400 — 조용히 자동 매칭으로 떨어뜨리면 **다른 세션**으로 간다.
    let session_id_value = body.get("sessionId");
    if let Some(value) = session_id_value {
        if !value.is_string() {
            return Ok(error_response(
                "sessionId must be a string",
                StatusCode::BAD_REQUEST,
            ));
        }
    }
    let requested = session_id_value.and_then(|v| v.as_str());
    let mut target: Option<&AgentSession> =
        requested.and_then(|wanted| result.sessions.iter().find(|s| s.session_id == wanted));
    if let Some(wanted) = requested {
        if target.is_none() {
            return Ok(error_response(
                &format!("활성 세션이 아니다: {wanted}"),
                StatusCode::BAD_REQUEST,
            ));
        }
    }
    if target.is_none() {
        // 자동 매칭 — 후보가 정확히 하나일 때만. 애매하면 사용자에게 되묻는다.
        let board_key = store
            .list_boards(true)?
            .into_iter()
            .find(|b| b.id == todo.board_id)
            .map(|b| b.key)
            .unwrap_or_default();
        let candidates = match_board(&result.sessions, &board_key);
        if candidates.len() != 1 {
            let error = if candidates.is_empty() {
                format!("\"{board_key}\" 에 해당하는 활성 세션이 없다 — 대상을 직접 고르라")
            } else {
                format!(
                    "\"{board_key}\" 후보가 {}개다 — 대상을 직접 고르라",
                    candidates.len()
                )
            };
            let listed: Vec<&AgentSession> = if candidates.is_empty() {
                result.sessions.iter().collect()
            } else {
                candidates
            };
            return Ok(json_response(
                &json!({ "error": error, "candidates": listed }),
                StatusCode::CONFLICT,
            ));
        }
        target = candidates.into_iter().next();
    }
    let target = target.expect("target resolved above");

    let handoff = store.create_handoff(&CreateHandoffInput {
        todo_ref: r.to_string(),
        session_id: target.session_id.clone(),
        session_name: Some(target.name.clone()),
        session_cwd: Some(target.cwd.clone()),
        note,
        actor: actor.to_string(),
        current_board_id: current_board_id.clone(),
    })?;
    // 큐에 넣는 것까지가 데몬의 전부 — 턴을 여는 건 호출자 몫이라 poke 를 함께 돌려준다.
    let todo_ref = ref_of(store, Some(&todo.board_id), todo.number, &todo.id)?;
    let poke = build_handoff_poke(&HandoffPokeInput {
        session_name: &target.name,
        todo_ref: &todo_ref,
        todo_title: &todo.title,
    });
    let mut out = serde_json::to_value(&handoff).map_err(|e| StoreError::new(e.to_string()))?;
    out["poke"] = serde_json::to_value(&poke).map_err(|e| StoreError::new(e.to_string()))?;
    Ok(json_response(&out, StatusCode::CREATED))
}

/// POST /api/todos/:ref/spawn — 순서가 계약이다 (contract.md 참고).
#[allow(clippy::too_many_arguments)]
async fn spawn_route(
    state: &Arc<ServerState>,
    r: &str,
    query: &HashMap<String, String>,
    headers: &HeaderMap,
    body: Body,
    actor: &str,
    local: bool,
) -> StoreResult<Response> {
    let store = &state.store;
    // 이슈 생성과 같은 등급의 게이트 — 보드 쓰기 권한이 프로세스 기동 권한으로 확대되는 지점.
    if !local {
        return Ok(error_response(
            NON_LOCAL_SPAWN_MESSAGE,
            StatusCode::FORBIDDEN,
        ));
    }
    let body = read_optional_body(headers, body).await?;
    let note = body
        .as_ref()
        .and_then(|b| str_field(b, "note"))
        .map(str::to_string);
    let current_board_id = current_board_id_of(store, query, r)?;
    let Some(todo) = store.get_todo(r, current_board_id.as_deref())? else {
        return Ok(error_response(
            &format!("todo not found: {r}"),
            StatusCode::NOT_FOUND,
        ));
    };
    if todo.archived_at.is_some() {
        return Ok(error_response(
            &format!("todo is archived: {r}"),
            StatusCode::BAD_REQUEST,
        ));
    }
    if store.pending_handoff_of(&todo.id)?.is_some() {
        return Ok(error_response(
            &format!("이 항목은 이미 다른 세션 앞에 대기 중이다: {r}"),
            StatusCode::CONFLICT,
        ));
    }

    // path override — spawn 이 **성공한 뒤에만** 영구 저장한다.
    let mut path_override: Option<String> = None;
    if let Some(body) = &body {
        if let Some(value) = body.get("path") {
            let Some(text) = value.as_str().map(str::trim).filter(|t| !t.is_empty()) else {
                return Ok(error_response(
                    "path must be a non-empty string",
                    StatusCode::BAD_REQUEST,
                ));
            };
            path_override = Some(text.to_string());
        }
    }

    let board = store
        .list_boards(true)?
        .into_iter()
        .find(|b| b.id == todo.board_id);
    let raw_board_path = path_override
        .clone()
        .or_else(|| board.as_ref().and_then(|b| b.path.clone()))
        .unwrap_or_default();
    if raw_board_path.is_empty() {
        return Ok(error_response(
            &format!(
                "보드 \"{}\" 에 메인 레포 경로가 없다 — rocky board path <절대경로> 로 설정하라",
                board.as_ref().map(|b| b.key.as_str()).unwrap_or("")
            ),
            StatusCode::BAD_REQUEST,
        ));
    }
    // 상대경로는 데몬 cwd 기준으로 풀린다 — 예측 불가이므로 막는다.
    if !raw_board_path.starts_with('/') {
        return Ok(error_response(
            &format!(
                "보드 경로는 절대경로여야 한다 — 데몬의 cwd 는 예측할 수 없다: {raw_board_path}"
            ),
            StatusCode::BAD_REQUEST,
        ));
    }
    // realpath — 이 값 하나가 워크트리 계산·spawn cwd·보드 저장에 전부 쓰인다.
    let board_path = match (state.real_path)(raw_board_path.trim_end_matches('/')) {
        Ok(path) => path,
        Err(_) => {
            return Ok(error_response(
                &format!("경로를 찾을 수 없다: {raw_board_path}"),
                StatusCode::BAD_REQUEST,
            ))
        }
    };
    if !(state.path_exists)(&format!("{}/.git", board_path.trim_end_matches('/'))) {
        return Ok(error_response(
            &format!("git 워크트리가 아니다: {board_path}"),
            StatusCode::BAD_REQUEST,
        ));
    }

    let worktree_path = worktree_path_for(&board_path, todo.number);

    // 등록 지연 창 안의 재요청은 409 — 재사용 분기로 보내면 짧은 id 로 pending 이
    // 만들어져 영영 배달되지 않는다.
    if state.recent_spawns.is_recent(&worktree_path) {
        return Ok(error_response(
            &format!("방금 이 워크트리에 세션을 띄웠다 — 잠시 후 다시 시도하라: {worktree_path}"),
            StatusCode::CONFLICT,
        ));
    }

    // handoff 라우트와 같은 코드로 답한다.
    let sessions = (state.spawn_sessions)().await;
    if !sessions.available {
        let reason = sessions
            .reason
            .as_deref()
            .unwrap_or("활성 세션 목록을 가져올 수 없다");
        return Ok(error_response(reason, StatusCode::CONFLICT));
    }

    let todo_ref = ref_of(store, Some(&todo.board_id), todo.number, &todo.id)?;
    let board_key = board.as_ref().map(|b| b.key.clone());

    // path override 가 여기까지 왔으면 유효함이 입증됐다 — 저장은 정규화된 값으로.
    let persist_path_if_given = |store: &TodoStore| -> StoreResult<()> {
        if let (Some(_), Some(key)) = (&path_override, &board_key) {
            store.set_board_path(key, &board_path, actor)?;
        }
        Ok(())
    };

    // 이미 도는 세션이 있으면 새로 띄우지 않는다 — 세션 재사용(기존 큐로 pending).
    if let Some(live) = find_live_session_at(&sessions.sessions, &worktree_path) {
        let handoff = store.create_handoff(&CreateHandoffInput {
            todo_ref: r.to_string(),
            session_id: live.session_id.clone(),
            session_name: Some(live.name.clone()),
            session_cwd: Some(live.cwd.clone()),
            note,
            actor: actor.to_string(),
            current_board_id: current_board_id.clone(),
        })?;
        persist_path_if_given(store)?;
        return Ok(json_response(
            &json!({ "handoff": handoff, "reused": true, "worktreePath": worktree_path }),
            StatusCode::CREATED,
        ));
    }

    let session_name = format!("{}-{}", board_key.as_deref().unwrap_or("todo"), todo.number);
    // 예약은 실행 **전** 동기 구간에서 — await 뒤로 미루면 겹친 요청이 나란히 통과한다.
    state.recent_spawns.remember(&worktree_path);
    let prompt = build_handoff_prompt_from(&HandoffPromptInput {
        actor,
        note: note.as_deref().unwrap_or("").trim(),
        todo_ref: &todo_ref,
        todo_title: &todo.title,
        remaining: 0,
    });
    let spawned = (state.spawn)(SpawnInput {
        board_path: board_path.clone(),
        worktree_name: worktree_name_for(todo.number),
        session_name: session_name.clone(),
        prompt,
    })
    .await;
    let short_id = match spawned {
        Ok(id) => id,
        Err(error) => {
            // 예약은 **확실히 안 떴을 때만** 되돌린다 — 모르면 유지(동시 실행 방지가 우선).
            if error.started == Some(false) {
                state.recent_spawns.forget(&worktree_path);
            }
            return Ok(error_response(&error.message, StatusCode::BAD_REQUEST));
        }
    };

    // 배달 기록·경로 저장은 spawn 성공 뒤에만.
    persist_path_if_given(store)?;
    let handoff = store.create_spawned_handoff(&CreateSpawnedHandoffInput {
        todo_ref: r.to_string(),
        session_id: short_id.clone(),
        session_name,
        session_cwd: worktree_path.clone(),
        note,
        actor: actor.to_string(),
        current_board_id,
    })?;
    Ok(json_response(
        &json!({ "handoff": handoff, "reused": false, "worktreePath": worktree_path, "sessionShortId": short_id }),
        StatusCode::CREATED,
    ))
}

/// GET /api/handoffs — stale/unstarted 판정 포함.
async fn handoffs_list_route(
    state: &Arc<ServerState>,
    query: &HashMap<String, String>,
) -> StoreResult<Response> {
    let store = &state.store;
    let board_key = query.get("board");
    let board_id = match board_key {
        Some(key) => store.board_id_of(key)?,
        None => None,
    };
    // board 를 명시했는데 안 풀리면 **빈 목록** — 보드는 지연 생성이라 CLI 가 흔히
    // 존재하지 않는 키를 붙인다(그 보드에 핸드오프가 있을 수 없으니 빈 목록이 사실이다).
    if board_key.is_some() && board_id.is_none() {
        return Ok(ok_json(&Vec::<HandoffViewOut>::new()));
    }
    let handoffs = store.list_handoffs(&ListHandoffsFilter {
        board_id,
        status: query.get("status").and_then(|s| HandoffStatus::parse(s)),
        open: query.get("open").map(String::as_str) == Some("true"),
        todo_id: None,
    })?;
    // 세션 조회는 pending 또는 미수락 delivered 가 있을 때만. available:false 면 stale
    // 을 판정하지 않는다(모름 ≠ 없음).
    let needs_sessions = handoffs.iter().any(|h| {
        h.status == HandoffStatus::Pending
            || (h.status == HandoffStatus::Delivered && h.accepted_at.is_none())
    });
    let sessions = if needs_sessions {
        Some((state.sessions)().await)
    } else {
        None
    };
    let live: Option<std::collections::HashSet<&str>> = sessions
        .as_ref()
        .filter(|s| s.available)
        .map(|s| s.sessions.iter().map(|x| x.session_id.as_str()).collect());
    let views: Vec<HandoffViewOut> = handoffs
        .into_iter()
        .map(|handoff| {
            let phase = handoff_phase(&handoff);
            let unstarted = sessions
                .as_ref()
                .map(|s| is_unstarted(&handoff, s))
                .unwrap_or(false);
            let stale = handoff.status == HandoffStatus::Pending
                && live
                    .as_ref()
                    .is_some_and(|l| !l.contains(handoff.session_id.as_str()));
            HandoffViewOut {
                handoff,
                phase,
                unstarted,
                stale,
            }
        })
        .collect();
    Ok(ok_json(&views))
}

/// GET /api/statusline — 모든 실패·빈 상태 = 빈 문자열(text/plain).
async fn statusline_of(state: &Arc<ServerState>, query: &HashMap<String, String>) -> Response {
    match statusline_inner(state, query).await {
        Ok(line) => plain(&line),
        Err(_) => plain(""),
    }
}

async fn statusline_inner(
    state: &Arc<ServerState>,
    query: &HashMap<String, String>,
) -> StoreResult<String> {
    let store = &state.store;
    let session = query.get("session");
    let cwd = query.get("cwd");
    let doing = store.list_todos(&ListTodosFilter {
        status: Some(TodoStatus::Doing),
        ..Default::default()
    })?;
    let pending = if session.is_some() {
        store.list_handoffs(&ListHandoffsFilter {
            status: Some(HandoffStatus::Pending),
            ..Default::default()
        })?
    } else {
        Vec::new()
    };
    // 보드 기준 마감·수집함 — 둘 다 세션 조회 없이 나온다. 수집함은 **기다리지 않는** 조회(캐시만).
    let (due, collect) = {
        let boards = store.list_boards(false)?;
        let locations: Vec<BoardLocation> = boards
            .iter()
            .map(|b| BoardLocation {
                key: b.key.clone(),
                path: b.path.clone(),
            })
            .collect();
        let board_key = board_key_for_cwd(&locations, cwd.map(String::as_str));
        let todos = store.list_todos(&ListTodosFilter {
            board: board_key.clone(),
            ..Default::default()
        })?;
        let today = today_local();
        let due = todos
            .iter()
            .filter(|t| t.status != TodoStatus::Done)
            .filter(|t| {
                t.due
                    .as_deref()
                    .and_then(|d| due_bucket(d, &today))
                    .is_some()
            })
            .count() as i64;
        let inbox = marked_inbox(
            state,
            InboxFetch::CachedOnly,
            InboxScope::Board(board_key.as_deref()),
        )
        .await?;
        (due, count_unpromoted(&inbox))
    };
    // 보여줄 게 없으면 **세션 조회 전에** 빈 문자열 — 초당 도는 최빈 경로의 비용 절감.
    if doing.is_empty() && pending.is_empty() && due == 0 && collect == 0 {
        return Ok(String::new());
    }

    let sessions = (state.statusline_sessions)().await;
    let aliases = session.map(|s| session_aliases(s, &sessions));
    let mine_todo = aliases.as_ref().and_then(|aliases| {
        doing.iter().find(|todo| {
            todo.doing_session_id
                .as_ref()
                .is_some_and(|id| aliases.contains(id))
        })
    });
    let mine = match mine_todo {
        Some(todo) => {
            let view = with_ref_todo(store, todo.clone())?;
            Some(StatuslineMine {
                r#ref: view.r#ref.clone(),
                title: view.todo.title.clone(),
                comments: view.comment_count,
            })
        }
        None => None,
    };

    // 보드가 안 풀리면 전체 doing 으로 폴백 — 0 으로 만들면 방치 경고가 조용히 사라진다.
    let boards = store.list_boards(false)?;
    let locations: Vec<BoardLocation> = boards
        .iter()
        .map(|b| BoardLocation {
            key: b.key.clone(),
            path: b.path.clone(),
        })
        .collect();
    let board_key = board_key_for_cwd(&locations, cwd.map(String::as_str));
    let board_id = match &board_key {
        Some(key) => store.board_id_of(key)?,
        None => None,
    };
    let board_doing: Vec<&Todo> = match &board_id {
        Some(id) => doing.iter().filter(|t| t.board_id == *id).collect(),
        None => doing.iter().collect(),
    };
    // boardKeyOf 는 보드마다 DB 쿼리 — 요청 안에서 boardId 당 한 번만 푼다.
    let mut board_keys: HashMap<String, String> = HashMap::new();
    let mut stale = 0i64;
    for todo in &board_doing {
        let key = match board_keys.get(&todo.board_id) {
            Some(key) => key.clone(),
            None => {
                let resolved = store.board_key_of(&todo.board_id)?.unwrap_or_default();
                board_keys.insert(todo.board_id.clone(), resolved.clone());
                resolved
            }
        };
        let doing_state = resolve_doing_state(todo, &key, &sessions);
        if matches!(
            doing_state,
            rocky_core::doing::DoingState::Idle | rocky_core::doing::DoingState::Gone
        ) {
            stale += 1;
        }
    }
    let inbox = aliases
        .as_ref()
        .map(|aliases| {
            pending
                .iter()
                .filter(|h| aliases.contains(&h.session_id))
                .count() as i64
        })
        .unwrap_or(0);

    Ok(render_statusline(
        &state.statusline_template,
        &StatuslineData {
            mine,
            inbox,
            stale,
            due,
            collect,
            doing: board_doing.len() as i64,
        },
        STATUSLINE_TITLE_MAX,
    ))
}

/// GET /api/events — store change 이벤트를 SSE 로 흘린다.
fn sse_response(state: &Arc<ServerState>) -> Response {
    // 구독자는 payload 를 보지 않고 refetch 만 하므로 밀려도 무해 — 조용히 이어 간다.
    sse_from(state.events.subscribe(), OnLag::Skip)
}

/// broadcast 가 밀렸을 때(`Lagged`) 어떻게 하나.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OnLag {
    /// 그 건만 버리고 계속 — 구독자가 어차피 refetch 하는 채널.
    Skip,
    /// 연결을 끝낸다 — 구독자가 건마다 상태를 쌓는 채널. 다시 붙으며 차분을 받는다.
    Close,
}

/// update 바이너리 ↔ JSON 문자열. 표준 base64(패딩 있음) — 쿼리에 실을 땐 클라이언트가
/// percent-encode 한다.
fn encode_b64(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn decode_b64(text: &str) -> Result<Vec<u8>, String> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(text.trim())
        .map_err(|e| format!("bad base64: {e}"))
}

/// broadcast 채널 하나를 SSE 응답으로 — 전역 `/api/events` 와 노트별 문서 스트림이 같이 쓴다.
fn sse_from(receiver: broadcast::Receiver<String>, on_lag: OnLag) -> Response {
    use tokio_stream::wrappers::BroadcastStream;
    use tokio_stream::StreamExt;

    let frame = |payload: String| {
        Ok::<_, std::convert::Infallible>(format!("data: {payload}\n\n").into_bytes())
    };
    let raw = BroadcastStream::new(receiver);
    let stream: std::pin::Pin<
        Box<dyn tokio_stream::Stream<Item = Result<Vec<u8>, std::convert::Infallible>> + Send>,
    > = match on_lag {
        OnLag::Skip => Box::pin(raw.filter_map(move |event| event.ok().map(frame))),
        // 첫 Lagged 에서 스트림을 끝낸다 — take_while 이 그 항목을 먹고 멈춘다.
        OnLag::Close => Box::pin(
            raw.take_while(|event| event.is_ok())
                .filter_map(move |event| event.ok().map(frame)),
        ),
    };
    let connected = tokio_stream::once(Ok::<_, std::convert::Infallible>(
        b": connected\n\n".to_vec(),
    ));
    let body = Body::from_stream(connected.chain(stream));
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .header(header::CONNECTION, "keep-alive")
        .body(body)
        .unwrap()
}

/// 오늘 날짜(로컬) `YYYY-MM-DD` — 마감 판정의 기준. 데몬은 사용자 기기에서 돈다.
fn today_local() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// 어댑터의 입력 칸 목록 — `command --describe` 를 5초 안에. 보드 설정 화면이 열 때와 등록 검증 때만 부른다.
async fn describe_adapter(
    state: &Arc<ServerState>,
    adapter: &InboxSource,
) -> Result<rocky_core::inbox::AdapterDescribe, String> {
    let mut argv = adapter.command.clone();
    argv.push("--describe".into());
    let output = (state.gh_runner)(argv, String::new(), Duration::from_secs(5)).await;
    if !output.ok() {
        let first = output.stderr.lines().map(str::trim).find(|l| !l.is_empty());
        return Err(match first {
            Some(line) => format!("exit {}: {line}", output.code),
            None => format!("exit {}", output.code),
        });
    }
    rocky_core::inbox::parse_describe(&output.stdout)
}

/// 세션 전달 기록의 상한.
pub const DELIVERY_LOG_MAX: usize = 50;

/// 조회할 수집함 = 설정 파일의 소스 + 보드마다 등록한 소스(어댑터 명령 + 화면에서 채운 값). 어댑터가
/// 설정에서 빠진 보드 소스는 조용히 건너뛴다 — 목록 라우트가 `adapterMissing` 으로 알려 준다.
fn board_sources_fn(
    store: Arc<TodoStore>,
    config: Vec<InboxSource>,
    adapters: Arc<Vec<InboxSource>>,
) -> SourcesFn {
    Arc::new(move || {
        let mut sources = config.clone();
        let board = store
            .list_board_inbox_sources(None)
            .unwrap_or_else(|error| {
                eprintln!("rocky: 보드 수집함 목록을 못 읽었다 — {error}");
                Vec::new()
            });
        for b in board {
            // 설정 파일에 같은 이름이 나중에 생겼으면 설정이 이긴다 — 목록 라우트가 `nameClash` 로 알린다.
            if config.iter().any(|c| c.name == b.name) {
                continue;
            }
            if let Some(adapter) = adapters.iter().find(|a| a.name == b.adapter) {
                sources.push(InboxSource {
                    name: b.name,
                    command: rocky_core::inbox::source_argv(&adapter.command, &b.params),
                    timeout_ms: adapter.timeout_ms,
                });
            }
        }
        sources
    })
}

/// 수집함 조회 + "이미 올라감" 표시 — 판정 근거는 전 보드(보관 포함)의 링크다(`Store::linked_urls`).
/// 항목이 하나도 없으면 스토어를 읽지 않는다(statusline 은 초 단위로 부른다).
/// 보드 소스는 결과에 보드 key 를 붙이고(`board`), `board` 로 거른다 — 설정 파일의 소스(공통) + 그
/// 보드의 소스. 보드를 모르면(`None`, cwd 가 어느 보드에도 안 맞음) 공통 소스만: 다른 보드의 전용 필터
/// 결과가 섞이지 않게. 보드 소스 목록을 못 읽어도 수집함 전체를 실패시키지 않는다(조회기와 같다).
/// 항목이 없으면 링크를 읽지 않는다(statusline 은 초 단위로 부른다).
/// 보드 key(별칭 포함) → 현재 key. 없는 보드면 None.
fn current_board_key(state: &Arc<ServerState>, key: &str) -> Option<String> {
    let id = state.store.board_id_of(key).ok().flatten()?;
    state.store.board_key_of(&id).ok().flatten()
}

/// `marked_inbox` 가 남길 소스.
#[derive(Debug, Clone, Copy)]
enum InboxScope<'a> {
    /// 전부 — `GET /api/inbox`(TUI 탭·`rocky inbox`).
    All,
    /// 설정 파일의 소스 + 그 보드의 소스. `None` 이면 공통 소스만(요약·statusline 이 보드를 모를 때).
    Board(Option<&'a str>),
}

async fn marked_inbox(
    state: &Arc<ServerState>,
    mode: InboxFetch,
    scope: InboxScope<'_>,
) -> StoreResult<InboxResponse> {
    let mut inbox = (state.inbox)(mode).await;
    if !inbox.sources.is_empty() {
        let owners: HashMap<String, String> = state
            .store
            .list_board_inbox_sources(None)
            .unwrap_or_default()
            .into_iter()
            .filter(|s| !state.inbox_config_names.contains(&s.name))
            .map(|s| (s.name, s.board))
            .collect();
        for source in &mut inbox.sources {
            source.board = owners.get(&source.name).cloned();
        }
        if let InboxScope::Board(board) = scope {
            // 옛 key(별칭)로 물어도 현재 key 로 비교한다 — 결과의 `board` 는 늘 현재 key 다.
            let current = match board {
                Some(key) => current_board_key(state, key),
                None => None,
            };
            inbox
                .sources
                .retain(|s| s.board.is_none() || (current.is_some() && s.board == current));
        }
    }
    if inbox.sources.iter().any(|s| !s.items.is_empty()) {
        mark_promoted(&mut inbox, &state.store.linked_urls()?);
    }
    Ok(inbox)
}

/// GET /api/summary?cwd=&cached= — 보드 요약 JSON. 렌더는 소비자(CLI·훅)가 core 로 한다.
async fn summary_of(
    state: &Arc<ServerState>,
    query: &HashMap<String, String>,
    inbox_mode: InboxFetch,
) -> StoreResult<Summary> {
    let store = &state.store;
    let boards = store.list_boards(false)?;
    let locations: Vec<BoardLocation> = boards
        .iter()
        .map(|b| BoardLocation {
            key: b.key.clone(),
            path: b.path.clone(),
        })
        .collect();
    let board_key = board_key_for_cwd(&locations, query.get("cwd").map(String::as_str));
    let todos = store.list_todos(&ListTodosFilter {
        board: board_key.clone(),
        ..Default::default()
    })?;
    let views: Vec<TodoView> = todos
        .into_iter()
        .map(|t| with_ref_todo(store, t))
        .collect::<StoreResult<_>>()?;
    let board_id = match &board_key {
        Some(key) => store.board_id_of(key)?,
        None => None,
    };
    // "대기" = pending + 미수락 delivered. 스토어의 `open`(대기 + 미완료 배달)은 세션이 이미
    // 착수한 것까지 세므로 여기엔 맞지 않는다 — 사람이 볼 건 "아직 아무도 안 집어간 것" 이다.
    // 보관된 todo 의 핸드오프는 제외 — `views` 가 미보관 목록이므로 그 id 로 거른다(스토어의
    // `open` 과 같은 규칙).
    let live_ids: std::collections::HashSet<&str> =
        views.iter().map(|v| v.todo.id.as_str()).collect();
    let open = store
        .list_handoffs(&ListHandoffsFilter {
            board_id,
            ..Default::default()
        })?
        .iter()
        .filter(|h| live_ids.contains(h.todo_id.as_str()))
        .filter(|h| match h.status {
            HandoffStatus::Pending => true,
            HandoffStatus::Delivered => h.accepted_at.is_none(),
            HandoffStatus::Cancelled => false,
        })
        .count() as i64;
    let inbox = marked_inbox(state, inbox_mode, InboxScope::Board(board_key.as_deref())).await?;
    // CachedOnly 로 비어 온 건 "모름" — collect 를 None 으로.
    let inbox_ref =
        (!inbox.sources.is_empty() || inbox_mode != InboxFetch::CachedOnly).then_some(&inbox);
    Ok(build_summary(
        board_key,
        &views,
        open,
        inbox_ref,
        &today_local(),
    ))
}
