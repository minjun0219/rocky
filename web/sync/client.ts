/**
 * 보드 싱크 클라이언트 — `/api/ws` 소켓 하나로 사본을 받고(`sync` → `rows`) 쓰기를 보낸다(`tx` → `ack`·`reject`).
 *
 * - 시작하면 저장해 둔 사본을 먼저 읽어 알리고, 소켓이 붙으면 `since`·`epoch` 로 이어 받는다.
 * - 쓰기는 op 하나짜리 트랜잭션이다. 보내는 순간 답을 기다리는 op 로 알리고(화면은 사본 위에 겹쳐 그린다 —
 *   `./replica` 의 `overlay`), 답이 오면 걷는다.
 * - 끊기면 1·2·4…30초로 다시 붙는다. 답을 받기 전에 끊긴 op 가운데 만들기는 다시 보내고(같은 id 라 두 번 생기지
 *   않는다), 나머지는 적용됐는지 알 수 없으니 오류로 돌려준다. 화면은 이어 받은 사본을 따른다.
 * - 한 번도 붙지 못하거나(프록시가 막는 등) 데몬이 `sync` 를 모르면 `onFallback` 을 부르고 멈춘다 — 화면은 REST 로 돈다.
 *
 * 소켓·저장소·시계를 주입받아 테스트가 가짜로 갈아끼운다. 설계는 `docs/design/specs/2026-10-15-dual-structure-sync-design.md`
 * 의 2번 조각, 프레임은 `rockyd::ws`.
 */
import {
  applyRows,
  emptyReplica,
  isRetrySafe,
  type PendingOp,
  type Replica,
  type RowsFrame,
  type SyncOp,
} from './replica';

type WebSocketLike = {
  readyState: number;
  onopen: ((e: Event) => void) | null;
  onmessage: ((e: MessageEvent) => void) | null;
  onclose: ((e: CloseEvent) => void) | null;
  onerror: ((e: Event) => void) | null;
  send(data: string): void;
  close(): void;
};
type WebSocketCtor = new (url: string) => WebSocketLike;
const WS_OPEN = 1;

/** 다시 붙는 간격의 상한 — 1초부터 두 배씩. */
export const RETRY_MAX_MS = 30_000;
/** 데몬이 밀기를 멈췄을 때(`id` 없는 `err`) 다시 `sync` 를 보내는 최소 간격 — 같은 실패가 되풀이돼도 쏟아내지 않게. */
export const RESYNC_MIN_MS = 5_000;
/** `sync` 요청의 id — 이 id 로 온 `err` 는 데몬이 싱크를 모른다는 뜻이다. */
const SYNC_ID = 'sync';

/** 사본을 남기는 곳 — `./persist` 의 IndexedDB 판과 메모리 판. */
export interface ReplicaStore {
  load(): Promise<Replica | null>;
  /** 받은 프레임과 적용한 뒤의 사본. `full` 이면 비우고 쓴다. */
  save(frame: RowsFrame, replica: Replica): Promise<void>;
  clear(): Promise<void>;
}

/** 아직 답을 받지 않은 쓰기 하나. */
interface Pending extends PendingOp {
  id: string;
  sent: boolean;
  resolve(): void;
  reject(error: Error): void;
}

export interface SyncSnapshot {
  replica: Replica;
  /** 답을 기다리는 op — 보낸 순서. 화면은 사본 위에 이것을 겹친다. */
  pending: readonly PendingOp[];
  connected: boolean;
}

export interface SyncClientOptions {
  url: string;
  /** 쓰는 사람 — 소켓 주소의 `actor` 와 같다. 겹쳐 그릴 때 doing 의 주인이 된다. */
  actor: string;
  store: ReplicaStore;
  onChange(snapshot: SyncSnapshot): void;
  /** 소켓으로는 싱크할 수 없다 — 부른 쪽이 REST 로 돈다. */
  onFallback(): void;
  WebSocketImpl?: WebSocketCtor;
  setTimer?: (fn: () => void, ms: number) => unknown;
  clearTimer?: (handle: unknown) => void;
  now?: () => number;
}

