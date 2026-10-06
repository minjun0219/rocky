// 데몬 REST 응답 타입 — Rust `rocky_core::types`/`refs` 의 JSON 모양(camelCase)을 그대로 옮긴 것.
// 정본은 Rust 쪽이고(`docs/rewrite/contract.md`), 여기는 UI 가 typecheck 에 쓰는 사본이다.
// 옛 TS 데몬(`src/store.ts` 등)의 선언에서 필요한 것만 모았다.

export type TodoStatus = 'todo' | 'doing' | 'done';

export type TodoPriority = 'p1' | 'p2' | 'p3' | 'p4';

export type StatusAction = 'start' | 'stop' | 'done' | 'reopen' | 'archive' | 'unarchive';

export type HistoryEntity = 'board' | 'section' | 'todo' | 'note';

export interface TodoLink {
  url: string;
  title?: string;
}

export interface Board {
  id: string;
  key: string;
  title: string;
  /** 이 보드가 무엇인가 — 한 줄 설명. 설정 전에는 undefined. */
  description?: string;
  /** `owner/name` — GitHub 이슈 생성 대상. 설정 전에는 undefined. */
  repo?: string;
  /** 메인 레포의 절대경로 — 백그라운드 세션을 띄우는 자리. 설정 전에는 undefined. */
  path?: string;
  /**
   * 이 보드가 예전에 쓰던 key 들 — {@link TodoStore.updateBoard} 의 key 변경이 남긴다.
   * 옛 참조(`gotgan-12`)와 옛 `board` 인자가 계속 이 보드로 풀린다. 없으면 생략된다.
   */
  previousKeys?: string[];
  /** PR 감시가 알릴 작성자(`@me`·login). 없으면 전부 알린다. */
  prAuthors?: string[];
  /** 리뷰가 붙으면 이 레포의 세션이 review-fix 를 돈다 — 켰을 때만 실린다. */
  reviewFix?: boolean;
  createdAt: string;
  archivedAt?: string;
}

export interface Section {
  id: string;
  boardId: string;
  title: string;
  position: number;
  archivedAt?: string;
}

export interface Todo {
  id: string;
  /** 보드별 순번 — 사람이 읽고 부르는 참조(rocky-12). id 와 달리 보드 안에서만 유일하다. */
  number: number;
  boardId: string;
  sectionId?: string;
  parentId?: string;
  title: string;
  description: string;
  status: TodoStatus;
  priority: TodoPriority;
  due?: string;
  labels: string[];
  links: TodoLink[];
  doingBy?: string;
  doingSince?: string;
  /**
   * 이 doing 을 들고 있는 Claude Code 세션. `/mcp` 는 stateless 라 도구 호출에 세션 식별자가 없어서,
   * 세션을 아는 두 경로에서만 채워진다 — claim 된 핸드오프, 그리고 세션이 스스로 `start` 한 것을
   * PostToolUse 훅이 붙인 것(`doingSessionClaimed`). 핸드오프 귀속이면 `/api/sessions` 와 대조해
   * "죽은 doing" 을 정확히 판정한다(훅 귀속은 판정에 쓰지 않는다 — `doingState` 는 서버가 낸다).
   */
  doingSessionId?: string;
  /** `doingSessionId` 가 훅이 붙인 귀속(세션이 스스로 start). */
  doingSessionClaimed?: boolean;
  position: number;
  createdAt: string;
  updatedAt: string;
  completedAt?: string;
  archivedAt?: string;
}

export interface Note {
  id: string;
  /** 보드별 순번 — 사람이 읽고 부르는 참조(rocky-12). id 와 달리 보드 안에서만 유일하다. */
  number: number;
  boardId?: string;
  title: string;
  content: string;
  position: number;
  createdAt: string;
  updatedAt: string;
  archivedAt?: string;
  /** 고정한 시각 — 있으면 노트 화면 맨 위에 카드로 펼쳐 둔다(고정한 순서). */
  pinnedAt?: string;
}

