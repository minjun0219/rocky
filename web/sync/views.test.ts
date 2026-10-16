import { describe, expect, test } from 'bun:test';
import type {
  AgentSession,
  Board,
  Comment,
  Handoff,
  HistoryEntry,
  Note,
  Section,
  Todo,
} from '../types';
import { applyRows, emptyReplica, type SyncTables, type Tables } from './replica';
import {
  boardsOf,
  DETAIL_HISTORY_LIMIT,
  handoffPhase,
  isRefSafeBoardKey,
  isStaleHandoff,
  isUnstarted,
  matchBoard,
  noteDetail,
  noteViews,
  openHandoffs,
  refOf,
  resolveDoingState,
  sectionsOf,
  type SessionsView,
  todoDetail,
  todoHandoffs,
  todoViews,
} from './views';

const AT = '2026-10-16T00:00:00.000Z';

const board = (over: Partial<Board> = {}): Board => ({
  id: 'b1',
  key: 'rocky',
  title: 'rocky',
  createdAt: AT,
  ...over,
});
const todo = (over: Partial<Todo> = {}): Todo => ({
  id: 't1',
  number: 1,
  boardId: 'b1',
  title: '할 일',
  description: '',
  status: 'todo',
  priority: 'p4',
  labels: [],
  links: [],
  position: 1,
  createdAt: AT,
  updatedAt: AT,
  ...over,
});
const comment = (over: Partial<Comment> = {}): Comment => ({
  id: 'c1',
  todoId: 't1',
  actor: 'logan',
  body: '댓글',
  createdAt: AT,
  updatedAt: AT,
  ...over,
});
const note = (over: Partial<Note> = {}): Note => ({
  id: 'n1',
  number: 1,
  boardId: 'b1',
  title: '노트',
  content: '',
  position: 1,
  createdAt: AT,
  updatedAt: AT,
  ...over,
});
const handoff = (over: Partial<Handoff> = {}): Handoff => ({
  id: 'h1',
  todoId: 't1',
  sessionId: 'sess-1',
  note: '',
  actor: 'logan',
  status: 'pending',
  createdAt: AT,
  ...over,
});
const session = (over: Partial<AgentSession> = {}): AgentSession => ({
  cwd: '/w/rocky',
  kind: 'interactive',
  sessionId: 'sess-1',
  name: 'rocky-1',
  status: 'busy',
  startedAt: 0,
  ...over,
});
const live = (...sessions: AgentSession[]): SessionsView => ({ available: true, sessions });

function tablesOf(rows: Partial<SyncTables>): Tables {
  return applyRows(emptyReplica(), { epoch: 'e', seq: 1, full: true, rows }).tables;
}

describe('ref', () => {
  test('보드 key-번호, 보드 없는 노트는 note-N, 번호를 아직 못 받았으면 ?', () => {
    expect(refOf('rocky', 12, 'abcd1234')).toBe('rocky-12');
    expect(refOf(undefined, 3, 'abcd1234')).toBe('note-3');
    expect(refOf('rocky', 0, 'abcd1234')).toBe('rocky-?');
  });

  test('참조로 되읽을 수 없는 key 면 id', () => {
    expect(isRefSafeBoardKey('rocky-todo')).toBe(true);
    for (const key of ['', 'a b', 'a#b', 'note']) {
      expect(isRefSafeBoardKey(key)).toBe(false);
    }
    expect(refOf('a b', 2, 'abcd1234')).toBe('abcd1234');
  });
});

