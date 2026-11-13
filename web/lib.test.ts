import { describe, expect, test } from 'bun:test';
import {
  alertRows,
  COPY_FEEDBACK_MS,
  type CopyRefDocument,
  type CopyRefTextArea,
  copyRef,
  copyRefWithFeedback,
  failingSurfaces,
  formatAge,
  formatStamp,
  mdToPlain,
  mineCount,
  nowRows,
  parseTurn,
  prRows,
  prStatus,
  shortTools,
  slowSurfaces,
  touchesPrs,
} from './lib';
import type { SessionRow } from './types';

/** copyRef 의 execCommand 폴백 경로를 DOM 없이 검증하기 위한 fake document. */
function makeFakeDocument(
  options: { execCommandResult?: boolean; execCommandThrows?: boolean } = {},
) {
  const calls: { appended: CopyRefTextArea[]; removed: CopyRefTextArea[]; execCommand: string[] } =
    {
      appended: [],
      removed: [],
      execCommand: [],
    };
  const document: CopyRefDocument = {
    createElement: (tagName) => {
      expect(tagName).toBe('textarea');
      const el: CopyRefTextArea = {
        value: '',
        style: { position: '', opacity: '' },
        setAttribute: () => {},
        select: () => {},
      };
      return el;
    },
    body: {
      appendChild: (node) => {
        calls.appended.push(node);
      },
      removeChild: (node) => {
        calls.removed.push(node);
      },
    },
    execCommand: (command) => {
      calls.execCommand.push(command);
      if (options.execCommandThrows) {
        throw new Error('execCommand 거부됨');
      }
      return options.execCommandResult ?? true;
    },
  };
  return { document, calls };
}

describe('copyRef', () => {
  test('navigator.clipboard 가 있으면 그것을 쓴다', async () => {
    const written: string[] = [];
    const ok = await copyRef('rocky#12', {
      clipboard: {
        writeText: async (t: string) => {
          written.push(t);
        },
      },
    });
    expect(ok).toBe(true);
    expect(written).toEqual(['rocky#12']);
  });

  test('clipboard 도 document 도 없으면 false 를 돌려준다 (호출자가 수동 복사 안내를 띄운다)', async () => {
    expect(await copyRef('rocky#12', {})).toBe(false);
  });

  test('clipboard 가 없고 document 가 있으면 execCommand 폴백을 쓴다 (LAN 평문 HTTP)', async () => {
    const { document, calls } = makeFakeDocument({ execCommandResult: true });
    const ok = await copyRef('rocky#12', { document });
    expect(ok).toBe(true);
    expect(calls.appended).toHaveLength(1);
    expect(calls.appended[0]?.value).toBe('rocky#12');
    expect(calls.execCommand).toEqual(['copy']);
    expect(calls.removed).toEqual(calls.appended);
  });

  test('execCommand 가 false 를 돌려주면 그 결과를 그대로 전달한다', async () => {
    const { document, calls } = makeFakeDocument({ execCommandResult: false });
    const ok = await copyRef('rocky#12', { document });
    expect(ok).toBe(false);
    expect(calls.removed).toHaveLength(1);
  });

  test('clipboard.writeText 가 거부되면 (권한 거부) document 폴백으로 내려간다', async () => {
    const { document, calls } = makeFakeDocument({ execCommandResult: true });
    const ok = await copyRef('rocky#12', {
      clipboard: {
        writeText: async () => {
          throw new Error('permission denied');
        },
      },
      document,
    });
    expect(ok).toBe(true);
    expect(calls.execCommand).toEqual(['copy']);
  });

  test('execCommand 가 throw 하면 false 를 돌려주고, 그래도 textarea 는 제거한다', async () => {
    const { document, calls } = makeFakeDocument({ execCommandThrows: true });
    const ok = await copyRef('rocky#12', { document });
    expect(ok).toBe(false);
    expect(calls.appended).toHaveLength(1);
    expect(calls.removed).toEqual(calls.appended);
  });

  test('인자를 하나만 넘기면 실제 전역(navigator/document)을 기본값으로 쓴다', async () => {
    // Bun 테스트 런타임에는 DOM 이 없어 document 가 없다 — clipboard 도 없으면 false.
    // expect 가 먼저 throw 해도 전역이 원상복구되도록 try/finally 로 감싼다 — 안 그러면
    // 이 테스트 실패 시 mutate 된 navigator 가 이후 테스트로 새어나간다.
    const original = globalThis.navigator;
    Object.defineProperty(globalThis, 'navigator', { value: {}, configurable: true });
    try {
      expect(await copyRef('rocky#12')).toBe(false);
    } finally {
      Object.defineProperty(globalThis, 'navigator', { value: original, configurable: true });
    }
  });
});