/** 쓰기가 거절됐다 — 데몬이 준 이유 그대로. */
export class SyncRejected extends Error {}

export class SyncClient {
  private replica: Replica = emptyReplica();
  private socket: WebSocketLike | null = null;
  private everOpened = false;
  private stopped = false;
  private retryMs = 1_000;
  private retryTimer: unknown = null;
  private lastResync = Number.NEGATIVE_INFINITY;
  private nextTx = 1;
  private pending: Pending[] = [];
  private readonly WebSocketImpl: WebSocketCtor;
  private readonly setTimer: (fn: () => void, ms: number) => unknown;
  private readonly clearTimer: (handle: unknown) => void;
  private readonly now: () => number;

  constructor(private readonly options: SyncClientOptions) {
    this.WebSocketImpl = options.WebSocketImpl ?? (WebSocket as unknown as WebSocketCtor);
    this.setTimer = options.setTimer ?? ((fn, ms) => setTimeout(fn, ms));
    this.clearTimer =
      options.clearTimer ?? ((handle) => clearTimeout(handle as ReturnType<typeof setTimeout>));
    this.now = options.now ?? Date.now;
  }

  /** 저장한 사본을 읽어 알리고 소켓을 연다. 저장소를 못 읽으면 빈 사본에서 시작한다. */
  async start(): Promise<void> {
    const saved = await this.options.store.load().catch(() => null);
    if (this.stopped) {
      return;
    }
    if (saved) {
      this.replica = saved;
      this.emit();
    }
    this.connect();
  }

  /** 끊겨 있으면 기다리지 않고 바로 다시 붙는다 — 화면이 다시 보일 때. */
  reconnectNow(): void {
    if (this.stopped || this.socket || !this.everOpened) {
      return;
    }
    this.clearTimer(this.retryTimer);
    this.retryMs = 1_000;
    this.connect();
  }

  /**
   * op 하나를 보낸다. 바로 답을 기다리는 op 로 알리고, `ack` 가 오면 끝난다. `reject` 면 데몬의 이유로, 답을 받기
   * 전에 끊긴 (다시 보내면 안 되는) op 면 "확인하지 못했다" 로 거절된다.
   */
  send(op: SyncOp): Promise<void> {
    if (this.stopped) {
      return Promise.reject(new Error('보드 싱크가 멈춰 있다'));
    }
    return new Promise((resolve, reject) => {
      this.pending.push({
        id: `tx${this.nextTx++}`,
        op,
        actor: this.options.actor,
        at: new Date(this.now()).toISOString(),
        sent: false,
        resolve,
        reject,
      });
      this.emit();
      this.flush();
    });
  }

  /** 소켓을 닫고 다시 붙지 않는다. 답을 기다리던 op 는 거절된다. */
  stop(): void {
    this.stopped = true;
    this.clearTimer(this.retryTimer);
    const socket = this.socket;
    this.socket = null;
    socket?.close();
    this.failPending(() => true, new Error('보드 싱크를 멈췄다'));
  }

  snapshot(): SyncSnapshot {
    return {
      replica: this.replica,
      pending: this.pending.map(({ op, actor, at }) => ({ op, actor, at })),
      connected: this.socket?.readyState === WS_OPEN,
    };
  }

  private emit(): void {
    this.options.onChange(this.snapshot());
  }

  private sendSync(socket: WebSocketLike): void {
    const { epoch, seq } = this.replica;
    socket.send(
      JSON.stringify(
        epoch === null ? { t: 'sync', id: SYNC_ID } : { t: 'sync', id: SYNC_ID, since: seq, epoch },
      ),
    );
  }

