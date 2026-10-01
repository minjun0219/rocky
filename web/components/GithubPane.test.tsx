import { afterEach, beforeEach, describe, expect, test } from 'bun:test';
import { act, cleanup, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { renderWithStore } from '../test-support';
import { GithubPane } from './GithubPane';
import { ViewSwitch } from './ViewSwitch';

afterEach(cleanup);

// 수집함(`/api/inbox`)·세션 전달(`/api/deliveries`)이 부른다 — 여기선 경로별 빈 응답으로.
const realFetch = globalThis.fetch;
beforeEach(() => {
  globalThis.fetch = (async (input: string) => {
    const body = String(input).startsWith('/api/deliveries')
      ? { sessions: [], subscriptions: [], recent: [] }
      : String(input).startsWith('/api/inbox/subscriptions')
        ? []
        : { sources: [] };
    return new Response(JSON.stringify(body), { status: 200 });
  }) as unknown as typeof fetch;
});
afterEach(() => {
  globalThis.fetch = realFetch;
});

describe('GithubPane — PR 현황', () => {
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
    updatedAt: '2026-09-28T10:00:00Z',
    ...over,
  });

  test('보고 있는 보드 레포의 열린 PR 전부 — 아이콘으로 상태, 누르면 GitHub', () => {
    renderWithStore(<GithubPane />, {
      nowTodos: [],
      nowHandoffs: [],
      collect: null,
      githubHidden: [],
      selected: 'rocky',
      boards: [
        {
          id: 'b1',
          key: 'rocky',
          title: 'rocky',
          repo: 'o/rocky',
          createdAt: '2026-09-01T00:00:00Z',
          updatedAt: '2026-09-01T00:00:00Z',
        } as never,
      ],
      prs: [
        pr({ number: 7, title: '대기 중인 PR', unhandled: 1 }),
        pr({ number: 8, title: '머지 가능 PR', ready: true, ci: 'pass' }),
        pr({ number: 9, title: '다른 레포', repo: 'o/tally' }),
      ] as never,
    });
    expect(screen.getByRole('heading', { name: /PR\s*2/ })).toBeTruthy();
    const link = screen.getByRole('link', { name: /대기 중인 PR/ });
    expect(link.getAttribute('href')).toBe('https://github.com/o/rocky/pull/1');
    expect(link.textContent).toContain('#7');
    expect(link.textContent).toContain('CI 도는 중 · 스레드 1');
    expect(link.querySelector('[aria-label="대기"] svg')).toBeTruthy();
    expect(screen.queryByText('다른 레포')).toBeNull();
  });
});

// Codex 지적 회귀 — PR 행만 있는 보드에서도 시각이 멈추지 않는다(1분 틱이 돈다).
describe('GithubPane — PR 만 있을 때도 시각이 흐른다', () => {
  test('1분 뒤 "방금" 이 "1분" 으로', async () => {
    const realNow = Date.now;
    const realSetInterval = globalThis.setInterval;
    let tick: (() => void) | null = null;
    let clock = Date.parse('2026-09-28T10:00:30Z');
    Date.now = () => clock;
    globalThis.setInterval = ((fn: () => void) => {
      tick = fn;
      return 1 as unknown as ReturnType<typeof setInterval>;
    }) as typeof setInterval;
    try {
      renderWithStore(<GithubPane />, {
        nowTodos: [],
        nowHandoffs: [],
        collect: null,
        githubHidden: [],
        selected: 'all',
        boards: [],
        prs: [
          {
            repo: 'o/rocky',
            number: 7,
            title: '대기 중인 PR',
            url: 'https://github.com/o/rocky/pull/7',
            state: 'OPEN',
            isDraft: false,
            base: 'main',
            head: 'abc',
            mergeState: 'BLOCKED',
            ci: 'pending',
            unhandled: 0,
            decision: 0,
            ready: false,
            updatedAt: '2026-09-28T10:00:00Z',
          },
        ] as never,
      });
      expect(screen.getByRole('link', { name: /대기 중인 PR/ }).textContent).toContain('방금');
      expect(tick).not.toBeNull();
      clock += 60_000;
      await act(async () => {
        tick?.();
      });
      expect(screen.getByRole('link', { name: /대기 중인 PR/ }).textContent).toContain('1분');
    } finally {
      Date.now = realNow;
      globalThis.setInterval = realSetInterval;
    }
  });
});