/** todo 한 건에 달리는 댓글 — 에이전트의 진행 보고와 사용자의 답이 같은 타임라인에 쌓인다. */
export interface Comment {
  id: string;
  todoId: string;
  actor: string;
  body: string;
  createdAt: string;
  updatedAt: string;
  archivedAt?: string;
}

export type HandoffStatus = 'pending' | 'delivered' | 'cancelled';

/** 배달 경로 — Stop 훅이 집었나, UserPromptSubmit 훅이 집었나. */
export type HandoffVia = 'stop' | 'prompt' | 'spawn';

/** 보드에서 실행 중인 Claude Code 세션으로 넘긴 작업 요청 한 건. */
export interface Handoff {
  id: string;
  todoId: string;
  sessionId: string;
  /** 표시용 스냅샷 — 세션이 사라지면 sessionId 만으로는 어디로 보냈는지 읽을 수 없다. */
  sessionName?: string;
  sessionCwd?: string;
  note: string;
  actor: string;
  status: HandoffStatus;
  createdAt: string;
  deliveredAt?: string;
  deliveredVia?: HandoffVia;
  /**
   * 대상 세션이 실제로 착수한 시각 — 그 todo 에 `start`(또는 start 를 건너뛴 `done`)가
   * 온 순간 `setTodoStatus` 가 찍는다. `delivered` 인데 이게 비어 있으면 "집어갔는데
   * 아무 일도 안 일어났다"는 뜻이다.
   */
  acceptedAt?: string;
  /** 그 todo 가 `done` 된 시각. */
  completedAt?: string;
}

export interface HistoryEntry {
  id: number;
  entity: HistoryEntity;
  entityId: string;
  actor: string;
  action: string;
  changes?: Record<string, [unknown, unknown]>;
  at: string;
}

export type DoingState = 'live' | 'idle' | 'gone' | 'unknown';

export type HandoffPhase = 'pending' | 'delivered' | 'accepted' | 'completed' | 'cancelled';

/** 응답 전용 핸드오프 — 저장 모델에 세션 대조로만 알 수 있는 판정을 얹은 형태. */
export interface HandoffView extends Handoff {
  phase: HandoffPhase;
  /** 배달됐는데 그 세션이 아무것도 안 했다 ({@link isUnstarted}). */
  unstarted: boolean;
  /** pending 인데 대상 세션이 사라졌다. 큐에는 그대로 남는다 — 표시만 하는 값이다. */
  stale: boolean;
}

/** `POST /api/todos/:ref/spawn` 응답. rc 가 켜진 기기는 `server`(핸드오프 서버), 아니면 `sessionShortId`(`claude --bg`)와 `warning`. */
export interface SpawnResult {
  handoff: Handoff;
  reused: boolean;
  worktreePath: string;
  sessionShortId?: string;
  server?: { pid: number; name: string };
  /** 받은 세션을 받은편지함으로 깨웠나 — false 면 그 세션의 다음 턴에 집는다. */
  woke?: boolean;
  warning?: string;
}

export interface AgentSession {
  /** 프로세스가 없는 background 세션(사람 답을 기다리며 잠든 `blocked` 등)에는 없다. */
  pid?: number;
  cwd: string;
  /** 'interactive' | 'background' — CLI 가 주는 값을 그대로 둔다. */
  kind: string;
  /**
   * 짧은 id(8자) — `claude attach/logs/stop/rm` 이 받는 값이자 `sessionId` 의 접두사다.
   * background 세션에만 붙는다.
   */
  id?: string;
  sessionId: string;
  /** 사람이 읽는 세션 이름 (예: `eelpout-a3`). */
  name: string;
  /** 'idle' | 'busy' — CLI 가 주는 값을 그대로 둔다. */
  status: string;
  /**
   * background 세션의 수명 상태 — 'working' | 'blocked'(사람 답을 기다림) | 'done'. interactive 세션에는 없다.
   * 없음(undefined)은 "죽지 않았다"로 읽는다 — 살아 있는 interactive 세션이 그 꼴이다.
   */
  state?: string;
  startedAt: number;
}