describe('copyRefWithFeedback', () => {
  test('복사 성공 시 copied 플래그를 세우고, COPY_FEEDBACK_MS 뒤 타이머로 지운다', async () => {
    const copiedCalls: boolean[] = [];
    const scheduled: Array<{ handler: () => void; ms: number }> = [];

    await copyRefWithFeedback('rocky#12', (copied) => copiedCalls.push(copied), {
      clipboard: {
        writeText: async () => {},
      },
      setTimeout: (handler, ms) => {
        scheduled.push({ handler, ms });
        return 0;
      },
    });

    expect(copiedCalls).toEqual([true]);
    expect(scheduled).toHaveLength(1);
    expect(scheduled[0]?.ms).toBe(COPY_FEEDBACK_MS);

    // 타이머를 직접 굴려 "그 뒤 지운다" 쪽도 확인 — 실제 1200ms 를 기다리지 않는다.
    scheduled[0]?.handler();
    expect(copiedCalls).toEqual([true, false]);
  });

  test('clipboard/document 둘 다 없어 복사에 실패하면 prompt 폴백을 띄우고 copied 는 세우지 않는다', async () => {
    const copiedCalls: boolean[] = [];
    const promptCalls: Array<{ message: string; defaultValue: string | undefined }> = [];

    await copyRefWithFeedback('rocky#12', (copied) => copiedCalls.push(copied), {
      prompt: (message, defaultValue) => {
        promptCalls.push({ message, defaultValue });
        return null;
      },
    });

    expect(copiedCalls).toEqual([]);
    expect(promptCalls).toEqual([
      {
        message: '클립보드에 접근하지 못했어요. 아래 텍스트를 직접 복사하세요:',
        defaultValue: 'rocky#12',
      },
    ]);
  });

  test('execCommand 폴백도 실패하면(false 반환) prompt 로 내려간다', async () => {
    const { document } = makeFakeDocument({ execCommandResult: false });
    const copiedCalls: boolean[] = [];
    const promptCalls: string[] = [];

    await copyRefWithFeedback('rocky#12', (copied) => copiedCalls.push(copied), {
      document,
      prompt: (message) => {
        promptCalls.push(message);
        return null;
      },
    });

    expect(copiedCalls).toEqual([]);
    expect(promptCalls).toEqual(['클립보드에 접근하지 못했어요. 아래 텍스트를 직접 복사하세요:']);
  });
});

describe('formatStamp', () => {
  test('shows only the time for today', () => {
    const now = new Date(2026, 6, 26, 15, 0);
    const at = new Date(2026, 6, 26, 9, 5);
    expect(formatStamp(at.toISOString(), now)).toBe('09:05');
  });

  test('shows month-day and time for other days', () => {
    const now = new Date(2026, 6, 26, 15, 0);
    const at = new Date(2026, 6, 24, 18, 30);
    expect(formatStamp(at.toISOString(), now)).toBe('07-24 18:30');
  });
});

