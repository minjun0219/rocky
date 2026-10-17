import { describe, expect, test } from 'bun:test';
import type { SyncOp } from '../../web/sync/replica';
import { OUTBOX_LIMIT, Outbox } from './outbox';
import { CLOSE_DAEMON_ERR, CLOSE_REPLACED, MESSAGES, Relay, type RelayOptions } from './relay';
import type { RelayStorage } from './storage';
import { FakePeer, memoryStorage } from './test-support';

const NOW = Date.parse('2026-10-17T00:00:00.000Z');

function setup() {
  const storage = memoryStorage();
  const peers: FakePeer[] = [];
  let n = 0;
  const options: RelayOptions = {
    peers: () => peers.filter((p) => !p.closed),
    now: () => NOW,
    newId: () => `id${++n}`,
  };
  let relay = new Relay(storage, options);
  return {
    storage,
    /** DO가 잠들었다 깼다. 같은 저장소와 소켓으로 새 객체를 만든다. */
    wake() {
      relay = new Relay(storage, options);
    },
    connectDaemon(): FakePeer {
      const peer = new FakePeer();
      peers.push(peer);
      relay.open(peer, { role: 'daemon' });
      return peer;
    },
    connectPhone(actor = 'minjun'): FakePeer {
      const peer = new FakePeer();
      peers.push(peer);
      relay.open(peer, { role: 'phone', actor });
      return peer;
    },
    send(peer: FakePeer, frame: unknown) {
      relay.message(peer, typeof frame === 'string' ? frame : JSON.stringify(frame));
    },
    /** 소켓이 닫혔다. */
    drop(peer: FakePeer) {
      peer.close(1000, 'gone');
      relay.close(peer);
    },
  };
}

function outboxOf(storage: RelayStorage): Outbox {
  return new Outbox(storage);
}

const todo = (id: string, seq: number) => ({ id, seq, boardId: 'b1', title: id });
const create: SyncOp = { op: 'todo.create', id: 'abcd1234', board: 'rocky', title: '새 할 일' };
const status: SyncOp = { op: 'todo.status', id: 'abcd1234', action: 'done' };

/** 데몬이 붙어 할 일 둘을 통째로 보낸 뒤(`e1`, 순번 3). */
function withBoard() {
  const ctx = setup();
  const daemon = ctx.connectDaemon();
  ctx.send(daemon, {
    t: 'rows',
    id: 'sync',
    epoch: 'e1',
    seq: 3,
    full: true,
    rows: { todos: [todo('a', 2), todo('b', 3)] },
  });
  daemon.take();
  return { ...ctx, daemon };
}

/** `rows` 프레임에 실린 할 일 id. */
function todoIds(frame: Record<string, unknown> | undefined): string[] {
  const rows = frame?.rows as { todos?: { id: string }[] } | undefined;
  return (rows?.todos ?? []).map((t) => t.id);
}

/** 사본을 받은 폰. */
function syncedPhone(ctx: ReturnType<typeof setup>, actor = 'minjun'): FakePeer {
  const phone = ctx.connectPhone(actor);
  ctx.send(phone, { t: 'sync', id: 'sync' });
  phone.take();
  return phone;
}

describe('데몬 연결', () => {
  test('붙으면 사본의 머리로 sync를 보내고, 새 연결이 오면 옛 연결을 닫는다', () => {
    const ctx = setup();
    const first = ctx.connectDaemon();
    expect(first.take()).toEqual([{ t: 'sync', id: 'sync' }]);
    ctx.send(first, { t: 'rows', id: 'sync', epoch: 'e1', seq: 3, full: true, rows: {} });

    const second = ctx.connectDaemon();
    expect(first.closed?.code).toBe(CLOSE_REPLACED);
    expect(second.take()).toEqual([{ t: 'sync', id: 'sync', since: 3, epoch: 'e1' }]);
  });

  test('옛 연결에서 늦게 온 프레임은 버린다', () => {
    const ctx = withBoard();
    const second = ctx.connectDaemon();
    second.take();
    ctx.send(ctx.daemon, { t: 'rows', epoch: 'e9', seq: 1, full: true, rows: {} });
    const phone = ctx.connectPhone();
    ctx.send(phone, { t: 'sync', id: 'sync' });
    expect(phone.take()[0]).toMatchObject({ epoch: 'e1.1', seq: 3 });
  });

  test('사본과 이어지지 않는 행은 버리고 통째로 다시 묻는다', () => {
    const ctx = withBoard();
    ctx.send(ctx.daemon, {
      t: 'rows',
      epoch: 'e2',
      seq: 9,
      full: false,
      rows: { todos: [todo('z', 9)] },
    });
    expect(ctx.daemon.take()).toEqual([{ t: 'sync', id: 'sync' }]);
  });

  test('err를 보내면 그 연결을 닫아 연결기가 간격을 두고 다시 붙게 한다', () => {
    const ctx = withBoard();
    ctx.send(ctx.daemon, { t: 'err', error: 'cannot read sync_state' });
    expect(ctx.daemon.closed?.code).toBe(CLOSE_DAEMON_ERR);
  });
});