/**
 * background 세션의 작업 요약 — Claude Code 의 `~/.claude/jobs/<id>/state.json` 에서 데몬이 고른 것
 * (`rocky_core::sessions::JobSummary`). 내부 파일이라 못 읽으면 통째로 없다.
 */
export interface JobSummary {
  /** 지금 하는 일 또는 멈춘 자리 한 줄. */
  detail?: string;
  /** 사람에게 필요한 것 — `blocked` 일 때 무엇을 기다리는지. */
  needs?: string;
  /** 요약을 마지막으로 고친 시각(ISO). */
  updatedAt?: string;
}

/** `GET /api/sessions` 의 한 행. */
export interface SessionRow extends AgentSession {
  /** `?board=` 와 cwd 가 맞는가. board 없이 물으면 늘 false. */
  matched: boolean;
  job?: JobSummary;
}

export interface SessionsResult {
  /** 세션 목록을 얻을 수 있었는가. false 면 이 기능 전체가 비활성이다. */
  available: boolean;
  sessions: AgentSession[];
  /** available 이 false 인 이유 — 사용자에게 그대로 보여준다. */
  reason?: string;
}

/** 응답 전용 todo — 저장 모델에 사람이 쓰는 참조(ref)와 댓글 집계를 얹은 형태. */
export interface TodoView extends Todo {
  /** `rocky-12` — 보드 접두사를 포함한 완전 참조. */
  ref: string;
  /** 보관되지 않은 댓글 수 — 목록의 배지용. */
  commentCount: number;
  /** 가장 최근 댓글 시각(ISO). 댓글이 없으면 undefined. */
  lastCommentAt?: string;
  /**
   * 이 doing 이 살아 있는가 — `doing` 인 항목에만, 그리고 서버가 세션 목록을 실제로
   * 조회한 응답에만 붙는다. **부재 = 판정하지 않았다**이며 `unknown` 과 같게 다룬다
   * (`src/doing.ts` 의 `resolveDoingState` 가 채운다).
   */
  doingState?: DoingState;
}

/** 응답 전용 note. 글로벌 메모는 보드 대신 예약 접두사가 붙어 `note-3` 이 된다. */
export interface NoteView extends Note {
  ref: string;
}

/**
 * 상세 화면의 히스토리에서 빼는 action — 댓글은 타임라인에 본문째 따로 나오므로 히스토리로
 * 중복 표시하지 않는다. Rust `rocky_core::types::DETAIL_HISTORY_EXCLUDED` 와 같은 목록.
 */
export const DETAIL_HISTORY_EXCLUDED: ReadonlySet<string> = new Set(['comment', 'comment-edit']);

/** 데몬 PR 감시의 스냅숏 — Rust `rocky_core::prwatch::PrSnapshot` 의 사본. */
export interface PrSnapshot {
  repo: string;
  number: number;
  title: string;
  url: string;
  state: 'OPEN' | 'MERGED' | 'CLOSED';
  isDraft: boolean;
  base: string;
  head: string;
  mergeState: string;
  ci: 'pass' | 'fail' | 'pending';
  unhandled: number;
  /** 처리 안 된 스레드 id — 데몬의 새 리뷰 판정용(웹은 쓰지 않는다). 옛 스냅숏엔 없다. */
  unhandledIds?: string[];
  decision: number;
  ready: boolean;
  updatedAt: string;
}

/** 보드 화면의 보기 — 피드(첫 화면) / 할 일 목록 / 노트 / 작업로그 / GitHub / 원격 제어(rc 서버). */
export type BoardView = 'feed' | 'todos' | 'notes' | 'worklog' | 'github' | 'agents' | 'rc';