describe('formatAge — DESIGN.md Time Display', () => {
  const NOW = Date.parse('2026-09-28T03:12:44.000Z');
  test('초가 흐르는 건 1시간 미만의 진행중뿐, 나머지는 분·시간·일, 30일부터 날짜', () => {
    expect(formatAge('2026-09-28T03:00:00.000Z', NOW, { live: true })).toBe('12:44');
    expect(formatAge('2026-09-28T03:00:00.000Z', NOW)).toBe('12분');
    expect(formatAge('2026-09-28T03:12:30.000Z', NOW)).toBe('방금');
    // 진행중이어도 1시간이 넘으면 초를 굴리지 않는다.
    expect(formatAge('2026-09-28T00:00:00.000Z', NOW, { live: true })).toBe('3시간');
    expect(formatAge('2026-09-25T00:00:00.000Z', NOW)).toBe('3일');
    // "56일 16:34:25" 대신 날짜 — 진행 기준 시각이면 "…부터".
    const old = new Date(2026, 7, 4, 12).toISOString();
    expect(formatAge(old, NOW)).toBe('8월 4일');
    expect(formatAge(old, NOW, { since: true })).toBe('8월 4일부터');
    // 미래 시각은 0 으로 — 시계가 어긋나도 음수를 보이지 않는다.
    expect(formatAge('2026-09-29T00:00:00.000Z', NOW)).toBe('방금');
    expect(formatAge('bad', NOW)).toBe('');
  });
});

describe('prRows — PR 현황', () => {
  const pr = (over: Partial<import('./types').PrSnapshot>): import('./types').PrSnapshot => ({
    repo: 'o/rocky',
    number: 1,
    title: 'PR',
    url: 'https://github.com/o/rocky/pull/1',
    state: 'OPEN',
    isDraft: false,
    base: 'main',
    head: 'abc',
    mergeState: 'BLOCKED',
    ci: 'pending',
    unhandled: 0,
    decision: 0,
    ready: false,
    updatedAt: '2026-09-28T10:00:00Z',
    ...over,
  });

  test('상태 판정 — 충돌이 먼저, 초안은 CI 와 무관하게 초안', () => {
    expect(prStatus(pr({ mergeState: 'DIRTY', ready: true }))).toBe('conflict');
    expect(prStatus(pr({ ready: true, ci: 'pass' }))).toBe('ready');
    expect(prStatus(pr({ isDraft: true, ci: 'fail' }))).toBe('draft');
    expect(prStatus(pr({ ci: 'fail', decision: 1 }))).toBe('failing');
    expect(prStatus(pr({ ci: 'pass', decision: 1 }))).toBe('decide');
    expect(prStatus(pr({ ci: 'pending', unhandled: 2 }))).toBe('waiting');
  });

  test('열린 것만, 손댈 순서 → 최근 갱신 순, 레포로 거른다', () => {
    const prs = [
      pr({ number: 1, updatedAt: '2026-09-28T09:00:00Z' }),
      pr({ number: 2, ready: true, ci: 'pass' }),
      pr({ number: 3, mergeState: 'DIRTY' }),
      pr({ number: 4, updatedAt: '2026-09-28T11:00:00Z', unhandled: 2 }),
      pr({ number: 5, state: 'MERGED' }),
      pr({ number: 6, repo: 'o/tally' }),
    ];
    const rows = prRows(prs, 'o/rocky');
    expect(rows.map((r) => [r.number, r.status])).toEqual([
      [3, 'conflict'],
      [2, 'ready'],
      [4, 'waiting'],
      [1, 'waiting'],
    ]);
    expect(rows[2]?.detail).toBe('CI 실행 중 · 스레드 2');
    expect(rows[1]?.detail).toBe('머지 후보');
    // 전체 보기(null)면 레포와 무관하게.
    expect(prRows(prs, null).map((r) => r.number)).toContain(6);
  });
});

