import { afterEach, describe, expect, mock, test } from 'bun:test';
import { cleanup, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { boardFixture, renderWithStore, todoFixture } from '../test-support';
import type { SessionRow } from '../types';
import { AgentsPane } from './AgentsPane';
import { ViewSwitch } from './ViewSwitch';

afterEach(cleanup);

const loadAgents = mock(async () => {});

const blocked: SessionRow = {
  kind: 'background',
  id: '0da6a98a',
  sessionId: '0da6a98a-full',
  name: 'rocky-25',
  cwd: '/w/rocky/.claude/worktrees/todo-25',
  status: 'idle',
  state: 'blocked',
  startedAt: Date.parse('2026-10-06T00:00:00Z'),
  matched: false,
  job: { needs: '룰셋을 끌지 정해 주세요', detail: '3 PR 머지' },
};

const busy: SessionRow = {
  pid: 7,
  kind: 'interactive',
  sessionId: 'sess-7',
  name: 'hail-mary',
  cwd: '/w/hail-mary',
  status: 'busy',
  startedAt: Date.parse('2026-10-06T00:00:00Z'),
  matched: false,
};

describe('AgentsPane', () => {
  test('내 차례 행은 기다리는 것을 싣고, 든 할 일이 있으면 눌러 연다', async () => {
    const openTodoDetail = mock(async () => {});
    const todo = todoFixture({ status: 'doing', doingSessionId: '0da6a98a', ref: 'rocky-25' });
    renderWithStore(<AgentsPane />, {
      agents: { available: true, list: [blocked, busy] },
      loadAgents,
      boards: [boardFixture({ key: 'rocky' })],
      selected: 'all',
      nowTodos: [todo],
      openTodoDetail,
    });
    const mine = screen.getByRole('region', { name: '내 차례' });
    expect(mine.textContent).toContain('룰셋을 끌지 정해 주세요');
    expect(mine.textContent).toContain('백그라운드');
    expect(mine.textContent).toContain('rocky-25');
    expect(screen.getByRole('region', { name: '실행 중' }).textContent).toContain('hail-mary');
    await userEvent.click(screen.getByRole('button', { name: /rocky-25/ }));
    expect(openTodoDetail).toHaveBeenCalledWith(todo.id);
    expect(loadAgents).toHaveBeenCalled();
  });

  test('세션 목록을 못 읽으면 사유를 말한다', () => {
    renderWithStore(<AgentsPane />, {
      agents: { available: false, reason: 'claude CLI 없음', list: [] },
      loadAgents,
    });
    expect(screen.getByText(/세션 목록을 읽지 못했어요 — claude CLI 없음/)).toBeTruthy();
  });

  test('보드를 고르면 그 보드에서 도는 것만, 없으면 한 줄', () => {
    renderWithStore(<AgentsPane />, {
      agents: { available: true, list: [busy] },
      loadAgents,
      boards: [boardFixture({ key: 'rocky' })],
      selected: 'rocky',
    });
    expect(screen.getByText('이 보드에서 도는 에이전트가 없어요')).toBeTruthy();
  });
});

describe('ViewSwitch', () => {
  test('메뉴에서 끄면 에이전트 탭이 없다', () => {
    renderWithStore(<ViewSwitch />, { showAgents: false });
    expect(screen.queryByRole('button', { name: '에이전트' })).toBeNull();
    cleanup();
    renderWithStore(<ViewSwitch />, { showAgents: true });
    expect(screen.getByRole('button', { name: '에이전트' })).toBeTruthy();
  });
});
