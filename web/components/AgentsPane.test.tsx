import { afterEach, describe, expect, mock, test } from 'bun:test';
import { act, cleanup, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { boardFixture, renderWithStore, todoFixture } from '../test-support';
import type { SessionRow } from '../types';
import { useUiStore } from '../store';
import { AgentsPane } from './AgentsPane';
import { MineSection } from './NowTable';
import { ViewSwitch } from './ViewSwitch';

afterEach(cleanup);

const loadAgents = mock(async () => {});

const blocked: SessionRow = {
  kind: 'background',
  id: '0da6a98a',
  sessionId: '0da6a98a-full',
  name: 'rocky-25',
  cwd: '/w/rocky',
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
  });

  test('살아 있는 세션에 메시지를 보낸다 — ⌘Enter 로, 보내면 입력칸을 닫는다', async () => {
    const sendSessionMessage = mock(async (_id: string, _text: string) => {});
    renderWithStore(<AgentsPane />, {
      agents: { available: true, list: [busy] },
      loadAgents,
      boards: [],
      selected: 'all',
      nowTodos: [],
      spawnAllowed: true,
      sendSessionMessage,
    });
    const row = screen.getByRole('region', { name: '실행 중' });
    await userEvent.click(within(row).getByRole('button', { name: '메시지' }));
    // 받는 쪽에서 승인으로 쓰이지 않는다는 사실을 밝힌다
    expect(within(row).getByText(/권한 허락·결정 답으로는 쓰이지 않아요/)).toBeTruthy();
    await userEvent.type(
      within(row).getByRole('textbox', { name: '보낼 메시지' }),
      '내일 보자{Meta>}{Enter}{/Meta}',
    );
    await waitFor(() => expect(sendSessionMessage).toHaveBeenCalledWith('sess-7', '내일 보자'));
    expect(within(row).queryByRole('textbox')).toBeNull();
    expect(within(row).getByRole('status').textContent).toBe('보냈어요');
  });

  test('못 보내면 데몬이 준 이유를 그 자리에 보이고 입력칸은 남긴다', async () => {
    const sendSessionMessage = mock(async () => {
      throw new Error('보내지 못했다 — 받을 세션 등록 없음');
    });
    renderWithStore(<AgentsPane />, {
      agents: { available: true, list: [busy] },
      loadAgents,
      boards: [],
      selected: 'all',
      nowTodos: [],
      spawnAllowed: true,
      sendSessionMessage,
    });
    await userEvent.click(screen.getByRole('button', { name: '메시지' }));
    await userEvent.type(screen.getByRole('textbox', { name: '보낼 메시지' }), '안녕');
    await userEvent.click(screen.getByRole('button', { name: '보내기' }));
    await waitFor(() =>
      expect(screen.getByRole('status').textContent).toContain('받을 세션 등록 없음'),
    );
    expect(screen.getByRole('textbox', { name: '보낼 메시지' })).toBeTruthy();
  });

  test('쓰던 글은 행이 다른 묶음으로 옮겨 가도 남는다', async () => {
    renderWithStore(<AgentsPane />, {
      agents: { available: true, list: [busy] },
      loadAgents,
      boards: [],
      selected: 'all',
      nowTodos: [],
      spawnAllowed: true,
    });
    await userEvent.click(screen.getByRole('button', { name: '메시지' }));
    await userEvent.type(screen.getByRole('textbox', { name: '보낼 메시지' }), '쓰던 글');
    // 폴링으로 턴이 끝나 실행 중 → 쉬는 중
    act(() =>
      useUiStore.setState({ agents: { available: true, list: [{ ...busy, status: 'idle' }] } }),
    );
    expect(screen.queryByRole('region', { name: '실행 중' })).toBeNull();
    expect(
      (screen.getByRole('textbox', { name: '보낼 메시지' }) as HTMLTextAreaElement).value,
    ).toBe('쓰던 글');
  });

  test('pid 없이 잠든 background 세션에는 버튼이 없다 — 받은편지함을 들을 프로세스가 없다', () => {
    renderWithStore(<AgentsPane />, {
      agents: { available: true, list: [blocked] },
      loadAgents,
      boards: [],
      selected: 'all',
      nowTodos: [],
      spawnAllowed: true,
    });
    expect(screen.queryByRole('button', { name: '메시지' })).toBeNull();
  });

  test('노출된 화면이거나 끝난 세션에는 메시지 버튼이 없다', () => {
    const done: SessionRow = { ...blocked, sessionId: 'done-1', name: 'old', state: 'done' };
    renderWithStore(<AgentsPane />, {
      agents: { available: true, list: [busy, done] },
      loadAgents,
      boards: [],
      selected: 'all',
      nowTodos: [],
      spawnAllowed: false,
    });
    expect(screen.queryByRole('button', { name: '메시지' })).toBeNull();
    cleanup();
    renderWithStore(<AgentsPane />, {
      agents: { available: true, list: [done] },
      loadAgents,
      boards: [],
      selected: 'all',
      nowTodos: [],
      spawnAllowed: true,
    });
    expect(screen.queryByRole('button', { name: '메시지' })).toBeNull();
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

describe('피드의 답을 기다리는 에이전트', () => {
  test('행을 누르면 에이전트 탭으로, 탭을 끄면 행도 없다', async () => {
    const setView = mock(() => {});
    renderWithStore(<MineSection />, {
      agents: { available: true, list: [blocked] },
      showAgents: true,
      setView,
    });
    await userEvent.click(screen.getByRole('button', { name: /룰셋을 끌지 정해 주세요/ }));
    expect(setView).toHaveBeenCalledWith('agents');
    cleanup();
    renderWithStore(<MineSection />, {
      agents: { available: true, list: [blocked] },
      showAgents: false,
    });
    expect(screen.queryByText('룰셋을 끌지 정해 주세요')).toBeNull();
  });
});
