import { describe, expect, test } from 'bun:test';
import { DETAIL_HISTORY_EXCLUDED as STORE_DETAIL_HISTORY_EXCLUDED } from './types';
import type { Comment, HistoryEntry } from './types';
import type { TodoView } from './types';
import type { NowRow } from './lib';
import {
  alertRows,
  prRows,
  prStatus,
  advanceSeen,
  hasNoteNews,
  boardCommand,
  COPY_FEEDBACK_MS,
  DETAIL_HISTORY_EXCLUDED,
  doingWarning,
  formatStamp,
  hasUnreadComments,
  isEditableTarget,
  markSeen,
  mergeTimeline,
  readSeen,
  STALE_MS,
  type CopyRefDocument,
  type CopyRefTextArea,
  type SeenStorage,
  copyRef,
  copyRefWithFeedback,
  type ReorderSibling,
  resolveDropBefore,
  formatAge,
  needsSecondTick,
  nowRows,
} from './lib';

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
        message: '클립보드에 접근할 수 없다 — 아래 텍스트를 직접 복사해라:',
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
    expect(promptCalls).toEqual(['클립보드에 접근할 수 없다 — 아래 텍스트를 직접 복사해라:']);
  });
});

describe('isEditableTarget', () => {
  test('input / textarea / select 는 편집 중으로 본다', () => {
    for (const tagName of ['INPUT', 'TEXTAREA', 'SELECT', 'input', 'textarea', 'select']) {
      expect(isEditableTarget({ tagName } as unknown as EventTarget)).toBe(true);
    }
  });

  test('contentEditable 요소도 편집 중으로 본다', () => {
    expect(
      isEditableTarget({ tagName: 'DIV', isContentEditable: true } as unknown as EventTarget),
    ).toBe(true);
  });

  test('일반 요소와 null 은 아니다', () => {
    expect(isEditableTarget({ tagName: 'DIV' } as unknown as EventTarget)).toBe(false);
    expect(isEditableTarget({ tagName: 'BUTTON' } as unknown as EventTarget)).toBe(false);
    expect(isEditableTarget(null)).toBe(false);
  });
});

function history(partial: Partial<HistoryEntry>): HistoryEntry {
  return {
    id: 1,
    entity: 'todo',
    entityId: 'abcd1234',
    actor: 'logan',
    action: 'update',
    at: '2026-07-26T01:00:00.000Z',
    ...partial,
  };
}

function comment(partial: Partial<Comment>): Comment {
  return {
    id: 'c1',
    todoId: 'abcd1234',
    actor: 'logan',
    body: '본문',
    createdAt: '2026-07-26T02:00:00.000Z',
    updatedAt: '2026-07-26T02:00:00.000Z',
    ...partial,
  };
}

/** localStorage 대신 쓰는 인메모리 저장소. */
function fakeStorage(
  initial: Record<string, string> = {},
): SeenStorage & { data: Record<string, string> } {
  const data = { ...initial };
  return {
    data,
    getItem: (key) => data[key] ?? null,
    setItem: (key, value) => {
      data[key] = value;
    },
  };
}

describe('mergeTimeline', () => {
  test('merges newest first', () => {
    const items = mergeTimeline(
      [
        history({ id: 2, at: '2026-07-26T03:00:00.000Z' }),
        history({ id: 1, at: '2026-07-26T01:00:00.000Z' }),
      ],
      [comment({ id: 'c1', createdAt: '2026-07-26T02:00:00.000Z' })],
    );
    expect(items.map((i) => i.at)).toEqual([
      '2026-07-26T03:00:00.000Z',
      '2026-07-26T02:00:00.000Z',
      '2026-07-26T01:00:00.000Z',
    ]);
    expect(items[1]?.kind).toBe('comment');
  });

  test('drops comment/comment-edit history rows (card-represented) but keeps comment-archive/comment-unarchive (card is gone)', () => {
    const items = mergeTimeline(
      [
        history({ id: 3, action: 'comment' }),
        history({ id: 4, action: 'comment-edit' }),
        history({ id: 5, action: 'comment-archive' }),
        history({ id: 6, action: 'comment-unarchive' }),
        history({ id: 7, action: 'done' }),
      ],
      [],
    );
    expect(items.map((i) => (i.kind === 'history' ? i.entry.action : i.kind))).toEqual([
      'comment-archive',
      'comment-unarchive',
      'done',
    ]);
  });
});

