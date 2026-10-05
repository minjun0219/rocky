import { afterEach, describe, expect, mock, test } from 'bun:test';
import { cleanup, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { renderWithStore, todoFixture } from '../test-support';
import { MineSection, NowTable } from './NowTable';

afterEach(cleanup);

describe('NowTable', () => {
  test('아무것도 없으면 "내 차례 없음" 한 줄, 돌고 있음 묶음은 숨긴다', () => {
    renderWithStore(
      <>
        <MineSection />
        <NowTable />
      </>,
      { nowTodos: [], nowHandoffs: [], collect: null },
    );
    expect(screen.getByText('내 차례 없음')).toBeTruthy();
    expect(screen.queryByText('실행 중')).toBeNull();
    expect(document.querySelector('table')).toBeNull();
  });

  test('내 차례와 돌고 있음을 나눠 싣는다 — 글리프로 상태, 누가는 글자, 눌러서 상세', async () => {
    const openTodoDetail = mock(async () => {});
    // 내 차례는 피드에, 돌고 있음은 할 일 화면 맨 위에 — 둘을 함께 그려 한 번에 본다.
    renderWithStore(
      <>
        <MineSection />
        <NowTable />
      </>,
      {
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
      },
    );
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
    expect(runRows[0]?.querySelector('[aria-label="실행 중"] svg')).toBeTruthy();
    // 같은 상태를 행마다 배지로 반복하지 않고, 묶음 머리에 개수로 한 번.
    expect(screen.getByRole('heading', { name: /내 차례\s*2/ })).toBeTruthy();
    expect(screen.getByRole('heading', { name: /실행 중\s*1/ })).toBeTruthy();
    await userEvent.click(screen.getByRole('button', { name: /검증 실패 응답 통일/ }));
    expect(openTodoDetail).toHaveBeenCalledWith('gone');
  });

  test('수집함 행은 출처를 말하고, 누르면 항목이 원래 앱 링크로 펼쳐진다', async () => {
    renderWithStore(<MineSection />, {
      nowTodos: [],
      nowHandoffs: [],
      collect: 4,
      collectItems: [
        { source: 'todoist', title: '페르소나 힌트', url: 'https://app.todoist.com/app/task/a' },
        { source: 'todoist', title: '문제 유형 옵션', url: 'https://app.todoist.com/app/task/b' },
      ],
    });
    const row = screen.getByRole('button', {
      name: /todoist 수집함에 보드로 옮기지 않은 항목 4건/,
    });
    expect(row.getAttribute('aria-expanded')).toBe('false');
    await userEvent.click(row);
    expect(row.getAttribute('aria-expanded')).toBe('true');
    const link = screen.getByRole('link', { name: /페르소나 힌트/ });
    expect(link.getAttribute('href')).toBe('https://app.todoist.com/app/task/a');
    // 요약은 3개까지만 싣는다 — 넘친 수는 한 줄로. 펼쳐도 내 차례 개수는 수집함 행 하나.
    expect(screen.getByText(/외 2건/)).toBeTruthy();
    expect(screen.getByRole('heading', { name: /내 차례\s*1/ })).toBeTruthy();
    await userEvent.click(row);
    expect(screen.queryByRole('link', { name: /페르소나 힌트/ })).toBeNull();
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
    renderWithStore(<MineSection />, { nowTodos, nowHandoffs: [], collect: null });
    expect(screen.queryByText('멈춘 일 6')).toBeNull();
    // 머리의 개수는 접힘과 무관하게 전체 — 펼치기 전에도 7.
    expect(screen.getByRole('heading', { name: /내 차례\s*7/ })).toBeTruthy();
    await userEvent.click(screen.getByRole('button', { name: '내 차례 2개 더' }));
    expect(screen.getByText('멈춘 일 6')).toBeTruthy();
    expect(screen.getByRole('heading', { name: /내 차례\s*7/ })).toBeTruthy();
  });
});

describe('NowTable — GitHub 은 탭으로', () => {
  test('열린 PR 이 있어도 지금 표에는 PR 이 없다', () => {
    renderWithStore(<NowTable />, {
      nowTodos: [],
      nowHandoffs: [],
      collect: null,
      selected: 'all',
      boards: [],
      prs: [
        {
          repo: 'o/rocky',
          number: 8,
          title: '머지 가능 PR',
          url: 'https://github.com/o/rocky/pull/8',
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
        },
      ] as never,
    });
    expect(screen.queryByText('머지 가능 PR')).toBeNull();
    expect(screen.queryByRole('heading', { name: /^PR/ })).toBeNull();
  });
});
