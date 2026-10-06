import { afterEach, describe, expect, test } from 'bun:test';
import { cleanup, screen, waitFor } from '@testing-library/react';
import { renderWithStore, todoFixture } from '../../test-support';
import type { HandoffView, SessionRow } from '../../types';
import { SessionLinks } from './SessionLinks';

afterEach(cleanup);

const realFetch = globalThis.fetch;
afterEach(() => {
  globalThis.fetch = realFetch;
});

/** 경로별 가짜 응답 — 부른 경로를 기록한다. */
function serve(routes: Record<string, unknown>): string[] {
  const calls: string[] = [];
  globalThis.fetch = (async (input: string) => {
    calls.push(input);
    const hit = routes[input.split('?')[0] ?? ''];
    return hit === undefined
      ? new Response(JSON.stringify({ error: 'no route' }), { status: 404 })
      : new Response(JSON.stringify(hit));
  }) as unknown as typeof fetch;
  return calls;
}

function handoff(over: Partial<HandoffView>): HandoffView {
  return {
    id: 'h1',
    todoId: 'todo1',
    sessionId: 'sess-aaaa-1111',
    sessionName: 'eelpout-a3',
    note: '',
    actor: 'logan',
    status: 'delivered',
    createdAt: '2026-10-06T01:00:00.000Z',
    phase: 'completed',
    unstarted: false,
    stale: false,
    ...over,
  };
}

function session(over: Partial<SessionRow>): SessionRow {
  return {
    cwd: '/repo/.claude/worktrees/todo-1',
    kind: 'interactive',
    sessionId: 'sess-aaaa-1111',
    name: 'eelpout-a3',
    status: 'busy',
    startedAt: 0,
    matched: false,
    ...over,
  };
}

const doing = todoFixture({ status: 'doing', doingSessionId: 'sess-aaaa-1111' });

describe('SessionLinks', () => {
  test('진행을 든 세션의 이름·상태·연결 방식과 보낸 기록을 보인다', async () => {
    const calls = serve({
      '/api/handoffs': [
        handoff({ id: 'h1', phase: 'accepted', createdAt: '2026-10-06T02:00:00.000Z' }),
        handoff({
          id: 'h0',
          sessionId: 'sess-old',
          sessionName: 'old-one',
          phase: 'cancelled',
          createdAt: '2026-10-05T02:00:00.000Z',
        }),
      ],
      '/api/sessions': { available: true, sessions: [session({})] },
    });
    renderWithStore(<SessionLinks todo={doing} />, { handoffs: [] });

    await waitFor(() => expect(screen.getByText('· 작업 중')).toBeDefined());
    expect(screen.getByText('· 보내서 받음')).toBeDefined();
    const rows = screen.getAllByRole('listitem').map((li) => li.textContent);
    // 진행 줄이 먼저, 기록은 최근 것부터
    expect(rows[0]).toContain('진행');
    expect(rows[1]).toContain('착수');
    expect(rows[2]).toContain('old-one');
    expect(rows[2]).toContain('취소');
    expect(calls).toContain('/api/handoffs?todo=todo1');
  });

  test('세션 목록을 못 얻으면 상태를 단정하지 않는다 — 이름만', async () => {
    serve({
      '/api/handoffs': [],
      '/api/sessions': { available: false, sessions: [] },
    });
    renderWithStore(<SessionLinks todo={{ ...doing, doingSessionClaimed: true }} />, {
      handoffs: [],
    });
    await waitFor(() => expect(screen.getByText('· 스스로 착수')).toBeDefined());
    expect(screen.queryByText('· 세션 없음')).toBeNull();
    expect(screen.getByText('sess-aaa')).toBeDefined();
  });

  test('목록에 없는 세션은 "세션 없음"', async () => {
    serve({
      '/api/handoffs': [],
      '/api/sessions': { available: true, sessions: [session({ sessionId: 'other' })] },
    });
    renderWithStore(<SessionLinks todo={doing} />, { handoffs: [] });
    await waitFor(() => expect(screen.getByText('· 세션 없음')).toBeDefined());
  });

  test('진행 세션도 보낸 기록도 없으면 그리지 않고, 세션 목록도 묻지 않는다', async () => {
    const calls = serve({ '/api/handoffs': [] });
    renderWithStore(<SessionLinks todo={todoFixture()} />, { handoffs: [] });
    await waitFor(() => expect(calls).toContain('/api/handoffs?todo=todo1'));
    expect(screen.queryByText('세션')).toBeNull();
    expect(calls.some((c) => c.startsWith('/api/sessions'))).toBe(false);
  });

  test('기록이 많으면 최근 다섯 건만 보이고 나머지는 접는다', async () => {
    serve({
      '/api/handoffs': Array.from({ length: 7 }, (_, i) =>
        handoff({ id: `h${i}`, createdAt: `2026-10-0${i + 1}T00:00:00.000Z` }),
      ),
    });
    renderWithStore(<SessionLinks todo={todoFixture()} />, { handoffs: [] });
    await waitFor(() => expect(screen.getByText('외 2건')).toBeDefined());
    expect(screen.getAllByText('eelpout-a3')).toHaveLength(5);
  });
});
