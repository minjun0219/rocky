import { describe, expect, test } from 'bun:test';
import type { Board, HistoryEntry, Todo } from '../types';
import { applyRows, emptyReplica, HISTORY_LIMIT, type Replica } from './replica';

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
