import { afterEach, beforeEach, describe, expect, test } from 'bun:test';
import { useUiStore } from './store';

// 이 파일만 따로 돌면 주소가 about:blank 라 replaceState 가 먹지 않는다(CI 의 파일 순서) — 기준 주소를 박는다.
beforeEach(() => {
  (window as unknown as { happyDOM: { setURL: (url: string) => void } }).happyDOM.setURL(
    'http://127.0.0.1:8636/',
  );
});

afterEach(() => {
  window.history.replaceState(null, '', '/');
  useUiStore.setState({ view: 'feed' });
});

const here = () => `${window.location.pathname}${window.location.search}`;

describe('보던 탭은 주소에 산다', () => {
  test('탭을 바꾸면 주소에 싣고 히스토리에 쌓는다 — 피드로 오면 맨 주소', () => {
    window.history.replaceState(null, '', '/');
    useUiStore.setState({ view: 'feed' });
    const before = window.history.length;
    useUiStore.getState().setView('worklog');
    expect(here()).toBe('/?view=worklog');
    expect(window.history.length).toBe(before + 1);
    useUiStore.getState().setView('feed');
    expect(here()).toBe('/');
  });

  test('주소를 따라간다 — 뒤로가기로 맨 주소에 오면 피드', () => {
    useUiStore.setState({ view: 'worklog' });
    window.history.replaceState(null, '', '/');
    useUiStore.getState().applyLocation();
    expect(useUiStore.getState().view).toBe('feed');
    window.history.replaceState(null, '', '/?view=worklog');
    useUiStore.getState().applyLocation();
    expect(useUiStore.getState().view).toBe('worklog');
  });

  test('옛 보드 · 할 일 · 노트 주소는 피드로 떨어지고 맨 주소로 고친다', () => {
    for (const old of ['/rocky/12', '/rocky/notes/3', '/?todo=rocky-12', '/rocky?view=todos']) {
      window.history.replaceState(null, '', old);
      useUiStore.getState().applyLocation();
      expect(useUiStore.getState().view).toBe('feed');
      expect(here()).toBe('/');
    }
  });

  test('탭을 끄면 그 탭을 보던 화면은 피드로, 주소도 맨 주소로', () => {
    window.history.replaceState(null, '', '/?view=github');
    useUiStore.setState({ view: 'github', showGithub: true });
    useUiStore.getState().setShowGithub(false);
    expect(useUiStore.getState().view).toBe('feed');
    expect(here()).toBe('/');
    useUiStore.getState().setShowGithub(true);
  });
});
