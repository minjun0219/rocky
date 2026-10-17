import { describe, expect, test } from 'bun:test';
import { SyncClient } from '../web/sync/client';
import { memoryReplicaStore } from '../web/sync/persist';
import { hostUrl, PING, PONG, RelayHost, whoFrom } from './host';
import { fakeContext, FakeSocket } from './test-support';

function setup() {
  const ctx = fakeContext();
  let host = new RelayHost(ctx);
  return {
    ctx,
    /** DO가 잠들었다 깼다. 같은 저장소와 소켓으로 새로 만든다. */
    wake() {
      host = new RelayHost(ctx);
    },
    connect(who: Parameters<RelayHost['accept']>[1]): FakeSocket {
      const socket = new FakeSocket();
      host.accept(socket, who);
      return socket;
    },
    send(socket: FakeSocket, frame: unknown) {
      host.message(socket, JSON.stringify(frame));
    },
    drop(socket: FakeSocket) {
      socket.close(1006, 'gone');
      host.close(socket);
    },
    host: () => host,
  };
}

const rows = { t: 'rows', id: 'sync', epoch: 'e1', seq: 3, full: true, rows: { todos: [] } };
const tx = {
  t: 'tx',
  id: 'tx1',
  ops: [{ op: 'todo.status', id: 'abcd1234', action: 'done' }],
};

describe('hostUrl · whoFrom', () => {
  test('Worker가 정한 역할과 actor를 그대로 주고받는다', () => {
    for (const who of [
      { role: 'daemon' } as const,
      { role: 'phone', actor: '민준 & co=1?#' } as const,
    ]) {
      expect(whoFrom(hostUrl(who))).toEqual(who);
    }
  });

  test('모르는 주소는 받지 않는다', () => {
    for (const url of ['https://relay.internal/phone', 'https://relay.internal/x', 'not a url']) {
      expect(whoFrom(url)).toBeNull();
    }
  });
});

describe('RelayHost', () => {
  test('데몬을 받으면 sync를 보내고, 폰은 사본이 생길 때까지 기다렸다가 받는다', () => {
    const s = setup();
    const daemon = s.connect({ role: 'daemon' });
    expect(s.ctx.sockets).toEqual([daemon]);
    expect(daemon.take()).toEqual([{ t: 'sync', id: 'sync' }]);
    const phone = s.connect({ role: 'phone', actor: 'minjun' });
    s.send(phone, { t: 'sync', id: 'sync' });
    expect(phone.take()).toEqual([]);
    s.send(daemon, rows);
    expect(phone.take()).toMatchObject([{ t: 'rows', epoch: 'e1.1', seq: 3, full: true }]);
  });

  test('잠들었다 깨도 소켓 첨부와 SQLite로 이어 간다', () => {
    const s = setup();
    const daemon = s.connect({ role: 'daemon' });
    s.send(daemon, rows);
    const phone = s.connect({ role: 'phone', actor: 'minjun' });
    s.send(phone, { t: 'sync', id: 'sync' });
    s.send(phone, tx);
    const [sent] = daemon.take().filter((frame) => frame.t === 'tx');
    expect(sent).toMatchObject({ actor: 'minjun', ops: tx.ops });
    phone.take();

    s.wake();
    s.send(daemon, { t: 'ack', id: sent?.id, seq: 4 });
    expect(phone.take()).toEqual([{ t: 'ack', id: 'tx1', seq: 4 }]);
  });

  test('데몬 소켓이 닫히면 다음 데몬 연결에 답을 기다리던 쓰기를 다시 보낸다', () => {
    const s = setup();
    const first = s.connect({ role: 'daemon' });
    s.send(first, rows);
    const phone = s.connect({ role: 'phone', actor: 'minjun' });
    s.send(phone, tx);
    const [sent] = first.take().filter((frame) => frame.t === 'tx');
    s.drop(first);

    const second = s.connect({ role: 'daemon' });
    expect(second.take()).toEqual([
      { t: 'sync', id: 'sync', since: 3, epoch: 'e1' },
      { t: 'tx', id: sent?.id, actor: 'minjun', ops: tx.ops },
    ]);
  });

  test('텍스트 프레임만 읽는다', () => {
    const s = setup();
    const phone = s.connect({ role: 'phone', actor: 'minjun' });
    s.host().message(phone, new TextEncoder().encode('{"t":"sync"}').buffer as ArrayBuffer);
    s.host().message(phone, '{"t":"nope"}');
    expect(phone.take()).toEqual([{ t: 'err', error: '중계소가 모르는 프레임이에요: nope' }]);
  });
});

describe('PING', () => {
  test('웹 소켓 클라이언트가 보내는 ping 글자 그대로다', async () => {
    const raw: string[] = [];
    class RawSocket {
      readyState = 0;
      onopen: ((e: Event) => void) | null = null;
      onmessage: ((e: MessageEvent) => void) | null = null;
      onclose: ((e: CloseEvent) => void) | null = null;
      onerror: ((e: Event) => void) | null = null;
      static last: RawSocket | null = null;
      constructor(readonly url: string) {
        RawSocket.last = this;
      }
      send(data: string) {
        raw.push(data);
      }
      close() {}
    }
    const client = new SyncClient({
      url: 'ws://h/api/ws?actor=minjun',
      actor: 'minjun',
      store: memoryReplicaStore(),
      onChange: () => {},
      WebSocketImpl: RawSocket,
      setTimer: () => null,
      clearTimer: () => {},
    });
    await client.start();
    const socket = RawSocket.last as RawSocket;
    socket.readyState = 1;
    socket.onopen?.({} as Event);
    client.wake(); // 붙어 있으면 살아 있는지 묻는다
    expect(raw).toContain(PING);
    expect(JSON.parse(PONG)).toEqual({ t: 'pong' });
  });
});
