// 데몬 REST 응답 타입 — Rust `rocky_core::types`/`refs` 의 JSON 모양(camelCase)을 그대로 옮긴 것.
// 정본은 Rust 쪽이고(`docs/rewrite/contract.md`), 여기는 UI 가 typecheck 에 쓰는 사본이다.
// 보드 · 노트 · 수집함 타입은 2026-11-13 웹 보드 화면을 걷어내며 뺐다.

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

/** 웹 화면의 탭 — 피드(첫 화면) / 작업로그 / GitHub / 에이전트 / 원격 제어(rc 서버). */
export type AppView = 'feed' | 'worklog' | 'github' | 'agents' | 'rc';

/** `GET /api/rc/servers` 의 대상 행 — Rust `rocky_core::rc::ServerRow` 의 사본. */
export interface RcServerRow {
  label: string;
  dir: string;
  pinned: boolean;
  /** 워크트리 모드 대상(`rc.worktree` 설정 값). 재시작 확인 줄이 쓴다(`rcRestartNote`). */
  worktree?: boolean;
  /** 떠 있는 서버가 워크트리 모드로 떴나(명령줄의 `--spawn worktree`). `worktree` 와 다르면 다시 띄워야 설정대로 뜬다. */
  liveWorktree?: boolean;
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
  outcome:
    | 'restarted'
    | 'current'
    | 'skipped'
    | 'down'
    | 'stopped'
    | 'would-restart'
    | 'would-wait'
    | 'would-stop';
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
  /** 로그아웃일 때 짚은 원인 — 새 로그인이 키체인이 아니라 `.credentials.json` 에만 갔다. 없으면 오지 않는다. */
  authHint?: 'credentialsFileNewer';
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