describe('alertRows — 피드의 PR 알림', () => {
  const now = Date.parse('2026-10-01T12:00:00Z');
  const pr = (over: Record<string, unknown>) =>
    ({
      repo: 'o/rocky',
      number: 1,
      title: 'PR',
      url: 'https://github.com/o/rocky/pull/1',
      state: 'OPEN',
      isDraft: false,
      base: 'main',
      head: 'abc',
      mergeState: 'BLOCKED',
      ci: 'pending',
      unhandled: 0,
      decision: 0,
      ready: false,
      updatedAt: '2026-10-01T11:50:00Z',
      ...over,
    }) as never;
  const old = '2026-10-01T11:00:00Z';

  test('결정 필요·머지 후보는 바로, 충돌·CI 실패는 30분 넘게 그대로일 때만', () => {
    const rows = alertRows(
      [
        pr({ number: 1, ready: true }),
        pr({ number: 2, decision: 1, unhandled: 1 }),
        pr({ number: 3, mergeState: 'DIRTY' }),
        pr({ number: 4, mergeState: 'DIRTY', updatedAt: old }),
        pr({ number: 5, ci: 'fail' }),
        pr({ number: 6, ci: 'fail', updatedAt: old }),
        pr({ number: 7 }),
      ],
      [],
      now,
    );
    expect(rows.map((r) => [r.kind, r.detail])).toEqual([
      ['decide', 'rocky #2'],
      ['merge', 'rocky #1'],
      ['conflict', 'rocky #4'],
      ['ci', 'rocky #6'],
    ]);
  });

  test('숨기기는 종류별 — 같은 PR 의 충돌을 숨겨도 머지 후보가 되면 다시 보인다', () => {
    const hidden = ['alert:conflict:o/rocky#4'];
    expect(
      alertRows([pr({ number: 4, mergeState: 'DIRTY', updatedAt: old })], hidden, now),
    ).toEqual([]);
    expect(alertRows([pr({ number: 4, ready: true })], hidden, now)).toHaveLength(1);
  });
});

describe('parseTurn — 작업로그', () => {
  test('턴 기록은 요청·도구·결과로 풀고, 다른 모양은 본문 그대로', () => {
    expect(parseTurn('req: 고쳐줘 | tools: Bash(×2), Edit | did: 고쳤다')).toEqual({
      req: '고쳐줘',
      tools: 'Bash(×2), Edit',
      did: '고쳤다',
    });
    expect(parseTurn('req: 물음 | tools: (none) | did: 답')).toEqual({
      req: '물음',
      tools: '',
      did: '답',
    });
    expect(parseTurn('결정: JSONL 이 진실')).toEqual({
      req: '',
      tools: '',
      did: '결정: JSONL 이 진실',
    });
  });
});

describe('slowSurfaces · failingSurfaces — 통계', () => {
  const stat = (name: string, count: number, errors: number, p95Ms?: number) => ({
    source: 'rest',
    name,
    count,
    errors,
    lastTs: '',
    p95Ms,
  });
  test('느린 표면은 표본이 충분한 것만 p95 순', () => {
    const rows = [
      stat('a', 100, 0, 50),
      stat('b', 2, 0, 9000),
      stat('c', 10, 0, 300),
      stat('d', 10, 0),
    ];
    expect(slowSurfaces(rows).map((s) => s.name)).toEqual(['c', 'a']);
  });
  test('실패는 실패율 순', () => {
    const rows = [stat('a', 100, 5), stat('b', 4, 4), stat('c', 10, 0), stat('d', 10, 5)];
    expect(failingSurfaces(rows).map((s) => s.name)).toEqual(['b', 'd', 'a']);
  });
});

describe('mdToPlain · shortTools — 작업로그 목록의 글자', () => {
  test('마크다운 표시는 걷고 글자만 남긴다', () => {
    expect(
      mdToPlain('[#293](https://github.com/o/r/pull/293) 를 열었습니다. **원인:** `ROCKY_BIN`'),
    ).toBe('#293 를 열었습니다. 원인: ROCKY_BIN');
    expect(mdToPlain('## 제목\n- 항목 *하나*\n- [ ] 할 것\n> 인용')).toBe(
      '제목 항목 하나 할 것 인용',
    );
    expect(mdToPlain('| 대상 | 상태 |\n|---|---|\n| 데몬 | 켜짐 |')).toBe(
      '대상 · 상태 데몬 · 켜짐',
    );
    expect(mdToPlain('a * b * c')).toBe('a * b * c');
  });

  test('코드는 글자 그대로 — 펜스는 구분자만 걷고 인라인 코드의 밑줄은 강조로 읽지 않는다', () => {
    expect(mdToPlain('```sh\necho ok\n```')).toBe('echo ok');
    expect(mdToPlain('실행:\n```\n- **a**\n```\n끝')).toBe('실행: - **a** 끝');
    expect(mdToPlain('`__init__`를 호출')).toBe('__init__를 호출');
  });

  test('MCP 도구 이름은 서버:도구 로 줄인다', () => {
    expect(shortTools('mcp__plugin_playwright_playwright__browser_click(×3), Bash')).toBe(
      'playwright:browser_click(×3), Bash',
    );
    expect(shortTools('mcp__context7__query-docs')).toBe('context7:query-docs');
  });
});

