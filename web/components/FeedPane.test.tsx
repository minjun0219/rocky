import { afterEach, describe, expect, test } from 'bun:test';
import { cleanup, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { useUiStore } from '../store';
import { renderWithStore } from '../test-support';
import { FeedPane } from './FeedPane';
import { ViewSwitch } from './ViewSwitch';

afterEach(cleanup);

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

describe('FeedPane — 내 차례', () => {
  test('답을 기다리는 에이전트가 피드에 뜨고 탭 숫자에 더해진다', () => {
    const waiting = {
      pid: undefined,
      cwd: '/w/rocky',
      kind: 'background',
      id: '0da6a98a',
      sessionId: '0da6a98a-full',
      name: 'acorn-25',
      status: 'idle',
      state: 'blocked',
      startedAt: Date.now() - 3_600_000,
      matched: false,
      job: { needs: '어느 쪽으로 갈지 정해 주세요' },
    };
    const state = { prs: [], showAgents: true, agents: { available: true, list: [waiting] } };
    renderWithStore(<FeedPane />, state);
    expect(screen.getByText('어느 쪽으로 갈지 정해 주세요')).toBeTruthy();
    expect(screen.getByRole('region', { name: '내 차례' }).textContent).toContain('답 기다림');
    cleanup();
    renderWithStore(<ViewSwitch />, state);
    expect(screen.getByRole('button', { name: /피드\s*1/ })).toBeTruthy();
  });

  test('피드가 맨 앞 탭이다', () => {
    renderWithStore(<ViewSwitch />, { prs: [] });
    const tabs = screen.getAllByRole('button').map((b) => b.textContent);
    expect(tabs[0]).toContain('피드');
    expect(tabs[1]).toContain('작업로그');
  });
});
