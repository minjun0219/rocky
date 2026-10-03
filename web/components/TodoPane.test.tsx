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

  test('완료된 항목은 하단에 접혀 있고 토글 버튼으로 펼치거나 접을 수 있다', async () => {
    const doneTodo = todoFixture({
      id: 't-done',
      boardId: 'b1',
      title: '완료된 작업 항목',
      status: 'done',
    });
    renderWithStore(<TodoPane />, {
      selected: 'rocky',
      boards,
      todos: [...todos.filter((t) => t.boardId === 'b1'), doneTodo],
      sections: [],
    });

    // 미완료 항목은 바로 보이지만 완료 항목은 초기에는 접힘
    expect(screen.getByText('rocky 할 일')).toBeDefined();
    expect(screen.queryByText('완료된 작업 항목')).toBeNull();

    // 완료 토글 버튼 확인 및 클릭하여 펼치기
    const toggleButton = screen.getByRole('button', { name: /완료된 작업 1개/ });
    expect(toggleButton.getAttribute('aria-expanded')).toBe('false');

    await userEvent.click(toggleButton);
    expect(toggleButton.getAttribute('aria-expanded')).toBe('true');
    expect(screen.getByText('완료된 작업 항목')).toBeDefined();

    // 다시 클릭하여 접기
    await userEvent.click(toggleButton);
    expect(toggleButton.getAttribute('aria-expanded')).toBe('false');
    expect(screen.queryByText('완료된 작업 항목')).toBeNull();
  });

  test('완료된 부모 아래에 열린 하위 작업이 있으면 부모와 하위 작업이 숨지 않고 활성 목록에 표시된다', () => {
    // 부모는 done이지만 자식은 todo
    const parentDone = todoFixture({
      id: 'p-done',
      boardId: 'b1',
      title: '완료 처리된 부모 작업',
      status: 'done',
    });
    const childOpen = todoFixture({
      id: 'c-open',
      boardId: 'b1',
      title: '아직 진행 중인 하위 작업',
      parentId: 'p-done',
      status: 'todo',
    });

    renderWithStore(<TodoPane />, {
      selected: 'rocky',
      boards,
      todos: [parentDone, childOpen],
      sections: [],
    });

    // 열린 하위 작업이 있으므로 부모와 자식 모두 활성 목록에 표시되어야 함
    expect(screen.getByText('완료 처리된 부모 작업')).toBeDefined();
    expect(screen.getByText('아직 진행 중인 하위 작업')).toBeDefined();
  });

  test('열린 부모 아래의 완료된 하위 작업도 완료 토글 상태에 따라 접히고 펼쳐진다', async () => {
    // 부모는 todo, 자식은 done
    const parentOpen = todoFixture({
      id: 'p-open',
      boardId: 'b1',
      title: '열린 부모 작업',
      status: 'todo',
    });
    const childDone = todoFixture({
      id: 'c-done',
      boardId: 'b1',
      title: '완료된 하위 작업',
      parentId: 'p-open',
      status: 'done',
    });

    renderWithStore(<TodoPane />, {
      selected: 'rocky',
      boards,
      todos: [parentOpen, childDone],
      sections: [],
    });

    // 부모는 보이고, 완료된 하위 작업은 기본으로 접힘
    expect(screen.getByText('열린 부모 작업')).toBeDefined();
    expect(screen.queryByText('완료된 하위 작업')).toBeNull();

    // 완료 토글 버튼(완료된 작업 1개) 누르면 하위 완료 작업 표시
    const toggleButton = screen.getByRole('button', { name: /완료된 작업 1개/ });
    await userEvent.click(toggleButton);
    expect(screen.getByText('완료된 하위 작업')).toBeDefined();

    // 다시 누르면 접힘
    await userEvent.click(toggleButton);
    expect(screen.queryByText('완료된 하위 작업')).toBeNull();
  });

  test('섹션이 여럿일 때 한 섹션의 완료 토글을 펼쳐도 다른 섹션은 함께 펼쳐지지 않는다', async () => {
    const sec1 = { id: 's1', boardId: 'b1', title: '백로그', position: 0 };
    const sec2 = { id: 's2', boardId: 'b1', title: '진행중', position: 1 };
    const done1 = todoFixture({
      id: 't-d1',
      boardId: 'b1',
      sectionId: 's1',
      title: '백로그 완료 작업',
      status: 'done',
    });
    const done2 = todoFixture({
      id: 't-d2',
      boardId: 'b1',
      sectionId: 's2',
      title: '진행중 완료 작업',
      status: 'done',
    });

    renderWithStore(<TodoPane />, {
      selected: 'rocky',
      boards,
      todos: [done1, done2],
      sections: [sec1, sec2],
    });

    // 두 섹션 모두 초기에는 완료 작업이 접혀 있음
    expect(screen.queryByText('백로그 완료 작업')).toBeNull();
    expect(screen.queryByText('진행중 완료 작업')).toBeNull();

    // 두 개의 '완료된 작업 1개' 토글 버튼
    const buttons = screen.getAllByRole('button', { name: /완료된 작업 1개/ });
    expect(buttons.length).toBe(2);
    const btn0 = buttons[0]!;
    const btn1 = buttons[1]!;

    // 첫 번째 섹션(백로그)만 클릭
    await userEvent.click(btn0);
    expect(btn0.getAttribute('aria-expanded')).toBe('true');
    expect(btn1.getAttribute('aria-expanded')).toBe('false');

    // 백로그 완료 작업은 보이고, 진행중 완료 작업은 여전히 접혀 있어야 함
    expect(screen.getByText('백로그 완료 작업')).toBeDefined();
    expect(screen.queryByText('진행중 완료 작업')).toBeNull();
  });
});
