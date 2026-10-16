import { describe, expect, test } from 'bun:test';
import type { Board, Todo } from '../types';
import { RESYNC_MIN_MS, RETRY_MAX_MS, SyncClient, type SyncSnapshot, SyncRejected } from './client';
import { memoryReplicaStore } from './persist';
import { applyRows, emptyReplica, type RowsFrame } from './replica';

const AT = '2026-10-16T00:00:00.000Z';
const board: Board = { id: 'b1', key: 'rocky', title: 'rocky', createdAt: AT };
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

class FakeSocket {
  static all: FakeSocket[] = [];
  readyState = 0;
  sent: Record<string, unknown>[] = [];
  onopen: ((e: Event) => void) | null = null;
  onmessage: ((e: MessageEvent) => void) | null = null;
  onclose: ((e: CloseEvent) => void) | null = null;
  onerror: ((e: Event) => void) | null = null;

  constructor(readonly url: string) {
    FakeSocket.all.push(this);
  }

  send(data: string) {
    this.sent.push(JSON.parse(data));
  }

  close() {
    this.readyState = 3;
    this.onclose?.({} as CloseEvent);
  }

  open() {
    this.readyState = 1;
    this.onopen?.({} as Event);
  }

  push(frame: Record<string, unknown>) {
    this.onmessage?.({ data: JSON.stringify(frame) } as MessageEvent);
  }

  frames(t: string) {
    return this.sent.filter((f) => f.t === t);
  }
}

function setup(options: { saved?: RowsFrame } = {}) {
  FakeSocket.all = [];
  const store = memoryReplicaStore();
  if (options.saved) {
    void store.save(options.saved, applyRows(emptyReplica(), options.saved));
  }
  const timers: { fn: () => void; ms: number }[] = [];
  const snapshots: SyncSnapshot[] = [];
  let fellBack = 0;
  let clock = 0;
  const client = new SyncClient({
    url: 'ws://h/api/ws?actor=logan',
    actor: 'logan',
    store,
    onChange: (s) => snapshots.push(s),
    onFallback: () => {
      fellBack += 1;
    },
    WebSocketImpl: FakeSocket,
    setTimer: (fn, ms) => {
      timers.push({ fn, ms });
      return timers.length;
    },
    clearTimer: () => {},
    now: () => clock,
  });
  return {
    client,
    store,
    timers,
    snapshots,
    socket: () => FakeSocket.all.at(-1) as FakeSocket,
    fellBack: () => fellBack,
    tick: (ms: number) => {
      clock += ms;
    },
  };
}

const full = (seq = 5): RowsFrame => ({
  epoch: 'e1',
  seq,
  full: true,
  rows: { boards: [board], todos: [todo()] },
});

