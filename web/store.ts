import { create } from 'zustand';
import type { CollectItem, HandoffView, PrSnapshot } from './types';
import type { NoteView, TodoView } from './types';
import type { BoardView, RcStatus, SessionRow } from './types';
import type { Board, Comment, HistoryEntry, Section, SpawnResult, StatusAction } from './types';
import {
  advanceSeen,
  markSeen,
  readSeen,
  rcVisible,
  readThemePref,
  resolveTheme,
  THEME_KEY,
  type ThemePref,
  type Slice,
} from './lib';
import { logUsage, setUsageActor } from './usage';
import {
  type BoardSelection,
  buildPath,
  findTodoIdByNumber,
  isAddressableBoardKey,
  parseRoute,
  resolveBoardKey,
  type Route,
  routeForTodo,
  todoRefFor,
} from './route';

/**
 * 웹 UI 상태 — zustand 단일 스토어.
 *
 * 서버가 단일 진실 공급원이므로 UI 는 낙관적 갱신을 하지 않는다:
 * mutation → 서버 확정 → SSE(or 응답) → refetch 로 수렴한다.
 * actor 는 localStorage 에 저장되고 모든 mutation 의 `x-rocky-actor` 헤더로 나간다.
 */

const ACTOR_KEY = 'rocky-actor';
/** 할 일 / 노트 보기 — 새로고침·cmux 의 페이지 재로드 뒤에도 보던 쪽으로 돌아온다. */
/** 노트 보기를 마지막으로 떠난(또는 연) 시각 — 그 뒤의 편집이 "노트 •" 표시가 된다. */
const NOTES_SEEN_KEY = 'rocky:notes-seen';
/** 숨긴 GitHub 항목(PR·수집함) 키 — 이 브라우저에만. `githubHideKey` 가 만든다. */
const GITHUB_HIDDEN_KEY = 'rocky:github-hidden';
/** GitHub 탭을 보이나 — 끄면 탭도 GitHub 줄도 안 보인다. 기본 켬. */
const GITHUB_TAB_KEY = 'rocky:github-tab';
/** 원격 제어 탭을 보이나 — 기본 켬. 이 기기에서 rc 가 꺼져 있으면(`configured: false`) 이 값과 상관없이 안 보인다. */
const RC_TAB_KEY = 'rocky:rc-tab';
/** 에이전트 탭을 보이나 — 기본 켬. */
const AGENTS_TAB_KEY = 'rocky:agents-tab';

function readHidden(): string[] {
  try {
    const raw = JSON.parse(readStored(GITHUB_HIDDEN_KEY) ?? '[]') as unknown;
    return Array.isArray(raw) ? raw.filter((k): k is string => typeof k === 'string') : [];
  } catch {
    return [];
  }
}

/** 노트 보기를 떠난 뒤 이만큼은 저장 중이던 내 편집이 돌아오는 refetch 를 "본 것" 으로 친다. */
const NOTES_SETTLE_MS = 5_000;
let notesSettleUntil = 0;

