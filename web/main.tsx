import { useEffect, useRef } from 'react';
import { createRoot } from 'react-dom/client';
import { AgentsPane, useAgentsPolling } from './components/AgentsPane';
import { FeedPane } from './components/FeedPane';
import { GithubPane } from './components/GithubPane';
import { RcPane, RcSummary } from './components/RcPane';
import { TopBar } from './components/TopBar';
import { VersionFooter } from './components/VersionFooter';
import { WorklogPane } from './components/WorklogPane';
import { touchesPrs } from './lib';
import { useUiStore } from './store';
import { setUsageActor } from './usage';

/**
 * 보드 싱크가 이 브라우저에 남긴 보드 사본(IndexedDB `rocky-sync`)을 지운다. 보드 화면과 싱크를 걷어낸 뒤로 읽는 곳이 없고,
 * Access·tailscale 로 연 폰 브라우저에는 보드가 그대로 남아 있었다. 없으면 아무 일도 없다 — 실패는 삼킨다.
 */
function dropBoardReplica(): void {
  try {
    globalThis.indexedDB?.deleteDatabase('rocky-sync');
  } catch {
    // 저장소가 막힌 브라우저(비공개 창 등) — 지울 사본도 없다.
  }
}

/**
 * rocky 웹 UI 루트 — `bun run build:ui` 가 dist/ 로 번들하고 데몬(rockyd)이 서빙한다.
 * 데몬 변경(SSE `/api/events`)이 PR 을 건드렸을 수 있으면 PR 스냅숏만 다시 읽는다. 세션 목록과 rc 현황은 각 화면이 묻는다.
 */
function App() {
  const refetch = useUiStore((s) => s.refetch);
  const setConnected = useUiStore((s) => s.setConnected);
  const themePref = useUiStore((s) => s.themePref);
  const setThemePref = useUiStore((s) => s.setThemePref);
  const view = useUiStore((s) => s.view);
  const loadRc = useUiStore((s) => s.loadRc);
  // 탭 줄이 원격 제어 탭을 보일지 알려면 rc 가 이 기기에서 켜졌는지부터 안다 — 기동 때 한 번.
  useEffect(() => {
    void loadRc();
  }, [loadRc]);
  // 에이전트 — 탭과 피드의 "내 차례"(답을 기다리는 background 세션)가 같은 목록을 본다.
  useAgentsPolling();
  const debounce = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const headRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const head = headRef.current;
    if (!head || typeof ResizeObserver === 'undefined') {
      return;
    }
    const publish = () =>
      document.documentElement.style.setProperty('--app-head-h', `${head.offsetHeight}px`);
    publish();
    const observer = new ResizeObserver(publish);
    observer.observe(head);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    // `refetch` 는 실패를 빈 목록으로 삼키므로 reject 하지 않는다. 연결 배지는 SSE 링크 상태만 말한다 — 진짜 링크
    // 단절은 `source.onerror` 가 알려주고, 데이터는 다음 SSE 이벤트나 60초 tick 이 따라잡는다.
    const sync = (): void => {
      void refetch();
    };
    dropBoardReplica();
    const store = useUiStore.getState();
    // 옛 보드 · 할 일 주소(`/rocky/12`)로 들어왔으면 지금 탭의 주소로 고친다.
    store.applyLocation();
    sync();
    // health 는 부팅과 SSE 재연결 때만 묻는다 (refetch 에 얹으면 SSE 이벤트마다 다시 묻게
    // 된다). 재연결 때 다시 묻는 이유는 버전이다 — 데몬이 재시작되면 SSE 가 끊겼다 붙는다.
    const loadHealth = (): void => {
      void useUiStore.getState().loadCapabilities();
    };
    loadHealth();
    // 사용 로그가 붙일 actor — 스토어를 import 하지 않는 모듈이라 여기서 한 번 맞춘다.
    setUsageActor(store.actor);

    const onPopState = () => useUiStore.getState().applyLocation();
    window.addEventListener('popstate', onPopState);

    // 모바일 브라우저는 탭이 백그라운드로 가면 EventSource 와 타이머를 얼린다. 돌아와도
    // 끊겨 있던 동안의 변경은 오지 않으므로, SSE 재연결을 기다리지 않고 즉시 다시 읽는다.
    const onVisible = () => {
      if (document.visibilityState === 'visible') {
        sync();
      }
    };
    document.addEventListener('visibilitychange', onVisible);

    const source = new EventSource('/api/events');
    // 첫 onopen 은 부팅 직후라 위의 health 조회와 겹친다 — 재연결일 때만 다시 묻는다.
    let opened = false;
    source.onopen = () => {
      setConnected(true);
      if (opened) {
        loadHealth();
      }
      opened = true;
    };
    source.onerror = () => setConnected(false);
    // 연속 이벤트를 한 번의 refetch 로 흡수한다(150ms 창). PR 을 건드리지 않는 이벤트뿐이면 다시 읽지 않는다.
    let pending: unknown[] = [];
    source.onmessage = (message) => {
      try {
        pending.push(JSON.parse(message.data));
      } catch {
        pending.push(null); // 모르는 모양 → 다시 읽는다
      }
      clearTimeout(debounce.current);
      debounce.current = setTimeout(() => {
        const touched = touchesPrs(pending);
        pending = [];
        if (touched) {
          sync();
        }
      }, 150);
    };
    // 경과 표시 갱신과 놓친 이벤트를 따라잡는 주기.
    const tick = setInterval(sync, 60_000);

    return () => {
      window.removeEventListener('popstate', onPopState);
      document.removeEventListener('visibilitychange', onVisible);
      source.close();
      clearTimeout(debounce.current);
      clearInterval(tick);
    };
  }, [refetch, setConnected]);

  useEffect(() => {
    // 저장값이 auto 일 때만 OS 를 따라간다 — 명시 선택은 OS 가 바뀌어도 유지돼야 한다.
    if (themePref !== 'auto') {
      return;
    }
    const query = window.matchMedia('(prefers-color-scheme: light)');
    // setThemePref('auto') 를 다시 부르면 해석이 새 OS 값으로 다시 돌아 data-theme 이
    // 갱신된다 — 해석 규칙이 store 한 곳에만 있게 된다.
    const onChange = () => setThemePref('auto');
    query.addEventListener('change', onChange);
    return () => query.removeEventListener('change', onChange);
  }, [themePref, setThemePref]);

  return (
    <div className="app">
      {/* 머리(탭)는 늘 보인다 — 좁은 창은 문서가 스크롤하니 sticky 로 붙여 두고, 그 높이를 `--app-head-h` 로 알린다. */}
      <div ref={headRef} className="app-head sticky top-0 z-20 bg-bg">
        <TopBar />
        {/* 원격 제어 요약 — 피드에서만, rc 가 꺼진 기기면 그리지 않는다. */}
        {view === 'feed' ? <RcSummary /> : null}
      </div>
      {/* 관제판 — 한 열. 첫 화면은 피드(PR 알림 + 내 차례). */}
      <div className="layout flex min-h-0 flex-1 flex-row">
        <div className="flex min-h-0 min-w-0 flex-1 flex-col">
          {view === 'feed' ? (
            <FeedPane />
          ) : view === 'worklog' ? (
            <WorklogPane />
          ) : view === 'rc' ? (
            <RcPane />
          ) : view === 'agents' ? (
            <AgentsPane />
          ) : (
            <GithubPane />
          )}
        </div>
      </div>
      <VersionFooter />
    </div>
  );
}

const rootElement = document.getElementById('root');
if (rootElement) {
  createRoot(rootElement).render(<App />);
}
