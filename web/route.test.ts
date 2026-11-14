import { describe, expect, test } from 'bun:test';
import { parseView, pathFor, VIEWS } from './route';

describe('parseView', () => {
  test('맨 주소와 모르는 값은 피드다', () => {
    expect(parseView('')).toBe('feed');
    expect(parseView('?view=feed')).toBe('feed');
    expect(parseView('?view=nope')).toBe('feed');
  });

  test('걷어낸 탭(할 일 · 노트)으로 온 옛 주소는 피드로 떨어진다', () => {
    expect(parseView('?view=todos')).toBe('feed');
    expect(parseView('?view=notes')).toBe('feed');
    expect(parseView('?todo=rocky-12')).toBe('feed');
  });

  test('남은 탭은 그대로 읽는다', () => {
    expect(parseView('?view=worklog')).toBe('worklog');
    expect(parseView('?view=github')).toBe('github');
    expect(parseView('?view=agents')).toBe('agents');
    expect(parseView('?view=rc')).toBe('rc');
  });
});

describe('pathFor', () => {
  test('피드는 맨 주소, 나머지는 view 쿼리', () => {
    expect(pathFor('feed')).toBe('/');
    expect(pathFor('worklog')).toBe('/?view=worklog');
  });

  test('모든 탭이 되읽힌다', () => {
    for (const view of VIEWS) {
      const path = pathFor(view);
      expect(parseView(path.slice(1))).toBe(view);
    }
  });
});
