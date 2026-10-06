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
use rocky_core::doing::{
    gone_handoffs, handoff_phase, is_unstarted, overdue_unaccepted, resolve_doing_state,
    HandoffPhase, GONE_HANDOFF_GRACE_SECS,
};
use rocky_core::handoff::{
    build_handoff_poke, build_handoff_prompt_from, HandoffPokeInput, HandoffPromptInput,
};
use rocky_core::inbox::INBOX_CACHE_TTL_SECS;
use rocky_core::inbox::{mark_promoted, InboxResponse};
use rocky_core::local_request::{
    access_user_email, is_cross_site_request, is_local_request, CROSS_SITE_MESSAGE,
    NON_LOCAL_AGY_MESSAGE, NON_LOCAL_BOARD_META_MESSAGE, NON_LOCAL_INBOX_SOURCE_MESSAGE,
    NON_LOCAL_ISSUE_MESSAGE, NON_LOCAL_PR_SUBSCRIPTION_MESSAGE, NON_LOCAL_SPAWN_MESSAGE,
    NON_LOCAL_VERIFY_RERUN_MESSAGE,
};
use rocky_core::refs::{
    ref_needs_board_context, ref_of, with_ref_note, with_ref_todo, NoteView, TodoView,
};
use rocky_core::sessions::{
    job_state_path, match_board, parse_job_state, takes_handoff, AgentSession, JobSummary,
    SessionsResult,
};
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
use crate::sessions_exec::{swr_sessions, uncached_sessions, SessionsProvider};
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
    /// statusline 라우트 전용 — 기본은 새 값 15초·낡은 값 30분의 SWR (초당 도는 유일한 경로라 요청이 조회를 기다리지 않는다).
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
    /// 로그 색인 DB(`logs.db`) — 색인 스레드가 쓰고 `/api/logs/*` 가 읽는다. 없으면 그 라우트는 빈 목록.
    pub logs_db: Option<std::path::PathBuf>,
    /// background 세션 작업 요약 폴더(`~/.claude/jobs`) — `/api/sessions` 가 행마다 `job` 을 붙인다. 없으면 붙이지 않는다.
    pub claude_jobs_dir: Option<std::path::PathBuf>,
    /// 토큰 추천 규칙(`rocky.json` 의 `tokens.recommend`).
    pub token_recommend: rocky_core::tokens::RecommendConfig,
    /// rc 서버 현황 조회기(`rocky.json` 의 `rc` 블록). 없으면 "설정 없음" 만 낸다.
    pub rc: Option<crate::rc::RcProvider>,
    /// `agy remote-control` 켜기·끄기 — `rc` 와 같은 캐시를 쓴다(`rc_handles`). 없으면 그 라우트는 404.
    pub agy_control: Option<crate::rc::AgyControl>,
    /// rc 서버 띄우기 · 재시작. 없으면 그 라우트는 404(rc 가 꺼진 기기와 같다).
    pub rc_control: Option<Arc<crate::rc::RcController>>,
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
            logs_db: None,
            claude_jobs_dir: None,
            token_recommend: Default::default(),
            rc: None,
            agy_control: None,
            rc_control: None,
        }
    }
}

/// 검증 잡의 대기열 — `ServerState::verify_queue`.
#[derive(Default)]
struct VerifyQueue {
    /// 다시 돌려 달라고 한 대상(보드, 브랜치). 메모리 — 데몬이 다시 뜨면 사라지니 다시 부탁한다.
    rerun: std::collections::HashSet<(String, String)>,
    /// 지금 단계를 돌고 있는 대상 — 잡이 하나라 많아야 하나. `last.json` 의 `running` 은 데몬이 죽은 뒤에도 남으니
    /// "도는 중" 판정은 이걸로 한다.
    in_flight: Option<(String, String)>,
}

pub struct ServerState {
    pub store: Arc<TodoStore>,
    statusline_template: String,
    sessions: SessionsProvider,
    spawn_sessions: SessionsProvider,
    pub(crate) statusline_sessions: SessionsProvider,
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
    logs_db: Option<std::path::PathBuf>,
    claude_jobs_dir: Option<std::path::PathBuf>,
    pub token_recommend: rocky_core::tokens::RecommendConfig,
    rc: crate::rc::RcProvider,
    agy_control: Option<crate::rc::AgyControl>,
    rc_control: Option<Arc<crate::rc::RcController>>,
    /// 토큰 추천 SSE(`GET /api/tokens/events`) — 색인 스레드가 추천이 바뀐 세션을 민다. 전역 `events` 와 나눈 이유:
    /// 그 채널의 구독자(웹·rocky 채널)는 `data:` 마다 보드를 다시 읽는다.
    pub token_events: broadcast::Sender<String>,
    /// SSE 팬아웃 — 스토어 리스너가 밀어 넣는다.
    pub events: broadcast::Sender<String>,
    /// 노트별 문서 스트림(`GET /api/notes/:ref/doc/events`) — CRDT update 와 프레즌스만.
    /// 전역 `events` 에 싣지 않는 이유: 그 채널의 구독자는 전부 refetch 하므로 글자마다
    /// 보드 전체를 다시 읽게 된다. 구독자가 0 이 된 노트의 채널은 다음 방송 때 걷는다.
    note_streams: Mutex<HashMap<String, broadcast::Sender<String>>>,
    /// PR 감시 잡의 마지막 결과 — health 가 낸다.
    pr_watch: Mutex<crate::prwatch::PrWatchStatus>,
    /// 기본 브랜치 검증 — 대상마다 지금 상태(`rockyd::verify`).
    verify: Mutex<Vec<crate::verify::VerifyTargetStatus>>,
    /// 다시 돌리기 요청과 지금 도는 대상 — 한 락 아래 둬서 "도는 중인가 보고 맡기기" 와 "도는 중으로 바꾸며 요청 지우기" 가
    /// 서로 끼어들지 못하게 한다(따로 두면 그 틈에 맡긴 요청이 남아 같은 커밋을 또 돈다).
    verify_queue: Mutex<VerifyQueue>,
    /// 검증 잡의 쉬는 시간을 끊는다 — 다시 돌리기 요청이 주기를 기다리지 않게.
    pub verify_wake: tokio::sync::Notify,
    /// 레포별 열린 PR 목록 캐시 — (가져온 시각 unix 초, 목록). GitHub 탭이 레포를 펼칠 때만 채운다.
    open_prs: Mutex<HashMap<String, (i64, Vec<rocky_core::prwatch::OpenPr>)>>,
    /// 기동 때 `PRAGMA quick_check` 결과 — "ok" 아니면 health 로 드러낸다. 테스트 상태는 None.
    db_integrity: Mutex<Option<String>>,
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

    /// 일반 라우트와 같은(오래된 값을 바로 주는) 세션 목록 — 읽기만 하는 쪽(요약·목록)과 기동 예열이 쓴다.
    pub async fn sessions(&self) -> SessionsResult {
        (self.sessions)().await
    }

