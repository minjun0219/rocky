import { afterEach, describe, expect, mock, test } from 'bun:test';
import { cleanup, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { boardFixture, renderWithStore } from '../test-support';
import { Sidebar } from './Sidebar';

afterEach(cleanup);

/** 보드 하나가 있는 사이드바를 띄우고 createBoard 스파이를 돌려준다. */
function mountSidebar(createBoard: (key: string) => Promise<void>) {
  const spy = mock(createBoard);
  renderWithStore(<Sidebar />, {
    boards: [boardFixture()],
    todos: [],
    selected: 'all',
    setSelected: mock(() => {}),
    createBoard: spy,
  });
  return spy;
}

const openButton = () => screen.getByRole('button', { name: '+ 새 보드' });
const keyInput = () => screen.getByRole('textbox', { name: '새 보드 이름' });

// 폼 닫힘/에러 표시는 `submit()` 의 `await createBoard(...)` **뒤에** 일어나는데
// `onKeyDown` 은 `void submit()` 로 던져만 놓는다 — 즉 `userEvent.type` 이 반환된
// 시점에 그 setState 가 이미 flush 되었다는 보장이 없다. 지금은 React 의 async act 가
// 마이크로태스크를 걷어가 우연히 맞지만, 기대는 명시해 둔다.
const findOpenButton = () => screen.findByRole('button', { name: '+ 새 보드' });
const findAlert = () => screen.findByRole('alert');

describe('Sidebar 보드 생성 폼', () => {
  test('"+ 새 보드" 를 누르면 input 으로 전환된다', async () => {
    mountSidebar(async () => {});
    await userEvent.click(openButton());
    expect(keyInput()).toBeDefined();
  });

  test('Enter 로 생성하면 폼이 닫힌다', async () => {
    const createBoard = mountSidebar(async () => {});
    await userEvent.click(openButton());
    await userEvent.type(keyInput(), 'newboard{Enter}');
    expect(createBoard).toHaveBeenCalledTimes(1);
    expect(createBoard.mock.calls[0]).toEqual(['newboard']);
    expect(await findOpenButton()).toBeDefined();
  });

  // 서버가 key 를 거절했을 때 조용히 닫으면 왜 안 만들어졌는지 알 수 없다.
  // 순수 함수로는 "이유가 화면에 남았는가" 를 증명할 수 없는 자리다.
  test('생성이 실패하면 사유를 알리고 입력을 유지한다', async () => {
    mountSidebar(async () => {
      throw new Error('board key cannot contain #');
    });
    await userEvent.click(openButton());
    await userEvent.type(keyInput(), 'bad#key{Enter}');

    expect((await findAlert()).textContent).toBe('board key cannot contain #');
    expect(keyInput()).toHaveProperty('value', 'bad#key');
  });

  test('다시 입력하면 이전 에러가 걷힌다', async () => {
    mountSidebar(async () => {
      throw new Error('board key cannot contain #');
    });
    await userEvent.click(openButton());
    await userEvent.type(keyInput(), 'bad#key{Enter}');
    // 에러가 실제로 떴는지부터 확인한다 — 이걸 빼면 아직 뜨지도 않은 상태를 "걷혔다" 로
    // 읽어 이 테스트가 빈 채로 통과한다.
    await findAlert();
    await userEvent.type(keyInput(), 'x');
    expect(screen.queryByRole('alert')).toBeNull();
  });

  test('Esc 는 생성하지 않고 폼을 닫는다', async () => {
    const createBoard = mountSidebar(async () => {});
    await userEvent.click(openButton());
    await userEvent.type(keyInput(), 'newboard{Escape}');
    expect(createBoard).not.toHaveBeenCalled();
    expect(await findOpenButton()).toBeDefined();
  });

  test('빈 입력으로 Enter 하면 만들지 않고 닫는다', async () => {
    const createBoard = mountSidebar(async () => {});
    await userEvent.click(openButton());
    await userEvent.type(keyInput(), '   {Enter}');
    expect(createBoard).not.toHaveBeenCalled();
    expect(await findOpenButton()).toBeDefined();
  });
});

// 튕김 버그 회귀 — refetch 마다 새 boards 배열이 와도 문서 스크롤을 건드리지 않고,
// 탭 행도 선택이 바뀌지 않았으면 그대로 둔다(사람이 옆으로 넘겨 둔 위치를 되돌리지 않는다).
describe('Sidebar 탭 행 스크롤', () => {
  test('scrollIntoView 를 부르지 않고, 같은 선택의 refetch 는 탭 행을 움직이지 않는다', async () => {
    const intoView = mock(() => {});
    const original = HTMLElement.prototype.scrollIntoView;
    HTMLElement.prototype.scrollIntoView = intoView;
    try {
      const { useUiStore } = await import('../store');
      renderWithStore(<Sidebar />, {
        boards: [boardFixture()],
        todos: [],
        selected: 'all',
        setSelected: mock(() => {}),
        createBoard: mock(async () => {}),
      });
      const nav = screen.getByRole('navigation', { name: '보드' });
      nav.scrollLeft = 42; // 사람이 옆으로 넘겨 둔 상태
      // refetch — 내용은 같지만 새 배열
      useUiStore.setState({ boards: [boardFixture()] });
      await Promise.resolve();
      expect(nav.scrollLeft).toBe(42);
      expect(intoView).not.toHaveBeenCalled();
    } finally {
      HTMLElement.prototype.scrollIntoView = original;
    }
  });
});