describe('GithubPane — 숨기기', () => {
  const board = {
    id: 'b1',
    key: 'rocky',
    title: 'rocky',
    repo: 'o/rocky',
    createdAt: '2026-09-01T00:00:00Z',
    updatedAt: '2026-09-01T00:00:00Z',
  } as never;
  const pr = (over: Record<string, unknown>) => ({
    repo: 'o/rocky',
    number: 1,
    title: 'PR',
    url: 'https://github.com/o/rocky/pull/1',
    state: 'OPEN',
    isDraft: false,
    base: 'main',
    head: 'abc',
    mergeState: 'CLEAN',
    ci: 'pass',
    unhandled: 0,
    decision: 0,
    ready: true,
    updatedAt: '2026-09-28T10:00:00Z',
    ...over,
  });

  test('× 로 숨기고, 숨긴 N건 · 다시 보이기 로 되돌린다', async () => {
    renderWithStore(<GithubPane />, {
      selected: 'rocky',
      boards: [board],
      githubHidden: [],
      prs: [pr({ number: 7, title: '숨길 PR' })] as never,
    });
    await userEvent.click(screen.getByRole('button', { name: '#7 숨기기' }));
    expect(screen.queryByText('숨길 PR')).toBeNull();
    expect(screen.getByText(/숨긴 항목 1건/)).toBeTruthy();
    await userEvent.click(screen.getByRole('button', { name: '다시 보이기' }));
    expect(screen.getByText('숨길 PR')).toBeTruthy();
  });

  test('숨긴 PR 도 상태가 바뀌면 다시 보인다', () => {
    renderWithStore(<GithubPane />, {
      selected: 'rocky',
      boards: [board],
      // "대기" 였을 때 숨겼다 — 지금은 머지 가능.
      githubHidden: ['pr:o/rocky#7:waiting'],
      prs: [pr({ number: 7, title: '다시 뜰 PR', ready: true })] as never,
    });
    expect(screen.getByText('다시 뜰 PR')).toBeTruthy();
  });

  test('GitHub 탭을 끄면 탭도 없다', () => {
    const state = { selected: 'rocky', boards: [board], notes: [], showGithub: false };
    renderWithStore(<ViewSwitch />, state);
    expect(screen.queryByRole('button', { name: /GitHub/ })).toBeNull();
  });
});

describe('GithubPane — 레포별 · 그 밖의 열린 PR', () => {
  const board = (key: string, repo: string) =>
    ({
      id: key,
      key,
      title: key,
      repo,
      createdAt: '2026-09-01T00:00:00Z',
      updatedAt: '2026-09-01T00:00:00Z',
    }) as never;

  test('전체 보기는 레포마다 묶고, 펼칠 때만 그 레포의 열린 PR 을 묻고, 지켜보기는 구독을 보낸다', async () => {
    const calls: { url: string; method: string; body?: string }[] = [];
    globalThis.fetch = (async (input: string, init?: RequestInit) => {
      const url = String(input);
      calls.push({ url, method: init?.method ?? 'GET', body: init?.body as string | undefined });
      if (url.startsWith('/api/prs/open')) {
        return new Response(
          JSON.stringify([
            {
              number: 7,
              title: '남의 PR',
              url: 'https://github.com/o/a/pull/7',
              isDraft: false,
              updatedAt: '2026-09-28T10:00:00Z',
              author: 'kim',
              subscribed: false,
            },
            {
              number: 8,
              title: '이미 구독',
              url: 'https://github.com/o/a/pull/8',
              isDraft: false,
              updatedAt: '2026-09-28T10:00:00Z',
              subscribed: true,
            },
          ]),
          { status: 200 },
        );
      }
      if (url.startsWith('/api/prs/subscriptions')) {
        return new Response(JSON.stringify({ repo: 'o/a', number: 7 }), { status: 201 });
      }
      const body = url.startsWith('/api/deliveries')
        ? { sessions: [], subscriptions: [], recent: [] }
        : { sources: [] };
      return new Response(JSON.stringify(body), { status: 200 });
    }) as unknown as typeof fetch;
    renderWithStore(<GithubPane />, {
      nowTodos: [],
      nowHandoffs: [],
      collect: null,
      githubHidden: [],
      selected: 'all',
      boards: [board('a', 'o/a'), board('b', 'o/b')],
      prs: [],
    });
    expect(screen.getByRole('region', { name: 'o/a' })).toBeTruthy();
    expect(screen.getByRole('region', { name: 'o/b' })).toBeTruthy();
    expect(
      calls.some((c) => c.url.startsWith('/api/prs/open')),
      '펼치기 전에는 묻지 않는다',
    ).toBe(false);

    const section = screen.getByRole('region', { name: 'o/a' });
    await userEvent.click(section.querySelector('button[aria-expanded]') as HTMLButtonElement);
    expect(await screen.findByText('남의 PR')).toBeTruthy();
    expect(screen.queryByText('이미 구독')).toBeNull();
    expect(calls.filter((c) => c.url === '/api/prs/open?repo=o%2Fa')).toHaveLength(1);

    await userEvent.click(screen.getByRole('button', { name: '지켜보기' }));
    const post = calls.find((c) => c.url === '/api/prs/subscriptions');
    expect(post?.method).toBe('POST');
    expect(JSON.parse(post?.body ?? '{}')).toEqual({ repo: 'o/a', number: 7 });
    expect(await screen.findByRole('button', { name: '지켜보는 중' })).toBeTruthy();
  });
});