    /// 캐시 없는 세션 목록 — 판단이 상태를 바꾸는 쪽(스윕의 자동 해제)이 쓴다. 오래된 목록으로 보면
    /// 그 사이 새로 뜬 세션이 쥔 doing 을 "세션 없음" 으로 오판한다. spawn 라우트와 같은 조회기다.
    pub async fn fresh_sessions(&self) -> SessionsResult {
        (self.spawn_sessions)().await
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

    pub fn set_verify(&self, statuses: Vec<crate::verify::VerifyTargetStatus>) {
        *self.verify.lock().expect("verify poisoned") = statuses;
    }

    pub fn verify(&self) -> Vec<crate::verify::VerifyTargetStatus> {
        self.verify.lock().expect("verify poisoned").clone()
    }

    /// 다시 돌리기를 맡겨 두고 잡을 깨운다. 그 대상이 지금 도는 중이면 맡지 않고 `false`.
    pub fn request_verify_rerun(&self, board: &str, branch: &str) -> bool {
        let key = (board.to_string(), branch.to_string());
        let mut queue = self.verify_queue.lock().expect("verify_queue poisoned");
        if queue.in_flight.as_ref() == Some(&key) {
            return false;
        }
        queue.rerun.insert(key);
        drop(queue);
        self.verify_wake.notify_one();
        true
    }

    pub fn verify_rerun_requested(&self, board: &str, branch: &str) -> bool {
        self.verify_queue
            .lock()
            .expect("verify_queue poisoned")
            .rerun
            .contains(&(board.to_string(), branch.to_string()))
    }

    /// 단계를 돌기 시작한다 — 도는 중으로 바꾸고, 여기까지 온 요청은 이번 실행이 흡수한다.
    pub fn begin_verify(&self, board: &str, branch: &str) {
        let key = (board.to_string(), branch.to_string());
        let mut queue = self.verify_queue.lock().expect("verify_queue poisoned");
        queue.rerun.remove(&key);
        queue.in_flight = Some(key);
    }

    pub fn end_verify(&self) {
        self.verify_queue
            .lock()
            .expect("verify_queue poisoned")
            .in_flight = None;
    }

    pub fn set_pr_watch(&self, status: crate::prwatch::PrWatchStatus) {
        *self.pr_watch.lock().expect("pr_watch poisoned") = status;
    }

    pub fn pr_watch(&self) -> crate::prwatch::PrWatchStatus {
        self.pr_watch.lock().expect("pr_watch poisoned").clone()
    }

    pub fn set_db_integrity(&self, result: String) {
        *self.db_integrity.lock().expect("db_integrity poisoned") = Some(result);
    }

    pub fn db_integrity(&self) -> Option<String> {
        self.db_integrity
            .lock()
            .expect("db_integrity poisoned")
            .clone()
    }

    /// 세션 받은편지함 등록 — 같은 세션이면 덮어쓴다(cwd·소켓이 바뀌었을 수 있다). 같은 프로세스에서 세션 id 만
    /// 바뀌었으면(`/clear`·세션 안 `/resume`) 옛 세션을 `/clear` 된 것으로 적고 그 등록을 걷는다 — 옛 등록이 남으면 같은
    /// 소켓이라 맥락 없는 새 세션이 옛 세션의 PR 알림을 받는다. 남은 구독은 웹에서 사람이 정한다.
    pub fn register_inbox(&self, registration: rocky_core::peer_inbox::InboxRegistration) {
        let mut inboxes = self.inboxes.lock().expect("inboxes poisoned");
        let now = registration.seen_at;
        inboxes.retain(|_, r| now - r.seen_at <= rocky_core::peer_inbox::REGISTRATION_TTL_SECS);
        // 소켓 파일은 프로세스가 뜰 때 생기고 그 뒤로 mtime 이 바뀌지 않는다(실측: 세션 시작 시각과 같다).
        let started = std::fs::metadata(&registration.socket)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64);
        let current: Vec<_> = inboxes.values().cloned().collect();
        for old in rocky_core::peer_inbox::superseded_sessions(&current, &registration, started) {
            match self
                .store
                .mark_session_cleared(&old, &registration.session_id, &registration.cwd)
            {
                // 세션 id 는 로그에 남기지 않는다(CodeQL: 민감 정보 평문 로깅).
                Ok(true) => eprintln!(
                    "rocky: 세션이 /clear 됐다 — 남은 구독은 웹 \"세션 전달\" 에서 정할 때까지 깨우지 않는다"
                ),
                Ok(false) => {}
                // 옛 등록을 그대로 둔다 — 다음 등록(다음 턴)에 다시 적는다.
                Err(e) => {
                    eprintln!("rocky: 세션이 /clear 됐는데 적지 못했다 — {e}");
                    continue;
                }
            }
            inboxes.remove(&old);
        }
        if let Err(e) = self.store.save_session_inbox(&registration) {
            // 세션 id 는 로그에 남기지 않는다(CodeQL: 민감 정보 평문 로깅).
            eprintln!("rocky: 받은편지함 등록을 저장하지 못했다 — {e}");
        }
        inboxes.insert(registration.session_id.clone(), registration);
    }

    /// 이 세션에 지금 보내도 되는 등록 — TTL 안이고, DB 에서 되살린 것이면 그 세션이 살아 있고 소켓이 그 세션의
    /// 것일 때만(`restored_registration_live`, 세션 목록은 캐시 없이). 목록을 못 읽으면 되살린 등록은 쓰지 않는다.
    pub async fn live_inbox(
        &self,
        session_id: &str,
    ) -> Result<rocky_core::peer_inbox::InboxRegistration, &'static str> {
        let now = chrono::Utc::now().timestamp();
        let Some(target) = self.inboxes().into_iter().find(|r| {
            r.session_id == session_id
                && now - r.seen_at <= rocky_core::peer_inbox::REGISTRATION_TTL_SECS
        }) else {
            return Err("받을 세션 등록 없음");
        };
        if !target.restored {
            return Ok(target);
        }
        let sessions = self.fresh_sessions().await;
        if !sessions.available {
            return Err("되살린 등록 — 세션 목록을 못 읽어 확인 못 함");
        }
        if rocky_core::peer_inbox::restored_registration_live(&target, &sessions.sessions) {
            Ok(target)
        } else {
            self.forget_inbox(session_id);
            Err("되살린 등록 — 그 세션이 끝났거나 소켓 주인이 다르다")
        }
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
        let _ = self.store.delete_session_inbox(session_id);
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

