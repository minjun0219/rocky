import { describe, expect, test } from 'bun:test';
import { HISTORY_LIMIT } from '../../web/sync/replica';
import { type DaemonRows, Mirror, needsFull } from './mirror';
import { migrate } from './storage';
import { memoryStorage } from './test-support';

function setup(): Mirror {
  const storage = memoryStorage();
  migrate(storage);
  return new Mirror(storage);
}

function todo(id: string, seq: number, title = id) {
  return { id, seq, boardId: 'b1', title };
}

function frame(partial: Partial<DaemonRows>): DaemonRows {
  return { epoch: 'e1', seq: 0, full: false, rows: {}, ...partial };
}

type Rows = {
  t: string;
  id?: unknown;
  epoch: string;
  seq: number;
  full: boolean;
  rows: Record<string, { id: string }[]>;
};

function parse(text: string | undefined): Rows {
  return JSON.parse(text ?? 'null') as Rows;
}

describe('Mirror', () => {
  test('데몬에게 받기 전에는 머리도 줄 행도 없다', () => {
    const mirror = setup();
    expect(mirror.head()).toBeNull();
    expect(mirror.rowsFor(null)).toBeNull();
  });

  test('통째로 받으면 세대를 올리고, 폰에게는 데몬 epoch에 세대를 붙여 준다', () => {
    const mirror = setup();
    const applied = mirror.apply(
      frame({
        seq: 5,
        full: true,
        rows: { todos: [todo('a', 4), todo('b', 5)], boards: [{ id: 'b1', seq: 1 }] },
      }),
    );
    expect(applied).toBe(true);
    expect(mirror.head()).toEqual({ daemonEpoch: 'e1', generation: 1, seq: 5 });

    const out = mirror.rowsFor(null, 'sync');
    expect(out?.full).toBe(true);
    expect(out?.count).toBe(3);
    expect(out?.cursor).toEqual({ epoch: 'e1.1', seq: 5 });
    const sent = parse(out?.text);
    expect(sent).toMatchObject({ t: 'rows', id: 'sync', epoch: 'e1.1', seq: 5, full: true });
    expect(sent.rows.todos?.map((r) => r.id).sort()).toEqual(['a', 'b']);
    // 일곱 테이블을 모두 싣는다. 데몬과 같은 모양이다
    expect(Object.keys(sent.rows)).toEqual([
      'boards',
      'sections',
      'todos',
      'comments',
      'notes',
      'history',
      'handoffs',
    ]);
  });

  test('이어 받기는 온 행을 덮어쓰고, 폰에게는 커서 뒤에 바뀐 행만 준다', () => {
    const mirror = setup();
    mirror.apply(frame({ seq: 5, full: true, rows: { todos: [todo('a', 4), todo('b', 5)] } }));
    expect(mirror.apply(frame({ seq: 7, rows: { todos: [todo('a', 7, 'a2')] } }))).toBe(true);
    expect(mirror.head()).toEqual({ daemonEpoch: 'e1', generation: 1, seq: 7 });

    const out = mirror.rowsFor({ epoch: 'e1.1', seq: 5 });
    expect(out?.full).toBe(false);
    expect(out?.count).toBe(1);
    const sent = parse(out?.text);
    expect(sent.id).toBeUndefined();
    expect(sent.rows.todos).toEqual([todo('a', 7, 'a2')]);

    const none = mirror.rowsFor({ epoch: 'e1.1', seq: 7 });
    expect(none).toMatchObject({ full: false, count: 0, cursor: { epoch: 'e1.1', seq: 7 } });
  });

  test('사본과 이어지지 않는 이어 받기는 적용하지 않는다', () => {
    const mirror = setup();
    expect(mirror.apply(frame({ seq: 3, rows: { todos: [todo('a', 3)] } }))).toBe(false);
    expect(mirror.head()).toBeNull();

    mirror.apply(frame({ seq: 5, full: true, rows: { todos: [todo('a', 4)] } }));
    expect(mirror.apply(frame({ epoch: 'e2', seq: 9, rows: { todos: [todo('z', 9)] } }))).toBe(
      false,
    );
    expect(mirror.head()).toEqual({ daemonEpoch: 'e1', generation: 1, seq: 5 });
    expect(parse(mirror.rowsFor(null)?.text).rows.todos?.map((r) => r.id)).toEqual(['a']);
  });

  test('다시 통째로 받으면 빠진 행이 사라지고 폰의 옛 커서는 통째로 받는다', () => {
    const mirror = setup();
    mirror.apply(frame({ seq: 5, full: true, rows: { todos: [todo('a', 4), todo('b', 5)] } }));
    // 같은 데몬이 보드 경계를 넘은 할 일 때문에 통째로 다시 보냈다
    mirror.apply(frame({ seq: 8, full: true, rows: { todos: [todo('a', 4)] } }));
    expect(mirror.head()).toEqual({ daemonEpoch: 'e1', generation: 2, seq: 8 });

    const out = mirror.rowsFor({ epoch: 'e1.1', seq: 5 });
    expect(out?.full).toBe(true);
    expect(out?.cursor.epoch).toBe('e1.2');
    expect(parse(out?.text).rows.todos?.map((r) => r.id)).toEqual(['a']);
  });

  test('다른 epoch, 음수, 머리보다 앞선 순번이면 통째로 준다', () => {
    const head = { daemonEpoch: 'e1', generation: 3, seq: 10 };
    expect(needsFull(null, head)).toBe(true);
    expect(needsFull({ epoch: 'e1.2', seq: 5 }, head)).toBe(true);
    expect(needsFull({ epoch: 'e1.3', seq: -1 }, head)).toBe(true);
    expect(needsFull({ epoch: 'e1.3', seq: 11 }, head)).toBe(true);
    expect(needsFull({ epoch: 'e1.3', seq: 10 }, head)).toBe(false);
    expect(needsFull({ epoch: 'e1.3', seq: 0 }, head)).toBe(false);
  });

  test('히스토리는 id를 숫자로 비교해 새 줄부터 2,000줄만 남긴다', () => {
    const mirror = setup();
    const history = Array.from({ length: HISTORY_LIMIT + 5 }, (_, i) => ({
      id: i + 1,
      seq: i + 1,
    }));
    mirror.apply(frame({ seq: history.length, full: true, rows: { history } }));
    const kept = parse(mirror.rowsFor(null)?.text).rows.history ?? [];
    expect(kept.length).toBe(HISTORY_LIMIT);
    const ids = kept.map((h) => Number(h.id));
    expect(Math.min(...ids)).toBe(6);
    expect(Math.max(...ids)).toBe(HISTORY_LIMIT + 5);
  });

  test('순번이 없는 행은 프레임의 순번으로 둔다', () => {
    const mirror = setup();
    mirror.apply(frame({ seq: 5, full: true, rows: { todos: [todo('a', 4)] } }));
    mirror.apply(frame({ seq: 6, rows: { todos: [{ id: 'b', title: 'b' }] } }));
    expect(
      parse(mirror.rowsFor({ epoch: 'e1.1', seq: 5 })?.text).rows.todos?.map((r) => r.id),
    ).toEqual(['b']);
  });
});
