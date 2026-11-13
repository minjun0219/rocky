import { create } from 'zustand';
import type { AppView, PrSnapshot, RcStatus, SessionRow } from './types';
import { rcVisible, readThemePref, resolveTheme, THEME_KEY, type ThemePref } from './lib';
import { logUsage, setUsageActor } from './usage';
import { parseView, pathFor } from './route';

/**
 * 웹 UI 상태 — zustand 단일 스토어.
 *
 * 서버(데몬)가 정본이다. 웹이 데몬에서 읽는 것은 PR 감시 스냅숏, 세션 목록(에이전트 탭), rc 현황과 `/api/health` 의
 * 힌트뿐이다 — 보드 · 노트 화면과 보드 싱크는 2026-11-13 걷어냈다(`docs/design/specs/2026-11-11-board-and-rc-removal-design.md`).
 * actor 는 localStorage 에 저장되고 모든 요청의 `x-rocky-actor` 헤더로 나간다.
 */

const ACTOR_KEY = 'rocky-actor';
/** 숨긴 GitHub 항목(PR) 키 — 이 브라우저에만. `githubHideKey` 가 만든다. */
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

/** 저장소가 막혀도(비공개 창·차단) 화면은 떠야 한다 — 읽기 실패는 저장값 없음으로. */
function readStored(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function writeStored(key: string, value: string): void {
  try {
    localStorage.setItem(key, value);
  } catch {
    // 다음 방문에 기본값으로 돌아갈 뿐 — 이번 화면은 정상 동작한다.
  }
}

/**
 * 주소가 가리키는 탭 — `?view=` 가 있으면 그 탭, 없으면 피드(첫 화면). 보던 탭을 주소에 실어 새로고침·앱 전환(폰 Safari 가
 * 페이지를 다시 부른다)·뒤로가기·탭 복제가 모두 같은 탭으로 돌아온다(2026-10-03 오너: "path 로 해야지"). 꺼 둔 탭이면 피드.
 */
function viewOf(search: string): AppView {
  const view = parseView(search);
  if (view === 'github' && readStored(GITHUB_TAB_KEY) === 'off') {
    return 'feed';
  }
  if (view === 'agents' && readStored(AGENTS_TAB_KEY) === 'off') {
    return 'feed';
  }
  return view === 'rc' && readStored(RC_TAB_KEY) === 'off' ? 'feed' : view;
}

interface UiState {
  /** 데몬 PR 감시의 열린 PR. */
  prs: PrSnapshot[];
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
  view: AppView;
  connected: boolean;
  /** `/api/health` 가 알려주는 힌트 — 이 출처에서 세션을 조종(멈추기·메시지)할 수 있는가. 강제는 서버 몫이다. */
  spawnAllowed: boolean;
  /**
   * `/api/health` 의 `rcControlAllowed` — 이 화면에서 원격 제어 탭의 서버 띄우기·재시작·닫기를 할 수 있는가. 로컬이거나
   * Cloudflare Access 로 들어온 허용 이메일(`rocky.json` 의 `access.remoteControl`)이면 true. 구버전 데몬이면 `spawnAllowed` 와 같다.
   */
  rcControlAllowed: boolean;
  /**
   * Cloudflare Access 로 들어온 화면이면 로그인한 이메일(`/api/health` 의 `accessUser`) — ⋯ 메뉴에 로그아웃을 그린다.
   * 로컬·테일넷 화면이거나 아직 모르면 null.
   */
  accessUser: string | null;
  /** 지금 도는 데몬의 버전(`/api/health`). 아직 모르거나 구버전 데몬이면 null. */
  daemonVersion: string | null;
  /** 그 데몬을 빌드한 커밋의 짧은 해시 — 표시용. 구버전 데몬이거나 git 밖에서 빌드했으면 null. */
  daemonCommit: string | null;
  /**
   * 이 화면을 연 뒤 데몬 버전이나 커밋이 바뀌었는가. 화면의 번들은 데몬이 서빙한 것이라, 데몬이
   * 새 빌드로 재시작되면 열려 있던 화면은 옛 코드로 남는다 — 새로고침하라는 신호다. main 빌드끼리는
   * 버전이 같으니 커밋도 본다.
   */
  daemonVersionChanged: boolean;
  /** 에이전트 탭을 보이나(이 브라우저의 선택). */
  showAgents: boolean;
  /** 에이전트 탭의 세션 목록 — 아직 못 읽었으면 null. */
  agents: { available: boolean; reason?: string; list: SessionRow[] } | null;

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
  /** 핸드오프 서버를 닫는다(pid 로만, 워크트리는 남는다). @throws 서버 거절 사유 그대로. */
  closeHandoff: (label: string) => Promise<void>;
  /** 대상 밖 서버를 닫는다(pid 로만). @throws 서버 거절 사유 그대로. */
  closeStray: (pid: number) => Promise<void>;
  setActor: (actor: string) => void;
  /** 테마 선호를 저장하고 `<html data-theme>` 까지 갱신한다. */
  setThemePref: (pref: ThemePref) => void;
  setView: (view: AppView) => void;
  setConnected: (connected: boolean) => void;
  /** 주소가 가리키는 탭으로 맞춘다 — 뒤로가기(popstate)가 쓴다. 옛 보드 · 할 일 주소는 맨 주소로 고친다. */
  applyLocation: () => void;
  /** PR 감시 스냅숏을 다시 받는다 — 부팅, SSE 의 PR 전이, 1분 주기. */
  refetch: () => Promise<void>;
  /**
   * `/api/health` 로 이 출처의 능력과 데몬 버전을 확인한다 — 부팅 때와 SSE 가 다시 붙을
   * 때만 부른다(데몬 재시작은 SSE 재연결로 드러난다). 실패는 삼킨다: 힌트를 못 얻어도 화면은
   * 그대로 동작해야 하고, 강제는 서버가 한다.
   */
  loadCapabilities: () => Promise<void>;
  /** 에이전트 탭 — 그 세션의 받은편지함에 한 줄(로컬 화면만). 못 보내면 데몬이 준 이유로 던진다. */
  sendSessionMessage: (sessionId: string, text: string) => Promise<void>;
  /**
   * 에이전트 탭 — background 세션을 멈춘다(로컬 화면만, `claude stop`). 성공하면 그 행을 바로 빼고 목록을 다시 읽는다(데몬이
   * 세션 목록 캐시를 비웠다). 못 멈추면 데몬이 준 이유로 던진다.
   */
  stopSession: (sessionId: string) => Promise<void>;
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

function currentPath(): string {
  return `${window.location.pathname}${window.location.search}`;
}

/** 주소가 이미 `path` 면 아무것도 하지 않고, 아니면 히스토리 항목을 **더하지 않고** 갈아끼운다. */
function replacePath(path: string): void {
  if (currentPath() !== path) {
    window.history.replaceState(null, '', path);
  }
}

/** 주소가 실제로 바뀔 때만 히스토리 항목을 만든다 — 같은 탭을 다시 눌러 같은 항목이 쌓이지 않게. */
function pushPath(path: string): void {
  if (currentPath() !== path) {
    window.history.pushState(null, '', path);
  }
}

/** 탭을 끄거나 rc 가 꺼진 기기여서 그 탭을 보던 화면을 피드로 돌린다 — 주소도 함께. */
function leaveView(view: AppView): Partial<UiState> {
  if (useUiStore.getState().view !== view) {
    return {};
  }
  replacePath(pathFor('feed'));
  return { view: 'feed' };
}

/** PR 감시 스냅숏 — 없거나 실패하면 빈 목록("모름" 이 아니라 "없음" 으로 보여도 무해). */
function fetchPrs(actor: string): Promise<PrSnapshot[]> {
  return api<PrSnapshot[]>('/api/prs?open=true', actor).catch((): PrSnapshot[] => []);
}

export const useUiStore = create<UiState>((set, get) => ({
  prs: [],
  githubHidden: readHidden(),
  showGithub: readStored(GITHUB_TAB_KEY) !== 'off',
  showRc: readStored(RC_TAB_KEY) !== 'off',
  rc: null,
  actor: readStored(ACTOR_KEY) ?? 'logan',
  themePref: readThemePref(readStored(THEME_KEY)),
  // 첫 화면은 피드다(2026-10-02 오너) — 맨 주소면 피드, 탭을 실은 주소(`?view=worklog`)면 그 탭.
  view: viewOf(window.location.search),
  connected: false,
  spawnAllowed: true,
  rcControlAllowed: true,
  accessUser: null,
  daemonVersion: null,
  daemonCommit: null,
  daemonVersionChanged: false,
  showAgents: readStored(AGENTS_TAB_KEY) !== 'off',
  agents: null,

  hideGithub: (key) => {
    const next = [...new Set([...get().githubHidden, key])];
    // 끝없이 쌓이지 않게 최근 200개만 — 오래된 PR 은 이미 닫혔다.
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
    set({ showGithub, ...(showGithub ? {} : leaveView('github')) });
  },
  setShowRc: (showRc) => {
    writeStored(RC_TAB_KEY, showRc ? 'on' : 'off');
    set({ showRc, ...(showRc ? {} : leaveView('rc')) });
  },
  setShowAgents: (showAgents) => {
    writeStored(AGENTS_TAB_KEY, showAgents ? 'on' : 'off');
    set({ showAgents, ...(showAgents ? {} : leaveView('agents')) });
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
      // 꺼진 기기에서 원격 제어 탭을 보던 중이면 주소도 피드로 — `?view=rc` 가 남으면 새로고침·뒤로가기마다 튕긴다.
      set({ rc, ...(rcVisible(rc) ? {} : leaveView('rc')) });
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
  closeHandoff: async (label) => {
    logUsage('web:handoff-close');
    try {
      const res = await api<{ down: boolean; name: string; pid: number }>(
        `/api/rc/handoffs/${encodeURIComponent(label)}/stop`,
        get().actor,
        { method: 'POST' },
      );
      if (!res.down) {
        throw new Error(
          `“${res.name}”(pid ${res.pid})이 내려가지 않았어요 — 원격 제어 탭을 다시 확인하세요`,
        );
      }
    } finally {
      // 이미 내려가 있다(404)·닫음 어느 쪽이든 줄을 새로 그린다.
      await get().loadRc();
    }
  },
  closeStray: async (pid) => {
    logUsage('web:stray-close');
    try {
      const res = await api<{ down: boolean; label: string; pid: number }>(
        `/api/rc/strays/${pid}/stop`,
        get().actor,
        { method: 'POST' },
      );
      if (!res.down) {
        throw new Error(
          `${res.label}(pid ${res.pid})이 내려가지 않았어요 — 원격 제어 탭을 다시 확인하세요`,
        );
      }
    } finally {
      await get().loadRc();
    }
  },
  setActor: (actor) => {
    setUsageActor(actor);
    writeStored(ACTOR_KEY, actor);
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
    writeStored(THEME_KEY, pref);
  },
  setView: (view) => {
    if (view === get().view) {
      return;
    }
    logUsage('web:view', { view });
    set({ view });
    // 탭은 주소에 산다 — 히스토리 항목으로 쌓아 뒤로가기가 앞 탭으로 돌아가게.
    pushPath(pathFor(view));
  },
  setConnected: (connected) => set({ connected }),
  applyLocation: () => {
    const view = viewOf(window.location.search);
    if (view !== get().view) {
      set({ view });
    }
    // 옛 보드 · 할 일 · 노트 주소(`/rocky/12`)나 꺼 둔 탭의 주소는 지금 탭의 주소로 고친다.
    replacePath(pathFor(view));
  },
  refetch: async () => {
    set({ prs: await fetchPrs(get().actor) });
  },
  loadCapabilities: async () => {
    try {
      const health = await api<{
        spawnAllowed?: boolean;
        rcControlAllowed?: boolean;
        accessUser?: string | null;
        version?: string;
        commit?: string | null;
      }>('/api/health', get().actor);
      const previous = get().daemonVersion;
      const previousCommit = get().daemonCommit;
      const version = health.version ?? null;
      const commit = health.commit ?? null;
      // 필드가 없는 구버전 데몬이면 낙관적으로 둔다 — 그 데몬에는 애초에 이 가드가 없다.
      set({
        spawnAllowed: health.spawnAllowed ?? true,
        rcControlAllowed: health.rcControlAllowed ?? health.spawnAllowed ?? true,
        accessUser: health.accessUser ?? null,
        daemonVersion: version,
        daemonCommit: commit,
        // 한 번 바뀌었으면 새로고침 전까지 켜 둔다 — 되돌아가도 번들은 이미 첫 버전의 것이다.
        daemonVersionChanged:
          get().daemonVersionChanged ||
          (previous !== null &&
            version !== null &&
            (previous !== version || previousCommit !== commit)),
      });
    } catch {
      // 힌트를 못 얻는 것으로 화면이 망가지면 안 된다. 강제는 서버 몫이다.
    }
  },
  sendSessionMessage: async (sessionId, text) => {
    logUsage('web:session-message');
    await api('/api/sessions/message', get().actor, {
      method: 'POST',
      body: JSON.stringify({ sessionId, text }),
    });
  },
  stopSession: async (sessionId) => {
    logUsage('web:session-stop');
    await api('/api/sessions/stop', get().actor, {
      method: 'POST',
      body: JSON.stringify({ sessionId }),
    });
    const { agents } = get();
    set({
      agents: agents && {
        ...agents,
        list: agents.list.filter((s) => s.sessionId !== sessionId),
      },
    });
    void get().loadAgents();
  },
}));