describe('doing 판정', () => {
  const doing = (over: Partial<Todo> = {}) =>
    todo({ status: 'doing', doingBy: 'claude-code', ...over });

  test('핸드오프로 귀속된 세션 하나만 본다 — busy 면 live, 아니면 idle, 없으면 gone', () => {
    const attributed = doing({ doingSessionId: 'sess-1' });
    expect(resolveDoingState(attributed, 'rocky', live(session()))).toBe('live');
    expect(resolveDoingState(attributed, 'rocky', live(session({ status: 'idle' })))).toBe('idle');
    expect(resolveDoingState(attributed, 'rocky', live(session({ state: 'done' })))).toBe('gone');
    expect(resolveDoingState(attributed, 'rocky', live())).toBe('gone');
    // spawn 세션은 짧은 id 로 저장된다
    expect(
      resolveDoingState(
        doing({ doingSessionId: 'abcd1234' }),
        'rocky',
        live(session({ id: 'abcd1234' })),
      ),
    ).toBe('live');
  });

  test('세션이 스스로 든 귀속은 보지 않고 보드로 근사한다', () => {
    const claimed = doing({ doingSessionId: 'sess-9', doingSessionClaimed: true });
    expect(resolveDoingState(claimed, 'rocky', live(session()))).toBe('unknown');
    expect(resolveDoingState(claimed, 'rocky', live(session({ cwd: '/w/other' })))).toBe('gone');
  });

  test('사람이 든 것, doing 이 아닌 것, 목록을 못 얻은 것은 모름', () => {
    expect(resolveDoingState(doing({ doingBy: 'logan' }), 'rocky', live())).toBe('unknown');
    expect(resolveDoingState(todo(), 'rocky', live())).toBe('unknown');
    expect(resolveDoingState(doing(), 'rocky', { available: false, sessions: [] })).toBe('unknown');
  });

  test('보드 매칭은 cwd 의 경로 조각이 key 와 같을 때만', () => {
    const sessions = [
      session({ cwd: '/w/rocky' }),
      session({ cwd: '/w/rocky-todo' }),
      session({ cwd: '/w/rocky/sub' }),
    ];
    expect(matchBoard(sessions, 'rocky')).toHaveLength(2);
    expect(matchBoard(sessions, '')).toHaveLength(0);
  });
});

describe('핸드오프 판정', () => {
  test('단계는 상태와 시각에서 접는다', () => {
    expect(handoffPhase(handoff())).toBe('pending');
    expect(handoffPhase(handoff({ status: 'cancelled' }))).toBe('cancelled');
    expect(handoffPhase(handoff({ status: 'delivered' }))).toBe('delivered');
    expect(handoffPhase(handoff({ status: 'delivered', acceptedAt: AT }))).toBe('accepted');
    expect(handoffPhase(handoff({ status: 'delivered', acceptedAt: AT, completedAt: AT }))).toBe(
      'completed',
    );
  });

  test('배달됐는데 착수가 없고 세션이 없거나 일하지 않으면 unstarted', () => {
    const delivered = handoff({ status: 'delivered' });
    expect(isUnstarted(delivered, live())).toBe(true);
    expect(isUnstarted(delivered, live(session({ status: 'idle' })))).toBe(true);
    expect(isUnstarted(delivered, live(session()))).toBe(false);
    expect(isUnstarted(handoff({ status: 'delivered', acceptedAt: AT }), live())).toBe(false);
    expect(isUnstarted(delivered, { available: false, sessions: [] })).toBe(false);
  });

  test('대기 중인데 대상 세션이 목록에 없으면 stale — 짧은 id 로는 보지 않는다', () => {
    expect(isStaleHandoff(handoff(), live())).toBe(true);
    expect(isStaleHandoff(handoff(), live(session()))).toBe(false);
    expect(
      isStaleHandoff(handoff({ sessionId: 'abcd1234' }), live(session({ id: 'abcd1234' }))),
    ).toBe(true);
    expect(isStaleHandoff(handoff(), { available: false, sessions: [] })).toBe(false);
  });
});