/** 저장소가 막혀도(비공개 창·차단) 화면은 떠야 한다 — 읽기 실패는 저장값 없음으로. */
function readStored(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

/**
 * 주소가 가리키는 탭 — 노트 상세면 노트, `?view=` 가 있으면 그 탭, 없으면 피드(첫 화면). 보던 탭을 주소에 실어
 * 새로고침·앱 전환(폰 Safari 가 페이지를 다시 부른다)·뒤로가기·탭 복제가 모두 같은 탭으로 돌아온다(2026-10-03 오너:
 * "path 로 해야지"). GitHub 탭을 꺼 뒀으면 피드.
 */
function viewOf(route: Route): BoardView {
  if (route.note !== undefined) {
    return 'notes';
  }
  const view = route.view ?? 'feed';
  if (view === 'github' && readStored(GITHUB_TAB_KEY) === 'off') {
    return 'feed';
  }
  if (view === 'agents' && readStored(AGENTS_TAB_KEY) === 'off') {
    return 'feed';
  }
  return view === 'rc' && readStored(RC_TAB_KEY) === 'off' ? 'feed' : view;
}

/** 지금 보는 탭을 실은 주소 — store 가 주소를 쓸 때는 늘 이것으로(탭이 주소에서 빠지지 않게). */
function pathFor(route: Route): string {
  const view = useUiStore.getState().view;
  return buildPath(view === 'feed' ? route : { ...route, view });
}

function writeStored(key: string, value: string): void {
  try {
    localStorage.setItem(key, value);
  } catch {
    // 다음 방문에 기본값으로 돌아갈 뿐 — 이번 화면은 정상 동작한다.
  }
}

// BoardSelection 은 './route' 가 소유한다 — store 가 route 를 import 하므로 반대 방향은
// 순환이 된다. 기존 import 경로(`from './store'`)를 쓰는 컴포넌트를 위해 재수출한다.
export type { BoardSelection };

interface DetailState {
  kind: 'todo' | 'note';
  todo?: TodoView;
  note?: NoteView;
  history: HistoryEntry[];
  comments: Comment[];
}

interface UiState {
  boards: Board[];
  todos: TodoView[];
  /** 전 보드의 미보관 todo — "지금" 표는 보고 있는 보드와 무관하게 전체를 본다. */
  nowTodos: TodoView[];
  /** 전 보드의 열린 핸드오프 — 위 `handoffs` 는 보고 있는 보드로 좁혀져 있어 표에는 못 쓴다. */
  nowHandoffs: HandoffView[];
  /** 데몬 PR 감시의 열린 PR(전 보드). */
  prs: PrSnapshot[];
  /** 수집함 미올림 수 — 데몬이 캐시로 모르면 null. */
  collect: number | null;
  /** 미올림 수집함 항목(최대 3개) — 피드의 수집함 행을 펼치면 보인다. */
  collectItems: CollectItem[];
  sections: Section[];
  notes: NoteView[];
  selected: BoardSelection;
  showArchived: boolean;
  /** 숨긴 GitHub 항목 키(`githubHideKey`). 이 브라우저에만 남는다. */
  githubHidden: string[];
  /** GitHub 탭을 보이나. 끄면 GitHub 줄은 어디에도 안 보인다. */
  showGithub: boolean;
  /** 원격 제어 탭을 보이나(이 브라우저의 선택). 실제로 보이려면 `rc.configured` 도 참이어야 한다. */
  showRc: boolean;
  /** rc 서버 현황 — 아직 못 읽었으면 null. `configured` 가 false 면 rc 표면을 그리지 않는다. */
  rc: RcStatus | null;
  actor: string;
  /**
   * 사용자의 테마 **의도**(auto/dark/light).
   * 해석된 결과(dark/light)는 상태로 들고 있지 않다 — 그 값을 쓰는 건 CSS 뿐이고 CSS 는
   * `<html data-theme>` 에서 직접 읽는다. 사본을 두면 DOM 과 어긋날 두 번째 진실만 생긴다.
   */
  themePref: ThemePref;
  /**
   * 할 일 / 노트 — 좁은 패널에서 노트를 목록 아래 두면 스크롤 1,000px 너머라 눈이 가지 않는다.
   * 둘을 전환해 노트가 화면 전체를 쓴다(`web/DESIGN.md` "Notes").
   */
  view: BoardView;
  /** 노트 화면에서 상세로 연 노트 id — null 이면 목록. */
  openNoteId: string | null;
  /** 노트 보기를 마지막으로 연/떠난 시각(ISO). 그 뒤의 노트 편집이 전환 버튼의 표시가 된다. */
  notesSeenAt: string;
  connected: boolean;
  detail: DetailState | null;
  /** todo id → 마지막으로 확인한 댓글 시각. localStorage 의 화면용 사본. */
  seenComments: Record<string, string>;
  /**
   * 이 화면의 출처에서 GitHub 이슈를 만들 수 있는지 — `/api/health` 가 알려준다.
   * 노출된 데몬(LAN/tailscale)을 거쳐 열린 화면에서는 false 다. 어디까지나 **힌트**로,
   * 실제 거부는 서버의 이슈 라우트가 403 으로 한다 — 여기서는 누를 수 없는 버튼을 그리지
   * 않으려고 본다. 아직 안 물어봤거나 health 조회가 실패하면 true 로 두어(낙관) 기존
   * 로컬 사용 흐름이 조용히 사라지지 않게 한다.
   */
  issueCreateAllowed: boolean;
  /** `/api/health` 가 알려주는 힌트 — 이 출처에서 세션을 띄울 수 있는가. */
  spawnAllowed: boolean;
  /**
   * Cloudflare Access 로 들어온 화면이면 로그인한 이메일(`/api/health` 의 `accessUser`) — ⋯ 메뉴에 로그아웃을 그린다.
   * 로컬·테일넷 화면이거나 아직 모르면 null.
   */
  accessUser: string | null;
  /** 지금 도는 데몬의 버전(`/api/health`). 아직 모르거나 구버전 데몬이면 null. */
  daemonVersion: string | null;
  /**
   * 이 화면을 연 뒤 데몬 버전이 바뀌었는가. 화면의 번들은 데몬이 서빙한 것이라, 데몬이
   * 새 버전으로 재시작되면 열려 있던 화면은 옛 코드로 남는다 — 새로고침하라는 신호다.
   */
  daemonVersionChanged: boolean;
  /** 현재 보드의 아직 안 끝난 핸드오프(대기 중 + 배달됐지만 미완료) — refetch 가 함께 갱신한다. */
  handoffs: HandoffView[];
  /** 보내기 패널을 열 때만 채운다. */
  sessions: {
    available: boolean;
    reason?: string;
    list: SessionRow[];
  };
  /** 에이전트 탭을 보이나(이 브라우저의 선택). */
  showAgents: boolean;
  /** 에이전트 탭의 세션 목록(전 보드) — 아직 못 읽었으면 null. 보내기 패널의 `sessions` 와 따로 둔다. */
  agents: { available: boolean; reason?: string; list: SessionRow[] } | null;

  setSelected: (selection: BoardSelection) => void;
  setShowArchived: (show: boolean) => void;
  hideGithub: (key: string) => void;
  unhideAllGithub: () => void;
  setShowGithub: (show: boolean) => void;
  setShowRc: (show: boolean) => void;
  setShowAgents: (show: boolean) => void;
  /** 에이전트 탭의 세션 목록을 다시 읽는다. 데몬이 안 닿으면 직전 값을 둔다. */
  loadAgents: () => Promise<void>;
  /** rc 현황을 다시 읽는다(데몬이 5초 캐시). 꺼진 기기에서 원격 제어 탭을 보던 중이면 피드로 돌린다. */
  loadRc: () => Promise<void>;
  /** agy remote-control 을 켜거나 끈다(로컬 전용) — 성공하면 새로 잰 현황으로 바꾸고, 실패하면 사유를 던진다. */
  controlAgy: (action: 'start' | 'stop') => Promise<void>;
  /** 데몬에 띄우기 · 재시작을 맡긴다(로컬 화면만). 바로 돌아오고, 진행은 현황의 `action` 으로 본다. */
  rcCommand: (label: string, verb: 'start' | 'restart', fresh?: boolean) => Promise<void>;
  setActor: (actor: string) => void;
  /** 테마 선호를 저장하고 `<html data-theme>` 까지 갱신한다. */
  setThemePref: (pref: ThemePref) => void;
  setView: (view: BoardView) => void;
  setConnected: (connected: boolean) => void;

  /** 데몬에서 다시 받는다 — `slices` 를 주면 그 묶음만(SSE 이벤트가 건드린 것), 없으면 전부. */
  refetch: (slices?: ReadonlySet<Slice>) => Promise<void>;
  /**
   * @param options.push false 면 히스토리 항목을 만들지 않는다. `refetch` 가 열린 상세를
   *   갱신할 때와 `applyRoute` 가 URL 을 따라갈 때 반드시 false 여야 한다 — 아니면
   *   SSE 이벤트 하나마다 히스토리가 한 칸씩 쌓인다.
   * @param options.refresh true 면 **이미 그 항목이 열려 있을 때만** 반영한다. `refetch`
   *   전용 — 그쪽은 "열린 상세 갱신"이라 응답이 늦게 도착했는데 그새 드로어가 닫혔거나
   *   다른 항목으로 바뀌었다면 되살리지 말아야 한다.
   */
  openTodoDetail: (id: string, options?: { push?: boolean; refresh?: boolean }) => Promise<void>;
  openNoteDetail: (id: string, options?: { refresh?: boolean }) => Promise<void>;
  closeDetail: () => void;

  /** URL 이 지정한 화면으로 상태를 맞춘다 — 부팅과 popstate 가 쓴다. */
  applyRoute: (route: Route) => Promise<void>;

  /**
   * 보드 생성 후 그 보드로 전환한다.
   * @throws 서버가 거절한 이유를 그대로 던진다 — key 에 공백/`#` 이 있으면 참조로 쓸 수
   *   없어 400 이 온다. 호출자가 사용자에게 보여줘야 한다 (조용히 삼키면 안 된다).
   */
  createBoard: (key: string) => Promise<void>;
  addTodo: (input: { board: string; title: string; section?: string }) => Promise<void>;
  /** 같은 보드 안 순서 이동 — before 앞으로, null 이면 맨 끝. */
  moveTodo: (id: string, before: string | null) => Promise<void>;
  /** 다른 보드로 이동 — 번호는 대상 보드에서 새로 발급된다. */
  moveTodoToBoard: (id: string, board: string) => Promise<void>;
  patchTodo: (id: string, patch: Record<string, unknown>) => Promise<void>;
  setTodoStatus: (id: string, action: StatusAction) => Promise<void>;
  addNote: (input: { board?: string; title: string }) => Promise<void>;
  saveNote: (id: string, patch: { title?: string; content?: string }) => Promise<void>;
  archiveNote: (id: string) => Promise<void>;
  /** 고정/해제 — 고정한 노트는 노트 화면 맨 위에 카드로 펼쳐 둔다. */
  pinNote: (id: string, pinned: boolean) => Promise<void>;
  /** 노트 상세(전체 높이 편집기)를 연다 — 주소가 `/{board}/notes/{n}` 로 바뀌고 뒤로 가면 목록. */
  openNote: (id: string) => void;
  closeNote: () => void;
  addComment: (todoId: string, body: string) => Promise<void>;
  editComment: (id: string, body: string) => Promise<void>;
  archiveComment: (id: string) => Promise<void>;
  unarchiveComment: (id: string) => Promise<void>;
  /**
   * todo 를 GitHub 이슈로 만든다. `repo` 를 주면 서버가 그 값으로 시도하고, `gh` 가
   * 성공했을 때만 todo 의 보드에 영구 저장한다 — 실패한 슬러그가 보드에 눌어붙지
   * 않는다(finding C: 예전에는 `gh` 호출 전에 먼저 저장해, 오타 슬러그가 성공 여부와
   * 무관하게 남아 입력창이 다시 열리지 않는 막다른 길이었다).
   * @throws 서버가 거절한 이유를 그대로 던진다 — repo 를 모르거나(400), 이미 이슈가
   *   있거나(409), gh 가 실패한 경우다. 호출자가 사용자에게 보여줘야 한다.
   */
  createIssue: (todoId: string, repo?: string) => Promise<void>;
  /**
   * `/api/health` 로 이 출처의 능력과 데몬 버전을 확인한다 — 부팅 때와 SSE 가 다시 붙을
   * 때만 부른다(데몬 재시작은 SSE 재연결로 드러난다). 실패는 삼킨다: 힌트를 못 얻어도 화면은
   * 그대로 동작해야 하고, 강제는 서버가 한다.
   */
  loadCapabilities: () => Promise<void>;

  fetchSessions: () => Promise<void>;
  /** @throws 서버가 거절한 이유를 그대로 던진다 — 호출자가 화면에 보여줘야 한다. */
  /** 세션의 받은편지함에 바로 꽂아 깨웠으면 true — 아니면 그 세션이 다음 턴에 큐에서 집는다. */
  sendHandoff: (todoId: string, input: { sessionId?: string; note?: string }) => Promise<boolean>;
  cancelHandoff: (handoffId: string) => Promise<void>;
  /**
   * 그 todo 전용 워크트리에 백그라운드 세션을 띄운다. 이미 도는 세션이 있으면 서버가
   * spawn 대신 큐잉하고 `reused: true` 로 알린다.
   *
   * `path` 를 주면 서버가 **이번 spawn 에 한해** 그 값으로 시도하고, spawn(또는 재사용
   * 판정)이 성공했을 때만 보드에 영구 저장한다 — `createIssue` 의 `repo` 와 같은 모양
   * 이다(finding: 예전에는 호출 전에 `setBoardPath` 를 먼저 불러, 오타난 경로가 spawn
   * 성공 여부와 무관하게 보드에 눌어붙어 다른 todo·다른 탭까지 같은 실패를 물려받았다).
   * @throws 서버가 거절한 이유를 그대로 던진다 — 호출자가 화면에 보여줘야 한다.
   */
  spawnSession: (todoId: string, input: { note?: string; path?: string }) => Promise<SpawnResult>;
  /** 보드의 메인 레포 경로를 설정한다. @throws 서버 거절 사유 그대로. */
  setBoardPath: (boardKey: string, path: string) => Promise<void>;
  /**
   * 보드 메타(key·title·description·repo·path)를 한 번에 고친다 — 서버가 한
   * 트랜잭션으로 적용하므로 일부만 반영되는 상태가 없다.
   *
   * key 가 바뀌면 선택도 새 key 로 옮긴다 — 지금 보고 있는 주소(`/gotgan`)가 가리키는
   * 이름이 사라지기 때문이다. 옛 key 는 서버가 별칭으로 계속 받으므로 이미 복사해 둔
   * 링크가 죽지는 않는다.
   * @throws 서버가 거절한 이유를 그대로 던진다 — 호출자가 화면에 보여줘야 한다.
   */
  updateBoard: (
    boardKey: string,
    patch: {
      key?: string;
      title?: string;
      description?: string | null;
      repo?: string | null;
      prAuthors?: string[] | null;
    },
  ) => Promise<void>;
}

export async function api<T>(path: string, actor: string, init?: RequestInit): Promise<T> {
  const res = await fetch(path, {
    ...init,
    headers: {
      ...(init?.body ? { 'content-type': 'application/json' } : {}),
      'x-rocky-actor': actor,
      'x-rocky-client': 'web',
    },
  });
  if (!res.ok) {
    const body = (await res.json().catch(() => ({}))) as { error?: string };
    throw new Error(body.error ?? `${res.status} ${res.statusText}`);
  }
  return (await res.json()) as T;
}

/**
 * 주소가 이미 `path` 면 아무것도 하지 않고, 아니면 히스토리 항목을 **더하지 않고** 갈아끼운다.
 *
 * 항상 부르는 대신 비교를 먼저 하는 이유: `replaceState` 는 히스토리 길이를 늘리지 않지만
 * 현재 항목의 state 를 덮어쓴다. 상세 드로어 마커(`rockyTodoDetail`)가 그 state 에 있어서,
 * 불필요한 호출이 마커를 지우면 `closeDetail` 이 뒤로가기 대신 잘못된 분기를 고른다.
 */
function currentPath(): string {
  return `${window.location.pathname}${window.location.search}`;
}

function replacePath(path: string, state: unknown = null): void {
  if (currentPath() !== path) {
    window.history.replaceState(state, '', path);
  }
}

/**
 * 주소가 실제로 바뀔 때만 히스토리 항목을 만든다.
 *
 * `buildPath` 가 `/` 로 접는 보드(예약어 `api`/`mcp`, 점 세그먼트)에서는 이미 `/` 에 있는
 * 채로 push 하면 **되읽을 수 없는 항목**이 쌓인다 — 뒤로가기가 그 항목으로 돌아가면
 * popstate 가 `/` 를 전체 보기로 읽어 보드 선택이 엉뚱하게 풀린다.
 *
 * @returns 항목을 만들었으면 true.
 */
function pushPath(path: string, state: unknown = null): boolean {
  if (currentPath() === path) {
    return false;
  }
  window.history.pushState(state, '', path);
  return true;
}

export const useUiStore = create<UiState>((set, get) => ({
  boards: [],
  todos: [],
  nowTodos: [],
  nowHandoffs: [],
  prs: [],
  collect: null,
  collectItems: [],
  sections: [],
  notes: [],
  // 첫 fetch 부터 올바른 보드를 조회하도록 URL 을 먼저 읽는다. 없는 보드였다면
  // 부팅 직후의 applyRoute 가 전체 보기로 되돌린다.
  selected: parseRoute(window.location.pathname, window.location.search).board,
  showArchived: false,
  githubHidden: readHidden(),
  showGithub: readStored(GITHUB_TAB_KEY) !== 'off',
  showRc: readStored(RC_TAB_KEY) !== 'off',
  rc: null,
  actor: localStorage.getItem(ACTOR_KEY) ?? 'logan',
  themePref: readThemePref(
    (() => {
      try {
        return localStorage.getItem(THEME_KEY);
      } catch {
        return null; // 저장소 차단은 저장값 없음(auto)으로 다룬다 — index.html 인라인 스크립트와 같은 규칙
      }
    })(),
  ),
  // 첫 화면은 피드다(2026-10-02 오너) — 맨 주소면 피드, 탭을 실은 주소(`?view=todos`)면 그 탭.
  view: viewOf(parseRoute(window.location.pathname, window.location.search)),
  notesSeenAt: readStored(NOTES_SEEN_KEY) ?? new Date(0).toISOString(),
  openNoteId: null,
  connected: false,
  detail: null,
  seenComments: readSeen(localStorage),
  issueCreateAllowed: true,
  spawnAllowed: true,
  accessUser: null,
  daemonVersion: null,
  daemonVersionChanged: false,
  handoffs: [],
  sessions: { available: true, list: [] },
  showAgents: readStored(AGENTS_TAB_KEY) !== 'off',
  agents: null,

  setSelected: (selected) => {
    logUsage('web:board-tab');
    // 같은 보드를 다시 고른 클릭도 refetch 는 그대로 수행한다(새로고침 용도로 쓰인다) —
    // 다만 선택이 실제로 바뀌지 않았으면 pushState 는 건너뛴다. 아니면 전체/같은 보드를
    // 다섯 번 눌렀을 때 동일한 히스토리 항목이 다섯 개 쌓여 뒤로가기를 다섯 번 눌러야
    // 벗어나게 된다.
    if (selected !== get().selected) {
      // 보드를 바꾸면 열린 상세도 닫는다 — 주소는 새 보드를 가리키는데 드로어가 이전
      // 보드의 todo 를 계속 띄우면, 같은 주소를 새로고침한 화면과 달라진다.
      set({ selected, detail: null, openNoteId: null });
      pushPath(pathFor({ board: selected }));
    }
    void get().refetch();
  },
  hideGithub: (key) => {
    const next = [...new Set([...get().githubHidden, key])];
    // 끝없이 쌓이지 않게 최근 200개만 — 오래된 PR·이슈는 이미 닫혔다.
    const trimmed = next.slice(-200);
    writeStored(GITHUB_HIDDEN_KEY, JSON.stringify(trimmed));
    set({ githubHidden: trimmed });
  },
  unhideAllGithub: () => {
    writeStored(GITHUB_HIDDEN_KEY, '[]');
    set({ githubHidden: [] });
  },
  setShowGithub: (showGithub) => {
    writeStored(GITHUB_TAB_KEY, showGithub ? 'on' : 'off');
    set(
      showGithub
        ? { showGithub }
        : { showGithub, view: get().view === 'github' ? 'feed' : get().view },
    );
    if (!showGithub) {
      const { view: _was, ...here } = parseRoute(window.location.pathname, window.location.search);
      replacePath(pathFor(here));
    }
  },
  setShowRc: (showRc) => {
    writeStored(RC_TAB_KEY, showRc ? 'on' : 'off');
    set(showRc ? { showRc } : { showRc, view: get().view === 'rc' ? 'feed' : get().view });
    if (!showRc) {
      const { view: _was, ...here } = parseRoute(window.location.pathname, window.location.search);
      replacePath(pathFor(here));
    }
  },
  setShowAgents: (showAgents) => {
    writeStored(AGENTS_TAB_KEY, showAgents ? 'on' : 'off');
    set(
      showAgents
        ? { showAgents }
        : { showAgents, view: get().view === 'agents' ? 'feed' : get().view },
    );
    if (!showAgents) {
      const { view: _was, ...here } = parseRoute(window.location.pathname, window.location.search);
      replacePath(pathFor(here));
    }
  },
  loadAgents: async () => {
    try {
      const result = await api<{ available: boolean; reason?: string; sessions: SessionRow[] }>(
        '/api/sessions',
        get().actor,
      );
      set({
        agents: { available: result.available, reason: result.reason, list: result.sessions },
      });
    } catch {
      // 데몬이 잠깐 안 닿으면 직전 값을 둔다 — 끊김은 머리줄이 따로 말한다.
    }
  },
  loadRc: async () => {
    try {
      const rc = await api<RcStatus>('/api/rc/servers', get().actor);
      if (!rcVisible(rc) && get().view === 'rc') {
        // 주소도 피드로 — `?view=rc` 가 남으면 새로고침·뒤로가기마다 rc 를 골랐다 튕긴다.
        set({ rc, view: 'feed' });
        const { view: _was, ...here } = parseRoute(
          window.location.pathname,
          window.location.search,
        );
        replacePath(pathFor(here));
        return;
      }
      set({ rc });
    } catch {
      // 데몬이 잠깐 안 닿으면 직전 값을 둔다 — 화면이 깜박이지 않게.
    }
  },
  controlAgy: async (action) => {
    const rc = await api<RcStatus>(`/api/rc/antigravity/${action}`, get().actor, {
      method: 'POST',
      body: '{}',
    });
    set({ rc });
  },
  rcCommand: async (label, verb, fresh) => {
    logUsage(verb === 'start' ? 'web:rc-start' : 'web:rc-restart');
    await api(`/api/rc/servers/${encodeURIComponent(label)}/${verb}`, get().actor, {
      method: 'POST',
      body: JSON.stringify(verb === 'restart' ? { fresh: Boolean(fresh) } : {}),
    });
    await get().loadRc();
  },
  setShowArchived: (showArchived) => {
    logUsage('web:archived-toggle');
    set({ showArchived });
    void get().refetch();
  },
  setActor: (actor) => {
    setUsageActor(actor);
    localStorage.setItem(ACTOR_KEY, actor);
    set({ actor });
  },
  setThemePref: (pref) => {
    // 화면 갱신을 먼저 한다. 저장은 다음 방문을 위한 부수 효과일 뿐이라, 그게 실패해도
    // 이번 클릭은 반드시 반영돼야 한다 — 순서가 반대면 저장이 막힌 브라우저에서 토글이
    // 통째로 죽고 auto 의 OS 추종까지 멈춘다.
    // 해석은 여기서 한 번만 한다 — 이 값을 상태로 복제하지 않고 DOM 에만 반영한다.
    const prefersLight = window.matchMedia('(prefers-color-scheme: light)').matches;
    document.documentElement.dataset.theme = resolveTheme(pref, prefersLight);
    set({ themePref: pref });
    try {
      localStorage.setItem(THEME_KEY, pref);
    } catch {
      // 저장 실패는 다음 방문에 auto 로 돌아간다는 뜻일 뿐 — 이번 세션은 정상 동작한다.
    }
  },
  setView: (view) => {
    if (get().openNoteId !== null) {
      // 노트 상세를 연 채 탭을 눌렀다 — 상세부터 닫는다(우리가 쌓은 항목이면 뒤로). "노트" 탭을
      // 다시 누른 것이면 목록으로 돌아가는 것으로 끝이다.
      get().closeNote();
    }
    if (view === get().view) {
      return;
    }
    logUsage('web:view', { view });
    // 노트 보기에 들어갈 때도, 떠날 때도 "여기까지 봤다" 로 친다 — 보는 동안의 편집은 새 소식이 아니다.
    // 기준은 브라우저 시계가 아니라 **서버가 찍은 노트 시각**이다(원격 브라우저의 시계 어긋남). 떠나는
    // 순간엔 아직 저장 중인 편집(제목 PATCH·편집기의 배치 flush)이 남아 있을 수 있어, 잠시 동안의
    // refetch 도 "본 것" 으로 올린다(`refetch` 의 `notesSettleUntil`).
    notesSettleUntil = view !== 'notes' ? Date.now() + NOTES_SETTLE_MS : 0;
    set({ view, notesSeenAt: advanceSeen(get().notesSeenAt, get().notes) });
    writeStored(NOTES_SEEN_KEY, get().notesSeenAt);
    // 탭은 주소에 산다 — 히스토리 항목으로 쌓아 뒤로가기가 앞 탭으로 돌아가게. 보드·열린 상세는 그대로 둔다.
    const { view: _was, ...here } = parseRoute(window.location.pathname, window.location.search);
    pushPath(pathFor(here));
  },
  setConnected: (connected) => set({ connected }),

  refetch: async (slices) => {
    const { selected, showArchived, actor, detail } = get();
    const want = (slice: Slice) => !slices || slices.has(slice);
    const params = new URLSearchParams();
    if (selected !== 'all') {
      params.set('board', selected);
    }
    if (showArchived) {
      params.set('includeArchived', 'true');
    }
    const qs = params.size > 0 ? `?${params.toString()}` : '';
    // 전체 보기면 보드의 할 일·핸드오프와 "지금" 의 재료(전 보드)가 같은 쿼리다 — 한 번만 받는다. 할 일 응답은 진행
    // 중인 것이 있으면 데몬이 세션 목록까지 조회해서(수백 ms) 두 번 부르면 그만큼 두 번 기다린다.
    const sameTodos = qs === '';
    const sameHandoffs = selected === 'all';
    const handoffsUrl = `/api/handoffs?open=true${
      selected === 'all' ? '' : `&board=${encodeURIComponent(selected)}`
    }`;
    const skip = Promise.resolve(undefined);

    const [boards, todos, notes, sections, handoffs, nowTodos, nowHandoffs, summary, prs] =
      await Promise.all([
        want('boards') ? api<Board[]>('/api/boards', actor) : skip,
        want('todos') ? api<TodoView[]>(`/api/todos${qs}`, actor) : skip,
        want('notes') ? api<NoteView[]>(`/api/notes${qs}`, actor) : skip,
        !want('sections')
          ? skip
          : selected === 'all'
            ? Promise.resolve([] as Section[])
            : api<Section[]>(`/api/sections?board=${encodeURIComponent(selected)}`, actor),
        // `open=true` — 대기 중인 것에 더해 **배달됐는데 아직 안 끝난** 것까지 받는다.
        // 후자가 없으면 "집어가 놓고 아무것도 안 한다"가 화면에 나타날 길이 없다.
        want('handoffs') ? api<HandoffView[]>(handoffsUrl, actor) : skip,
        // "지금"·피드의 재료 — 보고 있는 보드와 무관하게 전 보드.
        want('todos') && !sameTodos ? api<TodoView[]>('/api/todos', actor) : skip,
        want('handoffs') && !sameHandoffs
          ? api<HandoffView[]>('/api/handoffs?open=true', actor)
          : skip,
        // 수집함 미올림 수 — cached 라 어댑터를 새로 돌리지 않는다. 실패는 "모름".
        want('summary')
          ? api<{ collect?: number; collectItems?: CollectItem[] }>(
              '/api/summary?cached=true',
              actor,
            ).catch((): { collect?: number; collectItems?: CollectItem[] } => ({}))
          : skip,
        // PR 감시 스냅숏 — 없거나 실패하면 빈 목록("모름" 이 아니라 "없음" 으로 보여도 무해).
        want('prs')
          ? api<PrSnapshot[]>('/api/prs?open=true', actor).catch((): PrSnapshot[] => [])
          : skip,
      ]);
    set({
      ...(boards ? { boards } : {}),
      ...(todos ? { todos, nowTodos: nowTodos ?? todos } : {}),
      ...(notes ? { notes } : {}),
      ...(sections ? { sections } : {}),
      ...(handoffs ? { handoffs, nowHandoffs: nowHandoffs ?? handoffs } : {}),
      ...(prs ? { prs } : {}),
      ...(summary
        ? {
            collect: typeof summary.collect === 'number' ? summary.collect : null,
            collectItems: summary.collectItems ?? [],
          }
        : {}),
    });
    if (!notes) {
      // 노트를 안 받았으면 "본 것" 을 올릴 것도, 상세를 다시 받을 것도 노트 쪽엔 없다.
      if (todos && detail?.kind === 'todo' && detail.todo) {
        void get().openTodoDetail(detail.todo.id, { push: false, refresh: true });
      }
      return;
    }
    // 노트를 보는 중이거나 막 떠난 참이면, 방금 받은 노트까지 "본 것" — 내 편집이 새 소식이 되지 않게.
    if (get().view === 'notes' || Date.now() < notesSettleUntil) {
      const seen = advanceSeen(get().notesSeenAt, notes);
      if (seen !== get().notesSeenAt) {
        set({ notesSeenAt: seen });
        writeStored(NOTES_SEEN_KEY, seen);
      }
    }

    // 열린 상세가 있으면 함께 갱신 (SSE 로 들어온 변경 반영). await 하지 않으므로
    // `refresh: true` 로 "그 항목이 아직 열려 있을 때만" 반영하게 한다 — 그 사이 라우팅이
    // 드로어를 닫았다면(뒤로가기 등) 늦게 도착한 이 응답이 되살려선 안 된다.
    if (todos && detail?.kind === 'todo' && detail.todo) {
      void get().openTodoDetail(detail.todo.id, { push: false, refresh: true });
    } else if (detail?.kind === 'note' && detail.note) {
      void get().openNoteDetail(detail.note.id, { refresh: true });
    }
  },

  openTodoDetail: async (id, options) => {
    const { actor, showArchived } = get();
    // 전역 "보관 항목 보기" 토글을 댓글에도 그대로 연결한다 — 별도 스위치를 만들지
    // 않고 이미 있는 컨트롤 하나로 todo/note/comment 아카이브 뷰를 통일한다.
    const qs = showArchived ? '?includeArchived=true' : '';
    const body = await api<{ todo: TodoView; history: HistoryEntry[]; comments: Comment[] }>(
      `/api/todos/${id}${qs}`,
      actor,
    );
    if (options?.refresh && get().detail?.todo?.id !== id) {
      // 갱신하려던 상세가 await 도중 닫혔거나 다른 항목으로 바뀌었다 — 늦게 온 응답을 버린다.
      return;
    }
    set({
      detail: { kind: 'todo', todo: body.todo, history: body.history, comments: body.comments },
    });
    // 드로어를 연 시점에 이 todo 의 댓글은 모두 확인한 것으로 본다. localStorage(세션 간
    // 유지)와 상태 사본(리렌더 트리거)을 함께 갱신한다. push 여부와 무관하게 수행한다 —
    // URL 로 연 경우(applyRoute)도, SSE refetch 로 갱신된 경우도 사용자는 그 댓글을 보고 있다.
    if (body.todo.lastCommentAt) {
      markSeen(localStorage, body.todo.id, body.todo.lastCommentAt);
      set({ seenComments: readSeen(localStorage) });
    }
    if (options?.push === false) {
      return;
    }
    // boards 는 await 이후 다시 읽는다 — await 도중 SSE 로 새 보드가 들어와 배열이 바뀔 수
    // 있고, 낡은 배열을 쓰면 todoRefFor 가 boardId 를 못 찾아 상세 없는 주소가 된다.
    const selected = get().selected;
    const ref = todoRefFor(body.todo, get().boards);
    const selectedAddressable = selected === 'all' || isAddressableBoardKey(selected);
    if (ref === undefined || !isAddressableBoardKey(ref.board) || !selectedAddressable) {
      // 이 상세를 가리킬 주소가 없다(보드를 못 찾았거나 `buildPath` 가 `/` 로 접는 키).
      // 그래도 마커만 쌓으면 closeDetail 이 back() 을 골라, popstate 가 `/` 를 전체 보기로
      // 읽어 **닫기가 보드 전환을 일으킨다**. 항목을 만들지 않으면 closeDetail 의
      // replaceState 분기가 지금 보드를 그대로 들고 드로어만 닫는다.
      return;
    }
    // 보고 있는 보드는 그대로다 — 전체 보기(또는 "지금" 표에서 다른 보드의 항목)에서 연
    // 상세는 `/?todo=rocky-12` 처럼 선택과 todo 를 따로 싣는다. 예전엔 여기서 selected 를
    // todo 의 보드로 옮겼고, 그게 "상세를 열면 뒤 화면이 그 보드로 바뀐다" 로 보였다.
    // 상세를 연 것이 히스토리 항목을 만든다 — closeDetail 이 이 표식을 보고 back() 할지
    // 정한다(퍼머링크로 바로 진입한 경우엔 back() 이 앱 밖으로 나가버린다).
    pushPath(pathFor({ board: selected, todo: ref }), { rockyTodoDetail: true });
  },

  openNoteDetail: async (id, options) => {
    const { actor } = get();
    const body = await api<{ note: NoteView; history: HistoryEntry[] }>(`/api/notes/${id}`, actor);
    if (options?.refresh && get().detail?.note?.id !== id) {
      // openTodoDetail 과 같은 규칙 — 늦게 도착한 갱신이 닫힌 드로어를 되살리지 않는다.
      return;
    }
    set({ detail: { kind: 'note', note: body.note, history: body.history, comments: [] } });
  },

  closeDetail: () => {
    if (get().detail?.kind === 'note') {
      // 노트 히스토리 드로어는 주소를 만들지 않고 열린다 — 닫을 때도 주소를 건드리지 않는다(노트
      // 상세 `/{board}/notes/{n}` 위에서 열렸으면 그 주소와 뒤로가기 항목을 그대로 둔다).
      set({ detail: null });
      return;
    }
    const state = window.history.state as { rockyTodoDetail?: boolean } | null;
    if (state?.rockyTodoDetail) {
      // 우리가 만든 항목이니 뒤로가기로 되돌린다. popstate 의 applyRoute 도 어차피 닫지만,
      // 닫힘은 여기서 먼저 확정한다 — 사용자가 누른 것은 "닫기"이지 "뒤로"가 아니라서,
      // popstate 가 늦거나(back() 은 비동기다) 어떤 이유로 처리되지 않아도 드로어가 열린 채
      // 주소만 바뀌는 상태로 남으면 안 된다. 되돌아갈 항목은 늘 상세가 없는 보드 경로다
      // (드로어가 열려 있는 동안에는 백드롭이 목록 클릭을 막아 상세→상세 전환이 없다).
      set({ detail: null });
      window.history.back();
      return;
    }
    // 퍼머링크로 바로 들어온 경우: 되돌릴 항목이 없다. back() 하면 앱 밖으로 나간다.
    set({ detail: null });
    window.history.replaceState(null, '', pathFor({ board: get().selected }));
  },

  createBoard: async (key) => {
    const { actor } = get();
    const board = await api<Board>('/api/boards', actor, {
      method: 'POST',
      body: JSON.stringify({ key }),
    });
    // selected 를 먼저 바꾼 뒤 조회한다 — 순서가 반대면 refetch 가 이전 보드 기준으로
    // 돌아, 새 보드 화면에 직전 보드의 항목·섹션이 그대로 남는다.
    set({ selected: board.key, detail: null, openNoteId: null });
    pushPath(pathFor({ board: board.key }));
    await get().refetch();
  },

  applyRoute: async (route) => {
    // 옛 key 도 그 보드로 읽는다 — 이름을 바꾸기 전에 복사해 둔 링크(`/gotgan/12`)가 죽으면
    // "옛 참조는 계속 풀린다"는 약속을 웹 UI 만 안 지키는 셈이 된다. REST·MCP·CLI 는
    // 서버가 별칭을 풀어주지만 이 판정은 클라이언트에 있어 여기서 따로 봐야 한다.
    // 푼 뒤에는 **새 key** 로 정규화한다(별칭은 입력 전용).
    // 탭부터 — 아래의 주소 정규화(pathFor)가 지금 탭을 싣는다. 뒤로가기로 맨 주소에 오면 피드다.
    if (viewOf(route) !== get().view) {
      set({ view: viewOf(route) });
    }
    const matched = route.board === 'all' ? undefined : resolveBoardKey(get().boards, route.board);
    const known = route.board === 'all' || matched !== undefined;
    const board: BoardSelection = matched ?? 'all';
    if (!known) {
      // 낡은 링크에 에러 화면을 띄우지 않는다. 히스토리에 죽은 항목을 남기지 않으려
      // push 가 아니라 replace 를 쓴다. 아래 정규화가 어차피 같은 일을 하지만, 그 전의
      // refetch 가 실패해도 죽은 주소는 남지 않도록 여기서 먼저 걷어낸다.
      replacePath(pathFor({ board: 'all' }));
    }
    if (board !== get().selected) {
      set({ selected: board });
      await get().refetch();
    }
    if (route.note !== undefined) {
      // 노트 상세 퍼머링크 — 목록은 이미 읽었으니 ref(또는 id)로 찾는다. 없거나 보관된 노트면
      // 노트 목록만 열어 준다. 옛 key 로 온 `/old/notes/3` 은 새 key 의 ref 로 고쳐 찾는다.
      const oldPrefix = `${route.board}-`;
      const wanted =
        route.board !== board && route.note.startsWith(oldPrefix)
          ? `${board}-${route.note.slice(oldPrefix.length)}`
          : route.note;
      const note = get().notes.find((n) => n.ref === wanted || n.id === wanted);
      set({ detail: null, view: 'notes', openNoteId: note?.id ?? null });
      replacePath(
        pathFor(note ? { board, note: note.ref } : { board }),
        note ? window.history.state : null,
      );
      return;
    }
    set({ openNoteId: null });
    if (route.todo === undefined) {
      set({ detail: null });
      // `/demo/abc` 처럼 해석되지 않은 꼬리가 주소에 남지 않게 정규화한다.
      // push 가 아니라 replace 인 이유: 히스토리에 죽은 항목을 남기지 않는다.
      replacePath(pathFor({ board }));
      return;
    }
    // 상세의 보드도 별칭을 푼다. 목록은 선택한 보드 것만 있으므로(전체 보기가 아니면),
    // 다른 보드의 todo 는 "지금" 표의 재료(전 보드)에서 찾는다.
    const todoBoard = resolveBoardKey(get().boards, route.todo.board);
    const id =
      todoBoard === undefined
        ? undefined
        : (findTodoIdByNumber(get().todos, get().boards, todoBoard, route.todo.number) ??
          findTodoIdByNumber(get().nowTodos, get().boards, todoBoard, route.todo.number));
    if (id === undefined || todoBoard === undefined) {
      // 없거나 보관된 번호 — 보드만 열어 준다.
      set({ detail: null });
      replacePath(pathFor({ board }));
      return;
    }
    await get().openTodoDetail(id, { push: false });
    // 번호가 해석된 경로도 똑같이 정규화한다 — `/demo/12/extra` 의 꼬리 세그먼트를
    // parseRoute 는 무시하지만 주소에는 남아, 같은 화면이 여러 주소를 갖게 되고 복사해
    // 건넨 링크에 죽은 꼬리가 따라간다. 여기서는 현재 항목의 state(상세 마커)를 보존해야
    // 한다 — 지우면 closeDetail 이 back() 대신 replace 분기를 골라 뒤로가기가 어긋난다.
    replacePath(
      pathFor({ board, todo: { board: todoBoard, number: route.todo.number } }),
      window.history.state,
    );
  },

  addTodo: async (input) => {
    logUsage('web:quick-add');
    const { actor } = get();
    await api('/api/todos', actor, { method: 'POST', body: JSON.stringify(input) });
    await get().refetch();
  },

  moveTodoToBoard: async (id, board) => {
    const { actor } = get();
    const moved = await api<{ boardId: string; number: number }>(`/api/todos/${id}/board`, actor, {
      method: 'POST',
      body: JSON.stringify({ board }),
    });
    // 옛 주소(`/old/12`)는 비워진 번호라 새로고침·공유에서 깨진다. 드로어는 refetch 가
    // 같은 id 로 되살리므로 주소가 todo 를 따라간다(`/old?todo=new-3`) — 보고 있는 보드는
    // 그대로다. 히스토리 항목은 늘리지 않고 갈아끼운다(뒤로가기가 깨진 옛 주소로 돌아가지
    // 않게, state 는 보존해 드로어 마커를 유지).
    const route = routeForTodo(moved, get().boards, get().selected);
    if (route.todo !== undefined && isAddressableBoardKey(route.todo.board)) {
      replacePath(pathFor(route), window.history.state);
    }
    await get().refetch();
  },

  moveTodo: async (id, before) => {
    const { actor } = get();
    await api(`/api/todos/${id}/move`, actor, {
      method: 'POST',
      body: JSON.stringify({ before }),
    });
    await get().refetch();
  },

  patchTodo: async (id, patch) => {
    const { actor } = get();
    await api(`/api/todos/${id}`, actor, { method: 'PATCH', body: JSON.stringify(patch) });
    await get().refetch();
  },

  setTodoStatus: async (id, action) => {
    const { actor } = get();
    await api(`/api/todos/${id}/status`, actor, {
      method: 'POST',
      body: JSON.stringify({ action }),
    });
    await get().refetch();
  },

  addNote: async (input) => {
    const { actor } = get();
    const created = await api<NoteView>('/api/notes', actor, {
      method: 'POST',
      body: JSON.stringify(input),
    });
    await get().refetch();
    // 목록 한 줄로 두면 "새 메모" 를 다시 찾아 눌러야 한다 — 만든 노트를 바로 연다.
    get().openNote(created.id);
  },

  saveNote: async (id, patch) => {
    const { actor } = get();
    await api(`/api/notes/${id}`, actor, { method: 'PATCH', body: JSON.stringify(patch) });
    await get().refetch();
  },

  archiveNote: async (id) => {
    const { actor } = get();
    await api(`/api/notes/${id}/archive`, actor, { method: 'POST' });
    set({ detail: null });
    if (get().openNoteId === id) {
      get().closeNote();
    }
    await get().refetch();
  },

  pinNote: async (id, pinned) => {
    const { actor } = get();
    logUsage('web:note-pin', { action: pinned ? 'pin' : 'unpin' });
    await api(`/api/notes/${id}/${pinned ? 'pin' : 'unpin'}`, actor, { method: 'POST' });
    await get().refetch();
  },

  openNote: (id) => {
    const note = get().notes.find((n) => n.id === id);
    if (!note) {
      return;
    }
    logUsage('web:note-open');
    set({ openNoteId: id, view: 'notes' });
    pushPath(pathFor({ board: get().selected, note: note.ref }), { rockyNote: true });
  },

  closeNote: () => {
    const state = window.history.state as { rockyNote?: boolean } | null;
    set({ openNoteId: null });
    if (state?.rockyNote) {
      // 우리가 쌓은 항목이니 뒤로 — closeDetail 과 같은 규칙(닫힘은 먼저 확정한다).
      window.history.back();
      return;
    }
    // 퍼머링크로 바로 들어왔다 — back() 하면 앱 밖으로 나간다.
    replacePath(pathFor({ board: get().selected }));
  },

  addComment: async (todoId, body) => {
    const { actor } = get();
    await api(`/api/todos/${todoId}/comments`, actor, {
      method: 'POST',
      body: JSON.stringify({ body }),
    });
    await get().refetch();
  },

  editComment: async (id, body) => {
    const { actor } = get();
    await api(`/api/comments/${id}`, actor, { method: 'PATCH', body: JSON.stringify({ body }) });
    await get().refetch();
  },

  archiveComment: async (id) => {
    const { actor } = get();
    await api(`/api/comments/${id}/archive`, actor, { method: 'POST' });
    await get().refetch();
  },

  unarchiveComment: async (id) => {
    const { actor } = get();
    await api(`/api/comments/${id}/unarchive`, actor, { method: 'POST' });
    await get().refetch();
  },

  createIssue: async (todoId, repo) => {
    const { actor } = get();
    await api(`/api/todos/${todoId}/issue`, actor, {
      method: 'POST',
      ...(repo !== undefined ? { body: JSON.stringify({ repo }) } : {}),
    });
    await get().refetch();
  },

  /**
   * 실패를 **던지지 않고** `available:false + reason` 으로 흡수한다 — 화면이 실패를
   * 표현하는 경로를 하나로 묶기 위해서다(패널의 `sessions.available` 분기).
   * 조회 전에 목록을 비우는 것도 같은 이유: 그러지 않으면 서버가 죽었는데 직전 성공의
   * 세션 목록이 그대로 남아, 이제는 존재하지 않을 수도 있는 대상을 고르게 된다.
   */
  fetchSessions: async () => {
    const { actor, selected } = get();
    // `selected` 는 'all' 이거나 board key 문자열이다 (객체가 아니다).
    const board = selected === 'all' ? '' : selected;
    set({ sessions: { available: true, list: [] } });
    try {
      const result = await api<{
        available: boolean;
        reason?: string;
        sessions: SessionRow[];
      }>(`/api/sessions?board=${encodeURIComponent(board)}`, actor);
      set({
        sessions: { available: result.available, reason: result.reason, list: result.sessions },
      });
    } catch (error) {
      set({
        sessions: {
          available: false,
          reason: error instanceof Error ? error.message : String(error),
          list: [],
        },
      });
    }
  },

  sendHandoff: async (todoId, input) => {
    const { actor } = get();
    const res = await api<{ woke?: boolean }>(`/api/todos/${todoId}/handoff`, actor, {
      method: 'POST',
      body: JSON.stringify(input),
    });
    await get().refetch();
    return res?.woke === true;
  },

  loadCapabilities: async () => {
    try {
      const health = await api<{
        issueCreateAllowed?: boolean;
        spawnAllowed?: boolean;
        accessUser?: string | null;
        version?: string;
      }>('/api/health', get().actor);
      const previous = get().daemonVersion;
      const version = health.version ?? null;
      // 필드가 없는 구버전 데몬이면 낙관적으로 둔다 — 그 데몬에는 애초에 이 가드가 없다.
      set({
        issueCreateAllowed: health.issueCreateAllowed ?? true,
        spawnAllowed: health.spawnAllowed ?? true,
        accessUser: health.accessUser ?? null,
        daemonVersion: version,
        // 한 번 바뀌었으면 새로고침 전까지 켜 둔다 — 되돌아가도 번들은 이미 첫 버전의 것이다.
        daemonVersionChanged:
          get().daemonVersionChanged ||
          (previous !== null && version !== null && previous !== version),
      });
    } catch {
      // 힌트를 못 얻는 것으로 화면이 망가지면 안 된다. 강제는 서버 몫이다.
    }
  },

  cancelHandoff: async (handoffId) => {
    const { actor } = get();
    await api(`/api/handoffs/${handoffId}/cancel`, actor, { method: 'POST' });
    await get().refetch();
  },

  spawnSession: async (todoId, input) => {
    const { actor } = get();
    const result = await api<SpawnResult>(`/api/todos/${todoId}/spawn`, actor, {
      method: 'POST',
      body: JSON.stringify({
        ...(input.note ? { note: input.note } : {}),
        ...(input.path !== undefined ? { path: input.path } : {}),
      }),
    });
    await get().refetch();
    return result;
  },

  setBoardPath: async (boardKey, path) => {
    const { actor } = get();
    await api(`/api/boards/${encodeURIComponent(boardKey)}`, actor, {
      method: 'PATCH',
      body: JSON.stringify({ path }),
    });
    await get().refetch();
  },

  updateBoard: async (boardKey, patch) => {
    const { actor } = get();
    const board = await api<Board>(`/api/boards/${encodeURIComponent(boardKey)}`, actor, {
      method: 'PATCH',
      body: JSON.stringify(patch),
    });
    // 선택을 먼저 옮기고 조회한다 — 순서가 반대면 refetch 가 옛 key 로 돌아, 화면은 새
    // 이름인데 목록은 (별칭으로 풀리긴 해도) 옛 주소 기준으로 남는다. `createBoard` 와 같은 순서.
    if (get().selected === boardKey && board.key !== boardKey) {
      set({ selected: board.key });
      // 열린 상세가 있으면 그 번호를 유지한다 — 보드 이름만 바뀌었을 뿐 보고 있는 항목은
      // 그대로다. 히스토리 항목을 새로 만들지 않으려 replace 이고, 상세 마커(state)도 보존한다.
      const { todo } = parseRoute(window.location.pathname, window.location.search);
      const renamed =
        todo !== undefined && todo.board === boardKey ? { ...todo, board: board.key } : todo;
      replacePath(pathFor({ board: board.key, todo: renamed }), window.history.state);
    }
    await get().refetch();
  },
}));
