import { afterEach, beforeEach, describe, expect, test } from 'bun:test';
import { cleanup, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { boardFixture, renderWithStore, todoFixture } from '../test-support';
import { BOARD_COLLAPSED_KEY, TodoPane } from './TodoPane';

afterEach(cleanup);
beforeEach(() => localStorage.removeItem(BOARD_COLLAPSED_KEY));

const boards = [
  boardFixture({ id: 'b1', key: 'rocky', title: 'rocky' }),
  boardFixture({ id: 'b2', key: 'mdwire', title: 'mdwire' }),
];
const todos = [
  todoFixture({ id: 't1', boardId: 'b1', title: 'rocky 할 일' }),
  todoFixture({ id: 't2', boardId: 'b1', title: 'rocky 하위', parentId: 't1' }),
  todoFixture({ id: 't3', boardId: 'b2', title: 'mdwire 할 일' }),
];

describe('TodoPane — 전체 보기의 보드 접기', () => {
  test('보드 머리를 누르면 그 보드만 접히고, 다시 열어도 기억한다', async () => {
    renderWithStore(<TodoPane />, { selected: 'all', boards, todos, sections: [] });
    const head = screen.getByRole('button', { name: /^rocky\s*\d+$/ });
    expect(head.getAttribute('aria-expanded')).toBe('true');
    // 하위 항목까지 센다
    expect(head.textContent).toContain('2');

    await userEvent.click(head);
    expect(head.getAttribute('aria-expanded')).toBe('false');
    expect(screen.queryByText('rocky 할 일')).toBeNull();
    expect(screen.queryByText('rocky 하위')).toBeNull();
    expect(screen.getByText('mdwire 할 일')).toBeDefined();
    expect(JSON.parse(localStorage.getItem(BOARD_COLLAPSED_KEY) ?? '[]')).toEqual(['b1']);

    cleanup();
    renderWithStore(<TodoPane />, { selected: 'all', boards, todos, sections: [] });
    expect(screen.queryByText('rocky 할 일')).toBeNull();
    await userEvent.click(screen.getByRole('button', { name: /^rocky\s*\d+$/ }));
    expect(screen.getByText('rocky 할 일')).toBeDefined();
  });

  test('한 보드를 볼 때는 접지 않는다 — 섹션 머리는 버튼이 아니다', () => {
    localStorage.setItem(BOARD_COLLAPSED_KEY, JSON.stringify(['b1']));
    renderWithStore(<TodoPane />, {
      selected: 'rocky',
      boards,
      todos: todos.filter((t) => t.boardId === 'b1'),
      sections: [],
    });
    expect(screen.getByText('rocky 할 일')).toBeDefined();
    expect(screen.queryByRole('button', { name: /일반/ })).toBeNull();
  });
});