/** `GET /api/rc/servers` 의 대상 행 — Rust `rocky_core::rc::ServerRow` 의 사본. */
export interface RcServerRow {
  label: string;
  dir: string;
  pinned: boolean;
  running: boolean;
  pid?: number;
  uptimeSecs?: number;
  sessions: number;
  /** 데몬이 지금 하는 일 — 없으면 쉬는 중. */
  action?: 'starting' | 'restarting' | 'retrying' | 'waiting';
  /** 마지막 띄우기 · 재시작 결과(데몬이 다시 뜨면 사라진다). */
  lastResult?: { ok: boolean; message: string; at: string };
  /** 자격이 끊겼다 돌아오기 전에 뜬 서버 — 죽은 토큰을 들고 있을 수 있다(감시가 켜져 있을 때만). */
  authSuspect?: boolean;
  /** 기동 버전 기록이 지금 설치 버전과 다르다 — 야간 재시작이 쉬는 때 다시 띄운다. */
  stale?: boolean;
  /** 최근 활동(git) — `?activity=1` 로 부를 때만(로컬 전용). 웹은 아직 쓰지 않는다. */
  activity?: {
    repo: boolean;
    dirty?: boolean;
    branch?: string;
    defaultBranch?: string;
    commitAt?: number;
    subject?: string;
    active: boolean;
  };
}

/** 야간 재시작 한 대상의 결과 — Rust `rocky_core::rc::NightlyItem`. */
export interface RcNightlyItem {
  label: string;
  outcome: 'restarted' | 'current' | 'skipped' | 'down' | 'would-restart' | 'would-wait';
  note: string;
}

/** 야간 재시작 한 번 — Rust `rocky_core::rc::NightlyReport`. */
export interface RcNightlyReport {
  startedAt: string;
  finishedAt?: string;
  dryRun?: boolean;
  update: string;
  version?: string;
  blocked?: string;
  canaryFailed?: boolean;
  /** rocky 세 층의 버전과 최신 릴리스 태그 — 진짜 실행에만. */
  rocky?: { plugin?: string; cli?: string; daemon: string; latest?: string };
  /** agy 와 원격 제어 데몬 — 진짜 실행에만, agy 가 없으면 없다. 시각은 unix 초. */
  agy?: {
    version?: string;
    state?: string;
    pid?: number;
    instance?: string;
    started?: number;
    binaryMtime?: number;
    oldBinary: boolean;
  };
  items: RcNightlyItem[];
}

/** 대상 목록 밖의 폴더에서 도는 서버 — Rust `rocky_core::rc::StrayRow`. */
export interface RcStrayRow {
  label: string;
  dir: string;
  pid: number;
  uptimeSecs?: number;
  sessions: number;
}

/** 보드의 새 세션 띄우기가 띄운 rc 서버 — Rust `rocky_core::rc::HandoffServerRow`. 사람이 닫는다. */
export interface RcHandoffRow {
  label: string;
  /** claude.ai 에 보이는 이름(`<보드>-<n>: <요약>`). */
  name: string;
  todoRef: string;
  dir: string;
  pid: number;
  uptimeSecs?: number;
  sessions: number;
}

/** `GET /api/rc/servers` — Rust `rocky_core::rc::RcStatus`. `configured` 가 false 면 이 기기에선 rc 가 꺼져 있다. */
export interface RcStatus {
  configured: boolean;
  servers: RcServerRow[];
  strays: RcStrayRow[];
  /** 대상 밖 서버 중 데몬이 띄운 핸드오프 서버 — 없으면 오지 않는다. */
  handoffs?: RcHandoffRow[];
  auth: 'in' | 'out' | 'unknown';
  antigravity: { state?: string; pid?: number; instance?: string } | null;
  probeError?: string;
  /** 감시(`rc.supervise`)가 켜져 있으면 그 상태 — 꺼져 있으면 없다. */
  supervise?: { lastTick?: string; loggedOut: boolean };
  /** 야간 재시작(`rc.nightly`) — 일정이 켜졌거나 손으로 돌린 결과가 있으면. `at` 은 일정의 현지 시각. */
  nightly?: { at?: string; running: boolean; last?: RcNightlyReport };
}

/** `GET /api/logs/worklog` 한 줄 — Rust `rocky_core::logindex::IndexedWorklog` 의 사본. */
export interface WorklogEntry {
  id: string;
  projectKey: string;
  timestamp: string;
  kind: string;
  content: string;
  tags: string[];
  todoRef?: string;
}

/** `GET /api/summary` 의 `collectItems` — 아직 보드로 안 옮긴 수집함 항목(최대 3개, 제목은 한 줄로 편 것). */
export interface CollectItem {
  source: string;
  title: string;
  url?: string;
}

