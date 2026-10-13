// 엔진 테스트 키트에서 돈다 — `bun run test:panel`(claude CLI 필요, CI 밖). 순수 판정은 data.test.ts(bun, CI).
import type { On } from 'claude-code';
import { expect, mock, test } from 'claude-code/testing';

const LONG = 'Antigravity 앱·IDE 에서 rocky 번들이 붙는지 확인한다 — 긴 제목은 다음 줄로 내려간다';

const PANE_PROPS = (columns: number, isFocused = false) => ({
  title: 'rocky 보드',
  isFocused,
  bodyColumns: columns,
  placement: 'dock' as const,
  scroll: { offset: 0, bodyRows: 20 },
  view: {},
});

/** 가짜 데몬·세션 — 플러그인 아래에서 엔진 대신 답하고, 플러그인이 부른 것을 기록한다. */
function world(on: On, opts: { board: string | null }) {
  const seen = {
    fetched: [] as string[],
    usageMarked: [] as string[],
    clients: new Set<string>(),
    opened: 0,
    commands: [] as string[],
  };
  /** 데몬 쪽 상태 — 테스트가 바꾸면 다음 폴링이 본다. */
  const daemon = {
    lastId: 100,
    entries: [] as { boardKey: string | null }[],
    changesStatus: 200,
    collect: 2,
  };
  const clock = mock.clock(on);
  on('session.start', (_$, e) => ({ cwd: e.cwd }));
  on('session.cwd', () => ({ value: '/w/rocky' }));
  on('command.register', (_$, e) => {
    seen.commands.push(e.name);
    return { value: { command: e.name } };
  });
  on('ui.open', () => {
    seen.opened += 1;
    return { value: { isPlaced: true as const } };
  });
  on('http.fetch', (_$, e) => {
    const url = new URL(e.url);
    seen.fetched.push(url.pathname);
    const headers = e.init?.headers ?? {};
    if (headers['x-rocky-client']) {
      seen.clients.add(headers['x-rocky-client']);
    }
    if (headers['x-rocky-usage']) {
      seen.usageMarked.push(`${url.pathname}:${headers['x-rocky-usage']}`);
    }
    if (url.pathname === '/api/changes') {
      const entries = url.searchParams.get('limit') === '0' ? [] : daemon.entries;
      const text = JSON.stringify({ lastId: daemon.lastId, entries });
      const ok = daemon.changesStatus < 300;
      return { value: { status: daemon.changesStatus, ok, headers: {}, text } };
    }
    const body =
      url.pathname === '/api/summary'
        ? { board: opts.board, collect: daemon.collect }
        : [
            { ref: 'rocky-3', title: '진행 중인 일', status: 'doing' },
            { ref: 'rocky-45', title: LONG, status: 'todo' },
            { ref: 'rocky-9', title: '끝낸 일', status: 'done' },
          ];
    return { value: { status: 200, ok: true, headers: {}, text: JSON.stringify(body) } };
  });
  /** 데몬에 새 히스토리 행이 생긴 것처럼. */
  const touch = (...boards: (string | null)[]) => {
    daemon.lastId += boards.length;
    daemon.entries = boards.map((boardKey) => ({ boardKey }));
  };
  return { seen, daemon, clock, touch };
}

test('보드가 있는 폴더면 패널을 열고, 이 보드를 건드린 변화가 있을 때만 할 일을 다시 읽는다', async ($, on) => {
  const { seen, clock, touch } = world(on, { board: 'rocky' });
  await $.session.start({ cwd: '/w/rocky', surface: 'terminal', isInteractive: true });
  await clock.settle();
  expect(seen.commands).toEqual(['rocky-board']);
  expect(seen.opened).toBe(1);
  expect(seen.fetched).toEqual(['/api/summary', '/api/changes', '/api/todos']);
  // 패널의 요청은 모두 사용 로그용 클라이언트를 달고, 사용 표시는 첫 보드 읽기에만.
  expect([...seen.clients]).toEqual(['claude-code-panel']);
  expect(seen.usageMarked).toEqual(['/api/todos:open']);

  // 다음 폴링 — 새 히스토리가 없으면 그것만 묻는다.
  await clock.advance(5_000);
  expect(seen.fetched.slice(3)).toEqual(['/api/changes']);

  // 다른 보드만 바뀌었으면 다시 읽지 않는다.
  touch('other', null);
  await clock.advance(5_000);
  expect(seen.fetched.slice(4)).toEqual(['/api/changes']);

  // 이 보드가 바뀌면 할 일만 다시 읽는다(요약은 아니다). 사용 표시·패널 열기는 되풀이하지 않는다.
  touch('rocky');
  await clock.advance(5_000);
  expect(seen.fetched.slice(5)).toEqual(['/api/changes', '/api/todos']);
  expect(seen.usageMarked).toEqual(['/api/todos:open']);
  expect(seen.opened).toBe(1);
});

