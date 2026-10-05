import { afterEach, describe, expect, mock, test } from 'bun:test';
import { cleanup, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import type { HandoffView } from '../types';
import { renderWithStore, todoFixture } from '../test-support';
import { DetailDrawer } from './DetailDrawer';

afterEach(cleanup);

/** 제목 편집 상태로 드로어를 띄우고 patchTodo 스파이를 돌려준다. */
function mountDrawer() {
  const patchTodo = mock(async () => {});
  const closeDetail = mock(() => {});
  renderWithStore(<DetailDrawer />, {
    detail: { kind: 'todo', todo: todoFixture(), history: [], comments: [] },
    sections: [],
    handoffs: [],
    sessions: { available: false, reason: '테스트', list: [] },
    patchTodo,
    closeDetail,
  });
  return { patchTodo, closeDetail };
}

const titleButton = () => screen.getByRole('button', { name: '제목 수정: 원래 제목' });
const titleInput = () => screen.getByRole('textbox', { name: /제목 수정/ });

describe('DetailDrawer 제목 편집', () => {
  test('제목을 클릭하면 input 으로 전환된다', async () => {
    mountDrawer();
    await userEvent.click(titleButton());
    expect(titleInput()).toBeDefined();
  });

  // 커밋 경로를 onBlur 하나로 모아 둔 이유가 이것이다 (DetailDrawer.tsx 의 commitTitle 주석).
  // Enter 가 blur 로 빠지므로, 경로가 둘이면 같은 PATCH 가 두 번 나간다.
  test('Enter 는 PATCH 를 한 번만 보낸다', async () => {
    const { patchTodo } = mountDrawer();
    await userEvent.click(titleButton());
    await userEvent.clear(titleInput());
    await userEvent.type(titleInput(), '바뀐 제목{Enter}');
    expect(patchTodo).toHaveBeenCalledTimes(1);
    expect(patchTodo.mock.calls[0]).toEqual(['todo1', { title: '바뀐 제목' }] as never);
  });

  test('Esc 는 저장하지 않고 원래 제목으로 되돌린다', async () => {
    const { patchTodo } = mountDrawer();
    await userEvent.click(titleButton());
    await userEvent.clear(titleInput());
    await userEvent.type(titleInput(), '버릴 제목{Escape}');
    expect(patchTodo).not.toHaveBeenCalled();
    expect(titleButton()).toBeDefined();
  });

  // 편집 중 Esc 는 편집 취소지 드로어 닫기가 아니다 — 전역 keydown 리스너까지 올라가면
  // 안내한 "Esc 취소"와 다른 동작이 된다.
  test('편집 중 Esc 는 드로어를 닫지 않는다', async () => {
    const { closeDetail } = mountDrawer();
    await userEvent.click(titleButton());
    await userEvent.type(titleInput(), '{Escape}');
    expect(closeDetail).not.toHaveBeenCalled();
  });

  test('빈 제목은 저장하지 않는다', async () => {
    const { patchTodo } = mountDrawer();
    await userEvent.click(titleButton());
    await userEvent.clear(titleInput());
    await userEvent.type(titleInput(), '   {Enter}');
    expect(patchTodo).not.toHaveBeenCalled();
    expect(titleButton()).toBeDefined();
  });
});

describe('DetailDrawer 미착수 핸드오프', () => {
  function handoffFixture(over: Partial<HandoffView> = {}): HandoffView {
    return {
      id: 'h1',
      todoId: 'todo1',
      sessionId: 'sess-1',
      sessionName: 'eelpout-a3',
      note: '',
      actor: 'logan',
      status: 'delivered',
      createdAt: '2026-07-27T00:00:00.000Z',
      deliveredAt: '2026-07-27T00:00:01.000Z',
      deliveredVia: 'stop',
      phase: 'delivered',
      unstarted: true,
      stale: false,
      ...over,
    };
  }

  function mountWith(handoffs: HandoffView[]) {
    const fetchSessions = mock(async () => {});
    renderWithStore(<DetailDrawer />, {
      detail: { kind: 'todo', todo: todoFixture(), history: [], comments: [] },
      sections: [],
      handoffs,
      sessions: { available: false, reason: '테스트', list: [] },
      fetchSessions,
    });
    return { fetchSessions };
  }

  const unstartedNotice = () => screen.queryByRole('status');

  test('집어가 놓고 안 한 건을 세션 이름과 함께 알린다', () => {
    mountWith([handoffFixture()]);
    expect(unstartedNotice()?.textContent).toContain('eelpout-a3');
    expect(unstartedNotice()?.textContent).toContain('착수하지 않았어요');
  });

  test('착수한 건은 알리지 않는다', () => {
    mountWith([handoffFixture({ phase: 'accepted', unstarted: false })]);
    expect(unstartedNotice()).toBeNull();
  });

  // 이미 다시 보낸 상태라면 과거를 들출 이유가 없다.
  test('대기 중인 새 요청이 있으면 미착수 알림을 띄우지 않는다', () => {
    mountWith([
      handoffFixture(),
      handoffFixture({ id: 'h2', status: 'pending', phase: 'pending', unstarted: false }),
    ]);
    expect(unstartedNotice()).toBeNull();
  });

  // 같은 세션으로 되쏘지 않는다 — 그 세션은 사라졌을 수 있다. 패널을 열어 고르게 한다.
  test('다시 보내기는 세션 목록을 불러 보내기 패널을 연다', async () => {
    const { fetchSessions } = mountWith([handoffFixture()]);
    await userEvent.click(screen.getByRole('button', { name: '다시 보내기' }));
    expect(fetchSessions).toHaveBeenCalledTimes(1);
  });
});

describe('DetailDrawer 설명 편집', () => {
  async function openEditor() {
    const spies = mountDrawer();
    await userEvent.click(screen.getByText(/설명 없음/));
    const content = document.querySelector('.drawer-desc-cm .cm-content');
    if (!content) {
      throw new Error('편집기가 없다');
    }
    const { EditorView } = await import('@codemirror/view');
    const view = EditorView.findFromDOM(content as HTMLElement);
    if (!view) {
      throw new Error('EditorView 가 없다');
    }
    return { ...spies, view };
  }

  // 고정 높이 textarea(rows=8)였을 땐 긴 설명이 작은 스크롤 상자에 갇혔다 — 노트와 같은 편집기로 바꿨다.
  test('누르면 노트와 같은 마크다운 편집기와 서식 툴바가 뜬다', async () => {
    await openEditor();
    expect(document.querySelector('.drawer-desc-cm.note-cm .cm-editor')).not.toBeNull();
    expect(screen.getByRole('toolbar', { name: '서식' })).toBeDefined();
    expect(document.querySelector('textarea.drawer-desc-edit')).toBeNull();
  });

  test('저장은 편집기의 지금 글을 보낸다', async () => {
    const { patchTodo, view } = await openEditor();
    view.dispatch({ changes: { from: 0, insert: '## 할 것\n- [ ] 하나' } });
    await userEvent.click(screen.getByRole('button', { name: /저장/ }));
    expect(patchTodo.mock.calls[0]).toEqual([
      'todo1',
      { description: '## 할 것\n- [ ] 하나' },
    ] as never);
    expect(document.querySelector('.drawer-desc-cm')).toBeNull();
  });

  test('취소는 보내지 않고 편집기를 닫는다', async () => {
    const { patchTodo, view } = await openEditor();
    view.dispatch({ changes: { from: 0, insert: '버릴 글' } });
    await userEvent.click(screen.getByRole('button', { name: '취소' }));
    expect(patchTodo).not.toHaveBeenCalled();
    expect(document.querySelector('.drawer-desc-cm')).toBeNull();
  });

  test('서식 버튼은 편집기 글에 적용된다', async () => {
    const { view } = await openEditor();
    view.dispatch({ changes: { from: 0, insert: '굵게' }, selection: { anchor: 0, head: 2 } });
    await userEvent.click(screen.getByRole('button', { name: '굵게' }));
    expect(view.state.doc.toString()).toBe('**굵게**');
  });
});

describe('DetailDrawer 링크 칩의 PR 상태', () => {
  const snapshot = {
    repo: 'o/rocky',
    number: 7,
    title: 'PR',
    url: 'https://github.com/o/rocky/pull/7',
    state: 'OPEN',
    isDraft: false,
    base: 'main',
    head: 'abc',
    mergeState: 'DIRTY',
    ci: 'pass',
    unhandled: 0,
    decision: 0,
    ready: false,
    updatedAt: '2026-09-28T10:00:00Z',
  };

  test('감시 중인 PR 링크에는 상태가 붙고, 감시 밖 링크에는 없다', () => {
    renderWithStore(<DetailDrawer />, {
      detail: {
        kind: 'todo',
        todo: todoFixture({
          links: [
            { url: 'https://github.com/o/rocky/pull/7/files' },
            { url: 'https://github.com/o/rocky/pull/8' },
          ],
        }),
        history: [],
        comments: [],
      },
      sections: [],
      handoffs: [],
      sessions: { available: false, reason: '테스트', list: [] },
      prs: [snapshot] as never,
    });
    const chips = document.querySelectorAll('.chip-link');
    expect(chips).toHaveLength(2);
    expect(chips[0]?.querySelector('.chip-link-status')?.textContent).toBe('충돌');
    expect(chips[0]?.getAttribute('title')).toBe('충돌 — 충돌');
    expect(chips[1]?.querySelector('.chip-link-status')).toBeNull();
  });
});
