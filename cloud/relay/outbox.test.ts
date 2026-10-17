import { describe, expect, test } from 'bun:test';
import type { SyncOp } from '../../web/sync/replica';
import { createKey, FAILED_LIMIT, OUTBOX_LIMIT, Outbox } from './outbox';
import { migrate } from './storage';
import { memoryStorage } from './test-support';

function setup() {
  const storage = memoryStorage();
  migrate(storage);
  let n = 0;
  return { outbox: new Outbox(storage), newId: () => `o${++n}` };
}

const AT = '2026-10-17T00:00:00.000Z';
const create: SyncOp = { op: 'todo.create', id: 'abcd1234', board: 'rocky', title: '새 할 일' };
const status: SyncOp = { op: 'todo.status', id: 'abcd1234', action: 'done' };

describe('createKey', () => {
  test('op 하나짜리 만들기만 열쇠가 있다', () => {
    expect(createKey([create])).toBe('todo.create:abcd1234');
    expect(createKey([{ op: 'note.create', id: 'n1', title: '메모' }])).toBe('note.create:n1');
    expect(createKey([{ op: 'section.create', board: 'rocky', title: '할 일' }])).toBe(
      'section.create:["rocky","할 일"]',
    );
    expect(createKey([status])).toBeNull();
    expect(createKey([create, status])).toBeNull();
    expect(createKey([])).toBeNull();
  });
});

describe('Outbox', () => {
  test('도착 순서대로 두고 맨 앞부터 낸다', () => {
    const { outbox, newId } = setup();
    outbox.enqueue({ actor: 'minjun', ops: [create], at: AT, conn: 'p1', tx: 'tx1' }, newId);
    outbox.enqueue({ actor: 'minjun', ops: [status], at: AT, conn: 'p1', tx: 'tx2' }, newId);
    expect(outbox.size()).toBe(2);
    expect(outbox.head()).toEqual({
      id: 'o1',
      actor: 'minjun',
      ops: [create],
      at: AT,
      conn: 'p1',
      tx: 'tx1',
      sentTo: null,
    });
    outbox.markSent('o1', 'd1');
    expect(outbox.head()?.sentTo).toBe('d1');
    expect(outbox.entries().map((e) => e.id)).toEqual(['o1', 'o2']);
  });

  test('같은 만들기는 새 칸을 만들지 않고 답을 받을 연결만 바꾼다', () => {
    const { outbox, newId } = setup();
    outbox.enqueue({ actor: 'minjun', ops: [create], at: AT, conn: 'p1', tx: 'tx1' }, newId);
    outbox.markSent('o1', 'd1');
    const again = outbox.enqueue(
      { actor: 'minjun', ops: [create], at: AT, conn: 'p2', tx: 'tx9' },
      newId,
    );
    expect(again.kind).toBe('merged');
    expect(outbox.size()).toBe(1);
    expect(outbox.head()).toMatchObject({ id: 'o1', conn: 'p2', tx: 'tx9', sentTo: 'd1' });
    // 만들기가 아닌 쓰기는 같아도 따로 쌓는다. 두 번 보낸 것이 맞다
    outbox.enqueue({ actor: 'minjun', ops: [status], at: AT, conn: 'p2', tx: 'tx10' }, newId);
    outbox.enqueue({ actor: 'minjun', ops: [status], at: AT, conn: 'p2', tx: 'tx11' }, newId);
    expect(outbox.size()).toBe(3);
  });

  test('꽉 차면 받지 않지만 같은 만들기는 합친다', () => {
    const { outbox, newId } = setup();
    outbox.enqueue({ actor: 'minjun', ops: [create], at: AT, conn: 'p1', tx: 'tx0' }, newId);
    for (let i = 1; i < OUTBOX_LIMIT; i++) {
      outbox.enqueue({ actor: 'minjun', ops: [status], at: AT, conn: 'p1', tx: `tx${i}` }, newId);
    }
    expect(outbox.size()).toBe(OUTBOX_LIMIT);
    expect(
      outbox.enqueue({ actor: 'minjun', ops: [status], at: AT, conn: 'p1', tx: 'over' }, newId)
        .kind,
    ).toBe('full');
    expect(
      outbox.enqueue({ actor: 'minjun', ops: [create], at: AT, conn: 'p2', tx: 'retry' }, newId)
        .kind,
    ).toBe('merged');
  });

  test('답을 받으면 지우고, 거절은 최근 것부터 남긴다', () => {
    const { outbox, newId } = setup();
    outbox.enqueue({ actor: 'minjun', ops: [create], at: AT, conn: 'p1', tx: 'tx1' }, newId);
    outbox.enqueue({ actor: 'minjun', ops: [status], at: AT, conn: 'p1', tx: 'tx2' }, newId);

    expect(outbox.settle('o1')?.tx).toBe('tx1');
    expect(outbox.settle('o1')).toBeNull();
    expect(outbox.failed()).toEqual([]);

    const settled = outbox.settle('o2', { opIndex: 0, error: 'todo not found', failedAt: AT });
    expect(settled?.id).toBe('o2');
    expect(outbox.size()).toBe(0);
    expect(outbox.failed()).toEqual([
      {
        id: 'o2',
        actor: 'minjun',
        ops: [status],
        at: AT,
        opIndex: 0,
        error: 'todo not found',
        failedAt: AT,
      },
    ]);
  });

  test(`거절 기록은 ${FAILED_LIMIT}개까지`, () => {
    const { outbox, newId } = setup();
    for (let i = 0; i < FAILED_LIMIT + 3; i++) {
      const result = outbox.enqueue(
        { actor: 'minjun', ops: [status], at: AT, conn: 'p1', tx: `tx${i}` },
        newId,
      );
      if (result.kind === 'full') {
        throw new Error('unexpected full');
      }
      outbox.settle(result.entry.id, { opIndex: null, error: `e${i}`, failedAt: AT });
    }
    const failed = outbox.failed();
    expect(failed.length).toBe(FAILED_LIMIT);
    expect(failed[0]?.error).toBe(`e${FAILED_LIMIT + 2}`);
    expect(failed.at(-1)?.error).toBe('e3');
  });
});