test('/rocky-board 를 치거나 패널에 포커스를 주면 사용으로 세션마다 한 번 센다', async ($, on) => {
  const { seen, clock } = world(on, { board: 'rocky' });
  await $.session.start({ cwd: '/w/rocky', surface: 'terminal', isInteractive: true });
  await clock.settle();
  const run = () =>
    $.command.run({
      command: 'rocky-board',
      args: '',
      origin: { kind: 'composer' },
      presentation: { isFullscreen: false, columns: 120 },
    });
  expect(await run()).toEqual({ text: 'rocky 보드 패널을 열었다.' });
  await clock.settle();
  expect(seen.opened).toBe(2);
  expect(seen.usageMarked).toEqual(['/api/todos:open', '/api/changes:use']);
  // 다시 치거나 포커스를 줘도 더 세지 않는다.
  await run();
  const ui = await $.ui.mount({
    plugin: 'rocky',
    surface: 'terminal',
    component: 'Pane',
    requestId: 'rocky-board',
    props: PANE_PROPS(80, true),
    viewport: { columns: 80, rows: 24 },
  });
  await ui.drawn();
  await clock.settle();
  expect(seen.usageMarked).toEqual(['/api/todos:open', '/api/changes:use']);
});

test('패널에 포커스만 줘도 사용으로 센다', async ($, on) => {
  const { seen, clock } = world(on, { board: 'rocky' });
  await $.session.start({ cwd: '/w/rocky', surface: 'terminal', isInteractive: true });
  await clock.settle();
  const ui = await $.ui.mount({
    plugin: 'rocky',
    surface: 'terminal',
    component: 'Pane',
    requestId: 'rocky-board',
    props: PANE_PROPS(80, true),
    viewport: { columns: 80, rows: 24 },
  });
  await ui.drawn();
  await clock.settle();
  expect(seen.usageMarked).toEqual(['/api/todos:open', '/api/changes:use']);
});

test('수집함 수는 히스토리에 남지 않아 요약을 60초마다 다시 읽는다', async ($, on) => {
  const { seen, daemon, clock } = world(on, { board: 'rocky' });
  await $.session.start({ cwd: '/w/rocky', surface: 'terminal', isInteractive: true });
  await clock.settle();
  daemon.collect = 5;
  await clock.advance(60_000);
  expect(seen.fetched.filter((p) => p === '/api/summary')).toHaveLength(2);
  const ui = await $.ui.mount({
    plugin: 'rocky',
    surface: 'terminal',
    component: 'Pane',
    requestId: 'rocky-board',
    props: PANE_PROPS(80),
    viewport: { columns: 80, rows: 24 },
  });
  expect(await ui.find({ text: /수집함 5/ })).toBeDefined();
});

test('히스토리를 못 읽으면 사유만 남기고 할 일을 몇 초마다 통째로 다시 읽지 않는다', async ($, on) => {
  const { seen, daemon, clock } = world(on, { board: 'rocky' });
  await $.session.start({ cwd: '/w/rocky', surface: 'terminal', isInteractive: true });
  await clock.settle();
  daemon.changesStatus = 500;
  await clock.advance(15_000);
  expect(seen.fetched.slice(3)).toEqual(['/api/changes', '/api/changes', '/api/changes']);
  const ui = await $.ui.mount({
    plugin: 'rocky',
    surface: 'terminal',
    component: 'Pane',
    requestId: 'rocky-board',
    props: PANE_PROPS(80),
    viewport: { columns: 80, rows: 24 },
  });
  expect(await ui.find({ text: '/api/changes 응답 500' })).toBeDefined();
  // 지난 목록은 그대로 보인다.
  expect(await ui.find({ text: /rocky-45/ })).toBeDefined();
});

test('보드가 없는 폴더면 패널을 저절로 열지 않는다', async ($, on) => {
  const { seen, clock } = world(on, { board: null });
  await $.session.start({ cwd: '/tmp', surface: 'terminal', isInteractive: true });
  await clock.settle();
  expect(seen.opened).toBe(0);
  expect(seen.fetched).toEqual(['/api/summary', '/api/changes']);
  expect(seen.usageMarked).toEqual([]);
});

test('그릴 데가 없는 실행(-p·SDK)에서는 아무것도 하지 않는다', async ($, on) => {
  const { seen, clock } = world(on, { board: 'rocky' });
  await $.session.start({ cwd: '/w/rocky', surface: null, isInteractive: false });
  await clock.advance(10_000);
  expect(seen.commands).toEqual([]);
  expect(seen.fetched).toEqual([]);
});

for (const surface of ['terminal', 'desktop'] as const) {
  for (const columns of [28, 80]) {
    test(`${surface} ${columns}칸 — 끝낸 것은 빼고, 제목은 자르지 않고 줄을 바꿔 그린다`, async ($, on) => {
      const { clock } = world(on, { board: 'rocky' });
      await $.session.start({ cwd: '/w/rocky', surface, isInteractive: true });
      await clock.settle();
      const ui = await $.ui.mount({
        plugin: 'rocky',
        surface,
        component: 'Pane',
        requestId: 'rocky-board',
        props: PANE_PROPS(columns),
        viewport: { columns, rows: 24 },
      });
      // 표면의 요소 표가 트리를 받아들였다(거절이면 drawn 이 실패한다).
      await ui.drawn();
      expect((await ui.find({ type: 'Text', text: LONG }))?.props.wrap).toBe('wrap');
      expect(await ui.find({ text: /rocky-45/ })).toBeDefined();
      expect(await ui.find({ text: /수집함 2/ })).toBeDefined();
      expect(await ui.find({ text: '끝낸 일' })).toBeUndefined();
    });
  }
}
