import { afterEach, describe, expect, test } from 'bun:test';
import { cleanup, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { renderWithStore } from '../test-support';
import { BoardInbox } from './BoardInbox';

afterEach(cleanup);

const realFetch = globalThis.fetch;
afterEach(() => {
  globalThis.fetch = realFetch;
});

type Call = { path: string; method: string; body?: unknown };

/** 경로별 가짜 응답 — 부른 것을 기록한다. */
function serve(routes: Record<string, { status?: number; body: unknown }>): Call[] {
  const calls: Call[] = [];
  globalThis.fetch = (async (input: string, init?: RequestInit) => {
    const method = init?.method ?? 'GET';
    calls.push({
      path: input,
      method,
      body: init?.body ? JSON.parse(String(init.body)) : undefined,
    });
    const key = `${method} ${input.split('?')[0]}`;
    const hit = routes[key] ?? { status: 404, body: { error: `no route ${key}` } };
    return new Response(JSON.stringify(hit.body), { status: hit.status ?? 200 });
  }) as unknown as typeof fetch;
  return calls;
}

const inbox = {
  sources: [
    {
      name: 'gh-bugs',
      board: 'web',
      available: true,
      items: [
        { id: '1', title: '올린 것', url: 'https://x/1', promoted: true },
        { id: '2', title: '새 버그', url: 'https://x/2' },
      ],
    },
    { name: 'todoist', available: false, reason: 'exit 1: 토큰 없음\n자세히', items: [] },
  ],
};

describe('BoardInbox', () => {
  test('안 올린 항목만 보이고, 실패한 소스는 사유 한 줄', async () => {
    serve({ 'GET /api/inbox': { body: inbox } });
    renderWithStore(<BoardInbox board="web" />, { actor: 'me' });
    expect(await screen.findByText('새 버그')).toBeTruthy();
    expect(screen.queryByText('올린 것')).toBeNull();
    expect(screen.getByText('수집함 1')).toBeTruthy();
    expect(screen.getByText('todoist 실패: exit 1: 토큰 없음')).toBeTruthy();
  });

  test('설정 — 어댑터가 알려 준 칸만 그리고 그 칸만 보낸다', async () => {
    const calls = serve({
      'GET /api/inbox': { body: { sources: [] } },
      'GET /api/inbox/sources': { body: [] },
      'GET /api/inbox/adapters': {
        body: [
          {
            name: 'gh-project',
            title: 'GitHub 프로젝트 보드',
            params: [
              { flag: '--project', label: '보드', required: true },
              { flag: '--filter', label: '보드 필터', required: false },
            ],
          },
        ],
      },
      'POST /api/inbox/sources': { body: { id: 's1' } },
    });
    renderWithStore(<BoardInbox board="web" />, { actor: 'me' });
    await userEvent.click(await screen.findByRole('button', { name: '설정' }));
    await userEvent.type(await screen.findByRole('textbox', { name: '이름' }), 'gh-bugs');
    await userEvent.type(screen.getByRole('textbox', { name: '보드' }), 'acme/7');
    await userEvent.type(screen.getByRole('textbox', { name: '보드 필터' }), 'type:Bug');
    await userEvent.click(screen.getByRole('button', { name: '등록' }));
    await waitFor(() => expect(calls.some((c) => c.method === 'POST')).toBe(true));
    expect(calls.find((c) => c.method === 'POST')?.body).toEqual({
      board: 'web',
      name: 'gh-bugs',
      adapter: 'gh-project',
      params: { '--project': 'acme/7', '--filter': 'type:Bug' },
    });
  });

  test('노출된 화면이면 서버의 거절 사유를 보여 주고 폼은 그리지 않는다', async () => {
    serve({
      'GET /api/inbox': { body: { sources: [] } },
      'GET /api/inbox/sources': { body: [] },
      'GET /api/inbox/adapters': { status: 403, body: { error: '보드 수집함 설정은 로컬' } },
    });
    renderWithStore(<BoardInbox board="web" />, { actor: 'me' });
    await userEvent.click(await screen.findByRole('button', { name: '설정' }));
    expect((await screen.findByRole('alert')).textContent).toContain('로컬');
    expect(screen.queryByRole('button', { name: '등록' })).toBeNull();
  });
});
