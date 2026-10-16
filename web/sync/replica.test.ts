import { describe, expect, test } from 'bun:test';
import type { Board, Comment, HistoryEntry, Note, Section, Todo } from '../types';
import {
  applyRows,
  emptyReplica,
  HISTORY_LIMIT,
  isRetrySafe,
  newClientId,
  overlay,
  type PendingOp,
  type Replica,
  type SyncOp,
} from './replica';

const AT = '2026-10-16T00:00:00.000Z';
const NOW = '2026-10-16T01:00:00.000Z';

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
const section = (over: Partial<Section> = {}): Section => ({
  id: 's1',
  boardId: 'b1',
  title: '진행',
  position: 1,
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
const entry = (id: number): HistoryEntry => ({
  id,
  entity: 'todo',
  entityId: 't1',
  actor: 'logan',
  action: 'update',
  at: AT,
});

function full(rows: Parameters<typeof applyRows>[1]['rows'], seq = 10): Replica {
  return applyRows(emptyReplica(), { epoch: 'e1', seq, full: true, rows });
}

const pending = (...ops: SyncOp[]): PendingOp[] =>
  ops.map((op) => ({ op, actor: 'logan', at: NOW }));

describe('applyRows', () => {
  test('통째로 받으면 사본을 바꾸고 머리를 그 순번으로', () => {
    const first = full({ boards: [board()], todos: [todo()] });
    const second = applyRows(first, {
      epoch: 'e2',
      seq: 3,
      full: true,
      rows: { boards: [board({ id: 'b2', key: 'other' })] },
    });
    expect(second.epoch).toBe('e2');
    expect(second.seq).toBe(3);
    expect([...second.tables.boards.keys()]).toEqual(['b2']);
    expect(second.tables.todos.size).toBe(0);
  });

  test('이어 받으면 온 행만 덮고, 오지 않은 테이블은 같은 맵을 둔다', () => {
    const first = full({ boards: [board()], todos: [todo(), todo({ id: 't2', number: 2 })] });
    const next = applyRows(first, {
      epoch: 'e1',
      seq: 11,
      full: false,
      rows: { todos: [todo({ id: 't2', number: 2, title: '고침' })] },
    });
    expect(next.seq).toBe(11);
    expect(next.tables.todos.get('t2')?.title).toBe('고침');
    expect(next.tables.todos.get('t1')?.title).toBe('할 일');
    expect(next.tables.boards).toBe(first.tables.boards);
    expect(next.tables.todos).not.toBe(first.tables.todos);
  });

  test('히스토리는 새 줄부터 상한까지만 든다', () => {
    const many = Array.from({ length: HISTORY_LIMIT + 5 }, (_, i) => entry(i + 1));
    const replica = full({ history: many });
    expect(replica.tables.history.size).toBe(HISTORY_LIMIT);
    expect(replica.tables.history.has('1')).toBe(false);
    expect(replica.tables.history.has(String(HISTORY_LIMIT + 5))).toBe(true);
  });
});

describe('overlay', () => {
  const base = full({
    boards: [board()],
    sections: [section()],
    todos: [
      todo(),
      todo({ id: 't2', number: 2, position: 2 }),
      todo({ id: 't3', number: 3, position: 3 }),
    ],
    comments: [comment()],
    notes: [note()],
  });

  test('겹칠 op 가 없으면 같은 테이블', () => {
    expect(overlay(base.tables, [])).toBe(base.tables);
  });

  test('새 할 일은 번호 없이 보드 끝에, 모르는 섹션 이름은 임시 섹션으로', () => {
    const tables = overlay(
      base.tables,
      pending({
        op: 'todo.create',
        id: 'newtodo1',
        board: 'rocky',
        title: '새 일',
        section: ' 나중 ',
      }),
    );
    const made = tables.todos.get('newtodo1');
    expect(made?.number).toBe(0);
    expect(made?.position).toBe(4);
    expect(made?.createdAt).toBe(NOW);
    const temp = made?.sectionId ? tables.sections.get(made.sectionId) : undefined;
    expect(temp?.title).toBe('나중');
    expect(base.tables.todos.has('newtodo1')).toBe(false); // 확정된 테이블은 그대로
  });

  test('이미 확정된 만들기는 다시 그리지 않는다 — 결과 행이 먼저 왔다', () => {
    const tables = overlay(
      base.tables,
      pending({ op: 'todo.create', id: 't1', board: 'rocky', title: '다른 제목' }),
    );
    expect(tables.todos.get('t1')?.title).toBe('할 일');
  });

  test('없는 보드에 만들면 임시 보드를 둔다(데몬이 처음 쓸 때 만든다)', () => {
    const tables = overlay(
      base.tables,
      pending({ op: 'todo.create', id: 'newtodo1', board: 'fresh', title: '첫 일' }),
    );
    const made = tables.todos.get('newtodo1');
    expect(tables.boards.get(made?.boardId ?? '')?.key).toBe('fresh');
  });

  test('고치기는 빠진 칸을 두고 null 은 지운다', () => {
    const withDue = full({
      boards: [board()],
      sections: [section()],
      todos: [todo({ due: '2026-10-20', sectionId: 's1' })],
    });
    const tables = overlay(
      withDue.tables,
      pending({ op: 'todo.update', id: 't1', title: '새 제목', due: null, section: null }),
    );
    const updated = tables.todos.get('t1');
    expect(updated?.title).toBe('새 제목');
    expect(updated?.due).toBeUndefined();
    expect(updated?.sectionId).toBeUndefined();
    expect(updated?.priority).toBe('p4');
  });

  test('상태 바꾸기는 데몬과 같은 칸을 고친다', () => {
    const started = overlay(
      base.tables,
      pending({ op: 'todo.status', id: 't1', action: 'start' }),
    ).todos.get('t1');
    expect(started).toMatchObject({ status: 'doing', doingBy: 'logan', doingSince: NOW });
    const done = overlay(
      full({ todos: [todo({ status: 'doing', doingBy: 'logan', doingSince: AT })] }).tables,
      pending({ op: 'todo.status', id: 't1', action: 'done' }),
    ).todos.get('t1');
    expect(done).toMatchObject({ status: 'done', completedAt: NOW });
    expect(done?.doingBy).toBeUndefined();
    const archived = overlay(
      base.tables,
      pending({ op: 'todo.status', id: 't1', action: 'archive' }),
    ).todos.get('t1');
    expect(archived?.archivedAt).toBe(NOW);
    expect(archived?.status).toBe('todo');
  });

  test('순서 옮기기는 before 앞으로, null 이면 맨 뒤로 가고 1 부터 다시 매긴다', () => {
    const order = (ops: SyncOp[]) =>
      [...overlay(base.tables, pending(...ops)).todos.values()]
        .sort((a, b) => a.position - b.position)
        .map((t) => `${t.id}@${t.position}`);
    expect(order([{ op: 'todo.move', id: 't3', before: 't1' }])).toEqual(['t3@1', 't1@2', 't2@3']);
    expect(order([{ op: 'todo.move', id: 't1', before: null }])).toEqual(['t2@1', 't3@2', 't1@3']);
  });

  test('댓글 쓰기·고치기·보관', () => {
    const tables = overlay(
      base.tables,
      pending(
        { op: 'comment.create', id: 'newcmt01', todoId: 't1', body: '새 댓글' },
        { op: 'comment.update', id: 'c1', body: '고친 댓글' },
        { op: 'comment.archive', id: 'newcmt01' },
      ),
    );
    expect(tables.comments.get('c1')?.body).toBe('고친 댓글');
    expect(tables.comments.get('newcmt01')).toMatchObject({ actor: 'logan', archivedAt: NOW });
  });

  test('섹션 보관은 그 섹션의 할 일을 섹션 밖으로 뺀다', () => {
    const inSection = full({ sections: [section()], todos: [todo({ sectionId: 's1' })] });
    const tables = overlay(inSection.tables, pending({ op: 'section.archive', id: 's1' }));
    expect(tables.sections.get('s1')?.archivedAt).toBe(NOW);
    expect(tables.todos.get('t1')?.sectionId).toBeUndefined();
    expect(tables.todos.get('t1')?.archivedAt).toBeUndefined();
  });

  test('노트 만들기·제목·고정·보관, 보드 제목', () => {
    const tables = overlay(
      base.tables,
      pending(
        { op: 'note.create', id: 'newnote1', title: '전역 메모' },
        { op: 'note.update', id: 'n1', title: '새 노트 제목' },
        { op: 'note.pin', id: 'n1' },
        { op: 'board.update', key: 'rocky', title: '로키', description: null },
      ),
    );
    expect(tables.notes.get('newnote1')).toMatchObject({ number: 0, title: '전역 메모' });
    expect(tables.notes.get('newnote1')?.boardId).toBeUndefined();
    expect(tables.notes.get('n1')).toMatchObject({ title: '새 노트 제목', pinnedAt: NOW });
    // 고친 행의 updatedAt 은 서버 시계 그대로 — 브라우저 시계로 "본 것" 이 앞서 가지 않게.
    expect(tables.notes.get('n1')?.updatedAt).toBe(AT);
    expect(tables.boards.get('b1')?.title).toBe('로키');
  });
});

describe('client id', () => {
  test('8자 base36', () => {
    expect(newClientId()).toMatch(/^[0-9a-z]{8}$/);
    expect(newClientId((b) => b.fill(35))).toBe('zzzzzzzz');
  });

  test('다시 보내도 되는 것은 만들기뿐', () => {
    expect(isRetrySafe({ op: 'todo.create', id: 'x', board: 'rocky', title: 't' })).toBe(true);
    expect(isRetrySafe({ op: 'section.create', board: 'rocky', title: 's' })).toBe(true);
    expect(isRetrySafe({ op: 'todo.status', id: 'x', action: 'start' })).toBe(false);
    expect(isRetrySafe({ op: 'todo.move', id: 'x', before: null })).toBe(false);
  });
});