describe('touchesPrs — PR 을 건드린 이벤트만 다시 받는다', () => {
  test('할 일 · 섹션 · 노트 이벤트뿐이면 다시 받지 않는다', () => {
    expect(
      touchesPrs([
        { entity: 'todo', entityId: 't', action: 'update' },
        { entity: 'section', action: 'create' },
        { entity: 'note', action: 'update' },
      ]),
    ).toBe(false);
  });

  test('PR 감시 전이 · 보드 변경 · 모르는 모양은 다시 받는다', () => {
    expect(touchesPrs([{ entity: 'board', entityId: 'b', action: 'pr-ready' }])).toBe(true);
    expect(touchesPrs([{ entity: 'board', action: 'update' }])).toBe(true);
    expect(touchesPrs([{ nope: true }])).toBe(true);
    expect(touchesPrs([null])).toBe(true);
    expect(touchesPrs([])).toBe(false);
  });
});

describe('nowRows — 답을 기다리는 에이전트', () => {
  const session = (over: Partial<SessionRow>): SessionRow => ({
    kind: 'background',
    id: '0da6a98a',
    sessionId: '0da6a98a-full',
    name: 'acorn-25',
    cwd: '/w/acorn',
    status: 'idle',
    state: 'blocked',
    startedAt: Date.parse('2026-08-09T00:00:00.000Z'),
    matched: false,
    job: { needs: '룰셋을 끌지 정해 주세요', updatedAt: '2026-08-10T08:28:00.000Z' },
    ...over,
  });

  test('blocked background 세션은 기다리는 것을 제목으로 내 차례가 된다', () => {
    const rows = nowRows({ agents: [session({})] });
    expect(rows).toHaveLength(1);
    expect(rows[0]).toMatchObject({
      kind: 'agent',
      group: 'mine',
      ref: 'acorn-25',
      title: '룰셋을 끌지 정해 주세요',
      since: '2026-08-10T08:28:00.000Z',
      state: '답 기다림',
    });
    expect(mineCount(rows)).toBe(1);
  });

  test('요약이 없으면 하던 일, 그것도 없으면 기본 문구 · 시작 시각', () => {
    const [detail] = nowRows({ agents: [session({ job: { detail: '테스트 고치는 중' } })] });
    expect(detail?.title).toBe('테스트 고치는 중');
    const [bare] = nowRows({ agents: [session({ job: undefined })] });
    expect(bare?.title).toBe('답을 기다려요');
    expect(bare?.since).toBe('2026-08-09T00:00:00.000Z');
  });

  test('실행 중 · 쉬는 interactive 세션은 행이 아니다', () => {
    const agents = [
      session({ state: 'working' }),
      session({ kind: 'interactive', id: undefined, state: undefined, pid: 1 }),
    ];
    expect(nowRows({ agents })).toEqual([]);
  });

  test('오래 기다린 것이 위, 5행을 넘으면 "N개 더" — 개수는 접힘과 무관', () => {
    const agents = Array.from({ length: 7 }, (_, i) =>
      session({ sessionId: `s${i}`, job: { updatedAt: `2026-10-0${7 - i}T00:00:00Z` } }),
    );
    const rows = nowRows({ agents });
    expect(rows).toHaveLength(6);
    expect(rows[0]?.key).toBe('agent:s6');
    expect(rows.at(-1)).toMatchObject({ group: 'more', title: '내 차례 2개 더', hidden: 2 });
    expect(mineCount(rows)).toBe(7);
    expect(nowRows({ agents, expanded: true })).toHaveLength(7);
  });
});
