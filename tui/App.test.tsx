import { afterEach, expect, test } from 'bun:test';
import { EventEmitter } from 'node:events';
import { type Instance, render } from 'ink';
import { App } from './App';
import { boardFixture, fakeApi, sectionFixture, todoFixture } from './fixtures';
import { createTuiStore } from './store';

/**
 * ink-testing-library 와 같은 가짜 터미널 — 그쪽은 폭이 100열로 고정이라 한 열/두 열을 둘 다 볼 수 없어 폭을 받는다.
 */
class FakeStdout extends EventEmitter {
  last = '';
  constructor(
    readonly columns: number,
    readonly rows: number,
  ) {
    super();
  }
  write = (frame: string) => {
    this.last = frame;
  };
}

class FakeStdin extends EventEmitter {
  isTTY = true;
  data: string | null = null;
  write = (data: string) => {
    this.data = data;
    this.emit('readable');
    this.emit('data', data);
  };
  read = () => {
    const { data } = this;
    this.data = null;
    return data;
  };
  setEncoding() {}
  setRawMode() {}
  resume() {}
  pause() {}
  ref() {}
  unref() {}
}

const mounted: Instance[] = [];
afterEach(() => {
  for (const instance of mounted.splice(0)) {
    instance.unmount();
  }
});

/** 화면이 바뀔 때까지 — Ink 는 다음 틱에 그린다. 넓은 화면의 상세는 80ms 멈춘 뒤에 받는다. */
const tick = (ms = 20) => Bun.sleep(ms);

async function mount(columns = 60) {
  const api = fakeApi({
    boards: [boardFixture(), boardFixture({ id: 'board2', key: 'mdwire', title: 'mdwire' })],
    todos: [
      todoFixture({ id: 'a', number: 12, title: '첫 할 일', description: '본문 첫 줄' }),
      todoFixture({ id: 'b', number: 13, title: '둘째 할 일', sectionId: 'sec1' }),
    ],
    sections: { rocky: [sectionFixture()] },
  });
  const store = createTuiStore(api);
  await store.getState().load();
  const stdout = new FakeStdout(columns, 30);
  const stdin = new FakeStdin();
  const instance = render(<App store={store} />, {
    stdout: stdout as unknown as NodeJS.WriteStream,
    stderr: new FakeStdout(columns, 30) as unknown as NodeJS.WriteStream,
    stdin: stdin as unknown as NodeJS.ReadStream,
    debug: true,
    exitOnCtrlC: false,
    patchConsole: false,
  });
  mounted.push(instance);
  await tick();
  return { frame: () => stdout.last, press: (key: string) => stdin.write(key), store, api };
}

test('머리·지금·섹션 트리를 그리고 커서는 첫 할 일에', async () => {
  const { frame } = await mount();
  expect(frame()).toContain('rocky · rocky');
  expect(frame()).toContain('지금');
  expect(frame()).toContain('›○ #12 첫 할 일');
  expect(frame()).toContain('검증');
  expect(frame()).toContain('#13 둘째 할 일');
});

test('j 로 커서가 다음 할 일로, ] 로 다음 보드로', async () => {
  const { frame, press, store } = await mount();
  press('j');
  await tick();
  expect(frame()).toContain('›○ #13 둘째 할 일');
  press(']');
  await tick();
  expect(store.getState().boardKey).toBe('mdwire');
  expect(frame()).toContain('rocky · mdwire');
});

test('j 를 다시 그리기보다 빨리 연타해도 그만큼 움직인다', async () => {
  const { frame, press } = await mount();
  press('j');
  press('j');
  await tick();
  expect(frame()).toContain('›○ #13 둘째 할 일');
  press('k');
  press('k');
  await tick();
  expect(frame()).toContain('›○ #12 첫 할 일');
});

test('실패하면 상태 줄에 그대로', async () => {
  const { frame, store } = await mount();
  store.getState().setError('GET /api/todos → 500');
  await tick();
  expect(frame()).toContain('GET /api/todos → 500');
});

test('좁은 화면: Enter 로 상세를 목록 자리에 띄우고 Esc 로 돌아온다', async () => {
  const { frame, press, api } = await mount(60);
  press('\r');
  await tick();
  expect(api.calls).toContain('todo a');
  expect(frame()).toContain('본문 첫 줄');
  expect(frame()).not.toContain('#13 둘째 할 일');
  press('\u001b');
  await tick();
  expect(frame()).toContain('#13 둘째 할 일');
});

test('넓은 화면: 목록 옆에 커서를 따라가는 상세', async () => {
  const { frame, press, api } = await mount(120);
  await tick(120);
  expect(frame()).toContain('#13 둘째 할 일');
  expect(frame()).toContain('본문 첫 줄');
  press('j');
  await tick(120);
  expect(api.calls).toContain('todo b');
});

test('s·x·d 는 커서의 할 일 상태를 바꾸고 상태 줄에 한 줄', async () => {
  const { frame, press, api } = await mount();
  press('s');
  await tick();
  expect(api.calls).toContain('status a start');
  expect(frame()).toContain('✓ #12 착수');
  press('j');
  press('d');
  await tick();
  expect(api.calls).toContain('status b done');
});

test('a 로 빠른 추가 — 글자를 받고 Enter 로 보고 있는 보드에 더한다', async () => {
  const { frame, press, api } = await mount();
  press('a');
  await tick();
  for (const ch of ['배', '포', ' ', 'q']) {
    press(ch);
  }
  await tick();
  // 입력 중에는 q 도 글자다 — 끝나지 않는다
  expect(frame()).toContain('새 할 일: 배포 q');
  press('\r');
  await tick();
  expect(api.calls).toContain('add rocky 배포 q');
});

test('좁은 상세에서 s·x·d 는 화면에 띄운 할 일에 — 갱신으로 커서가 옮겨도', async () => {
  const { press, store, api } = await mount(60);
  press('\r');
  await tick();
  // SSE 갱신으로 커서가 다른 줄로 옮겨진 상황
  store.getState().setCursor('t:b', 6);
  press('s');
  await tick();
  expect(api.calls).toContain('status a start');
  expect(api.calls).not.toContain('status b start');
});

test('동작 실패는 다음 키에서 지운다', async () => {
  const { frame, press, store } = await mount();
  store.getState().setError('POST /api/todos/a/status → 409');
  await tick();
  expect(frame()).toContain('→ 409');
  press('j');
  await tick();
  expect(frame()).not.toContain('→ 409');
});
