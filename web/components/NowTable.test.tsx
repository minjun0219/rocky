import { afterEach, describe, expect, mock, test } from 'bun:test';
import { act, cleanup, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { renderWithStore, todoFixture } from '../test-support';
import { NowTable } from './NowTable';

afterEach(cleanup);

describe('NowTable', () => {
  test('아무것도 없으면 "내 차례 없음" 한 줄, 돌고 있음 묶음은 숨긴다', () => {
    renderWithStore(<NowTable />, { nowTodos: [], nowHandoffs: [], collect: null });
    expect(screen.getByText('내 차례 없음')).toBeTruthy();
    expect(screen.queryByText('돌고 있음')).toBeNull();
    expect(document.querySelector('table')).toBeNull();
  });

  test('내 차례와 돌고 있음을 나눠 싣는다 — 글리프로 상태, 누가는 글자, 눌러서 상세', async () => {
    const openTodoDetail = mock(async () => {});
    renderWithStore(<NowTable />, {
      nowTodos: [
        todoFixture({
          id: 'gone',
          ref: 'acorn-server-28',
          title: '검증 실패 응답 통일',
          status: 'doing',
          doingBy: 'claude-code',
          doingSince: '2026-08-18T00:00:00.000Z',
          doingState: 'gone',
        }),
        todoFixture({
          id: 'live',
          ref: 'tally-11',
          title: '현대카드 수집',
          status: 'doing',
          doingBy: 'logan',
          doingSince: '2026-09-28T02:00:00.000Z',
          doingState: 'live',
        }),
      ],
      nowHandoffs: [],
      collect: 2,
      openTodoDetail,
    });
    const [mine, run] = [...document.querySelectorAll('ul')];
    const mineRows = [...(mine?.querySelectorAll('li') ?? [])];
    expect(mineRows).toHaveLength(2);
    expect(mineRows[0]?.textContent).toContain('acorn-server-28');
    expect(mineRows[0]?.textContent).toContain('AGENT');
    expect(mineRows[0]?.textContent).toContain('세션 없음');
    // 세션 없음은 색만이 아니라 모양(점선 원)으로도 말한다.
    expect(mineRows[0]?.querySelector('[aria-label="세션 없음"] svg')).toBeTruthy();
    // 오래된 진행중은 초를 굴리지 않고 날짜로 — "41일 03:12:44" 가 아니다.
    expect(mineRows[0]?.textContent).toContain('8월 18일부터');
    expect(mineRows[1]?.textContent).toContain('수집함');
    const runRows = [...(run?.querySelectorAll('li') ?? [])];
    expect(runRows).toHaveLength(1);
    expect(runRows[0]?.textContent).toContain('YOU');
    expect(runRows[0]?.querySelector('[aria-label="돌고 있음"] svg')).toBeTruthy();
    // 같은 상태를 행마다 배지로 반복하지 않고, 묶음 머리에 개수로 한 번.
    expect(screen.getByRole('heading', { name: /내 차례\s*2/ })).toBeTruthy();
    expect(screen.getByRole('heading', { name: /돌고 있음\s*1/ })).toBeTruthy();
    await userEvent.click(screen.getByRole('button', { name: /검증 실패 응답 통일/ }));
    expect(openTodoDetail).toHaveBeenCalledWith('gone');
  });

  test('내 차례가 5행을 넘으면 "N개 더" — 누르면 펼친다', async () => {
    const nowTodos = Array.from({ length: 7 }, (_, i) =>
      todoFixture({
        id: `g${i}`,
        ref: `g-${i}`,
        title: `멈춘 일 ${i}`,
        status: 'doing',
        doingBy: 'claude-code',
        doingSince: `2026-09-2${i}T00:00:00.000Z`,
        doingState: 'gone',
      }),
    );
    renderWithStore(<NowTable />, { nowTodos, nowHandoffs: [], collect: null });
    expect(screen.queryByText('멈춘 일 6')).toBeNull();
    // 머리의 개수는 접힘과 무관하게 전체 — 펼치기 전에도 7.
    expect(screen.getByRole('heading', { name: /내 차례\s*7/ })).toBeTruthy();
    await userEvent.click(screen.getByRole('button', { name: '내 차례 2개 더' }));
    expect(screen.getByText('멈춘 일 6')).toBeTruthy();
    expect(screen.getByRole('heading', { name: /내 차례\s*7/ })).toBeTruthy();
  });
});

describe('NowTable — PR 현황', () => {
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
    renderWithStore(<NowTable />, {
      nowTodos: [],
      nowHandoffs: [],
      collect: null,
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
describe('NowTable — PR 만 있을 때도 시각이 흐른다', () => {
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
      renderWithStore(<NowTable />, {
        nowTodos: [],
        nowHandoffs: [],
        collect: null,
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