/** `GET /api/inbox` — 수집함 항목 하나. `promoted` 는 데몬이 전 보드의 링크로 채운다. */
export interface InboxItem {
  id: string;
  title: string;
  url?: string;
  note?: string;
  promoted?: boolean;
}

/** `GET /api/inbox` — 소스 하나의 결과. `board` 는 보드에 등록한 소스일 때만. */
export interface InboxSourceResult {
  name: string;
  board?: string;
  available: boolean;
  reason?: string;
  items: InboxItem[];
}

/** `GET /api/inbox/adapters` — 어댑터 하나와 그 입력 칸(`--describe`). 실패면 `error`. */
export interface InboxAdapter {
  name: string;
  title?: string;
  params?: { flag: string; label: string; placeholder?: string; required: boolean }[];
  error?: string;
}

/** `GET /api/inbox/sources` — 보드에 등록한 수집함. */
export interface BoardInboxSource {
  id: string;
  board: string;
  name: string;
  adapter: string;
  params: { flag: string; value: string }[];
  adapterMissing?: boolean;
}

/** `GET /api/deliveries` — 데몬이 세션 받은편지함에 보내는 현황(로컬 전용). */
export interface DeliveryStatus {
  sessions: {
    sessionId: string;
    cwd: string;
    board?: string | null;
    seenAt?: string | null;
    muted: boolean;
    /** 이 세션이 지금 PR 알림을 받는 보드들 — 보드마다 가장 최근의(보내지 않기가 아닌) 세션. */
    receivesPrFor: string[];
  }[];
  /** 등록은 남았지만 끝난 세션 수 — 목록에서 뺐다(등록은 하루 TTL 로 걷힌다). 옛 데몬은 없다. */
  ended?: number;
  subscriptions: { source: string; sessionId: string }[];
  recent: {
    at: string;
    kind: string;
    subject: string;
    url?: string;
    sessionId: string;
    ok: boolean;
    /** 못 보냈으면 왜 — `받을 세션 등록 없음`·`세션이 /clear 됨 …` 또는 소켓 에러. */
    reason?: string;
  }[];
  /** `/clear` 돼 남은 구독을 사람이 정할 때까지 깨우지 않는 세션들(`rocky_core::peer_inbox::ClearedSession`). 옛 데몬은 없다. */
  cleared?: ClearedSession[];
}

/** `/clear` 된 세션 하나와 남은 구독 — 넘기기·지켜보기만·해지 중 하나를 고른다(`POST /api/sessions/cleared`). */
export interface ClearedSession {
  sessionId: string;
  /** 같은 프로세스의 지금 세션 — "새 세션으로 넘기기" 의 대상. */
  successorId: string;
  cwd: string;
  clearedAt: string;
  /** `owner/repo#N`. */
  prs: string[];
  filters: string[];
  inbox: string[];
  /** 기본 브랜치 검증 구독(`보드 브랜치`). 옛 데몬은 없다. */
  verify?: string[];
}

/** `POST /api/sessions/cleared` 의 action. */
export type ClearedAction = 'handover' | 'watch' | 'unsubscribe';

/** 레포의 열린 PR 한 줄 — `GET /api/prs/open?repo=`(GitHub 탭이 레포를 펼칠 때만). */
export interface OpenPr {
  number: number;
  title: string;
  url: string;
  isDraft: boolean;
  updatedAt: string;
  author?: string;
  subscribed: boolean;
}

/** `GET /api/logs/stats` — Rust `rocky_core::logindex::LogStats` 의 사본. */
export interface SurfaceStat {
  source: string;
  name: string;
  count: number;
  errors: number;
  lastTs: string;
  p50Ms?: number;
  p95Ms?: number;
}
export interface LogStats {
  since: string;
  worklog: {
    turns: number;
    byProject: [string, number][];
    byWeekday: number[];
    byTodo: [string, number][];
  };
  usage: { total: number; surfaces: SurfaceStat[]; unused: [string, string][] };
}
