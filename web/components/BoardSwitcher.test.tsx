import { afterEach, describe, expect, mock, test } from 'bun:test';
import { cleanup, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { boardFixture, renderWithStore, todoFixture } from '../test-support';
import { BoardSwitcher } from './BoardSwitcher';

afterEach(cleanup);

/** 보드 하나가 있는 스위처를 띄운다. */
function mountSwitcher(over: Record<string, unknown> = {}) {
  const setSelected = mock(() => {});
  const createBoard = mock(async (_key: string) => {});
  renderWithStore(<BoardSwitcher />, {
    boards: [boardFixture()],
    nowTodos: [],
    selected: 'all',
    setSelected,
    createBoard,
    ...over,
  });
  return { setSelected, createBoard };
}

const trigger = () => screen.getByRole('button', { name: /^보드 — 지금/ });

describe('BoardSwitcher', () => {
  test('머리줄 버튼이 지금 보드를 말하고, 누르면 목록 — 고르면 닫힌다', async () => {
    const { setSelected } = mountSwitcher();
    expect(trigger().textContent).toContain('전체');
    expect(screen.queryByRole('menu')).toBeNull();
    await userEvent.click(trigger());
    const board = boardFixture();
    await userEvent.click(screen.getByRole('menuitemradio', { name: new RegExp(board.title) }));
    expect(setSelected).toHaveBeenCalledWith(board.key);
    expect(screen.queryByRole('menu')).toBeNull();
  });

  test('보드마다 진행중 개수 — 전 보드 기준', async () => {
    const board = boardFixture();
    mountSwitcher({
      nowTodos: [
        todoFixture({ id: 'a', boardId: board.id, status: 'doing' }),
        todoFixture({ id: 'b', boardId: board.id, status: 'doing' }),
      ],
    });
    await userEvent.click(trigger());
    expect(screen.getByRole('menuitemradio', { name: /진행 2/ })).toBeTruthy();
  });

  test('Esc 로 닫는다', async () => {
    mountSwitcher();
    await userEvent.click(trigger());
    await userEvent.keyboard('{Escape}');
    expect(screen.queryByRole('menu')).toBeNull();
  });
});

describe('BoardSwitcher 보드 생성', () => {
  test('Enter 로 만들면 목록이 닫힌다', async () => {
    const { createBoard } = mountSwitcher();
    await userEvent.click(trigger());
    await userEvent.click(screen.getByRole('button', { name: '+ 새 보드' }));
    await userEvent.type(screen.getByRole('textbox', { name: '새 보드 이름' }), 'newboard{Enter}');
    expect(createBoard.mock.calls[0]).toEqual(['newboard']);
    expect(screen.queryByRole('menu')).toBeNull();
  });

  // 서버가 key 를 거절했을 때 조용히 닫으면 왜 안 만들어졌는지 알 수 없다.
  test('생성이 실패하면 사유를 알리고 입력을 유지한다', async () => {
    mountSwitcher({
      createBoard: mock(async () => {
        throw new Error('board key cannot contain #');
      }),
    });
    await userEvent.click(trigger());
    await userEvent.click(screen.getByRole('button', { name: '+ 새 보드' }));
    await userEvent.type(screen.getByRole('textbox', { name: '새 보드 이름' }), 'bad#key{Enter}');
    expect((await screen.findByRole('alert')).textContent).toBe('board key cannot contain #');
    expect(screen.getByRole('textbox', { name: '새 보드 이름' })).toHaveProperty(
      'value',
      'bad#key',
    );
  });
});
