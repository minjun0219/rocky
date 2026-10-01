import { afterEach, describe, expect, test } from 'bun:test';
import { cleanup, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { useUiStore } from '../store';
import { renderWithStore } from '../test-support';
import { AlertsPane } from './AlertsPane';
import { ViewSwitch } from './ViewSwitch';

afterEach(cleanup);

const board = {
  id: 'b1',
  key: 'rocky',
  title: 'rocky',
  repo: 'o/rocky',
  createdAt: '2026-09-01T00:00:00Z',
  updatedAt: '2026-09-01T00:00:00Z',
} as never;
const recent = () => new Date(Date.now() - 60_000).toISOString();
const pr = (over: Record<string, unknown>) => ({
  repo: 'o/rocky',
  number: 1,
  title: 'PR',
  url: 'https://github.com/o/rocky/pull/1',
  state: 'OPEN',
  isDraft: false,
  base: 'main',
  head: 'abc',
  mergeState: 'BLOCKED',
  ci: 'pending',
  unhandled: 0,
  decision: 0,
  ready: false,
  updatedAt: recent(),
  ...over,
});

describe('AlertsPane — 오너가 손댈 것', () => {
  const state = {
    selected: 'rocky',
    boards: [board],
    notes: [],
    githubHidden: ['alert:merge:o/rocky#9'],
    prs: [
      pr({ number: 8, title: '여덟', ready: true }),
      pr({ number: 9, title: '아홉', ready: true }),
      // 방금 충돌 — 세션이 풀 틈을 준다
      pr({ number: 10, title: '열', mergeState: 'DIRTY' }),
    ] as never,
  };

  test('알림 탭 옆 숫자 = 화면의 줄 수(숨긴 것·아직 이른 충돌은 빠진다)', () => {
    renderWithStore(<ViewSwitch />, state);
    expect(screen.getByRole('button', { name: /알림\s*1/ })).toBeTruthy();
  });

  test('× 로 숨기면 사라진다', async () => {
    renderWithStore(<AlertsPane />, state);
    expect(screen.getByText('여덟')).toBeTruthy();
    expect(screen.queryByText('열')).toBeNull();
    await userEvent.click(screen.getByRole('button', { name: 'rocky #8 숨기기' }));
    expect(useUiStore.getState().githubHidden).toContain('alert:merge:o/rocky#8');
    expect(screen.getByText('손댈 것 없음')).toBeTruthy();
  });
});
