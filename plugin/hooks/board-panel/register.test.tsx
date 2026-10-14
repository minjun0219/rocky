// 엔진 테스트 키트에서 돈다 — `bun run test:panel`(claude CLI 필요, CI 밖). 순수 판정은 data.test.ts(bun, CI).
import type { On } from 'claude-code';
import type { Engine } from 'claude-code/testing';
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
function world(
  on: On,
  opts: {
    board: string | null;
    /** 시작할 때 이미 붙어 있는 표면(`$.session.surfaces()`). */
    surfaces?: ('terminal' | 'desktop' | 'vscode' | 'mobile')[];
    /** 시작할 때 엔진의 기록(`$.ui.panes()`)에 패널이 이미 열려 있다 — 모듈이 다시 로드된 경우. */
    paneOpen?: boolean;
    /** `$.ui.panes()` 가 거절된다. */
    panesFail?: boolean;
  },
) {
  const seen = {
    fetched: [] as string[],
    usageMarked: [] as string[],
    clients: new Set<string>(),
    opened: 0,
    /** `$.ui.panes()` 를 물은 횟수 — 타이머가 도는지 본다. */
    panes: 0,
    commands: [] as string[],
  };
  /** 데몬 쪽 상태 — 테스트가 바꾸면 다음 폴링이 본다. */
  const daemon = {
    lastId: 100,
    entries: [] as { boardKey: string | null }[],
    changesStatus: 200,
    collect: 2,
  };
  /** 엔진의 패널 기록 — `$.ui.open` 이 열고, 테스트가 `closePane` 으로 닫는다(사람이 닫은 것처럼). `late` 면 1초 늦게 답한다. */
  const pane = { open: opts.paneOpen ?? false, late: false };
  const clock = mock.clock(on);
  on('session.start', (_$, e) => ({ cwd: e.cwd }));
  on('session.attach', (_$, e) => ({ clientId: e.clientId }));
  on('session.surfaces', () => ({ value: opts.surfaces ?? [] }));
  on('session.cwd', () => ({ value: '/w/rocky' }));
  on('command.register', (_$, e) => {
    seen.commands.push(e.name);
    return { value: { command: e.name } };
  });
  on('ui.open', () => {
    seen.opened += 1;
    pane.open = true;
    return { value: { isPlaced: true as const } };
  });
  on('ui.panes', async () => {
    seen.panes += 1;
    if (opts.panesFail) {
      throw new Error('panes refused');
    }
    // 물은 순간의 기록 — 늦게 답해도 그때 것을 돌려준다.
    const open = pane.open;
    if (pane.late) {
      await clock.sleep(1_000);
    }
    return {
      value: open
        ? [
            {
              id: 'rocky-board',
              title: 'rocky 보드',
              isShown: true,
              isFocused: false,
              isPlaced: true,
            },
          ]
        : [],
    };
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
  /** 사람이 패널을 닫은 것처럼 — 엔진의 기록에서 빠진다. */
  const closePane = () => {
    pane.open = false;
  };
  /** 이제부터 패널 기록을 1초 늦게 답한다. */
  const panesLate = () => {
    pane.late = true;
  };
  return { seen, daemon, clock, touch, closePane, panesLate };
}

/** 사람이 `/rocky-board` 를 친다. */
const openPanel = ($: Engine) =>
  $.command.run({
    command: 'rocky-board',
    args: '',
    origin: { kind: 'composer' },
    presentation: { isFullscreen: false, columns: 120 },
  });

const sorted = (xs: readonly string[]) => [...xs].sort();

test('세션을 열어도 패널을 저절로 열지 않고 데몬도 묻지 않는다 — 명령만 등록하고 타이머도 켜지 않는다', async ($, on) => {
  const { seen, clock } = world(on, { board: 'rocky' });
  await $.session.start({ cwd: '/w/rocky', surface: 'terminal', isInteractive: true });
  await clock.advance(15_000);
  expect(seen.commands).toEqual(['rocky-board']);
  expect(seen.opened).toBe(0);
  expect(seen.fetched).toEqual([]);
  // 시작할 때 이어받을 패널이 있는지 한 번 볼 뿐이다.
  expect(seen.panes).toBe(1);
});

test('/rocky-board 로 열면 그때부터 읽고, 이 보드를 건드린 변화가 있을 때만 다시 읽는다', async ($, on) => {
  const { seen, clock, touch } = world(on, { board: 'rocky' });
  await $.session.start({ cwd: '/w/rocky', surface: 'terminal', isInteractive: true });
  expect(await openPanel($)).toEqual({ text: 'rocky 보드 패널을 열었다.' });
  await clock.settle();
  expect(seen.opened).toBe(1);
  // 첫 읽기(요약·마지막 id·할 일)와 사용 표시(`use`, 가장 가벼운 읽기) — 둘은 나란히 간다.
  expect(sorted(seen.fetched)).toEqual([
    '/api/changes',
    '/api/changes',
    '/api/summary',
    '/api/todos',
  ]);
  // 패널의 요청은 모두 사용 로그용 클라이언트를 달고, 사용 표시는 세션마다 한 번씩.
  expect([...seen.clients]).toEqual(['claude-code-panel']);
  expect(sorted(seen.usageMarked)).toEqual(['/api/changes:use', '/api/todos:open']);

  // 다음 폴링 — 새 히스토리가 없으면 그것만 묻는다.
  let at = seen.fetched.length;
  await clock.advance(5_000);
  expect(seen.fetched.slice(at)).toEqual(['/api/changes']);

  // 다른 보드만 바뀌었으면 다시 읽지 않는다.
  touch('other', null);
  at = seen.fetched.length;
  await clock.advance(5_000);
  expect(seen.fetched.slice(at)).toEqual(['/api/changes']);

  // 이 보드가 바뀌면 할 일만 다시 읽는다(요약은 아니다). 사용 표시는 되풀이하지 않는다.
  touch('rocky');
  at = seen.fetched.length;
  await clock.advance(5_000);
  expect(seen.fetched.slice(at)).toEqual(['/api/changes', '/api/todos']);
  expect(sorted(seen.usageMarked)).toEqual(['/api/changes:use', '/api/todos:open']);
});

test('패널을 닫으면 데몬을 묻지 않고, 다시 열면 이어 간다', async ($, on) => {
  const { seen, clock, closePane } = world(on, { board: 'rocky' });
  await $.session.start({ cwd: '/w/rocky', surface: 'terminal', isInteractive: true });
  await openPanel($);
  await clock.settle();
  closePane();
  let at = seen.fetched.length;
  await clock.advance(15_000);
  expect(seen.fetched.slice(at)).toEqual([]);
  // 다시 열면 마지막 id 부터 이어 간다 — 바뀐 게 없으면 히스토리만 묻는다.
  await openPanel($);
  await clock.settle();
  expect(seen.fetched.slice(at)).toEqual(['/api/changes']);
  at = seen.fetched.length;
  await clock.advance(5_000);
  expect(seen.fetched.slice(at)).toEqual(['/api/changes']);
  expect(seen.opened).toBe(2);
});

test('닫혔다고 본 바퀴가 도는 중에 다시 열어도 다음 바퀴를 기다리지 않고 읽는다', async ($, on) => {
  const { seen, clock, closePane, panesLate } = world(on, { board: 'rocky' });
  await $.session.start({ cwd: '/w/rocky', surface: 'terminal', isInteractive: true });
  await openPanel($);
  await clock.settle();
  closePane();
  panesLate();
  // 5초 바퀴가 패널 기록을 묻는다 — 답(닫힘)은 1초 뒤에 온다.
  await clock.advance(5_000);
  // 그 사이에 다시 연다 — 도는 바퀴는 닫힘을 보고 돌아서지만, 끝나자마자 한 번 더 돈다.
  await openPanel($);
  const at = seen.fetched.length;
  await clock.advance(2_000);
  expect(seen.fetched.slice(at)).toEqual(['/api/changes']);
});

test('패널 기록을 못 읽으면 데몬 탓으로 돌리지 않고 그 사유를 보인다', async ($, on) => {
  const { seen, clock } = world(on, { board: 'rocky', panesFail: true });
  await $.session.start({ cwd: '/w/rocky', surface: 'terminal', isInteractive: true });
  await openPanel($);
  await clock.settle();
  // 사용 표시(`use`)만 나가고 보드는 묻지 않는다.
  expect(seen.fetched).toEqual(['/api/changes']);
  const ui = await $.ui.mount({
    plugin: 'rocky',
    surface: 'terminal',
    component: 'Pane',
    requestId: 'rocky-board',
    props: PANE_PROPS(80),
    viewport: { columns: 80, rows: 24 },
  });
  expect(await ui.find({ text: /패널 상태를 읽지 못했다/ })).toBeDefined();
  expect(await ui.find({ text: /데몬에 닿지 않는다/ })).toBeUndefined();
});

test('모듈이 다시 로드됐는데 패널이 열려 있으면 바로 읽는다', async ($, on) => {
  const { seen, clock } = world(on, { board: 'rocky', paneOpen: true });
  await $.session.start({ cwd: '/w/rocky', surface: 'terminal', isInteractive: true });
  await clock.settle();
  expect(seen.opened).toBe(0);
  expect(seen.fetched).toEqual(['/api/summary', '/api/changes', '/api/todos']);
});

test('첫 읽기 전에는 보드가 없다고 하지 않고 읽는 중이라고 보인다', async ($, on) => {
  const { clock } = world(on, { board: 'rocky' });
  await $.session.start({ cwd: '/w/rocky', surface: 'terminal', isInteractive: true });
  await clock.settle();
  const ui = await $.ui.mount({
    plugin: 'rocky',
    surface: 'terminal',
    component: 'Pane',
    requestId: 'rocky-board',
    props: PANE_PROPS(80),
    viewport: { columns: 80, rows: 24 },
  });
  expect(await ui.find({ text: '읽는 중…' })).toBeDefined();
  expect(await ui.find({ text: /보드 없음/ })).toBeUndefined();
});

test('/rocky-board 를 다시 치거나 패널에 포커스를 줘도 사용은 세션마다 한 번만 센다', async ($, on) => {
  const { seen, clock } = world(on, { board: 'rocky' });
  await $.session.start({ cwd: '/w/rocky', surface: 'terminal', isInteractive: true });
  await openPanel($);
  await clock.settle();
  await openPanel($);
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
  expect(sorted(seen.usageMarked)).toEqual(['/api/changes:use', '/api/todos:open']);
});

test('패널에 포커스만 줘도 사용으로 센다', async ($, on) => {
  const { seen, clock } = world(on, { board: 'rocky' });
  await $.session.start({ cwd: '/w/rocky', surface: 'terminal', isInteractive: true });
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
  expect(seen.usageMarked).toEqual(['/api/changes:use']);
});

test('수집함 수는 히스토리에 남지 않아 요약을 60초마다 다시 읽는다', async ($, on) => {
  const { seen, daemon, clock } = world(on, { board: 'rocky' });
  await $.session.start({ cwd: '/w/rocky', surface: 'terminal', isInteractive: true });
  await openPanel($);
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
  await openPanel($);
  await clock.settle();
  daemon.changesStatus = 500;
  const at = seen.fetched.length;
  await clock.advance(15_000);
  expect(seen.fetched.slice(at)).toEqual(['/api/changes', '/api/changes', '/api/changes']);
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

test('보드가 없는 폴더에서 열면 붙은 보드가 없다고 보이고 할 일은 묻지 않는다', async ($, on) => {
  const { seen, clock } = world(on, { board: null });
  await $.session.start({ cwd: '/tmp', surface: 'terminal', isInteractive: true });
  await openPanel($);
  await clock.settle();
  expect(seen.fetched).not.toContain('/api/todos');
  expect(seen.usageMarked).toEqual(['/api/changes:use']);
  const ui = await $.ui.mount({
    plugin: 'rocky',
    surface: 'terminal',
    component: 'Pane',
    requestId: 'rocky-board',
    props: PANE_PROPS(80),
    viewport: { columns: 80, rows: 24 },
  });
  expect(await ui.find({ text: '이 폴더에 붙은 보드가 없다.' })).toBeDefined();
});

test('아무 표면도 없는 실행(-p)에서는 아무것도 하지 않는다', async ($, on) => {
  const { seen, clock } = world(on, { board: 'rocky' });
  await $.session.start({ cwd: '/w/rocky', surface: null, isInteractive: false });
  await clock.advance(10_000);
  expect(seen.commands).toEqual([]);
  expect(seen.fetched).toEqual([]);
});

test('원격 제어처럼 SDK 로 시작한 세션은 앱이 붙을 때 명령을 등록한다 — 여러 번 붙어도 한 번', async ($, on) => {
  const { seen, clock } = world(on, { board: 'rocky' });
  await $.session.start({ cwd: '/w/rocky', surface: null, isInteractive: false });
  await clock.advance(10_000);
  expect(seen.commands).toEqual([]);
  await $.session.attach({ surface: 'desktop', clientId: 'app-1' });
  await clock.settle();
  expect(seen.commands).toEqual(['rocky-board']);
  await $.session.attach({ surface: 'mobile', clientId: 'phone-1' });
  await clock.settle();
  expect(seen.commands).toEqual(['rocky-board']);
  // 여기서도 저절로 열거나 묻지 않는다.
  expect(seen.opened).toBe(0);
  expect(seen.fetched).toEqual([]);
});

test('시작할 때 이미 붙은 원격 표면이 있으면 바로 명령을 등록한다', async ($, on) => {
  const { seen, clock } = world(on, { board: 'rocky', surfaces: ['desktop'] });
  await $.session.start({ cwd: '/w/rocky', surface: null, isInteractive: false });
  await clock.settle();
  expect(seen.commands).toEqual(['rocky-board']);
  expect(seen.opened).toBe(0);
});

for (const surface of ['terminal', 'desktop'] as const) {
  for (const columns of [28, 80]) {
    test(`${surface} ${columns}칸 — 끝낸 것은 빼고, 제목은 자르지 않고 줄을 바꿔 그린다`, async ($, on) => {
      const { clock } = world(on, { board: 'rocky' });
      await $.session.start({ cwd: '/w/rocky', surface, isInteractive: true });
      await openPanel($);
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