    /// 로그 색인(`logs.db`)을 읽는 질의 하나 — 블로킹 스레드에서 자기 연결로. 색인이 없으면 `None`.
    pub async fn query_logs<T: Send + 'static, E: std::fmt::Display + Send + 'static>(
        &self,
        what: &'static str,
        query: impl FnOnce(&rocky_core::logindex::LogIndex) -> Result<T, E> + Send + 'static,
    ) -> Result<Option<T>, StoreError> {
        let Some(db) = self.logs_db.clone() else {
            return Ok(None);
        };
        tokio::task::spawn_blocking(move || {
            let index = rocky_core::logindex::LogIndex::open(&db)
                .map_err(|e| format!("{}: {e}", db.display()))?;
            query(&index).map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| StoreError::new(format!("{what} 조회 스레드: {e}")))?
        .map(Some)
        .map_err(|e| StoreError::new(format!("{what} 조회: {e}")))
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

/// `reviewFix` 의 옛 이름 — 한 릴리스 동안 `PATCH /api/boards/:key` 입력 별칭으로만 받는다(응답은 늘 `reviewFix`).
const LEGACY_REVIEW_FIX_KEY: &str = "autoResolve";

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
    let sessions = injected.clone().unwrap_or_else(|| {
        // 오래된 값은 30분까지 바로 주고 뒤에서 새로 받는다 — 60초였을 땐 잠깐 쉬고 온 첫 요청마다
        // `claude agents --json`(콜드 3초)을 기다렸다(rocky-23). 상태를 바꾸는 판단(스윕)은 `fresh_sessions`.
        swr_sessions(
            default_gh.clone(),
            Duration::from_secs(3),
            Duration::from_secs(30 * 60),
        )
    });
    // spawn 라우트만 기본이 **캐시 없는** 조회기 — 가드가 spawn 이전 스냅샷을 보면 안 된다.
    let spawn_sessions = options
        .spawn_sessions
        .or_else(|| injected.clone())
        .unwrap_or_else(|| uncached_sessions(default_gh.clone()));
    let statusline_sessions = options
        .statusline_sessions
        .or(injected)
        // 15초가 지나면 낡은 값을 바로 주고 뒤에서 한 번 새로 받는다 — 만료 때마다 그 요청이 `claude agents --json`
        // (~220ms, 콜드 수 초)을 기다리면 CLI 의 300ms 마감에 걸려 보드 줄이 비었다(rocky-47: curl p99 441ms · max 1s).
        // 새로 받는 간격은 15초 그대로라 배경 부하는 늘지 않는다.
        .unwrap_or_else(|| {
            swr_sessions(
                default_gh.clone(),
                Duration::from_secs(15),
                Duration::from_secs(30 * 60),
            )
        });
    // 받은편지함 등록은 DB 에서 되살린다 — 다시 뜬 뒤 세션이 다시 등록하기 전에 도는 첫 PR 감시 tick 의 알림이 갈 곳이 있게.
    let since = chrono::Utc::now().timestamp() - rocky_core::peer_inbox::REGISTRATION_TTL_SECS;
    let inboxes: HashMap<String, rocky_core::peer_inbox::InboxRegistration> = options
        .store
        .load_session_inboxes(since)
        .unwrap_or_default()
        .into_iter()
        .map(|r| (r.session_id.clone(), r))
        .collect();
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
        logs_db: options.logs_db,
        claude_jobs_dir: options.claude_jobs_dir,
        token_recommend: options.token_recommend,
        rc_control: options.rc_control,
        rc: options.rc.unwrap_or_else(|| {
            Arc::new(|| Box::pin(async { rocky_core::rc::RcStatus::unconfigured() }))
        }),
        agy_control: options.agy_control,
        token_events: broadcast::channel::<String>(64).0,
        events,
        note_streams: Mutex::new(HashMap::new()),
        pr_watch: Mutex::new(crate::prwatch::PrWatchStatus::default()),
        verify: Mutex::new(Vec::new()),
        verify_queue: Mutex::new(VerifyQueue::default()),
        verify_wake: tokio::sync::Notify::new(),
        db_integrity: Mutex::new(None),
        open_prs: Mutex::new(HashMap::new()),
        gh_viewer: Mutex::new(None),
        inboxes: Mutex::new(inboxes),
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
    /// background 세션의 작업 요약 — 못 읽으면 없다.
    #[serde(skip_serializing_if = "Option::is_none")]
    job: Option<JobSummary>,
}

/// background 세션의 작업 요약 — 짧은 id 가 있는 행만, 읽기 실패는 조용히 `None`(Claude Code 내부 파일이다).
fn read_job_summary(
    jobs_dir: Option<&std::path::Path>,
    session: &AgentSession,
) -> Option<JobSummary> {
    let path = job_state_path(jobs_dir?, session.id.as_deref()?)?;
    parse_job_state(&std::fs::read_to_string(path).ok()?)
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
            // Cloudflare Access 로 들어온 화면이면 로그인한 이메일 — 웹이 ⋯ 메뉴에 로그아웃을 그린다.
            "accessUser": access_user_email(|name| header_of(headers, name)),
            "prWatch": state.pr_watch(),
            "dbIntegrity": state.db_integrity(),
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
            restored: false,
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
            "cleared": state.store.cleared_sessions()?,
        })));
    }
    // `/clear` 된 세션의 남은 구독을 정한다 — 넘기기·지켜보기만·해지. 세션을 조종하는 동작이라 로컬 전용.
    if path == "/api/sessions/cleared" && *method == Method::POST {
        if !local {
            return Ok(error_response(
                NON_LOCAL_INBOX_SOURCE_MESSAGE,
                StatusCode::FORBIDDEN,
            ));
        }
        let body = read_body(headers, body).await?;
        let session_id = str_field(&body, "sessionId").unwrap_or("").trim();
        let action = body
            .get("action")
            .cloned()
            .and_then(|a| serde_json::from_value::<rocky_core::peer_inbox::ClearedAction>(a).ok());
        let (false, Some(action)) = (session_id.is_empty(), action) else {
            return Ok(error_response(
                "sessionId 와 action(handover|watch|unsubscribe) 이 필요하다",
                StatusCode::BAD_REQUEST,
            ));
        };
        let Some((changed, successor)) = state.store.resolve_cleared_session(session_id, action)?
        else {
            return Ok(error_response(
                &format!("/clear 로 결정을 기다리는 세션이 아니다: {session_id}"),
                StatusCode::NOT_FOUND,
            ));
        };
        // 넘기면 "보내지 않기" 도 따라간다 — 사람이 그 세션에 꺼 둔 것이다.
        if action == rocky_core::peer_inbox::ClearedAction::Handover && state.is_muted(session_id) {
            state.set_muted(&successor, true);
        }
        state.set_muted(session_id, false);
        return Ok(ok_json(
            &json!({ "sessionId": session_id, "changed": changed }),
        ));
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
            // path/repo 는 소비 지점이 로컬 전용인 값 — 변경도 로컬 전용이다. reviewFix 도 같다:
            // 켜면 데몬이 세션에 일을 시키므로 보드 쓰기가 세션 조종으로 넓어지는 지점이다.
            if (body.contains_key("path")
                || body.contains_key("repo")
                || body.contains_key("reviewFix")
                || body.contains_key(LEGACY_REVIEW_FIX_KEY)
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
            // 옛 이름 `autoResolve` 는 한 릴리스 동안 입력 별칭으로 받는다 — 둘 다 오면 `reviewFix` 가 이긴다.
            let review_fix = body.get("reviewFix").map(|v| ("reviewFix", v)).or_else(|| {
                body.get(LEGACY_REVIEW_FIX_KEY)
                    .map(|v| (LEGACY_REVIEW_FIX_KEY, v))
            });
            if let Some((name, value)) = review_fix {
                let Some(on) = value.as_bool() else {
                    return Ok(error_response(
                        &format!("{name} must be true or false"),
                        StatusCode::BAD_REQUEST,
                    ));
                };
                patch.review_fix = Some(on);
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
                    "key, title, description, repo, path, reviewFix or prAuthors is required",
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
            if action == StatusAction::Start {
                drop_gone_handoffs(state, &r, current_board_id.as_deref(), actor).await;
            }
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
            return handoff_route(state, &r, query, headers, body, actor, local).await;
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
                return Ok(sse_from(state.subscribe_note(&note.id), OnLag::Close, None));
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
                job: read_job_summary(state.claude_jobs_dir.as_deref(), session),
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
            restored: false,
        });
        return Ok(Response::builder()
            .status(StatusCode::NO_CONTENT)
            .body(Body::empty())
            .unwrap());
    }

    // ── 세션이 스스로 든 doing 의 귀속 — PostToolUse 훅만 부른다 ──
    if *method == Method::POST && path == "/api/sessions/doing" {
        // 데몬의 할 일에 세션을 붙이는 쓰기라 원격에는 존재 자체를 드러내지 않는다(404 위장).
        if !local {
            return Ok(error_response(
                &format!("not found: {method} {path}"),
                StatusCode::NOT_FOUND,
            ));
        }
        let body = read_body(headers, body).await?;
        let session_id = str_field(&body, "sessionId").unwrap_or("");
        let todo_id = str_field(&body, "todoId").unwrap_or("");
        if session_id.is_empty() || todo_id.is_empty() {
            return Ok(error_response(
                "sessionId and todoId are required",
                StatusCode::BAD_REQUEST,
            ));
        }
        let doing_since = str_field(&body, "doingSince");
        return Ok(
            match store.claim_doing_session(todo_id, session_id, doing_since)? {
                Some(todo) => ok_json(&with_ref_todo(store, todo)?),
                // 조건이 안 맞으면(이미 귀속·방금 시작이 아님 등) 아무것도 하지 않았다 — 실패가 아니다.
                None => Response::builder()
                    .status(StatusCode::NO_CONTENT)
                    .body(Body::empty())
                    .unwrap(),
            },
        );
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

    // ── rc 서버 현황(읽기 전용) — 설정의 대상과 떠 있는 `claude rc` 서버를 맞댄 것. 5초 캐시 ──
    if *method == Method::GET && path == "/api/rc/servers" {
        // `activity=1` — 대상마다 git 을 몇 번씩 캐시 없이 띄운다(최근 활동). 그래서 그것만은 로컬 전용이다.
        let activity = query.get("activity").is_some_and(|v| v == "1");
        if activity && !local {
            return Ok(error_response(
                "최근 활동(activity=1)은 대상마다 git 을 띄우므로 로컬 요청만 받는다",
                StatusCode::FORBIDDEN,
            ));
        }
        let mut status = (state.rc)().await;
        if let Some(control) = &state.rc_control {
            control.decorate(&mut status);
            if activity {
                control.add_activity(&mut status).await;
            }
        }
        return Ok(json_response(&status, StatusCode::OK));
    }
    // ── 야간 재시작 리허설 — 지금 설치 버전으로 판정만(손대지 않는다). `claude --version` 과 프로브를 캐시 없이 띄우므로
    // 로컬 전용 — 노출된 화면에서 되풀이해 부르면 그만큼 프로세스가 뜬다 ──
    if *method == Method::GET && path == "/api/rc/nightly/preview" {
        if !local {
            return Ok(error_response(
                "야간 리허설은 claude 와 프로브를 띄우므로 로컬 요청만 받는다",
                StatusCode::FORBIDDEN,
            ));
        }
        let Some(control) = state.rc_control.clone() else {
            return Ok(error_response(
                "이 기기에서는 rc 가 꺼져 있다",
                StatusCode::NOT_FOUND,
            ));
        };
        return Ok(json_response(
            &control.nightly_preview().await,
            StatusCode::OK,
        ));
    }
    // ── agy remote-control 켜기·끄기 — 이 기계의 원격 접속 데몬을 바꾸므로 로컬 전용. 답은 새로 잰 현황 ──
    if *method == Method::POST {
        if let Some(name) = path.strip_prefix("/api/rc/antigravity/") {
            let Some(action) = rocky_core::rc::AgyAction::parse(name) else {
                return Ok(error_response(
                    &format!("모르는 동작: {name} — start 나 stop"),
                    StatusCode::NOT_FOUND,
                ));
            };
            if !local {
                return Ok(error_response(NON_LOCAL_AGY_MESSAGE, StatusCode::FORBIDDEN));
            }
            let Some(control) = state.agy_control.as_ref() else {
                return Ok(error_response(
                    "agy 제어가 이 데몬에 연결돼 있지 않다",
                    StatusCode::NOT_FOUND,
                ));
            };
            return Ok(match control(action).await {
                Ok(status) => json_response(&status, StatusCode::OK),
                Err(message) => error_response(&message, StatusCode::BAD_GATEWAY),
            });
        }
    }
    // ── 야간 재시작 손 실행 — 서버를 내리고 띄우므로 로컬 전용. 일은 백그라운드, 바로 202. 배너는 띄우지 않는다(부른 사람이
    // `rocky rc` 로 결과를 본다) ──
    if *method == Method::POST && path == "/api/rc/nightly" {
        if !local {
            return Ok(error_response(
                NON_LOCAL_SPAWN_MESSAGE,
                StatusCode::FORBIDDEN,
            ));
        }
        let Some(control) = state.rc_control.clone() else {
            return Ok(error_response(
                "이 기기에서는 rc 가 꺼져 있다",
                StatusCode::NOT_FOUND,
            ));
        };
        return Ok(match control.begin_nightly() {
            Ok(()) => {
                tokio::spawn(async move {
                    let quiet: crate::rc::RcNotifier = Arc::new(|_, _| {});
                    control.run_nightly(&quiet).await;
                });
                json_response(&json!({ "accepted": true }), StatusCode::ACCEPTED)
            }
            Err(crate::rc::RcRefusal::NotFound(m)) => error_response(&m, StatusCode::NOT_FOUND),
            Err(crate::rc::RcRefusal::Busy(m) | crate::rc::RcRefusal::Ambiguous(m)) => {
                error_response(&m, StatusCode::CONFLICT)
            }
        });
    }
    // ── 핸드오프 서버 닫기 — 세션을 끝내는 일이라 로컬 전용. pid 로만, 그 폴더의 rc 서버일 때만 내린다 ──
    if *method == Method::POST {
        if let Some(key) = path
            .strip_prefix("/api/rc/handoffs/")
            .and_then(|rest| rest.strip_suffix("/stop"))
        {
            if !local {
                return Ok(error_response(
                    "핸드오프 서버 닫기는 이 기기(루프백)에서 온 요청만 받는다 — 세션을 끝내는 일이다",
                    StatusCode::FORBIDDEN,
                ));
            }
            let Some(control) = state.rc_control.clone() else {
                return Ok(error_response(
                    "이 기기에서는 rc 가 꺼져 있다",
                    StatusCode::NOT_FOUND,
                ));
            };
            use crate::rc::RcRefusal;
            return Ok(match control.stop_handoff(&percent_decode(key)).await {
                Ok((record, down)) => ok_json(&json!({
                    "label": record.label,
                    "name": record.name,
                    "pid": record.pid,
                    "dir": record.dir,
                    "todoRef": record.todo_ref,
                    "down": down,
                })),
                Err(RcRefusal::NotFound(m)) => error_response(&m, StatusCode::NOT_FOUND),
                Err(RcRefusal::Busy(m)) | Err(RcRefusal::Ambiguous(m)) => {
                    error_response(&m, StatusCode::CONFLICT)
                }
            });
        }
    }
    // ── 대상 밖 서버 닫기 — 핸드오프 서버 닫기와 같은 등급(로컬 전용, pid 로만, 그 폴더의 rc 서버일 때만) ──
    if *method == Method::POST {
        if let Some(key) = path
            .strip_prefix("/api/rc/strays/")
            .and_then(|rest| rest.strip_suffix("/stop"))
        {
            if !local {
                return Ok(error_response(
                    "대상 밖 서버 닫기는 이 기기(루프백)에서 온 요청만 받는다 — 세션을 끝내는 일이다",
                    StatusCode::FORBIDDEN,
                ));
            }
            let Some(control) = state.rc_control.clone() else {
                return Ok(error_response(
                    "이 기기에서는 rc 가 꺼져 있다",
                    StatusCode::NOT_FOUND,
                ));
            };
            use crate::rc::RcRefusal;
            return Ok(match control.stop_stray(&percent_decode(key)).await {
                Ok((stray, down)) => ok_json(&json!({
                    "label": stray.label,
                    "pid": stray.pid,
                    "dir": stray.dir,
                    "down": down,
                })),
                Err(RcRefusal::NotFound(m)) => error_response(&m, StatusCode::NOT_FOUND),
                Err(RcRefusal::Busy(m)) | Err(RcRefusal::Ambiguous(m)) => {
                    error_response(&m, StatusCode::CONFLICT)
                }
            });
        }
    }
    // ── rc 서버 띄우기 · 재시작 — 프로세스를 띄우므로 세션 띄우기와 같은 등급(로컬 전용). 일은 백그라운드, 바로 202 ──
    if *method == Method::POST {
        if let Some((label, verb)) = path
            .strip_prefix("/api/rc/servers/")
            .and_then(|rest| rest.split_once('/'))
        {
            if verb == "start" {
                // `serverOnly` — 세션 없이 서버만, 떠 있으면 그대로(`rocky rc start --all`, 옛 CLI `-a`). 본문보다 로컬부터.
                if !local {
                    return Ok(error_response(
                        NON_LOCAL_SPAWN_MESSAGE,
                        StatusCode::FORBIDDEN,
                    ));
                }
                let body = read_optional_body(headers, body).await?;
                let server_only = body
                    .as_ref()
                    .and_then(|b| b.get("serverOnly"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let command = if server_only {
                    crate::rc::RcCommand::Revive(rocky_core::rc::LaunchMode::Server)
                } else {
                    crate::rc::RcCommand::Start
                };
                return Ok(rc_command_route(state, label, command, local));
            }
            if verb == "restart" {
                if !local {
                    return Ok(error_response(
                        NON_LOCAL_SPAWN_MESSAGE,
                        StatusCode::FORBIDDEN,
                    ));
                }
                let body = read_optional_body(headers, body).await?;
                let field = |name: &str| body.as_ref().and_then(|b| b.get(name));
                let fresh = field("fresh").and_then(|v| v.as_bool()).unwrap_or(false);
                // `session` — 이어받을 세션을 못 박는다(claude.ai 쪽 id 만). 셸을 거치지 않지만 argv 에 들어가니 모양을 거른다.
                let session = match field("session") {
                    None | Some(Value::Null) => None,
                    Some(Value::String(id)) => Some(id.as_str()),
                    Some(_) => {
                        return Ok(error_response(
                            "session 은 문자열(claude.ai 세션 id)이어야 한다",
                            StatusCode::BAD_REQUEST,
                        ))
                    }
                };
                let command = match session {
                    Some(_) if fresh => return Ok(error_response(
                        "fresh(이어받지 않음)와 session(이 세션으로 이어받음)은 같이 줄 수 없다",
                        StatusCode::BAD_REQUEST,
                    )),
                    Some(id) if !rocky_core::rc::valid_session_id(id) => {
                        return Ok(error_response(
                            &format!("session 은 claude.ai 쪽 세션 id(cse_… · session_…)다: {id}"),
                            StatusCode::BAD_REQUEST,
                        ))
                    }
                    Some(id) => crate::rc::RcCommand::Pin(id.to_string()),
                    None => crate::rc::RcCommand::Restart { fresh },
                };
                return Ok(rc_command_route(state, label, command, local));
            }
        }
    }

    // ── 레포의 열린 PR(필요할 때만) — GitHub 탭이 레포를 펼칠 때. 주기 조회가 아니라 이때만 1포인트, 60초 캐시 ──
    if *method == Method::GET && path == "/api/verify" {
        // 기본 브랜치 검증 — 대상마다 마지막(또는 도는 중인) 결과. 설정에 대상이 없으면 빈 목록.
        return Ok(ok_json(&json!({ "targets": state.verify() })));
    }
    if *method == Method::POST && path == "/api/verify/rerun" {
        // 같은 커밋을 다시 — 환경 탓 거짓 실패를 다음 커밋까지 빨강으로 두지 않게. 프로세스를 띄우는 동작이라 로컬 전용.
        // 본문 `board`·`branch` 로 좁히고, 없으면 대상 전부. 도는 중인 대상은 건너뛴다.
        if !local {
            return Ok(error_response(
                NON_LOCAL_VERIFY_RERUN_MESSAGE,
                StatusCode::FORBIDDEN,
            ));
        }
        let body = read_optional_body(headers, body).await?.unwrap_or_default();
        // 빠진 필터만 "전부" 다 — 잘못 준 값(숫자·빈 문자열)을 전부로 읽으면 오타 하나로 모든 빌드가 돈다.
        let filter = |name: &str| match body.get(name) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(s)) if !s.trim().is_empty() => Ok(Some(s.trim())),
            Some(other) => Err(format!(
                "{name} 는 비어 있지 않은 문자열이어야 한다(빼면 전부): {other}"
            )),
        };
        let (board, branch) = match (filter("board"), filter("branch")) {
            (Ok(board), Ok(branch)) => (board, branch),
            (Err(e), _) | (_, Err(e)) => return Ok(error_response(&e, StatusCode::BAD_REQUEST)),
        };
        // 보드는 별칭까지 푼다 — 설정의 key 와 CLI 가 고른 key 가 달라도 같은 보드면 맞다.
        let resolve = |key: &str| {
            store
                .get_board(key)
                .ok()
                .flatten()
                .map(|b| b.key)
                .unwrap_or_else(|| key.to_string())
        };
        let wanted = board.map(resolve);
        let mut queued = Vec::new();
        let mut running = Vec::new();
        for t in state.verify() {
            if wanted.as_ref().is_some_and(|w| *w != resolve(&t.board))
                || branch.is_some_and(|b| b != t.branch)
            {
                continue;
            }
            let target = json!({ "board": t.board, "branch": t.branch });
            if state.request_verify_rerun(&t.board, &t.branch) {
                queued.push(target);
            } else {
                running.push(target);
            }
        }
        if queued.is_empty() && running.is_empty() {
            return Ok(error_response(
                &format!(
                    "검증 대상이 없다(board={}, branch={}) — rocky.json 의 verify.targets[] 를 본다",
                    board.unwrap_or("*"),
                    branch.unwrap_or("*")
                ),
                StatusCode::NOT_FOUND,
            ));
        }
        return Ok(ok_json(&json!({ "queued": queued, "running": running })));
    }
    if *method == Method::GET && path == "/api/tokens/summary" {
        // 모델×effort(기본) · 모델 · effort · 세션 · 브랜치별 토큰 합계. 구간은 from/to(ISO) 또는 days(기본 30).
        let group_raw = query
            .get("groupBy")
            .or_else(|| query.get("group_by"))
            .map(String::as_str)
            .unwrap_or("model,effort");
        let Some(group_by) = rocky_core::tokens::GroupBy::parse(group_raw) else {
            return Ok(error_response(
                &format!("groupBy 는 model,effort · model · effort · session · branch 중 하나다: {group_raw:?}"),
                StatusCode::BAD_REQUEST,
            ));
        };
        let (from, to) = rocky_core::tokens::range(
            query.get("from").map(String::as_str),
            query.get("to").map(String::as_str),
            query.get("days").and_then(|d| d.parse::<i64>().ok()),
            chrono::Utc::now(),
        );
        let (f, t) = (from.clone(), to.clone());
        let rows = state
            .query_logs("토큰 요약", move |index| {
                rocky_core::tokens::summary(index.conn(), &f, &t, group_by)
            })
            .await?
            .unwrap_or_default();
        return Ok(ok_json(
            &json!({ "from": from, "to": to, "groupBy": group_raw, "rows": rows }),
        ));
    }
    if *method == Method::GET && path == "/api/tokens/current" {
        // 이 디렉터리(또는 그 아래)에서 가장 최근에 움직인 세션.
        let Some(cwd) = query
            .get("cwd")
            .map(|c| c.trim().to_string())
            .filter(|c| !c.is_empty())
        else {
            return Ok(error_response("cwd 가 필요하다", StatusCode::BAD_REQUEST));
        };
        let limit = token_turn_limit(query);
        let cfg = state.token_recommend.clone();
        let found = state
            .query_logs("현재 세션", move |index| {
                rocky_core::tokens::current_session(index.conn(), &cwd, limit, &cfg)
            })
            .await?
            .flatten();
        return Ok(match found {
            Some(detail) => ok_json(&detail),
            None => error_response("이 디렉터리의 세션이 색인에 없다", StatusCode::NOT_FOUND),
        });
    }
    if *method == Method::GET && path == "/api/tokens/recommendation" {
        // 세션 하나의 추천 — sessionId 또는 cwd(그 아래 최근 세션).
        let pick = |k: &str| {
            query
                .get(k)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        let (session_id, cwd) = (pick("sessionId"), pick("cwd"));
        if session_id.is_none() && cwd.is_none() {
            return Ok(error_response(
                "sessionId 나 cwd 가 필요하다",
                StatusCode::BAD_REQUEST,
            ));
        }
        let cfg = state.token_recommend.clone();
        let found = state
            .query_logs("토큰 추천", move |index| {
                let id = match (session_id, cwd) {
                    (Some(id), _) => {
                        rocky_core::tokens::session_info(index.conn(), &id)?.map(|s| s.session_id)
                    }
                    (None, Some(cwd)) => {
                        rocky_core::tokens::latest_session_for_cwd(index.conn(), &cwd)?
                    }
                    (None, None) => None,
                };
                id.map(|id| rocky_core::tokens::recommendation_for(index.conn(), &id, &cfg))
                    .transpose()
            })
            .await?
            .flatten();
        return Ok(match found {
            Some(rec) => ok_json(&rec),
            None => error_response("세션이 색인에 없다", StatusCode::NOT_FOUND),
        });
    }
    if *method == Method::GET && path == "/api/tokens/events" {
        return Ok(sse_from(
            state.token_events.subscribe(),
            OnLag::Skip,
            Some(rocky_core::tokens::RECOMMENDATION_EVENT),
        ));
    }
    if *method == Method::GET {
        if let Some(id) = path.strip_prefix("/api/tokens/sessions/") {
            let id = id.trim().to_string();
            let limit = token_turn_limit(query);
            let found = state
                .query_logs("세션 턴", move |index| {
                    rocky_core::tokens::session_detail(index.conn(), &id, limit)
                })
                .await?
                .flatten();
            return Ok(match found {
                Some(detail) => ok_json(&detail),
                None => error_response("세션이 색인에 없다", StatusCode::NOT_FOUND),
            });
        }
    }
    if *method == Method::GET && path == "/api/logs/stats" {
        // 회고(작업로그)와 rocky 개선(사용 로그) 통계 — 기본 30일, 최대 365일.
        let days = query
            .get("days")
            .and_then(|d| d.parse::<i64>().ok())
            .unwrap_or(30)
            .clamp(1, 365);
        let now = chrono::Utc::now();
        let iso = |t: chrono::DateTime<chrono::Utc>| {
            t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
        };
        let (since, until) = (iso(now - chrono::Duration::days(days)), iso(now));
        let Some(db) = state.logs_db.clone() else {
            return Ok(error_response(
                "로그 색인이 없다",
                StatusCode::SERVICE_UNAVAILABLE,
            ));
        };
        let stats = tokio::task::spawn_blocking(move || {
            rocky_core::logindex::LogIndex::open(&db).and_then(|index| index.stats(&since, &until))
        })
        .await
        .map_err(|e| StoreError::new(format!("통계 조회 스레드: {e}")))?
        .map_err(|e| StoreError::new(format!("통계 조회: {e}")))?;
        return Ok(ok_json(&stats));
    }
    if *method == Method::GET && path == "/api/logs/worklog" {
        // 보드를 고르면 그 보드 `path` 의 레포 작업로그만 — 키는 작업로그와 같은 함수로 계산한다(워크트리는 레포
        // 루트로 접힌다). 보드에 path 가 없으면 고를 레포가 없다(`unlinked`).
        let board = query
            .get("board")
            .map(|b| b.trim().to_string())
            .filter(|b| !b.is_empty() && b != "all");
        let board_path = match &board {
            Some(key) => {
                let Some(found) = store.list_boards(true)?.into_iter().find(|b| &b.key == key)
                else {
                    return Ok(error_response(
                        &format!("보드가 없다: {key}"),
                        StatusCode::NOT_FOUND,
                    ));
                };
                match found.path {
                    Some(path) => Some(path),
                    None => return Ok(ok_json(&json!({ "entries": [], "unlinked": true }))),
                }
            }
            None => None,
        };
        let text = |k: &str| {
            query
                .get(k)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        let limit = query
            .get("limit")
            .and_then(|l| l.parse::<usize>().ok())
            .unwrap_or(50);
        let mut worklog_query = rocky_core::logindex::WorklogQuery {
            project_keys: None,
            todo_ref: text("todo"),
            kind: text("kind"),
            text: text("q"),
            before: text("before"),
            limit,
        };
        let Some(db) = state.logs_db.clone() else {
            return Ok(ok_json(&json!({ "entries": [] })));
        };
        let entries = tokio::task::spawn_blocking(move || {
            if let Some(path) = board_path {
                worklog_query.project_keys = Some(vec![rocky_core::worklog::default_project_key(
                    std::path::Path::new(&path),
                    &rocky_core::worklog::git_common_dir,
                )]);
            }
            rocky_core::logindex::LogIndex::open(&db)
                .and_then(|index| index.worklog(&worklog_query))
        })
        .await
        .map_err(|e| StoreError::new(format!("작업로그 조회 스레드: {e}")))?
        .map_err(|e| StoreError::new(format!("작업로그 조회: {e}")))?;
        return Ok(ok_json(&json!({ "entries": entries })));
    }
    if *method == Method::GET && path == "/api/prs/open" {
        let repo = query.get("repo").map(String::as_str).unwrap_or("").trim();
        if !rocky_core::prwatch::is_repo_slug(repo) {
            return Ok(error_response(
                &format!("repo(owner/name)가 필요하다: {repo:?}"),
                StatusCode::BAD_REQUEST,
            ));
        }
        // 아무 레포나 물어 GitHub 예산을 쓰지 못하게 — 보드에 붙은 레포이거나 구독한 레포만.
        let lower = repo.to_lowercase();
        let known = store
            .list_boards(false)?
            .into_iter()
            .filter_map(|b| b.repo)
            .chain(store.pr_subscriptions()?.into_iter().map(|s| s.repo))
            .any(|r| r.to_lowercase() == lower);
        if !known {
            return Ok(error_response(
                &format!(
                    "보드나 구독에 없는 레포다: {repo} — 보드에 GitHub 을 붙이거나 PR 을 구독한다"
                ),
                StatusCode::BAD_REQUEST,
            ));
        }
        let now = chrono::Utc::now().timestamp();
        let cached = state
            .open_prs
            .lock()
            .expect("open_prs poisoned")
            .get(&lower)
            .filter(|(at, _)| now - at < 60)
            .map(|(_, prs)| prs.clone());
        let mut prs = match cached {
            Some(prs) => prs,
            None => {
                let (owner, name) = repo.split_once('/').unwrap_or_default();
                let data = match crate::prwatch::query_github(
                    &state.runner(),
                    rocky_core::prwatch::OPEN_PRS_QUERY,
                    &[
                        ("owner".to_string(), owner.to_string()),
                        ("name".to_string(), name.to_string()),
                    ],
                )
                .await
                {
                    Ok(data) => data,
                    Err(reason) => {
                        return Ok(error_response(
                            &format!("{repo} 의 열린 PR 을 못 읽었다: {reason}"),
                            StatusCode::BAD_GATEWAY,
                        ))
                    }
                };
                let prs = match rocky_core::prwatch::parse_open_prs(&data) {
                    Ok(prs) => prs,
                    Err(reason) => {
                        return Ok(error_response(
                            &format!("{repo} 응답을 못 읽었다: {reason}"),
                            StatusCode::BAD_GATEWAY,
                        ))
                    }
                };
                state
                    .open_prs
                    .lock()
                    .expect("open_prs poisoned")
                    .insert(lower, (now, prs.clone()));
                prs
            }
        };
        for pr in &mut prs {
            pr.subscribed = store.pr_subscription(repo, pr.number)?.is_some();
        }
        return Ok(ok_json(&prs));
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
#[allow(clippy::too_many_arguments)]
async fn handoff_route(
    state: &Arc<ServerState>,
    r: &str,
    query: &HashMap<String, String>,
    headers: &HeaderMap,
    body: Body,
    actor: &str,
    local: bool,
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

    // 핸드오프는 큐에 쓰는 일이라 캐시 없는 목록으로 대상을 고른다 — 지난 목록이면 이미 끝난 세션을
    // 살아 있는 대상으로 보고 아무도 집지 않을 핸드오프를 남긴다(핸드오프엔 TTL 이 없다).
    let result = state.fresh_sessions().await;
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
        // 잠든(`blocked`)·끝난 background 세션은 자동 대상이 아니다 — 잠든 세션의 cwd 는 레포 루트라 보드와 맞는다.
        let candidates: Vec<&AgentSession> = match_board(&result.sessions, &board_key)
            .into_iter()
            .filter(|s| takes_handoff(s))
            .collect();
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
    // 그 세션이 받은편지함 소켓을 등록했으면 poke 를 바로 꽂아 턴을 연다 — 웹의 "에이전트에게 보내기" 는
    // poke 를 보낼 길이 없어, 쉬는 세션은 다음 턴(사람이 뭔가 칠 때)까지 집지 못했다. 열린 턴의
    // UserPromptSubmit 훅이 큐에서 집어 전체 지시를 주입한다. 등록이 없으면 지금처럼 큐에서 기다린다.
    // 세션을 깨워 턴을 여는 건 세션을 움직이는 일이라 로컬 요청만 한다(세션 띄우기와 같은 경계) — 노출된 주소
    // (tailscale·Cloudflare)로 온 핸드오프는 큐에만 넣고 그 세션의 다음 턴을 기다린다.
    let woke = local
        && wake_session(
            state,
            &target.session_id,
            &poke.message,
            format!("{todo_ref} {}", todo.title),
        )
        .await;
    let mut out = serde_json::to_value(&handoff).map_err(|e| StoreError::new(e.to_string()))?;
    out["poke"] = serde_json::to_value(&poke).map_err(|e| StoreError::new(e.to_string()))?;
    out["woke"] = json!(woke);
    Ok(json_response(&out, StatusCode::CREATED))
}

/// 핸드오프 대상 세션의 받은편지함에 한 줄 — 썼으면 true. "보내지 않기" 는 보지 않는다: 그건 자동 알림을 끄는
/// 스위치이고, 핸드오프는 사람이 그 세션을 골라 누른 것이다. 전달 기록(`/api/deliveries`)에 `handoff` 로 남긴다.
async fn wake_session(
    state: &Arc<ServerState>,
    session_id: &str,
    text: &str,
    subject: String,
) -> bool {
    let Ok(target) = state.live_inbox(session_id).await else {
        return false;
    };
    let line = rocky_core::peer_inbox::inbox_line(text);
    let socket = target.socket.clone();
    let reason = match tokio::task::spawn_blocking(move || {
        crate::prwatch::write_inbox(&socket, &line)
    })
    .await
    {
        Ok(Ok(())) => None,
        Ok(Err(e)) => Some(e.to_string()),
        Err(e) => Some(e.to_string()),
    };
    let ok = reason.is_none();
    state.record_delivery(rocky_core::peer_inbox::Delivery {
        at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        kind: "handoff".to_string(),
        subject,
        url: None,
        session_id: session_id.to_string(),
        ok,
        reason,
    });
    if !ok {
        state.forget_inbox(session_id);
    }
    ok
}

/// POST /api/rc/servers/:label/{start,restart} — 받으면 202, 결과는 현황(`action` · `lastResult`)으로 본다.
fn rc_command_route(
    state: &Arc<ServerState>,
    label: &str,
    command: crate::rc::RcCommand,
    local: bool,
) -> Response {
    use crate::rc::RcRefusal;
    if !local {
        return error_response(NON_LOCAL_SPAWN_MESSAGE, StatusCode::FORBIDDEN);
    }
    let Some(control) = state.rc_control.clone() else {
        return error_response("이 기기에서는 rc 가 꺼져 있다", StatusCode::NOT_FOUND);
    };
    let label = percent_decode(label);
    match control.begin(&label, command.clone()) {
        Ok(target) => {
            tokio::spawn(async move {
                control.run(target, command).await;
            });
            json_response(
                &json!({ "label": label, "accepted": true }),
                StatusCode::ACCEPTED,
            )
        }
        Err(RcRefusal::NotFound(m)) => error_response(&m, StatusCode::NOT_FOUND),
        Err(RcRefusal::Busy(m)) => error_response(&m, StatusCode::CONFLICT),
        Err(RcRefusal::Ambiguous(m)) => error_response(&m, StatusCode::CONFLICT),
    }
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

    // 이미 도는 세션이 있으면 새로 띄우지 않는다 — 세션 재사용(기존 큐로 pending). 받은편지함을 등록한 세션이면 핸드오프
    // 라우트처럼 깨운다 — 쉬는 rc 세션은 깨우지 않으면 사람이 뭔가 칠 때까지 집지 않는다.
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
        let woke =
            wake_for_handoff(state, &live.session_id, &live.name, &todo_ref, &todo.title).await;
        return Ok(json_response(
            &json!({ "handoff": handoff, "reused": true, "worktreePath": worktree_path, "woke": woke }),
            StatusCode::CREATED,
        ));
    }

    let session_name = format!("{}-{}", board_key.as_deref().unwrap_or("todo"), todo.number);

    // rc 가 켜진 기기 — 워크트리에서 단일 세션 rc 서버를 띄우고, 그 세션에 핸드오프를 넣어 깨운다(`rc::handoff`). `claude --bg`
    // 세션은 로그인 세션 밖이라 ssh · 자격이 끊겨 PR 로 끝나는 일을 끝내지 못한다.
    if let Some(control) = state.rc_control.clone() {
        return spawn_rc(
            state,
            &control,
            SpawnRc {
                r,
                todo: &todo,
                todo_ref: &todo_ref,
                board_key: board_key.as_deref().unwrap_or("todo"),
                board_path: &board_path,
                worktree_path: &worktree_path,
                note,
                actor,
                current_board_id,
            },
            persist_path_if_given,
        )
        .await;
    }
    // 예약은 실행 **전**, 확인과 함께 한 락 안에서 — 앞의 `is_recent` 와 여기 사이에 세션 목록 await 가 끼어 겹친 요청이 둘 다
    // 여기까지 올 수 있다. 진 쪽은 409.
    let Some(reservation) = state.recent_spawns.try_reserve(&worktree_path) else {
        return Ok(error_response(
            &format!("방금 이 워크트리에 세션을 띄웠다 — 잠시 후 다시 시도하라: {worktree_path}"),
            StatusCode::CONFLICT,
        ));
    };
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
                reservation.release();
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
        &json!({
            "handoff": handoff,
            "reused": false,
            "worktreePath": worktree_path,
            "sessionShortId": short_id,
            "warning": BG_SPAWN_WARNING,
        }),
        StatusCode::CREATED,
    ))
}

/// spawn 의 rc 갈래에 넘기는 것 — 라우트가 이미 검증 · 정규화한 값.
struct SpawnRc<'a> {
    r: &'a str,
    todo: &'a Todo,
    todo_ref: &'a str,
    board_key: &'a str,
    board_path: &'a str,
    worktree_path: &'a str,
    note: Option<String>,
    actor: &'a str,
    current_board_id: Option<String>,
}