describe('폰의 사본', () => {
  test('sync에 중계소의 epoch로 통째로 답하고, 그 뒤 데몬의 행을 민다', () => {
    const ctx = withBoard();
    const phone = ctx.connectPhone();
    ctx.send(phone, { t: 'sync', id: 'sync' });
    const [first] = phone.take();
    expect(first).toMatchObject({ t: 'rows', id: 'sync', epoch: 'e1.1', seq: 3, full: true });
    expect(todoIds(first).sort()).toEqual(['a', 'b']);

    ctx.send(ctx.daemon, {
      t: 'rows',
      epoch: 'e1',
      seq: 4,
      full: false,
      rows: { todos: [todo('c', 4)] },
    });
    expect(phone.take()).toEqual([
      {
        t: 'rows',
        epoch: 'e1.1',
        seq: 4,
        full: false,
        rows: {
          boards: [],
          sections: [],
          todos: [todo('c', 4)],
          comments: [],
          notes: [],
          history: [],
          handoffs: [],
        },
      },
    ]);

    // 바뀐 행이 없으면 보내지 않는다
    ctx.send(ctx.daemon, { t: 'rows', epoch: 'e1', seq: 4, full: false, rows: {} });
    expect(phone.take()).toEqual([]);
  });

  test('sync를 보내지 않은 연결에는 밀지 않는다', () => {
    const ctx = withBoard();
    const phone = ctx.connectPhone();
    ctx.send(ctx.daemon, {
      t: 'rows',
      epoch: 'e1',
      seq: 4,
      full: false,
      rows: { todos: [todo('c', 4)] },
    });
    expect(phone.take()).toEqual([]);
  });

  test('커서가 이 사본의 것이면 그 뒤만, 아니면 통째로 준다', () => {
    const ctx = withBoard();
    const resumed = ctx.connectPhone();
    ctx.send(resumed, { t: 'sync', id: 'sync', since: 2, epoch: 'e1.1' });
    const [delta] = resumed.take();
    expect(delta).toMatchObject({ full: false, seq: 3 });
    expect(todoIds(delta)).toEqual(['b']);

    // 이어 받을 것이 없어도 답한다. 웹은 첫 rows로 따라잡았다고 본다
    ctx.send(resumed, { t: 'sync', id: 'sync', since: 3, epoch: 'e1.1' });
    expect(resumed.take()).toMatchObject([{ t: 'rows', id: 'sync', full: false, seq: 3 }]);

    const stale = ctx.connectPhone();
    ctx.send(stale, { t: 'sync', id: 'sync', since: 2, epoch: 'e0.1' });
    expect(stale.take()).toMatchObject([{ full: true, epoch: 'e1.1' }]);
  });

  test('사본이 비어 있으면 답하지 않고 기다렸다가, 데몬의 첫 행을 통째로 민다', () => {
    const ctx = setup();
    const phone = ctx.connectPhone();
    ctx.send(phone, { t: 'sync', id: 'sync' });
    expect(phone.take()).toEqual([]);

    const daemon = ctx.connectDaemon();
    ctx.send(daemon, {
      t: 'rows',
      id: 'sync',
      epoch: 'e1',
      seq: 2,
      full: true,
      rows: { todos: [todo('a', 2)] },
    });
    expect(phone.take()).toMatchObject([{ t: 'rows', epoch: 'e1.1', seq: 2, full: true }]);
  });

  test('데몬이 다시 뜨면 폰도 통째로 받는다', () => {
    const ctx = withBoard();
    const phone = syncedPhone(ctx);
    const restarted = ctx.connectDaemon();
    ctx.send(restarted, {
      t: 'rows',
      id: 'sync',
      epoch: 'e2',
      seq: 1,
      full: true,
      rows: { todos: [todo('a', 1)] },
    });
    expect(phone.take()).toMatchObject([{ full: true, epoch: 'e2.2', seq: 1 }]);
  });
});

