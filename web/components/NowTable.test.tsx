import { afterEach, describe, expect, mock, test } from 'bun:test';
import { cleanup, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { renderWithStore, todoFixture } from '../test-support';
import { NowTable } from './NowTable';

afterEach(cleanup);

describe('NowTable', () => {
  test('행이 없으면 한 줄 문장만', () => {
    renderWithStore(<NowTable />, { nowTodos: [], handoffs: [], collect: null });
    expect(screen.getByText('도는 일도, 내 차례도 없다.')).toBeTruthy();
    expect(document.querySelector('table')).toBeNull();
  });

  test('전 보드의 진행중을 표로 — 누가는 글자, 세션 없음은 점선, 눌러서 상세', async () => {
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
      handoffs: [],
      collect: 2,
      openTodoDetail,
    });
    const rows = [...document.querySelectorAll('tbody tr')];
    expect(rows).toHaveLength(3);
    expect(rows[0]?.textContent).toContain('acorn-server-28');
    expect(rows[0]?.textContent).toContain('AGENT');
    expect(rows[0]?.querySelector('.border-dashed')?.textContent).toBe('세션 없음');
    expect(rows[1]?.textContent).toContain('YOU');
    expect(rows[2]?.textContent).toContain('수집함');
    // 헤더의 "내 차례" 는 run 이 아닌 행 수.
    expect(screen.getByText('내 차례 2')).toBeTruthy();
    await userEvent.click(screen.getByRole('button', { name: /검증 실패 응답 통일/ }));
    expect(openTodoDetail).toHaveBeenCalledWith('gone');
  });
});
