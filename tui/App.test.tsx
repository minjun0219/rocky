import { afterEach, expect, test } from 'bun:test';
import { cleanup, render } from 'ink-testing-library';
import { App } from './App';
import { boardFixture, fakeApi, sectionFixture, todoFixture } from './fixtures';
import { createTuiStore } from './store';

afterEach(cleanup);

/** 화면이 바뀔 때까지 — Ink 는 다음 틱에 그린다. */
const tick = () => Bun.sleep(20);

async function mount() {
  const api = fakeApi({
    boards: [boardFixture(), boardFixture({ id: 'board2', key: 'mdwire', title: 'mdwire' })],
    todos: [
      todoFixture({ id: 'a', number: 12, title: '첫 할 일' }),
      todoFixture({ id: 'b', number: 13, title: '둘째 할 일', sectionId: 'sec1' }),
    ],
    sections: { rocky: [sectionFixture()] },
  });
  const store = createTuiStore(api);
  await store.getState().load();
  const view = render(<App store={store} />);
  await tick();
  return { view, store };
}

test('머리·지금·섹션 트리를 그리고 커서는 첫 할 일에', async () => {
  const { view } = await mount();
  const frame = view.lastFrame() ?? '';
  expect(frame).toContain('rocky · rocky');
  expect(frame).toContain('지금');
  expect(frame).toContain('›○ #12 첫 할 일');
  expect(frame).toContain('검증');
  expect(frame).toContain('#13 둘째 할 일');
});

test('j 로 커서가 다음 할 일로, ] 로 다음 보드로', async () => {
  const { view, store } = await mount();
  view.stdin.write('j');
  await tick();
  expect(view.lastFrame()).toContain('›○ #13 둘째 할 일');
  view.stdin.write(']');
  await tick();
  expect(store.getState().boardKey).toBe('mdwire');
  expect(view.lastFrame()).toContain('rocky · mdwire');
});

test('j 를 다시 그리기보다 빨리 연타해도 그만큼 움직인다', async () => {
  const { view } = await mount();
  view.stdin.write('j');
  view.stdin.write('j');
  await tick();
  expect(view.lastFrame()).toContain('›○ #13 둘째 할 일');
  view.stdin.write('k');
  view.stdin.write('k');
  await tick();
  expect(view.lastFrame()).toContain('›○ #12 첫 할 일');
});

test('실패하면 상태 줄에 그대로', async () => {
  const { view, store } = await mount();
  store.getState().setError('GET /api/todos → 500');
  await tick();
  expect(view.lastFrame()).toContain('GET /api/todos → 500');
});
