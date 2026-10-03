import { useEffect, useRef } from 'react';
import { createRoot } from 'react-dom/client';
import { DetailDrawer } from './components/DetailDrawer';
import { FeedPane } from './components/FeedPane';
import { GithubPane } from './components/GithubPane';
import { WorklogPane } from './components/WorklogPane';
import { NotesRail } from './components/NotesRail';
import { NowTable } from './components/NowTable';
import { TodoPane } from './components/TodoPane';
import { TopBar } from './components/TopBar';
import { VersionFooter } from './components/VersionFooter';
import { slicesFor } from './lib';
import { parseRoute } from './route';
import { useUiStore } from './store';
import { setUsageActor } from './usage';

/**
 * rocky 웹 UI 루트 — `bun run build:ui` 가 dist/ 로 번들하고 데몬(rockyd)이 서빙한다.
 * SSE(/api/events) 를 구독해 어떤 경로(CLI/MCP/다른 브라우저)의 변경이든 실시간 반영.
 */
function App() {
  const refetch = useUiStore((s) => s.refetch);
  const setConnected = useUiStore((s) => s.setConnected);
  const themePref = useUiStore((s) => s.themePref);
  const setThemePref = useUiStore((s) => s.setThemePref);
  const view = useUiStore((s) => s.view);
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
    // `refetch` 는 네트워크·서버 오류로 reject 한다(`api()` 가 !res.ok 에 throw). 아래 모든
    // 호출 지점이 이 가드를 거친다 — 빠뜨리면 처리되지 않은 rejection 으로 남는다.
    //
    // **여기서 `connected` 를 내리지 않는다.** 그 배지는 SSE 링크 상태이지 REST 성패가
    // 아니다. EventSource 는 열려 있는데 재조회 한 번이 실패한 경우까지 NO LINK 로 내리면,
    // 열린 EventSource 는 `onopen` 을 다시 쏘지 않으므로 이후 SSE 메시지가 정상으로 도착해도
    // 배지가 영영 내려간 채 남는다. 진짜 링크 단절은 `source.onerror` 가 알려주고, 데이터는
    // 다음 SSE 이벤트나 60초 tick 이 따라잡는다.
    //
    // 다만 조용히 삼키지도 않는다. 배지를 SSE 전용으로 둔 대가로, 데몬이 살아 SSE 는 흐르는데
    // REST 만 실패하는 경우 화면에는 아무 신호도 남지 않는다 — 배지는 초록인데 보드만 낡는다.
    // 그때 콘솔이 유일한 단서다.
    const onSyncError = (err: unknown): void => {
      console.warn('[rocky] 보드 재조회 실패 — 화면이 낡았을 수 있다', err);
    };
    const sync = (): void => {
      void refetch().catch(onSyncError);
    };

    // 주소 해석은 목록 재조회와 실패 원인이 다르다. `applyRoute` 는 보드를 바꾸면 다시
    // refetch 하고, 번호가 풀리면 `/api/todos/:id` 로 상세를 연다 — 뒤쪽이 실패한 것을
    // "보드 재조회 실패" 라고 적으면 로그가 엉뚱한 곳을 가리킨다.
    const applyRoute = (): Promise<void> =>
      useUiStore
        .getState()
        .applyRoute(parseRoute(window.location.pathname, window.location.search))
        .catch((err: unknown) => {
          console.warn('[rocky] 주소가 가리키는 화면을 열지 못했다', err);
        });

    // 초기 목록을 받은 뒤에야 URL 의 번호를 todo id 로 해석할 수 있다. 두 번째 인자로
    // 넘겨야 `onSyncError` 가 목록 재조회의 실패만 받는다 — `.catch()` 로 이으면 뒤따르는
    // 주소 해석의 실패까지 함께 삼킨다.
    void refetch().then(applyRoute, onSyncError);
    // health 는 부팅과 SSE 재연결 때만 묻는다 (refetch 에 얹으면 SSE 이벤트마다 다시 묻게
    // 된다). 재연결 때 다시 묻는 이유는 버전이다 — 데몬이 재시작되면 SSE 가 끊겼다 붙는다.
    const loadHealth = (): void => {
      void useUiStore.getState().loadCapabilities();
    };
    loadHealth();
    // 사용 로그가 붙일 actor — 스토어를 import 하지 않는 모듈이라 여기서 한 번 맞춘다.
    setUsageActor(useUiStore.getState().actor);

    const onPopState = () => {
      void applyRoute();
    };
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
    // 연속 mutation 을 한 번의 refetch 로 흡수하되, 이벤트가 건드린 묶음만 받는다(`slicesFor`) — 예전엔 이벤트마다
    // 9개를 다 받았다. 묶음은 150ms 창 안의 이벤트를 합친다.
    let pending: unknown[] = [];
    source.onmessage = (message) => {
      try {
        pending.push(JSON.parse(message.data));
      } catch {
        pending.push(null); // 모르는 모양 → 전부
      }
      clearTimeout(debounce.current);
      debounce.current = setTimeout(() => {
        const slices = slicesFor(pending);
        pending = [];
        void refetch(slices).catch(onSyncError);
      }, 150);
    };
    // doing 경과 표시 갱신용 주기 리렌더
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
      {/* 머리(보드 전환 · 탭)는 늘 보인다 — 좁은 창은 문서가 스크롤하니 sticky 로 붙여 두고, 그 높이를
          `--app-head-h` 로 알려 아래의 sticky(빠른 추가 · 노트 툴바)가 그 밑에 붙게 한다. */}
      <div ref={headRef} className="app-head sticky top-0 z-20 bg-bg">
        <TopBar />
        {/* 돌고 있음 — 탭과 상관없이 늘 보인다(2026-10-02 오너). 없으면 자리를 차지하지 않는다. */}
        <NowTable />
      </div>
      {/* 관제판 — 한 열. 머리(보드 스위처 · 돌고 있음 · 탭) 아래 첫 화면은 피드(PR 알림 + 내 차례).
          할 일: 그 보드의 목록. 노트: 그 보드의 노트가 화면 전체(`web/DESIGN.md` "Notes"). */}
      <div className="layout flex min-h-0 flex-1 flex-col">
        {view === 'feed' ? (
          <FeedPane />
        ) : view === 'todos' ? (
          <TodoPane />
        ) : view === 'notes' ? (
          <NotesRail />
        ) : view === 'worklog' ? (
          <WorklogPane />
        ) : (
          <GithubPane />
        )}
      </div>
      <VersionFooter />
      <DetailDrawer />
    </div>
  );
}

const rootElement = document.getElementById('root');
if (rootElement) {
  createRoot(rootElement).render(<App />);
}