describe('목록', () => {
  const tables = tablesOf({
    boards: [
      board(),
      board({ id: 'b2', key: 'acorn', previousKeys: ['old-acorn'] }),
      board({ id: 'b3', key: 'gone', archivedAt: AT }),
    ],
    sections: [
      { id: 's2', boardId: 'b1', title: '둘', position: 2 },
      { id: 's1', boardId: 'b1', title: '하나', position: 1 },
      { id: 's3', boardId: 'b1', title: '보관', position: 3, archivedAt: AT } as Section,
    ],
    todos: [
      todo({ id: 't2', number: 2, position: 2 }),
      todo(),
      todo({ id: 't3', number: 3, position: 3, archivedAt: AT }),
      todo({
        id: 't4',
        number: 1,
        boardId: 'b2',
        position: 1,
        status: 'doing',
        doingBy: 'claude-code',
      }),
    ],
    comments: [
      comment(),
      comment({ id: 'c2', createdAt: '2026-10-16T02:00:00.000Z' }),
      comment({ id: 'c3', createdAt: '2026-10-16T03:00:00.000Z', archivedAt: AT }),
    ],
    notes: [note(), note({ id: 'n2', number: 1, boardId: undefined, title: '전역' })],
  });

  test('보드는 보관하지 않은 것만 key 순', () => {
    expect(boardsOf(tables).map((b) => b.key)).toEqual(['acorn', 'rocky']);
  });

  test('할 일은 보드·보관으로 거르고 위치 순, 댓글 수와 마지막 댓글 시각을 얹는다', () => {
    const rocky = todoViews(tables, { board: 'rocky', includeArchived: false });
    expect(rocky.map((t) => t.ref)).toEqual(['rocky-1', 'rocky-2']);
    expect(rocky[0]).toMatchObject({ commentCount: 2, lastCommentAt: '2026-10-16T02:00:00.000Z' });
    expect(rocky[1]?.lastCommentAt).toBeUndefined();
    expect(todoViews(tables, { board: 'rocky', includeArchived: true })).toHaveLength(3);
    expect(todoViews(tables, { board: 'all', includeArchived: false })).toHaveLength(3);
    expect(
      todoViews(tables, { board: 'old-acorn', includeArchived: false }).map((t) => t.ref),
    ).toEqual(['acorn-1']);
    expect(todoViews(tables, { board: 'nope', includeArchived: false })).toEqual([]);
  });

  test('doing 판정은 세션 목록이 있을 때 doing 인 것에만', () => {
    const without = todoViews(tables, { board: 'acorn', includeArchived: false });
    expect(without[0]?.doingState).toBeUndefined();
    const withSessions = todoViews(tables, {
      board: 'all',
      includeArchived: false,
      sessions: live(),
    });
    expect(withSessions.find((t) => t.id === 't4')?.doingState).toBe('gone');
    expect(withSessions.find((t) => t.id === 't1')?.doingState).toBeUndefined();
  });

  test('노트는 고른 보드의 것만, 전체 보기에는 전역 메모까지', () => {
    expect(noteViews(tables, { board: 'rocky', includeArchived: false }).map((n) => n.ref)).toEqual(
      ['rocky-1'],
    );
    expect(
      noteViews(tables, { board: 'all', includeArchived: false })
        .map((n) => n.ref)
        .sort(),
    ).toEqual(['note-1', 'rocky-1']);
  });

  test('섹션은 보관하지 않은 것을 위치 순, 전체 보기는 없음', () => {
    expect(sectionsOf(tables, 'rocky').map((s) => s.title)).toEqual(['하나', '둘']);
    expect(sectionsOf(tables, 'all')).toEqual([]);
  });
});

describe('열린 핸드오프', () => {
  const tables = tablesOf({
    boards: [board(), board({ id: 'b2', key: 'acorn' })],
    todos: [
      todo(),
      todo({ id: 't2', number: 2, archivedAt: AT }),
      todo({ id: 't5', boardId: 'b2' }),
    ],
    handoffs: [
      handoff({ id: 'h1', createdAt: '2026-10-16T01:00:00.000Z' }),
      handoff({ id: 'h2', status: 'delivered', createdAt: '2026-10-16T02:00:00.000Z' }),
      handoff({ id: 'h3', status: 'delivered', completedAt: AT }),
      handoff({ id: 'h4', status: 'cancelled' }),
      handoff({ id: 'h5', todoId: 't2' }),
      handoff({ id: 'h6', todoId: 't5' }),
    ],
  });

  test('대기 중이거나 배달됐는데 안 끝난 것, 보관한 할 일의 것은 빼고 새것부터', () => {
    expect(openHandoffs(tables, { board: 'rocky' }).map((h) => h.id)).toEqual(['h2', 'h1']);
    expect(openHandoffs(tables, { board: 'all' }).map((h) => h.id)).toContain('h6');
  });

  test('세션 목록이 있으면 stale·unstarted 를 판정한다', () => {
    const views = openHandoffs(tables, { board: 'rocky', sessions: live() });
    expect(views.find((h) => h.id === 'h1')).toMatchObject({ phase: 'pending', stale: true });
    expect(views.find((h) => h.id === 'h2')).toMatchObject({ phase: 'delivered', unstarted: true });
    const unknown = openHandoffs(tables, { board: 'rocky' });
    expect(unknown.every((h) => !h.stale && !h.unstarted)).toBe(true);
  });
});