describe('DETAIL_HISTORY_EXCLUDED drift guard (finding B)', () => {
  test('the browser-safe copy in ./lib matches src/store.ts exactly', () => {
    // 두 파일이 독립적으로 export 하는 같은 값 쌍이다(브라우저 번들 제약 때문에 하나로
    // 합칠 수 없다 — 각 선언부 JSDoc 참고). 셋째 액션이 추가될 때 한쪽만 고치는 걸
    // 막는 게 이 테스트의 목적이다.
    expect([...DETAIL_HISTORY_EXCLUDED].sort()).toEqual([...STORE_DETAIL_HISTORY_EXCLUDED].sort());
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

describe('seen cursor', () => {
  test('unread when there is a comment newer than the cursor', () => {
    const seen = { abcd1234: '2026-07-26T01:00:00.000Z' };
    expect(
      hasUnreadComments({ id: 'abcd1234', lastCommentAt: '2026-07-26T02:00:00.000Z' }, seen),
    ).toBe(true);
    expect(
      hasUnreadComments({ id: 'abcd1234', lastCommentAt: '2026-07-26T00:00:00.000Z' }, seen),
    ).toBe(false);
  });

  test('no comments means nothing unread', () => {
    expect(hasUnreadComments({ id: 'abcd1234' }, {})).toBe(false);
  });

  test('never seen but has a comment counts as unread', () => {
    expect(
      hasUnreadComments({ id: 'abcd1234', lastCommentAt: '2026-07-26T02:00:00.000Z' }, {}),
    ).toBe(true);
  });

  test('markSeen persists and readSeen survives malformed json', () => {
    const storage = fakeStorage();
    markSeen(storage, 'abcd1234', '2026-07-26T02:00:00.000Z');
    expect(readSeen(storage)).toEqual({ abcd1234: '2026-07-26T02:00:00.000Z' });

    const broken = fakeStorage({ 'rocky-seen-comments': '{not json' });
    expect(readSeen(broken)).toEqual({});
  });

  test('readSeen drops non-string cursors and they read as unread', () => {
    const storage = fakeStorage({
      'rocky-seen-comments': JSON.stringify({
        good: '2026-07-26T02:00:00.000Z',
        num: 1753490000000,
        obj: { at: '2026-07-26T02:00:00.000Z' },
        nul: null,
      }),
    });
    expect(readSeen(storage)).toEqual({ good: '2026-07-26T02:00:00.000Z' });

    // 걸러진 커서는 "본 적 없음" — 미확인으로 떨어져야 배지가 켜진다.
    const seen = readSeen(storage);
    expect(hasUnreadComments({ id: 'num', lastCommentAt: '2026-07-26T02:00:00.000Z' }, seen)).toBe(
      true,
    );
    expect(hasUnreadComments({ id: 'good', lastCommentAt: '2026-07-26T01:00:00.000Z' }, seen)).toBe(
      false,
    );
  });

  test('readSeen ignores a non-object payload', () => {
    expect(readSeen(fakeStorage({ 'rocky-seen-comments': '["a"]' }))).toEqual({});
    expect(readSeen(fakeStorage({ 'rocky-seen-comments': 'null' }))).toEqual({});
  });
});

describe('boardCommand', () => {
  test('참조를 보드 스킬 슬래시 커맨드로 감싼다', () => {
    expect(boardCommand('rocky-12')).toBe('/rocky:board rocky-12');
  });

  test('글로벌 메모 참조도 같은 모양이다', () => {
    expect(boardCommand('note-3')).toBe('/rocky:board note-3');
  });

  // 레거시 malformed board key 의 항목은 ref 가 raw id 로 폴백한다(`refOf`) — 그것도
  // 그대로 감싼다. 스킬은 raw id 도 참조 문법으로 받는다.
  test('raw id 폴백 ref 도 그대로 감싼다', () => {
    expect(boardCommand('921gvwnr')).toBe('/rocky:board 921gvwnr');
  });
});

describe('doingWarning', () => {
  const NOW = Date.parse('2026-07-30T12:00:00.000Z');

  /** doing 인 todo — 검증에 쓰는 필드만 넘긴다. */
  function doing(over: Partial<TodoView> = {}): TodoView {
    return {
      id: 'todo1',
      number: 1,
      boardId: 'board1',
      title: 'x',
      description: '',
      status: 'doing',
      priority: 'p4',
      labels: [],
      links: [],
      doingBy: 'claude-code',
      doingSince: new Date(NOW - 60_000).toISOString(),
      position: 0,
      createdAt: '2026-07-30T00:00:00.000Z',
      updatedAt: '2026-07-30T00:00:00.000Z',
      ref: 'rocky-todo-1',
      commentCount: 0,
      ...over,
    };
  }

  test('세션이 사라졌으면 가장 강한 경고다', () => {
    expect(doingWarning(doing({ doingState: 'gone' }), NOW)).toEqual({
      label: '세션 없음',
      title: '이 항목을 들고 있던 세션이 사라졌다',
      tone: 'dead',
    });
  });

  test('세션이 idle 이면 "멈춤" — 말을 걸면 이어지는 상태다', () => {
    expect(doingWarning(doing({ doingState: 'idle' }), NOW)?.tone).toBe('idle');
  });

  test('세션이 live 면 오래 걸려도 경고하지 않는다 — 시간 규칙보다 정확하다', () => {
    const long = doing({
      doingState: 'live',
      doingSince: new Date(NOW - STALE_MS - 60_000).toISOString(),
    });
    expect(doingWarning(long, NOW)).toBeNull();
  });

  test('판정이 없으면(구버전 데몬/unknown) 30분 규칙으로 물러난다', () => {
    const stale = doing({ doingSince: new Date(NOW - STALE_MS - 60_000).toISOString() });
    expect(doingWarning(stale, NOW)?.tone).toBe('slow');
    expect(doingWarning(doing({ doingState: 'unknown' }), NOW)).toBeNull();
  });

  test('막 시작한 항목은 조용하다', () => {
    expect(doingWarning(doing(), NOW)).toBeNull();
  });
});

describe('resolveDropBefore — 드래그 정렬 판정', () => {
  const sib = (id: string, over: Partial<ReorderSibling> = {}) => ({
    id,
    boardId: 'b1',
    ...over,
  });
  const list = [sib('a'), sib('b'), sib('c')];

  test('위 절반 드롭 = 그 항목 앞', () => {
    expect(resolveDropBefore(list, 'c', 'a', false)).toEqual({ before: 'a' });
  });

  test('아래 절반 드롭 = 다음 항목 앞 (마지막이면 맨 끝)', () => {
    expect(resolveDropBefore(list, 'a', 'b', true)).toEqual({ before: 'c' });
    expect(resolveDropBefore(list, 'a', 'c', true)).toEqual({ before: null });
  });

  test('제자리 드롭은 undefined — 불필요한 API 호출을 막는다', () => {
    expect(resolveDropBefore(list, 'a', 'a', false)).toBeUndefined();
    expect(resolveDropBefore(list, 'b', 'a', true)).toBeUndefined(); // a 아래 = b 제자리
    expect(resolveDropBefore(list, 'c', 'c', true)).toBeUndefined(); // 마지막의 아래 = 제자리
  });

  test('섹션·부모·보드가 다르면 undefined — 정렬이 아니라 소속 변경', () => {
    const mixed = [
      sib('a'),
      sib('s', { sectionId: 's1' }),
      sib('p', { parentId: 'a' }),
      sib('o', { boardId: 'b2' }),
    ];
    expect(resolveDropBefore(mixed, 'a', 's', false)).toBeUndefined();
    expect(resolveDropBefore(mixed, 'a', 'p', false)).toBeUndefined();
    expect(resolveDropBefore(mixed, 'a', 'o', false)).toBeUndefined();
  });
});

describe('nowRows', () => {
  const NOW = Date.parse('2026-09-28T03:00:00.000Z');
  const base = (over: Partial<TodoView>): TodoView => ({
    id: 't',
    number: 1,
    boardId: 'b',
    title: 't',
    description: '',
    status: 'todo',
    priority: 'p4',
    labels: [],
    links: [],
    position: 0,
    createdAt: '2026-09-01T00:00:00.000Z',
    updatedAt: '2026-09-01T00:00:00.000Z',
    ref: 'rocky-1',
    commentCount: 0,
    ...over,
  });

  test('세션 없음 → 핸드오프 → 진행중 → 읽지 않음 → 수집함 순이고, 같은 todo 는 한 행', () => {
    const todos = [
      base({
        id: 'live',
        ref: 'a-1',
        title: '도는 중',
        status: 'doing',
        doingBy: 'claude-code',
        doingSince: '2026-09-28T02:00:00.000Z',
        doingState: 'live',
        commentCount: 2,
        lastCommentAt: '2026-09-28T02:30:00.000Z',
      }),
      base({
        id: 'gone',
        ref: 'a-2',
        title: '죽음',
        status: 'doing',
        doingBy: 'claude-code',
        doingSince: '2026-08-18T00:00:00.000Z',
        doingState: 'gone',
      }),
      base({
        id: 'human',
        ref: 'a-3',
        title: '내가 듦',
        status: 'doing',
        doingBy: 'logan',
        doingSince: '2026-09-28T01:00:00.000Z',
        doingState: 'unknown',
      }),
      base({
        id: 'unread',
        ref: 'a-4',
        title: '댓글만',
        commentCount: 1,
        lastCommentAt: '2026-09-28T02:50:00.000Z',
      }),
      base({
        id: 'seen',
        ref: 'a-5',
        title: '읽은 댓글',
        commentCount: 1,
        lastCommentAt: '2026-09-27T00:00:00.000Z',
      }),
    ];
    const handoffs = [
      {
        id: 'h1',
        todoId: 'unread',
        sessionId: 's',
        note: '',
        actor: 'logan',
        status: 'pending',
        createdAt: '2026-09-28T02:40:00.000Z',
        phase: 'pending',
        unstarted: false,
        stale: false,
      },
    ] as unknown as import('./types').HandoffView[];
    const rows = nowRows(
      { todos, handoffs, seen: { seen: '2026-09-28T00:00:00.000Z' }, collect: 2 },
      NOW,
    );
    // 내 차례(우선순위순: 세션 없음 → 넘김 → 수집함) 다음에 돌고 있음(오래된 것부터).
    expect(rows.map((r) => `${r.group}:${r.kind}:${r.ref}`)).toEqual([
      'mine:dead:a-2',
      'mine:handoff:a-4',
      'mine:collect:수집함',
      'run:doing:a-3',
      'run:doing:a-1',
    ]);
    // 진행중이면서 댓글이 안 읽힌 것은 행 하나에 합쳐진다.
    const live = rows.find((r) => r.ref === 'a-1');
    expect(live?.unread).toBe(2);
    expect(live?.who).toBe('AGENT');
    expect([live?.glyph, live?.state, live?.live]).toEqual(['run', '진행중', true]);
    // 세션 판정이 없는(unknown) 진행중은 경고가 아니라 무채색 — 모름은 없음이 아니다.
    const human = rows.find((r) => r.ref === 'a-3');
    expect([human?.who, human?.glyph, human?.live]).toEqual(['YOU', 'unknown', false]);
    expect(rows.find((r) => r.ref === 'a-2')?.glyph).toBe('dead');
    // 핸드오프가 걸린 todo 의 읽지 않은 댓글은 핸드오프 행에 실린다 — 따로 행을 만들지 않는다.
    expect(rows.filter((r) => r.ref === 'a-4')).toHaveLength(1);
    expect(rows.find((r) => r.ref === 'a-4')?.unread).toBe(1);
  });

  test('집어갔는데 미착수 배달은 내 차례, 착수한 배달은 진행중 행이 대신한다', () => {
    const todos = [
      base({
        id: 'x',
        ref: 'b-1',
        title: 'x',
        status: 'doing',
        doingBy: 'claude-code',
        doingSince: '2026-09-28T02:00:00.000Z',
        doingState: 'live',
      }),
    ];
    const handoffs = [
      {
        id: 'h1',
        todoId: 'x',
        sessionId: 's',
        note: '',
        actor: 'logan',
        status: 'delivered',
        createdAt: '2026-09-28T01:00:00.000Z',
        acceptedAt: '2026-09-28T02:00:00.000Z',
        phase: 'accepted',
        unstarted: false,
        stale: false,
      },
      {
        id: 'h2',
        todoId: 'y',
        sessionId: 's2',
        note: '',
        actor: 'logan',
        status: 'delivered',
        createdAt: '2026-09-28T01:30:00.000Z',
        phase: 'delivered',
        unstarted: true,
        stale: false,
      },
    ] as unknown as import('./types').HandoffView[];
    const rows = nowRows({ todos, handoffs, seen: {} }, NOW);
    expect(rows.map((r) => `${r.group}:${r.kind}`)).toEqual(['mine:handoff', 'run:doing']);
    expect(rows[0]?.state).toBe('넘김 · 집었는데 미착수');
    expect(rows[0]?.title).toBe('(항목)');
  });

  test('읽지 않은 댓글은 끝난 일을 빼고 최근 3일 안의 것만 최신순 3개, 나머지는 요약 한 줄', () => {
    // NOW = 09-28T03:00 — 09-25T03:00 이후가 3일 안이다.
    const todos = Array.from({ length: 8 }, (_, i) =>
      base({
        id: `c${i}`,
        ref: `c-${i}`,
        title: `댓글 ${i}`,
        commentCount: 1,
        lastCommentAt: `2026-09-2${i}T12:00:00.000Z`,
      }),
    ).concat([
      base({
        id: 'done',
        ref: 'c-done',
        title: '끝난 일',
        status: 'done',
        commentCount: 3,
        lastCommentAt: '2026-09-28T00:00:00.000Z',
      }),
    ]);
    const rows = nowRows({ todos, handoffs: [], seen: {} }, NOW);
    // 3일 안: c-7(27일)·c-6·c-5 — c-8 은 없다. 행은 3개, 나머지 5건(오래된 것 포함)은 요약.
    expect(rows.map((r) => r.ref)).toEqual(['c-7', 'c-6', 'c-5', '']);
    expect(rows[3]?.group).toBe('more');
    expect(rows[3]?.title).toContain('5건 더');
    expect(rows[3]?.todoId).toBeUndefined();
  });

  test('내 차례는 5행까지, 넘치면 "N개 더" — expanded 면 다 싣는다', () => {
    const todos = Array.from({ length: 7 }, (_, i) =>
      base({
        id: `g${i}`,
        ref: `g-${i}`,
        status: 'doing',
        doingBy: 'claude-code',
        doingSince: `2026-09-2${i}T00:00:00.000Z`,
        doingState: 'gone',
      }),
    );
    const rows = nowRows({ todos, handoffs: [], seen: {} }, NOW);
    expect(rows.filter((r) => r.group === 'mine')).toHaveLength(5);
    expect(rows.at(-1)?.title).toBe('내 차례 2개 더');
    // 같은 순위 안에서는 오래 방치된 것이 위.
    expect(rows[0]?.ref).toBe('g-0');
    const all = nowRows({ todos, handoffs: [], seen: {}, expanded: true }, NOW);
    expect(all.filter((r) => r.group === 'mine')).toHaveLength(7);
    expect(all.some((r) => r.group === 'more')).toBe(false);
  });

  test('아무것도 없으면 빈 배열, 수집함은 0 이면 안 나온다', () => {
    expect(nowRows({ todos: [], handoffs: [], seen: {}, collect: 0 }, NOW)).toEqual([]);
    expect(nowRows({ todos: [], handoffs: [], seen: {}, collect: null }, NOW)).toEqual([]);
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

  test('1초 틱은 초가 흐르는 행이 있을 때만', () => {
    const row = (over: Partial<NowRow>): NowRow => ({
      key: 'k',
      kind: 'doing',
      group: 'run',
      glyph: 'run',
      ref: 'r',
      title: 't',
      who: 'AGENT',
      live: true,
      unread: 0,
      state: '진행중',
      ...over,
    });
    expect(needsSecondTick([row({ since: '2026-09-28T03:00:00.000Z' })], NOW)).toBe(true);
    expect(needsSecondTick([row({ since: '2026-09-28T01:00:00.000Z' })], NOW)).toBe(false);
    expect(needsSecondTick([row({ since: '2026-09-28T03:00:00.000Z', live: false })], NOW)).toBe(
      false,
    );
    expect(needsSecondTick([], NOW)).toBe(false);
  });
});

describe('nowRows — PR 감시', () => {
  const pr = (over: Partial<import('./types').PrSnapshot>): import('./types').PrSnapshot => ({
    repo: 'o/rocky',
    number: 1,
    title: 'PR',
    url: 'https://github.com/o/rocky/pull/1',
    state: 'OPEN',
    isDraft: false,
    base: 'main',
    head: 'abc',
    mergeState: 'CLEAN',
    ci: 'pass',
    unhandled: 0,
    decision: 0,
    ready: false,
    updatedAt: '2026-09-28T10:00:00Z',
    ...over,
  });
  test('확인·머지 가능한 것과 충돌난 것만 행이 된다 — 링크를 들고', () => {
    const rows = nowRows({
      todos: [],
      handoffs: [],
      seen: {},
      prs: [
        pr({ number: 1, ready: true }),
        pr({ number: 2, mergeState: 'DIRTY' }),
        pr({ number: 3 }),
        pr({ number: 4, state: 'MERGED', ready: true }),
      ],
    });
    // 충돌이 머지 가능보다 위다 — 손을 더 급하게 대야 한다.
    expect(rows.map((r) => [r.ref, r.state, r.glyph])).toEqual([
      ['rocky #2', 'PR 충돌', 'dead'],
      ['rocky #1', 'PR 확인·머지', 'mine'],
    ]);
    expect(rows[1]?.url).toBe('https://github.com/o/rocky/pull/1');
    expect(rows[1]?.todoId).toBeUndefined();
    expect(rows[1]?.kind).toBe('pr');
  });
});

describe('hasNoteNews', () => {
  test('본 시각 뒤에 고쳐진 노트가 있을 때만', () => {
    const seen = '2026-09-28T00:00:00.000Z';
    expect(hasNoteNews([{ updatedAt: '2026-09-28T00:00:01.000Z' }], seen)).toBe(true);
    expect(hasNoteNews([{ updatedAt: '2026-09-27T23:59:59.000Z' }], seen)).toBe(false);
    expect(hasNoteNews([], seen)).toBe(false);
    // 본 시각을 못 읽으면(저장값 깨짐) 처음부터 본 적 없는 것으로.
    expect(hasNoteNews([{ updatedAt: '2026-09-01T00:00:00.000Z' }], 'bad')).toBe(true);
  });
});

describe('advanceSeen — 서버 시각으로만 전진', () => {
  test('노트의 가장 늦은 updatedAt 으로 올리고, 뒤로 가지 않는다', () => {
    const seen = '2026-09-28T00:00:00.000Z';
    expect(
      advanceSeen(seen, [
        { updatedAt: '2026-09-28T01:00:00.000Z' },
        { updatedAt: '2026-09-28T03:00:00.000Z' },
      ]),
    ).toBe('2026-09-28T03:00:00.000Z');
    expect(advanceSeen(seen, [{ updatedAt: '2026-09-27T00:00:00.000Z' }])).toBe(seen);
    expect(advanceSeen(seen, [])).toBe(seen);
    // 기준이 깨져 있으면 노트 시각으로 새로 잡는다.
    expect(advanceSeen('bad', [{ updatedAt: '2026-09-28T01:00:00.000Z' }])).toBe(
      '2026-09-28T01:00:00.000Z',
    );
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
    expect(rows[2]?.detail).toBe('CI 도는 중 · 스레드 2');
    expect(rows[1]?.detail).toBe('확인·머지');
    // 전체 보기(null)면 레포와 무관하게.
    expect(prRows(prs, null).map((r) => r.number)).toContain(6);
  });
});

function todoFixture(over: Partial<TodoView>): TodoView {
  return {
    id: 't',
    number: 1,
    boardId: 'b',
    title: 't',
    description: '',
    status: 'todo',
    priority: 'p4',
    labels: [],
    links: [],
    position: 0,
    createdAt: '2026-09-01T00:00:00.000Z',
    updatedAt: '2026-09-01T00:00:00.000Z',
    ref: 'rocky-1',
    commentCount: 0,
    ...over,
  };
}

describe('alertRows — 알림 탭', () => {
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
      [],
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

  test('세션이 사라진 진행 중 할 일 — 보드 ref 로, 숨긴 것은 빠진다', () => {
    const todo = todoFixture({
      id: 't1',
      number: 7,
      status: 'doing',
      doingState: 'gone',
      boardId: 'b1',
    });
    const live = todoFixture({ id: 't2', status: 'doing', doingState: 'live', boardId: 'b1' });
    const rows = alertRows([], [todo, live], [{ id: 'b1', key: 'rocky' }], [], now);
    expect(rows).toHaveLength(1);
    expect(rows[0]?.detail).toBe('rocky-7');
    expect(rows[0]?.todoId).toBe('t1');
    expect(
      alertRows([], [todo], [{ id: 'b1', key: 'rocky' }], ['alert:abandoned:t1'], now),
    ).toEqual([]);
  });

  test('숨기기는 종류별 — 같은 PR 의 충돌을 숨겨도 머지 후보가 되면 다시 보인다', () => {
    const hidden = ['alert:conflict:o/rocky#4'];
    expect(
      alertRows([pr({ number: 4, mergeState: 'DIRTY', updatedAt: old })], [], [], hidden, now),
    ).toEqual([]);
    expect(alertRows([pr({ number: 4, ready: true })], [], [], hidden, now)).toHaveLength(1);
  });
});