describe('변경함', () => {
  test('맥이 없는 동안 받은 쓰기는 데몬이 붙으면 넘기고, 폰은 결과 행 다음에 답을 받는다', () => {
    const ctx = withBoard();
    ctx.drop(ctx.daemon);
    const phone = syncedPhone(ctx);

    ctx.send(phone, { t: 'tx', id: 'tx1', ops: [create] });
    expect(phone.take()).toEqual([]);
    const [queued] = outboxOf(ctx.storage).entries();

    const daemon = ctx.connectDaemon();
    expect(daemon.take()).toEqual([
      { t: 'sync', id: 'sync', since: 3, epoch: 'e1' },
      { t: 'tx', id: queued?.id, actor: 'minjun', ops: [create] },
    ]);

    ctx.send(daemon, { t: 'rows', id: 'sync', epoch: 'e1', seq: 3, full: false, rows: {} });
    ctx.send(daemon, {
      t: 'rows',
      epoch: 'e1',
      seq: 4,
      full: false,
      rows: { todos: [todo('abcd1234', 4)] },
    });
    ctx.send(daemon, { t: 'ack', id: queued?.id, seq: 4 });
    expect(phone.take()).toMatchObject([
      { t: 'rows', seq: 4, full: false },
      { t: 'ack', id: 'tx1', seq: 4 },
    ]);
    expect(outboxOf(ctx.storage).size()).toBe(0);
  });

  test('답을 받아야 다음 쓰기를 보낸다', () => {
    const ctx = withBoard();
    const phone = syncedPhone(ctx);
    ctx.send(phone, { t: 'tx', id: 'tx1', ops: [create] });
    ctx.send(phone, { t: 'tx', id: 'tx2', ops: [status] });
    const sent = ctx.daemon.take();
    expect(sent).toMatchObject([{ t: 'tx', ops: [create] }]);
    const [first] = sent;

    ctx.send(ctx.daemon, { t: 'ack', id: first?.id, seq: 4 });
    expect(ctx.daemon.take()).toMatchObject([{ t: 'tx', ops: [status] }]);
    expect(phone.take()).toEqual([{ t: 'ack', id: 'tx1', seq: 4 }]);
  });

  test('거절은 폰에 그대로 전하고 기록을 남긴 뒤 다음으로 간다', () => {
    const ctx = withBoard();
    const phone = syncedPhone(ctx);
    ctx.send(phone, { t: 'tx', id: 'tx1', ops: [status] });
    ctx.send(phone, { t: 'tx', id: 'tx2', ops: [create] });
    const [first] = ctx.daemon.take();

    ctx.send(ctx.daemon, { t: 'reject', id: first?.id, at: 0, error: 'todo is archived' });
    expect(phone.take()).toEqual([{ t: 'reject', id: 'tx1', at: 0, error: 'todo is archived' }]);
    expect(ctx.daemon.take()).toMatchObject([{ t: 'tx', ops: [create] }]);
    expect(outboxOf(ctx.storage).failed()).toMatchObject([
      { id: first?.id, actor: 'minjun', ops: [status], opIndex: 0, error: 'todo is archived' },
    ]);
  });

  test('답을 받기 전에 데몬이 끊기면 다시 붙은 연결로 같은 쓰기를 보낸다', () => {
    const ctx = withBoard();
    const phone = syncedPhone(ctx);
    ctx.send(phone, { t: 'tx', id: 'tx1', ops: [status] });
    const [sent] = ctx.daemon.take();
    ctx.drop(ctx.daemon);

    const daemon = ctx.connectDaemon();
    expect(daemon.take()).toMatchObject([{ t: 'sync' }, { t: 'tx', id: sent?.id, ops: [status] }]);
  });

  test('폰이 다시 보낸 만들기는 합치고, 답은 새 연결로 간다', () => {
    const ctx = withBoard();
    ctx.drop(ctx.daemon);
    const before = syncedPhone(ctx);
    ctx.send(before, { t: 'tx', id: 'tx1', ops: [create] });
    ctx.drop(before);
    const after = syncedPhone(ctx);
    ctx.send(after, { t: 'tx', id: 'tx1', ops: [create] });
    expect(outboxOf(ctx.storage).size()).toBe(1);

    const daemon = ctx.connectDaemon();
    const [, tx] = daemon.take();
    ctx.send(daemon, { t: 'ack', id: tx?.id, seq: 4 });
    expect(after.take()).toEqual([{ t: 'ack', id: 'tx1', seq: 4 }]);
    expect(before.take()).toEqual([]);
  });

  test('DO가 잠들었다 깨도 사본·변경함·커서가 남는다', () => {
    const ctx = withBoard();
    const phone = syncedPhone(ctx);
    ctx.send(phone, { t: 'tx', id: 'tx1', ops: [status] });
    const [sent] = ctx.daemon.take();

    ctx.wake();
    ctx.send(ctx.daemon, {
      t: 'rows',
      epoch: 'e1',
      seq: 4,
      full: false,
      rows: { todos: [todo('a', 4)] },
    });
    ctx.send(ctx.daemon, { t: 'ack', id: sent?.id, seq: 4 });
    expect(phone.take()).toMatchObject([
      { t: 'rows', epoch: 'e1.1', seq: 4, full: false },
      { t: 'ack', id: 'tx1' },
    ]);
  });
});

