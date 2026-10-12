import { expect, test } from 'bun:test';
import { boardFixture, fakeApi, sectionFixture, todoFixture } from './fixtures';
import { createTuiStore } from './store';

const boards = [boardFixture(), boardFixture({ id: 'board2', key: 'mdwire', title: 'mdwire' })];

test('처음 load 는 첫 보드를 고르고 그 섹션을 받는다', async () => {
  const api = fakeApi({ boards, todos: [todoFixture()], sections: { rocky: [sectionFixture()] } });
  const store = createTuiStore(api);
  await store.getState().load();
  const s = store.getState();
  expect(s.boardKey).toBe('rocky');
  expect(s.sections.map((x) => x.id)).toEqual(['sec1']);
  expect(s.todos).toHaveLength(1);
  expect(s.ready).toBe(true);
});

test('보던 보드가 없어지면 첫 보드로', async () => {
  const store = createTuiStore(fakeApi({ boards }), 'gone');
  await store.getState().load();
  expect(store.getState().boardKey).toBe('rocky');
});

test('보드를 앞뒤로 돌린다 — 끝에서 처음으로, 그 보드의 섹션을 받는다', async () => {
  const api = fakeApi({ boards });
  const store = createTuiStore(api);
  await store.getState().load();
  store.getState().shiftBoard(1);
  expect(store.getState().boardKey).toBe('mdwire');
  store.getState().shiftBoard(1);
  expect(store.getState().boardKey).toBe('rocky');
  await Bun.sleep(0);
  expect(api.calls.filter((c) => c.startsWith('sections'))).toEqual([
    'sections rocky',
    'sections mdwire',
    'sections rocky',
  ]);
});

test('실패는 상태 줄로, 다음 성공에서 지운다', async () => {
  const api = fakeApi({ boards });
  api.todos = async () => {
    throw new Error('GET /api/todos → 500');
  };
  const store = createTuiStore(api);
  await store.getState().load();
  expect(store.getState().error).toBe('GET /api/todos → 500');
  api.todos = async () => [];
  await store.getState().load();
  expect(store.getState().error).toBeUndefined();
});

test('SSE 이벤트는 150ms 로 모아 건드린 묶음만 다시 받는다', async () => {
  const api = fakeApi({ boards });
  let todoCalls = 0;
  let boardCalls = 0;
  api.todos = async () => {
    todoCalls++;
    return [];
  };
  api.boards = async () => {
    boardCalls++;
    return boards;
  };
  const store = createTuiStore(api);
  store.getState().queueEvents([{ entity: 'todo', action: 'update' }]);
  store.getState().queueEvents([{ entity: 'todo', action: 'status' }]);
  await Bun.sleep(200);
  expect(todoCalls).toBe(1);
  expect(boardCalls).toBe(0);
});

test('읽는 사이 보드를 바꾸면 그 선택을 두고 옛 보드의 섹션은 버린다', async () => {
  const api = fakeApi({ boards, sections: { rocky: [sectionFixture()], mdwire: [] } });
  const store = createTuiStore(api);
  await store.getState().load();
  const sections = api.sections;
  api.sections = async (board: string) => {
    await Bun.sleep(30);
    return sections(board);
  };
  const loading = store.getState().load(new Set(['sections']));
  store.getState().shiftBoard(1);
  await loading;
  await Bun.sleep(40);
  const s = store.getState();
  expect(s.boardKey).toBe('mdwire');
  expect(s.sectionsBoard).toBe('mdwire');
  expect(s.sections).toEqual([]);
});

test('동작 실패는 관계없는 읽기가 지우지 않는다 — 읽기 실패만 다음 읽기가 지운다', async () => {
  const store = createTuiStore(fakeApi({ boards }));
  store.getState().setError('POST /api/todos/t1/status → 409');
  await store.getState().load();
  expect(store.getState().error).toBe('POST /api/todos/t1/status → 409');
  store.getState().setError(undefined);
  expect(store.getState().error).toBeUndefined();
});
