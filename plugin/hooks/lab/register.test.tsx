// 엔진 테스트 키트에서 돈다 — `bun run test:lab`(claude CLI 필요, CI 밖). 순수 판정은 lib.test.ts(bun, CI).
import type { On, SessionMeasureInput } from 'claude-code';
import { expect, mock, test } from 'claude-code/testing';

const PR_READY =
  'rocky: minjun0219/rocky #391 머지 후보 — feat(cli): doctor\nhttps://github.com/minjun0219/rocky/pull/391\n\nCI 녹색…';

/** 가짜 사용자 설정·데몬·세션 — 플러그인 아래에서 엔진 대신 답하고, 플러그인이 부른 것을 기록한다. */
function world(on: On, rockyJson: string | undefined) {
  const seen = {
    commands: [] as string[],
    toasts: [] as string[],
    fetched: [] as string[],
    statuses: [] as (string | undefined)[],
  };
  mock.env(on, { HOME: '/home/me' });
  on('fs.read', (_$, e) => {
    if (e.path !== '/home/me/.config/rocky/rocky.json' || rockyJson === undefined) {
      throw new Error(`ENOENT ${e.path}`);
    }
    return { value: rockyJson };
  });
  on('http.fetch', (_$, e) => {
    const url = new URL(e.url);
    seen.fetched.push(`${url.pathname}${url.search}`);
    const body =
      url.pathname === '/api/deliveries'
        ? {
            sessions: [
              { sessionId: 'S1', board: 'rocky', receivesPrFor: ['minjun0219/rocky#391'] },
            ],
            recent: [],
          }
        : { board: 'rocky', doing: 2, overdue: 0, today: 0, handoffsOpen: 0, collect: 1 };
    return { value: { status: 200, ok: true, headers: {}, text: JSON.stringify(body) } };
  });
  on('session.id', () => ({ value: 'S1' }));
  on('session.cwd', () => ({ value: '/repo' }));
  on('session.start', (_$, e) => ({ cwd: e.cwd }));
  on('session.receive', (_$, e) => ({ text: e.text }));
  on('session.measure', (_$, e) => ({ changed: [...e.changed] }));
  on('turn.complete', () => ({ text: '', reason: 'answer' }));
  on('command.register', (_$, e) => {
    seen.commands.push(e.name);
    return { value: { command: e.name } };
  });
  on('ui.toast', (_$, e) => {
    seen.toasts.push(e.text);
    return { value: undefined };
  });
  on('ui.status', (_$, e) => {
    seen.statuses.push(e.text);
    return { value: undefined };
  });
  return seen;
}

async function settle(clock: { advance: (ms: number) => Promise<void> }) {
  for (let i = 0; i < 20; i++) {
    await clock.advance(0);
  }
}

const START = { cwd: '/repo', surface: 'terminal', isInteractive: true } as const;
const PEER = { kind: 'peer-send-message' } as const;
const TURN = {
  answer: '',
  durationMs: 1,
  isAborted: false,
  turnId: 't1',
  reason: 'answer',
} as const;
const MEASURE: SessionMeasureInput = {
  context: { window: 200_000, tokens: 62_000, percent: 31 },
  rateLimits: [{ kind: 'five_hour', percentUsed: 42 }],
  changed: ['context', 'rateLimits'],
};

test('lab 블록이 없으면 명령·toast·status·데몬 요청이 하나도 없다', async ($, on) => {
  const clock = mock.clock(on);
  const seen = world(on, '{"rc":{}}');
  await $.session.start(START);
  await $.session.receive({ origin: PEER, text: PR_READY });
  await $.turn.complete(TURN);
  await $.session.measure(MEASURE);
  await settle(clock);
  expect(seen).toEqual({ commands: [], toasts: [], fetched: [], statuses: [] });
});

test('켜져 있으면 rocky 메시지만 toast 하고 본문은 그대로 넘긴다', async ($, on) => {
  const clock = mock.clock(on);
  const seen = world(on, '{"lab":{}}');
  await $.session.start(START);
  const passed = await $.session.receive({ origin: PEER, text: PR_READY });
  await $.session.receive({ origin: PEER, text: '다른 세션이 보낸 메시지' });
  await $.session.measure(MEASURE);
  await settle(clock);
  expect(seen.commands).toEqual(['rocky-lab']);
  expect(seen.toasts).toEqual(['rocky · #391 머지 후보 — feat(cli): doctor']);
  expect(seen.statuses).toEqual(['rocky lab 5h 42% · ctx 31%']);
  expect(passed.text).toBe(PR_READY);
  // 요약은 캐시 모드로만 읽는다 — 데몬은 `cached=true` 만 알아듣는다.
  expect(seen.fetched).toContain('/api/summary?cached=true&cwd=%2Frepo');
});

test('toast 를 꺼도 band 는 마지막 rocky 메시지를 받고, 서브에이전트 턴에는 읽지 않는다', async ($, on) => {
  const clock = mock.clock(on);
  const seen = world(on, '{"lab":{"toast":false}}');
  await $.session.start(START);
  await settle(clock);
  const before = seen.fetched.length;
  await $.turn.complete({ ...TURN, agentId: 'sub-1' });
  await settle(clock);
  expect(seen.fetched.length).toBe(before);
  await $.session.receive({ origin: PEER, text: PR_READY });
  await settle(clock);
  expect(seen.toasts).toEqual([]);
  const ui = await $.ui.mount({
    plugin: 'rocky',
    surface: 'terminal',
    component: 'AbovePrompt',
    props: {
      hasSurvey: false,
      isWorking: false,
      maxRows: 10,
      bodyColumns: 80,
      scroll: { offset: 0, bodyRows: 10 },
      view: {},
    },
  });
  expect(await ui.find({ type: 'Text', text: /#391 머지 후보/ })).toBeDefined();
  await ui.unmount();
});

test('band 는 터미널·데스크톱에서 보드 요약과 마지막 rocky 메시지를 그린다', async ($, on) => {
  const clock = mock.clock(on);
  world(on, '{"lab":{}}');
  await $.session.start(START);
  await $.session.receive({ origin: PEER, text: PR_READY });
  await settle(clock);
  for (const surface of ['terminal', 'desktop'] as const) {
    const ui = await $.ui.mount({
      plugin: 'rocky',
      surface,
      component: 'AbovePrompt',
      props: {
        hasSurvey: false,
        isWorking: false,
        maxRows: 10,
        bodyColumns: 80,
        scroll: { offset: 0, bodyRows: 10 },
        view: {},
      },
    });
    expect(
      await ui.find({ type: 'Text', text: /rocky rocky · 진행중 2 · 수집함 1 · PR 1건 감시/ }),
    ).toBeDefined();
    expect(await ui.find({ type: 'Text', text: /#391 머지 후보/ })).toBeDefined();
    await ui.unmount();
  }
});
