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

describe('보던 탭은 주소에 산다', () => {
  test('탭을 바꾸면 주소에 싣고 히스토리에 쌓는다 — 피드로 오면 맨 주소', () => {
    window.history.replaceState(null, '', '/');
    useUiStore.setState({ view: 'feed', openNoteId: null, detail: null, selected: 'all' });
    const before = window.history.length;
    useUiStore.getState().setView('todos');
    expect(`${window.location.pathname}${window.location.search}`).toBe('/?view=todos');
    expect(window.history.length).toBe(before + 1);
    useUiStore.getState().setView('feed');
    expect(`${window.location.pathname}${window.location.search}`).toBe('/');
  });

  test('주소를 따라간다 — 뒤로가기로 맨 주소에 오면 피드', async () => {
    useUiStore.setState({ view: 'todos' });
    await useUiStore.getState().applyRoute({ board: 'all' });
    expect(useUiStore.getState().view).toBe('feed');
    await useUiStore.getState().applyRoute({ board: 'all', view: 'worklog' });
    expect(useUiStore.getState().view).toBe('worklog');
  });
});
