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

  test('/clear 된 세션을 보이고, 고른 갈래를 데몬에 보낸 뒤 다시 읽는다', async () => {
    const calls: { path: string; method: string; body?: unknown }[] = [];
    let decided = false;
    globalThis.fetch = (async (input: string, init?: RequestInit) => {
      calls.push({
        path: input,
        method: init?.method ?? 'GET',
        body: init?.body ? JSON.parse(String(init.body)) : undefined,
      });
      if (input.startsWith('/api/sessions/cleared')) {
        decided = true;
        return new Response(JSON.stringify({ sessionId: 'oldsession1', changed: 2 }), {
          status: 200,
        });
      }
      const body = {
        sessions: [],
        subscriptions: [],
        recent: [],
        cleared: decided
          ? []
          : [
              {
                sessionId: 'oldsession1',
                successorId: 'newsession2',
                cwd: '/w/rocky',
                clearedAt: new Date().toISOString(),
                prs: ['o/r#7'],
                filters: ['author:@me'],
                inbox: ['gh-bugs'],
              },
            ],
      };
      return new Response(JSON.stringify(body), { status: 200 });
    }) as unknown as typeof fetch;
    renderWithStore(<SessionDelivery />, { actor: 'me' });
    expect(await screen.findByText(/\/clear 된 세션 1/)).toBeTruthy();
    expect(screen.getByText('PR o/r#7 · 필터 author:@me · 수집함 gh-bugs')).toBeTruthy();
    expect(screen.getByText('newsessi')).toBeTruthy();
    expect(screen.getByRole('button', { name: '지켜보기만' })).toBeTruthy();
    expect(screen.getByRole('button', { name: '구독 해지' })).toBeTruthy();
    await userEvent.click(screen.getByRole('button', { name: '새 세션으로 넘기기' }));
    await waitFor(() => expect(screen.queryByText(/\/clear 된 세션/)).toBeNull());
    expect(calls.find((c) => c.method === 'POST')).toEqual({
      path: '/api/sessions/cleared',
      method: 'POST',
      body: { sessionId: 'oldsession1', action: 'handover' },
    });
  });

  test('끝난 세션을 뺐으면 몇 개인지 한 줄로 알린다', async () => {
    globalThis.fetch = (async () =>
      new Response(
        JSON.stringify({ sessions: [], ended: 3, subscriptions: [], recent: [] }),
      )) as unknown as typeof fetch;
    renderWithStore(<SessionDelivery />, { actor: 'me' });
    expect(await screen.findByText(/끝난 세션 3개는 뺐어요/)).toBeTruthy();
  });

  test('옛 데몬(cleared 없음)이면 그 칸을 그리지 않는다', async () => {
    globalThis.fetch = (async () =>
      new Response(JSON.stringify({ sessions: [], subscriptions: [], recent: [] }), {
        status: 200,
      })) as unknown as typeof fetch;
    renderWithStore(<SessionDelivery />, { actor: 'me' });
    expect(await screen.findByText(/받은편지함을 등록한 세션이 없어요/)).toBeTruthy();
    expect(screen.queryByText(/\/clear 된 세션/)).toBeNull();
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