  private connect(): void {
    if (this.socket || this.stopped) {
      return;
    }
    let socket: WebSocketLike;
    try {
      socket = new this.WebSocketImpl(this.options.url);
    } catch {
      this.fallback();
      return;
    }
    this.socket = socket;
    socket.onopen = () => {
      this.everOpened = true;
      this.retryMs = 1_000;
      this.sendSync(socket);
      this.flush();
      this.emit();
    };
    socket.onmessage = (e) => this.receive(String(e.data));
    socket.onerror = () => {};
    socket.onclose = () => {
      if (this.socket !== socket) {
        return; // 이미 버린 소켓
      }
      this.socket = null;
      if (!this.everOpened) {
        this.fallback();
        return;
      }
      for (const tx of this.pending) {
        if (tx.sent && isRetrySafe(tx.op)) {
          tx.sent = false; // 다시 붙으면 같은 id 로 다시 보낸다
        }
      }
      this.failPending(
        (tx) => tx.sent,
        new Error('연결이 끊겨 적용됐는지 확인하지 못했다 — 화면은 데몬의 상태로 돌아간다'),
      );
      this.emit();
      if (!this.stopped) {
        this.retryTimer = this.setTimer(() => this.connect(), this.retryMs);
        this.retryMs = Math.min(this.retryMs * 2, RETRY_MAX_MS);
      }
    };
  }

  private fallback(): void {
    if (this.stopped) {
      return;
    }
    this.stop();
    this.options.onFallback();
  }

  /** 아직 안 보낸 op 를 보낸다(붙어 있을 때만). */
  private flush(): void {
    const socket = this.socket;
    if (!socket || socket.readyState !== WS_OPEN) {
      return;
    }
    for (const tx of this.pending) {
      if (!tx.sent) {
        tx.sent = true;
        socket.send(JSON.stringify({ t: 'tx', id: tx.id, ops: [tx.op] }));
      }
    }
  }

  private failPending(which: (tx: Pending) => boolean, error: Error): void {
    const failed = this.pending.filter(which);
    if (failed.length === 0) {
      return;
    }
    this.pending = this.pending.filter((tx) => !which(tx));
    for (const tx of failed) {
      tx.reject(error);
    }
  }

  private settle(id: unknown, error?: Error): void {
    const index = this.pending.findIndex((tx) => tx.id === id);
    if (index === -1) {
      return;
    }
    const [tx] = this.pending.splice(index, 1);
    if (error) {
      tx?.reject(error);
    } else {
      tx?.resolve();
    }
    this.emit();
  }

  private receive(raw: string): void {
    let frame: { t?: string; id?: unknown; error?: string } & Partial<RowsFrame>;
    try {
      frame = JSON.parse(raw);
    } catch {
      return;
    }
    switch (frame.t) {
      case 'rows': {
        if (typeof frame.epoch !== 'string' || typeof frame.seq !== 'number' || !frame.rows) {
          return;
        }
        const rows: RowsFrame = {
          epoch: frame.epoch,
          seq: frame.seq,
          full: frame.full === true,
          rows: frame.rows,
        };
        this.replica = applyRows(this.replica, rows);
        // 저장은 기다리지 않는다 — 실패해도 이번 화면은 맞다. 다음 부팅이 조금 낡은 사본에서 이어 받을 뿐이다.
        void this.options.store.save(rows, this.replica).catch(() => {});
        this.emit();
        return;
      }
      case 'ack':
        this.settle(frame.id);
        return;
      case 'reject':
        this.settle(frame.id, new SyncRejected(frame.error ?? 'rejected'));
        return;
      case 'err': {
        if (frame.id === SYNC_ID) {
          this.fallback(); // `sync` 를 모르는 데몬
          return;
        }
        if (frame.id !== undefined) {
          return; // 다른 요청의 실패 — 이 클라이언트는 id 를 단 요청이 sync 뿐이다
        }
        // 데몬이 밀다가 읽기에 실패해 밀기를 멈췄다 — 커서부터 다시 받는다.
        const socket = this.socket;
        if (socket && this.now() - this.lastResync >= RESYNC_MIN_MS) {
          this.lastResync = this.now();
          this.sendSync(socket);
        }
        return;
      }
    }
  }
}