describe('SyncClient', () => {
  test('저장한 사본으로 먼저 알리고, 붙으면 그 머리부터 이어 받는다', async () => {
    const s = setup({ saved: full(7) });
    await s.client.start();
    expect(s.snapshots[0]?.replica.tables.todos.has('t1')).toBe(true);
    s.socket().open();
    expect(s.socket().frames('sync')).toEqual([{ t: 'sync', id: 'sync', since: 7, epoch: 'e1' }]);
  });

  test('처음이면 since 없이 묻고, 받은 rows 를 적용해 저장한다', async () => {
    const s = setup();
    await s.client.start();
    s.socket().open();
    expect(s.socket().frames('sync')).toEqual([{ t: 'sync', id: 'sync' }]);
    s.socket().push({ t: 'rows', id: 'sync', ...full() });
    expect(s.client.snapshot().replica.seq).toBe(5);
    expect((await s.store.load())?.seq).toBe(5);
  });

  test('쓰기는 바로 답을 기다리는 op 가 되고, ack 가 오면 걷힌다', async () => {
    const s = setup();
    await s.client.start();
    s.socket().open();
    const done = s.client.send({ op: 'todo.status', id: 't1', action: 'start' });
    expect(s.client.snapshot().pending).toHaveLength(1);
    const [tx] = s.socket().frames('tx');
    expect(tx).toMatchObject({ ops: [{ op: 'todo.status', id: 't1', action: 'start' }] });
    s.socket().push({ t: 'ack', id: tx?.id, seq: 6 });
    await done;
    expect(s.client.snapshot().pending).toHaveLength(0);
  });

  test('reject 면 데몬의 이유로 거절된다', async () => {
    const s = setup();
    await s.client.start();
    s.socket().open();
    const done = s.client.send({ op: 'todo.update', id: 't1', title: '' });
    const [tx] = s.socket().frames('tx');
    s.socket().push({ t: 'reject', id: tx?.id, at: 0, error: 'title is required' });
    await expect(done).rejects.toBeInstanceOf(SyncRejected);
    await expect(done).rejects.toThrow('title is required');
    expect(s.client.snapshot().pending).toHaveLength(0);
  });

  test('끊긴 동안 쓴 것은 다시 붙으면 보낸다', async () => {
    const s = setup();
    await s.client.start();
    s.socket().open();
    s.socket().close();
    void s.client.send({ op: 'note.pin', id: 'n1' }).catch(() => {});
    expect(FakeSocket.all).toHaveLength(1);
    s.timers.at(-1)?.fn();
    s.socket().open();
    expect(s.socket().frames('tx')).toHaveLength(1);
  });

  test('답 전에 끊기면 만들기는 다시 보내고, 나머지는 확인하지 못했다고 거절한다', async () => {
    const s = setup();
    await s.client.start();
    s.socket().open();
    const create = s.client.send({
      op: 'todo.create',
      id: 'newtodo1',
      board: 'rocky',
      title: '새 일',
    });
    const status = s.client.send({ op: 'todo.status', id: 't1', action: 'done' });
    s.socket().close();
    await expect(status).rejects.toThrow('확인하지 못했다');
    expect(s.client.snapshot().pending.map((p) => p.op.op)).toEqual(['todo.create']);
    s.timers.at(-1)?.fn();
    s.socket().open();
    const [resent] = s.socket().frames('tx');
    expect(resent).toMatchObject({ ops: [{ op: 'todo.create', id: 'newtodo1' }] });
    s.socket().push({ t: 'ack', id: resent?.id, seq: 9 });
    await create;
  });

  test('다시 붙는 간격은 1초부터 두 배, 상한까지', async () => {
    const s = setup();
    await s.client.start();
    s.socket().open();
    for (let i = 0; i < 8; i++) {
      s.socket().close();
      s.timers.at(-1)?.fn();
    }
    expect(s.timers.map((t) => t.ms)).toEqual([
      1_000,
      2_000,
      4_000,
      8_000,
      16_000,
      RETRY_MAX_MS,
      RETRY_MAX_MS,
      RETRY_MAX_MS,
    ]);
  });

  test('한 번도 붙지 못하면 REST 로 돌린다', async () => {
    const s = setup();
    await s.client.start();
    const pending = s.client.send({ op: 'note.pin', id: 'n1' });
    s.socket().close();
    expect(s.fellBack()).toBe(1);
    await expect(pending).rejects.toThrow();
    expect(s.timers).toHaveLength(0);
  });

  test('데몬이 sync 를 모르면 REST 로 돌린다', async () => {
    const s = setup();
    await s.client.start();
    s.socket().open();
    s.socket().push({ t: 'err', id: 'sync', error: 'note is required' });
    expect(s.fellBack()).toBe(1);
  });

  test('밀기가 멈췄다는 err 면 커서부터 다시 묻되, 몰아서 묻지 않는다', async () => {
    const s = setup();
    await s.client.start();
    s.socket().open();
    s.socket().push({ t: 'rows', id: 'sync', ...full(5) });
    s.tick(RESYNC_MIN_MS);
    s.socket().push({ t: 'err', error: 'read failed' });
    s.socket().push({ t: 'err', error: 'read failed' });
    expect(s.socket().frames('sync')).toEqual([
      { t: 'sync', id: 'sync' },
      { t: 'sync', id: 'sync', since: 5, epoch: 'e1' },
    ]);
  });
});
