import { afterEach, describe, expect, test } from 'bun:test';
import { cleanup, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { renderWithStore } from '../test-support';
import { SessionDelivery } from './SessionDelivery';

afterEach(cleanup);
const realFetch = globalThis.fetch;
afterEach(() => {
  globalThis.fetch = realFetch;
});

describe('SessionDelivery', () => {
  test('받는 세션·최근 기록을 보이고, 보내지 않기를 누르면 데몬에 알린다', async () => {
    const calls: { path: string; method: string; body?: unknown }[] = [];
    globalThis.fetch = (async (input: string, init?: RequestInit) => {
      calls.push({
        path: input,
        method: init?.method ?? 'GET',
        body: init?.body ? JSON.parse(String(init.body)) : undefined,
      });
      const body = input.startsWith('/api/deliveries/mute')
        ? { ok: true }
        : {
            sessions: [
              {
                sessionId: 'abcdef1234',
                cwd: '/w/rocky',
                board: 'rocky',
                muted: false,
                receivesPrFor: ['rocky'],
              },
            ],
            subscriptions: [{ source: 'gh-bugs', sessionId: 'abcdef1234' }],
            recent: [
              {
                at: new Date().toISOString(),
                kind: 'pr-conflict',
                subject: 'o/r#9 PR 9',
                url: 'https://x/9',
                sessionId: 'abcdef1234',
                ok: true,
              },
            ],
          };
      return new Response(JSON.stringify(body), { status: 200 });
    }) as unknown as typeof fetch;
    renderWithStore(<SessionDelivery />, { actor: 'me' });
    expect(await screen.findByText(/PR 알림 받는 중\(rocky\)/)).toBeTruthy();
    expect(screen.getByText(/충돌/)).toBeTruthy();
    expect(screen.getByRole('button', { name: '구독 해지' })).toBeTruthy();
    await userEvent.click(screen.getByRole('button', { name: '보내지 않기' }));
    await waitFor(() => expect(calls.some((c) => c.method === 'POST')).toBe(true));
    expect(calls.find((c) => c.method === 'POST')?.body).toEqual({
      sessionId: 'abcdef1234',
      muted: true,
    });
  });

  test('노출된 화면이면 서버의 거절 사유를 보인다', async () => {
    globalThis.fetch = (async () =>
      new Response(JSON.stringify({ error: '로컬 요청만' }), {
        status: 403,
      })) as unknown as typeof fetch;
    renderWithStore(<SessionDelivery />, { actor: 'me' });
    expect((await screen.findByRole('alert')).textContent).toContain('로컬');
  });
});