/// spawn 의 rc 갈래 — 예약 → 점검(현황 · 자격 · 이미 있는 서버) → 워크트리 → 대기 중 핸드오프 재확인 → 서버 → 세션 등록 →
/// 핸드오프 + 깨우기. 예약은 확실히 아무것도 안 띄웠을 때만 되돌리고, 서버가 뜬 뒤의 실패는 서버를 남긴 채 그 이름을 알린다.
async fn spawn_rc(
    state: &Arc<ServerState>,
    control: &crate::rc::RcController,
    req: SpawnRc<'_>,
    persist_path_if_given: impl Fn(&TodoStore) -> StoreResult<()>,
) -> StoreResult<Response> {
    let store = &state.store;
    let worktree_path = req.worktree_path;
    // 확인과 예약을 한 락 안에서 — 앞의 `is_recent` 와 여기 사이의 await 로 겹친 요청이 둘 다 올 수 있다. 요청이 끝날 때까지
    // 진행 중으로 잡혀 오래 걸려도(워크트리 · 서버 · 세션 등록) TTL 이 도중에 끝나지 않는다.
    let Some(reservation) = state.recent_spawns.try_reserve(worktree_path) else {
        return Ok(error_response(
            &format!("방금 이 워크트리에 세션을 띄웠다(또는 띄우는 중이다) — 잠시 후 다시 시도하라: {worktree_path}"),
            StatusCode::CONFLICT,
        ));
    };
    if let Err(e) = control.check_handoff_dir(worktree_path).await {
        reservation.release();
        return Ok(error_response(&e, StatusCode::CONFLICT));
    }
    // 기동 로그 · 이벤트 라벨 — 보드 key 는 원격에서 바꿀 수 있어 파일 이름에 안전한 글자만.
    let log_label = rocky_core::rc::handoff_log_label(req.board_key, req.todo.number);
    let base = match control
        .ensure_worktree(
            req.board_path,
            worktree_path,
            &worktree_name_for(req.todo.number),
            &log_label,
        )
        .await
    {
        Ok(base) => base,
        Err(e) => {
            reservation.release();
            return Ok(error_response(&e, StatusCode::BAD_REQUEST));
        }
    };
    // 시작할 때 본 대기 중 핸드오프를 띄우기 직전에 한 번 더 — 그 사이 다른 길로 넘겨졌으면 서버를 띄우지 않는다.
    if store.pending_handoff_of(&req.todo.id)?.is_some() {
        reservation.release();
        return Ok(error_response(
            &format!("이 항목은 이미 다른 세션 앞에 대기 중이다: {}", req.r),
            StatusCode::CONFLICT,
        ));
    }
    let server_name =
        rocky_core::rc::handoff_server_name(req.board_key, req.todo.number, &req.todo.title);
    let since = chrono::Utc::now().timestamp();
    let server_pid = match control
        .launch_handoff(&log_label, worktree_path, &server_name, req.todo_ref)
        .await
    {
        Ok(pid) => pid,
        Err(e) => {
            reservation.release();
            return Ok(error_response(&e, StatusCode::BAD_REQUEST));
        }
    };
    let session = match control
        .wait_handoff_session(&log_label, server_pid, since, || state.inboxes())
        .await
    {
        crate::rc::HandoffWait::Found(session) => session,
        crate::rc::HandoffWait::ServerGone(tail) => {
            control.forget_handoff(&log_label);
            reservation.release();
            return Ok(error_response(
                &format!("rc 서버 \"{server_name}\"(pid {server_pid}) 가 세션 등록 전에 내려갔다 — {tail}"),
                StatusCode::BAD_REQUEST,
            ));
        }
        crate::rc::HandoffWait::TimedOut { ps_error } => {
            // 서버는 떴다 — 남긴다(폰 · 웹에서 열 수 있다). 예약도 남긴다.
            let why = ps_error
                .map(|e| format!(" — 그동안 ps 를 못 읽었다: {e}"))
                .unwrap_or_default();
            return Ok(error_response(
                &format!(
                    "rc 서버 \"{server_name}\"(pid {server_pid}) 는 떴는데 {}초 안에 세션이 받은편지함을 등록하지 않았다{why}. 폰 · 웹에서 열 수 있다 — 서버는 그대로 두었다",
                    crate::rc::HANDOFF_SESSION_WAIT.as_secs()
                ),
                StatusCode::BAD_REQUEST,
            ));
        }
    };
    // 여기서부터의 실패는 서버를 남긴다 — 사람이 그 서버를 알아보게 이름 · pid 를 붙인다.
    let recorded = persist_path_if_given(store).and_then(|()| {
        store.create_handoff(&CreateHandoffInput {
            todo_ref: req.r.to_string(),
            session_id: session.session_id.clone(),
            session_name: Some(server_name.clone()),
            session_cwd: Some(worktree_path.to_string()),
            note: req.note,
            actor: req.actor.to_string(),
            current_board_id: req.current_board_id,
        })
    });
    let handoff = match recorded {
        Ok(handoff) => handoff,
        Err(e) => {
            return Ok(error_response(
                &format!(
                    "rc 서버 \"{server_name}\"(pid {server_pid}) 의 세션을 찾았는데 핸드오프를 남기지 못했다 — {e}. 서버는 그대로 두었다"
                ),
                StatusCode::CONFLICT,
            ))
        }
    };
    let woke = wake_for_handoff(
        state,
        &session.session_id,
        &server_name,
        req.todo_ref,
        &req.todo.title,
    )
    .await;
    Ok(json_response(
        &json!({
            "handoff": handoff,
            "reused": false,
            "worktreePath": worktree_path,
            "server": { "pid": server_pid, "name": server_name },
            "base": base.flatten(),
            "woke": woke,
        }),
        StatusCode::CREATED,
    ))
}

