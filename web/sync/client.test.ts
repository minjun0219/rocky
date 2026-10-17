import { describe, expect, test } from 'bun:test';
import type { Board, Todo } from '../types';
import {
  LOAD_TIMEOUT_MS,
  PING_DEADLINE_MS,
  PING_IDLE_MS,
  type ReplicaStore,
  RESYNC_MIN_MS,
  RETRY_MAX_MS,
  STABLE_MS,
  SyncClient,
  SyncRejected,
  type TimerKind,
} from './client';
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

type Timer = { fn: () => void; ms: number; kind: TimerKind; cleared: boolean };

function setup(options: { saved?: RowsFrame; store?: ReplicaStore } = {}) {
  FakeSocket.all = [];
  const store = options.store ?? memoryReplicaStore();
  if (options.saved) {
    void store.save(options.saved, applyRows(emptyReplica(), options.saved));
  }
  const timers: Timer[] = [];
  let clock = 0;
  const client = new SyncClient({
    url: 'ws://h/api/ws?actor=logan',
    actor: 'logan',
    store,
    onChange: () => {},
    WebSocketImpl: FakeSocket,
    setTimer: (fn, ms, kind) => {
      const timer = { fn, ms, kind, cleared: false };
      timers.push(timer);
      return timer;
    },
    clearTimer: (handle) => {
      if (handle) {
        (handle as Timer).cleared = true;
      }
    },
    now: () => clock,
  });
  /** 그 종류의 마지막 타이머(지우지 않은 것)를 돌린다. */
  const fire = (kind: TimerKind) => {
    const timer = timers.filter((t) => t.kind === kind && !t.cleared).at(-1);
    if (!timer) {
      throw new Error(`no ${kind} timer`);
    }
    timer.cleared = true;
    timer.fn();
  };
  return {
    client,
    store,
    timers,
    fire,
    socket: () => FakeSocket.all.at(-1) as FakeSocket,
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
    expect(s.client.snapshot().replica.tables.todos.has('t1')).toBe(true);
    s.socket().open();
    expect(s.socket().frames('sync')).toEqual([{ t: 'sync', id: 'sync', since: 7, epoch: 'e1' }]);
  });

  test('사본 읽기가 멈춰도 기다리다 소켓을 연다', async () => {
    const stuck: ReplicaStore = {
      load: () => new Promise(() => {}),
      save: async () => {},
      clear: async () => {},
    };
    const s = setup({ store: stuck });
    const started = s.client.start();
    expect(FakeSocket.all).toHaveLength(0);
    expect(s.timers.find((t) => t.kind === 'load')?.ms).toBe(LOAD_TIMEOUT_MS);
    s.fire('load');
    await started;
    expect(FakeSocket.all).toHaveLength(1);
  });

  test('rows 를 받으면 live, 끊기면 live 가 아니다', async () => {
    const s = setup();
    await s.client.start();
    s.socket().open();
    expect(s.client.snapshot().live).toBe(false);
    s.socket().push({ t: 'rows', id: 'sync', ...full() });
    expect(s.client.snapshot().live).toBe(true);
    await Bun.sleep(0); // 저장은 차례로 — 다음 틱에 끝난다
    expect((await s.store.load())?.seq).toBe(5);
    s.socket().close();
    expect(s.client.snapshot().live).toBe(false);
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
    s.fire('retry');
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
    await expect(status).rejects.toThrow('확인하지 못했어요');
    expect(s.client.snapshot().pending.map((p) => p.op.op)).toEqual(['todo.create']);
    s.fire('retry');
    s.socket().open();
    const [resent] = s.socket().frames('tx');
    expect(resent).toMatchObject({ ops: [{ op: 'todo.create', id: 'newtodo1' }] });
    s.socket().push({ t: 'ack', id: resent?.id, seq: 9 });
    await create;
  });

  test('붙자마자 끊기는 연결은 간격을 늘리고, 오래 붙어 있던 연결이 끊기면 1초부터 다시', async () => {
    const s = setup();
    await s.client.start();
    for (let i = 0; i < 7; i++) {
      s.socket().open();
      s.socket().close();
      s.fire('retry');
    }
    const retries = () => s.timers.filter((t) => t.kind === 'retry').map((t) => t.ms);
    expect(retries()).toEqual([1_000, 2_000, 4_000, 8_000, 16_000, RETRY_MAX_MS, RETRY_MAX_MS]);
    s.socket().open();
    s.tick(STABLE_MS);
    s.socket().close();
    expect(retries().at(-1)).toBe(1_000);
  });

  test('한 번도 못 붙어도 멈추지 않고 계속 붙어 본다 — 쌓인 op 는 붙으면 보낸다', async () => {
    const s = setup();
    await s.client.start();
    const pending = s.client.send({ op: 'note.pin', id: 'n1' });
    s.socket().close();
    s.fire('retry');
    s.socket().close();
    expect(s.client.snapshot().pending).toHaveLength(1);
    s.fire('retry');
    s.socket().open();
    const [tx] = s.socket().frames('tx');
    s.socket().push({ t: 'ack', id: tx?.id, seq: 1 });
    await pending;
  });

  test('sync 의 읽기가 실패하거나 밀기가 멈추면 커서부터 다시 묻고, 5초 안이면 간격을 채운 뒤에 묻는다', async () => {
    const s = setup();
    await s.client.start();
    s.socket().open();
    s.socket().push({ t: 'rows', id: 'sync', ...full(5) });
    s.tick(RESYNC_MIN_MS);
    s.socket().push({ t: 'err', id: 'sync', error: 'read failed' });
    s.tick(1_000);
    s.socket().push({ t: 'err', error: 'read failed' });
    s.socket().push({ t: 'err', error: 'read failed' });
    const resyncs = () => s.socket().frames('sync').length;
    expect(resyncs()).toBe(2);
    expect(s.timers.filter((t) => t.kind === 'resync').map((t) => t.ms)).toEqual([
      RESYNC_MIN_MS - 1_000,
    ]);
    s.tick(RESYNC_MIN_MS - 1_000);
    s.fire('resync');
    expect(s.socket().frames('sync').at(-1)).toEqual({
      t: 'sync',
      id: 'sync',
      since: 5,
      epoch: 'e1',
    });
    expect(resyncs()).toBe(3);
  });

  test('한동안 조용하면 ping 으로 묻고, 답이 없으면 닫고 다시 붙는다', async () => {
    const s = setup();
    await s.client.start();
    s.socket().open();
    s.socket().push({ t: 'rows', id: 'sync', ...full() });
    s.tick(PING_IDLE_MS);
    s.fire('ping');
    expect(s.socket().frames('ping')).toHaveLength(1);
    s.tick(PING_DEADLINE_MS);
    s.fire('deadline');
    expect(s.client.snapshot().live).toBe(false);
    expect(s.timers.some((t) => t.kind === 'retry' && !t.cleared)).toBe(true);
  });

  test('ping 에 답이 오면 그대로 둔다', async () => {
    const s = setup();
    await s.client.start();
    s.socket().open();
    s.socket().push({ t: 'rows', id: 'sync', ...full() });
    s.tick(PING_IDLE_MS);
    s.fire('ping');
    s.tick(10);
    s.socket().push({ t: 'pong' });
    s.fire('deadline');
    expect(s.client.snapshot().live).toBe(true);
    expect(FakeSocket.all).toHaveLength(1);
  });

  test('화면이 다시 보이면 — 끊겨 있으면 바로 붙고, 붙어 있으면 살아 있는지 묻는다', async () => {
    const s = setup();
    await s.client.start();
    s.socket().open();
    s.client.wake();
    expect(s.socket().frames('ping')).toHaveLength(1);
    s.socket().close();
    s.client.wake();
    expect(FakeSocket.all).toHaveLength(2);
  });

  test('ping 의 답을 기다리는 사이 닫혀도 다음 연결은 다시 ping 으로 묻는다', async () => {
    const s = setup();
    await s.client.start();
    s.socket().open();
    s.socket().push({ t: 'rows', id: 'sync', ...full() });
    s.client.wake(); // 답을 기다리는 ping
    s.socket().close(); // 그 답이 오기 전에 브라우저가 닫힘을 알린다
    s.client.wake();
    s.socket().open();
    s.socket().push({ t: 'rows', id: 'sync', ...full() });
    s.tick(10);
    s.client.wake();
    expect(s.socket().frames('ping')).toHaveLength(1);
    s.tick(PING_DEADLINE_MS);
    s.fire('deadline');
    expect(s.client.snapshot().live).toBe(false);
  });

  test('저장이 한 번 실패하면 다음 저장은 사본 전체를 쓴다', async () => {
    const saved: RowsFrame[] = [];
    let failNext = true;
    const flaky: ReplicaStore = {
      load: async () => null,
      save: async (frame) => {
        if (failNext) {
          failNext = false;
          throw new Error('quota');
        }
        saved.push(frame);
      },
      clear: async () => {},
    };
    const s = setup({ store: flaky });
    await s.client.start();
    s.socket().open();
    s.socket().push({ t: 'rows', id: 'sync', ...full(5) });
    await Bun.sleep(0);
    s.socket().push({
      t: 'rows',
      epoch: 'e1',
      seq: 6,
      full: false,
      rows: { todos: [todo({ id: 't2' })] },
    });
    await Bun.sleep(0);
    expect(saved).toHaveLength(1);
    expect(saved[0]?.full).toBe(true);
    expect(saved[0]?.rows.todos?.map((t) => t.id).sort()).toEqual(['t1', 't2']);
  });

  test('앞 저장이 실패하기 전에 줄 선 델타도 사본 전체로 쓴다', async () => {
    const saved: RowsFrame[] = [];
    let failSlow: (() => void) | undefined;
    const slow: ReplicaStore = {
      load: async () => null,
      save: async (frame) => {
        if (frame.seq === 6) {
          await new Promise<void>((_, reject) => {
            failSlow = () => reject(new Error('aborted'));
          });
        }
        saved.push(frame);
      },
      clear: async () => {},
    };
    const delta = (seq: number, id: string) => ({
      t: 'rows',
      epoch: 'e1',
      seq,
      full: false,
      rows: { todos: [todo({ id })] },
    });
    const s = setup({ store: slow });
    await s.client.start();
    s.socket().open();
    s.socket().push({ t: 'rows', id: 'sync', ...full(5) });
    s.socket().push(delta(6, 't2')); // 이 저장이 걸려 있는 동안
    s.socket().push(delta(7, 't3')); // 다음 프레임이 온다
    await Bun.sleep(0);
    expect(failSlow).toBeDefined();
    failSlow?.();
    await s.client.idle();
    expect(saved).toHaveLength(2);
    expect(saved[1]?.full).toBe(true);
    expect(saved[1]?.rows.todos?.map((t) => t.id).sort()).toEqual(['t1', 't2', 't3']);
  });
});
