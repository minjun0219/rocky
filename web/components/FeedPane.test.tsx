import { afterEach, describe, expect, test } from 'bun:test';
import { cleanup, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { useUiStore } from '../store';
import { renderWithStore, todoFixture } from '../test-support';
import { FeedPane } from './FeedPane';
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

describe('FeedPane — 오너가 손댈 것', () => {
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

  test('피드 탭 옆 숫자 = PR 알림 줄 수(숨긴 것·아직 이른 충돌은 빠진다) + 내 차례', () => {
    renderWithStore(<ViewSwitch />, state);
    expect(screen.getByRole('button', { name: /피드\s*1/ })).toBeTruthy();
  });

  test('× 로 숨기면 사라진다', async () => {
    renderWithStore(<FeedPane />, state);
    expect(screen.getByText('여덟')).toBeTruthy();
    expect(screen.queryByText('열')).toBeNull();
    await userEvent.click(screen.getByRole('button', { name: 'rocky #8 숨기기' }));
    expect(useUiStore.getState().githubHidden).toContain('alert:merge:o/rocky#8');
    expect(screen.queryByText('여덟')).toBeNull();
    expect(screen.getByText('내 차례 없음')).toBeTruthy();
  });
});

// 예전 할 일 화면 "지금" 표의 내 차례가 피드로 왔다 — 세션이 사라진 진행 중 할 일이 그 예.
describe('FeedPane — 내 차례', () => {
  test('세션이 사라진 진행 중 할 일이 피드에 뜨고 탭 숫자에 더해진다', () => {
    const stuck = todoFixture({
      id: 'stuck',
      title: '들고 있다 사라진 일',
      status: 'doing',
      doingState: 'gone',
      doingBy: 'claude-code',
      doingSince: new Date(Date.now() - 3_600_000).toISOString(),
    });
    const state = { selected: 'all', boards: [board], notes: [], prs: [], nowTodos: [stuck] };
    renderWithStore(<FeedPane />, state);
    expect(screen.getByText('들고 있다 사라진 일')).toBeTruthy();
    expect(screen.getByRole('region', { name: '내 차례' }).textContent).toContain('세션 없음');
    cleanup();
    renderWithStore(<ViewSwitch />, state);
    expect(screen.getByRole('button', { name: /피드\s*1/ })).toBeTruthy();
  });

  test('피드가 맨 앞 탭이다', () => {
    renderWithStore(<ViewSwitch />, { boards: [board], notes: [], prs: [] });
    const tabs = screen.getAllByRole('button').map((b) => b.textContent);
    expect(tabs[0]).toContain('피드');
    expect(tabs[1]).toContain('할 일');
  });
});