/// rc 가 꺼진 기기의 `claude --bg` 세션 — 로그인 세션 밖에서 돌아 ssh · 자격이 끊길 수 있다(2026-09-04 · 2026-10-05 실측).
const BG_SPAWN_WARNING: &str =
    "claude --bg 세션은 로그인 세션 밖에서 돌아 ssh · 자격이 끊길 수 있다 — PR 로 끝나는 일은 끝내지 못할 수 있다(rc 가 켜진 기기는 rc 서버로 띄운다)";

/// 핸드오프를 받은 세션을 깨운다 — 핸드오프 라우트와 같은 poke(늘리지 않는다), 받은편지함이 없으면 false.
async fn wake_for_handoff(
    state: &Arc<ServerState>,
    session_id: &str,
    session_name: &str,
    todo_ref: &str,
    todo_title: &str,
) -> bool {
    let poke = build_handoff_poke(&HandoffPokeInput {
        session_name,
        todo_ref,
        todo_title,
    });
    wake_session(
        state,
        session_id,
        &poke.message,
        format!("{todo_ref} {todo_title}"),
    )
    .await
}

/// 에이전트의 `start` 직전 — 이 할 일 앞으로 배달됐지만 착수 안 된 핸드오프 중 **버려진 것**(받은 세션이 사라졌거나, 다시
/// 보내 밀렸고 그 세션이 일하지 않는 것 — `gone_handoffs`)을 취소한다. 그대로 두면 `start` 가 그것을 수락해 doing 이 엉뚱한 세션에 귀속된다 — Stop 확인 · 턴 태그가 엉뚱한 세션으로 가고,
/// 24시간 뒤 자동 해제가 일하는 중인 할 일을 멈춘다(2026-10-05 실측). 배달 직후(유예 안)는 더 새 요청에 밀리지 않았으면
/// 보지 않고, 세션 목록을 못 얻으면 손대지 않는다. 사람이 누른 `start` 는 핸드오프를 수락하지 않으므로 보지 않는다. 주기 스윕이 아니라 여기서
/// 하는 이유: 아무도 착수하지 않은 동안엔 할 일 상세의 "받았지만 착수하지 않았어요" 경고가 사람에게 남아야 한다.
pub async fn drop_gone_handoffs(
    state: &Arc<ServerState>,
    todo_ref: &str,
    current_board_id: Option<&str>,
    actor: &str,
) {
    if !rocky_core::actors::is_agent_actor(actor) {
        return;
    }
    let store = &state.store;
    let Ok(Some(todo)) = store.get_todo(todo_ref, current_board_id) else {
        return;
    };
    let Ok(handoffs) = store.list_handoffs(&ListHandoffsFilter {
        todo_id: Some(todo.id),
        ..Default::default()
    }) else {
        return;
    };
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let overdue = overdue_unaccepted(&handoffs, &now, GONE_HANDOFF_GRACE_SECS);
    if overdue.is_empty() {
        return;
    }
    // 상태를 바꾸는 판단이라 캐시 없는 목록으로 본다(스윕과 같은 이유).
    let sessions = state.fresh_sessions().await;
    for handoff in gone_handoffs(&overdue, &sessions) {
        // 그 사이 누가 수락했으면 거절된다 — 그대로 둔다.
        let _ = store.cancel_handoff(&handoff.id, crate::sweep::SWEEP_ACTOR);
    }
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
    // 이 세션이 구독했을 수 있는 PR 중 알릴 것(머지 후보·충돌) — 세션 별칭은 아래에서 푼다.
    let (pr_subscriptions, notable_prs) = if session.is_some() {
        let prs: Vec<_> = store
            .list_prs(None, true)?
            .into_iter()
            .filter(|p| p.ready || p.merge_state == "DIRTY")
            .collect();
        if prs.is_empty() {
            (Vec::new(), prs)
        } else {
            (store.pr_subscriptions()?, prs)
        }
    } else {
        (Vec::new(), Vec::new())
    };
    // 보여줄 게 없으면 **세션 조회 전에** 빈 문자열 — 초당 도는 최빈 경로의 비용 절감.
    if doing.is_empty()
        && pending.is_empty()
        && due == 0
        && collect == 0
        && pr_subscriptions.is_empty()
    {
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
    let (pr_ready, pr_conflict) = aliases
        .as_ref()
        .map(|aliases| {
            rocky_core::statusline::session_pr_counts(&pr_subscriptions, &notable_prs, aliases)
        })
        .unwrap_or((0, 0));

    Ok(render_statusline(
        &state.statusline_template,
        &StatuslineData {
            mine,
            inbox,
            pr_ready,
            pr_conflict,
            stale,
            due,
            collect,
            doing: board_doing.len() as i64,
        },
        STATUSLINE_TITLE_MAX,
    ))
}

/// `?limit=` — 세션 상세에 실을 최근 턴 수(기본 50, 최대 500).
fn token_turn_limit(query: &HashMap<String, String>) -> usize {
    query
        .get("limit")
        .and_then(|l| l.parse::<usize>().ok())
        .unwrap_or(50)
        .clamp(1, 500)
}

/// GET /api/events — store change 이벤트를 SSE 로 흘린다.
fn sse_response(state: &Arc<ServerState>) -> Response {
    // 구독자는 payload 를 보지 않고 refetch 만 하므로 밀려도 무해 — 조용히 이어 간다.
    sse_from(state.events.subscribe(), OnLag::Skip, None)
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
pub(crate) fn encode_b64(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

pub(crate) fn decode_b64(text: &str) -> Result<Vec<u8>, String> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(text.trim())
        .map_err(|e| format!("bad base64: {e}"))
}

/// broadcast 채널 하나를 SSE 응답으로 — 전역 `/api/events` 와 노트별 문서 스트림이 같이 쓴다.
fn sse_from(
    receiver: broadcast::Receiver<String>,
    on_lag: OnLag,
    event: Option<&'static str>,
) -> Response {
    use tokio_stream::wrappers::BroadcastStream;
    use tokio_stream::StreamExt;

    // 이름 붙은 이벤트(`event:`)는 EventSource 의 기본 onmessage 로 가지 않는다 — 구독자가 이름으로 고른다.
    let frame = move |payload: String| {
        let head = event.map(|e| format!("event: {e}\n")).unwrap_or_default();
        Ok::<_, std::convert::Infallible>(format!("{head}data: {payload}\n\n").into_bytes())
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
