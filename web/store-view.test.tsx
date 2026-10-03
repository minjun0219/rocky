import { afterEach, describe, expect, test } from 'bun:test';
import { readView, useUiStore } from './store';

afterEach(() => {
  sessionStorage.clear();
  localStorage.clear();
});

describe('보던 탭 — 같은 브라우저 탭의 새로고침에만 남는다', () => {
  test('탭이 바뀌면 sessionStorage 에 적고, 다음 부팅이 그 탭으로 연다', () => {
    useUiStore.setState({ view: 'feed' });
    useUiStore.setState({ view: 'todos' });
    expect(sessionStorage.getItem('rocky:view')).toBe('todos');
    expect(readView()).toBe('todos');
  });

  test('적힌 게 없거나 모르는 값이면 피드 — 새로 연 탭의 첫 화면', () => {
    expect(readView()).toBe('feed');
    sessionStorage.setItem('rocky:view', 'nope');
    expect(readView()).toBe('feed');
  });

  test('GitHub 탭을 꺼 뒀으면 GitHub 으로 열지 않는다', () => {
    sessionStorage.setItem('rocky:view', 'github');
    localStorage.setItem('rocky:github-tab', 'off');
    expect(readView()).toBe('feed');
  });
});
