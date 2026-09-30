import { afterEach, beforeEach, describe, expect, test } from 'bun:test';
import { act, cleanup, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { renderWithStore } from '../test-support';
import { GithubPane } from './GithubPane';
import { ViewSwitch } from './ViewSwitch';

afterEach(cleanup);

// 보드를 고르면 수집함이 `/api/inbox` 를 부른다 — 여기선 빈 응답으로.
const realFetch = globalThis.fetch;
beforeEach(() => {
  globalThis.fetch = (async () =>
    new Response(JSON.stringify({ sources: [] }), { status: 200 })) as unknown as typeof fetch;
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

  test('탭 옆 숫자는 움직일 PR 만 — 숨긴 것은 빼고, 탭을 끄면 탭도 없다', () => {
    const state = {
      selected: 'rocky',
      boards: [board],
      notes: [],
      githubHidden: ['pr:o/rocky#9:ready'],
      showGithub: true,
      prs: [
        pr({ number: 8, ready: true }),
        pr({ number: 9, ready: true }),
        pr({ number: 10, ready: false, mergeState: 'BLOCKED', ci: 'pending' }),
      ] as never,
    };
    renderWithStore(<ViewSwitch />, state);
    expect(screen.getByRole('button', { name: /GitHub\s*1/ })).toBeTruthy();
    cleanup();
    renderWithStore(<ViewSwitch />, { ...state, showGithub: false });
    expect(screen.queryByRole('button', { name: /GitHub/ })).toBeNull();
  });
});