describe('중계소가 거절하는 쓰기', () => {
  test('에이전트 actor의 쓰기는 변경함에 쌓지 않는다', () => {
    const ctx = withBoard();
    const agent = syncedPhone(ctx, 'claude-code');
    ctx.send(agent, { t: 'tx', id: 'tx1', ops: [status] });
    expect(agent.take()).toEqual([
      { t: 'reject', id: 'tx1', at: null, error: MESSAGES.agent('claude-code') },
    ]);
    expect(outboxOf(ctx.storage).size()).toBe(0);
  });

  test('모르는 op와 빈 ops는 받지 않는다', () => {
    const ctx = withBoard();
    const phone = syncedPhone(ctx);
    ctx.send(phone, { t: 'tx', id: 'tx1', ops: [create, { op: 'board.create', key: 'x' }] });
    ctx.send(phone, { t: 'tx', id: 'tx2', ops: [] });
    expect(phone.take()).toEqual([
      { t: 'reject', id: 'tx1', at: 1, error: MESSAGES.unknownOp('board.create') },
      { t: 'reject', id: 'tx2', at: null, error: MESSAGES.noOps },
    ]);
    expect(outboxOf(ctx.storage).size()).toBe(0);
  });

  test('변경함이 꽉 차면 받지 않는다', () => {
    const ctx = withBoard();
    ctx.drop(ctx.daemon);
    const outbox = outboxOf(ctx.storage);
    let n = 0;
    for (let i = 0; i < OUTBOX_LIMIT; i++) {
      outbox.enqueue(
        { actor: 'minjun', ops: [status], at: '', conn: 'old', tx: i },
        () => `fill${++n}`,
      );
    }
    const phone = syncedPhone(ctx);
    ctx.send(phone, { t: 'tx', id: 'tx1', ops: [status] });
    expect(phone.take()).toEqual([
      { t: 'reject', id: 'tx1', at: null, error: MESSAGES.outboxFull },
    ]);
  });
});

describe('그 밖의 프레임', () => {
  test('노트 문서 프레임은 아직 받지 않는다', () => {
    const ctx = withBoard();
    const phone = ctx.connectPhone();
    ctx.send(phone, { t: 'doc', id: 1, note: 'n1' });
    ctx.send(phone, { t: 'sub', note: 'n1' });
    ctx.send(phone, { t: 'unsub', note: 'n1' });
    expect(phone.take()).toEqual([
      { t: 'err', id: 1, error: MESSAGES.notes },
      { t: 'err', error: MESSAGES.notes },
    ]);
  });

  test('ping에는 pong, 읽지 못한 프레임과 모르는 프레임에는 err로 답한다', () => {
    const ctx = withBoard();
    const phone = ctx.connectPhone();
    ctx.send(phone, { t: 'ping' });
    ctx.send(phone, { t: 'ping', id: 7 });
    ctx.send(phone, 'not json');
    ctx.send(phone, { t: 'hello', id: 2 });
    ctx.send(ctx.daemon, { t: 'ping', id: 'k' });
    expect(phone.take()).toEqual([
      { t: 'pong' },
      { t: 'pong', id: 7 },
      { t: 'err', error: MESSAGES.badFrame },
      { t: 'err', id: 2, error: MESSAGES.unknownFrame('hello') },
    ]);
    expect(ctx.daemon.take()).toEqual([{ t: 'pong', id: 'k' }]);
  });
});