describe('상세', () => {
  test('그 할 일의 히스토리(댓글 줄 빼고 새것부터 50줄)와 댓글(오래된 것부터)', () => {
    const history: HistoryEntry[] = [
      ...Array.from({ length: DETAIL_HISTORY_LIMIT + 3 }, (_, i) => ({
        id: i + 1,
        entity: 'todo' as const,
        entityId: 't1',
        actor: 'logan',
        action: 'update',
        at: AT,
      })),
      { id: 900, entity: 'todo', entityId: 't1', actor: 'logan', action: 'comment', at: AT },
      { id: 901, entity: 'todo', entityId: 't9', actor: 'logan', action: 'update', at: AT },
    ];
    const tables = tablesOf({
      boards: [board()],
      todos: [todo()],
      history,
      comments: [
        comment({ id: 'c2', createdAt: '2026-10-16T02:00:00.000Z' }),
        comment(),
        comment({ id: 'c3', archivedAt: AT }),
      ],
    });
    const detail = todoDetail(tables, 't1', { includeArchived: false });
    expect(detail?.history).toHaveLength(DETAIL_HISTORY_LIMIT);
    expect(detail?.history[0]?.id).toBe(DETAIL_HISTORY_LIMIT + 3);
    expect(detail?.history.some((h) => h.action === 'comment' || h.entityId === 't9')).toBe(false);
    expect(detail?.comments.map((c) => c.id)).toEqual(['c1', 'c2']);
    expect(todoDetail(tables, 't1', { includeArchived: true })?.comments).toHaveLength(3);
    expect(todoDetail(tables, 'nope', { includeArchived: false })).toBeUndefined();
  });
});

describe('노트 히스토리', () => {
  test('그 노트의 줄만 새것부터', () => {
    const tables = tablesOf({
      boards: [board()],
      notes: [note()],
      history: [
        { id: 1, entity: 'note', entityId: 'n1', actor: 'logan', action: 'create', at: AT },
        { id: 2, entity: 'note', entityId: 'n1', actor: 'logan', action: 'update', at: AT },
        { id: 3, entity: 'note', entityId: 'n9', actor: 'logan', action: 'update', at: AT },
      ],
    });
    const detail = noteDetail(tables, 'n1');
    expect(detail?.note.ref).toBe('rocky-1');
    expect(detail?.history.map((h) => h.id)).toEqual([2, 1]);
    expect(noteDetail(tables, 'nope')).toBeUndefined();
  });
});

describe('할 일의 핸드오프', () => {
  test('끝난 것·취소한 것까지 그 할 일 것만 새것부터', () => {
    const tables = tablesOf({
      boards: [board()],
      todos: [todo()],
      handoffs: [
        handoff({ id: 'h1', createdAt: '2026-10-16T01:00:00.000Z' }),
        handoff({ id: 'h2', status: 'cancelled', createdAt: '2026-10-16T03:00:00.000Z' }),
        handoff({
          id: 'h3',
          status: 'delivered',
          completedAt: AT,
          createdAt: '2026-10-16T02:00:00.000Z',
        }),
        handoff({ id: 'h4', todoId: 't9' }),
      ],
    });
    const views = todoHandoffs(tables, 't1', live());
    expect(views.map((h) => `${h.id}:${h.phase}`)).toEqual([
      'h2:cancelled',
      'h3:completed',
      'h1:pending',
    ]);
    expect(views.find((h) => h.id === 'h1')?.stale).toBe(true);
  });
});

describe('세션 목록보다 새 행', () => {
  test('목록을 물은 뒤에 바뀐 행은 판정하지 않는다 — 방금 뜬 세션을 "없음" 으로 그리지 않게', () => {
    const tables = tablesOf({
      boards: [board()],
      todos: [
        {
          ...todo({ id: 't1', status: 'doing', doingBy: 'claude-code', doingSessionId: 'sess-9' }),
          seq: 9,
        } as Todo,
        {
          ...todo({ id: 't2', status: 'doing', doingBy: 'claude-code', doingSessionId: 'sess-9' }),
          seq: 3,
        } as Todo,
      ],
      handoffs: [
        { ...handoff({ id: 'h1', todoId: 't1', status: 'delivered' }), seq: 9 } as Handoff,
        { ...handoff({ id: 'h2', todoId: 't2', status: 'delivered' }), seq: 3 } as Handoff,
      ],
    });
    const sessions: SessionsView = { available: true, sessions: [], seq: 5 };
    const todos = todoViews(tables, { board: 'rocky', includeArchived: false, sessions });
    expect(todos.find((t) => t.id === 't1')?.doingState).toBeUndefined();
    expect(todos.find((t) => t.id === 't2')?.doingState).toBe('gone');
    const handoffs = openHandoffs(tables, { board: 'rocky', sessions });
    expect(handoffs.find((h) => h.id === 'h1')?.unstarted).toBe(false);
    expect(handoffs.find((h) => h.id === 'h2')?.unstarted).toBe(true);
  });
});
