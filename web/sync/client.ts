/**
 * 보드 싱크 클라이언트 — `/api/ws` 소켓 하나로 사본을 받고(`sync` → `rows`) 쓰기를 보낸다(`tx` → `ack`·`reject`).
 *
 * - 시작하면 저장해 둔 사본을 먼저 읽어 알리고(오래 걸리면 기다리지 않는다), 소켓이 붙으면 `since`·`epoch` 로 이어 받는다.
 * - 이 연결로 `rows` 를 한 번 받으면 `live` 다. 끊기면 `live` 가 아니고, 1·2·4…30초로 다시 붙는다(오래 붙어 있던
 *   연결이 끊겼을 때만 1초부터 다시). 한 번도 못 붙어도 멈추지 않고 계속 붙어 본다 — 그동안 화면은 REST 로 돈다.
 * - 쓰기는 op 하나짜리 트랜잭션이다. 보내는 순간 답을 기다리는 op 로 알리고(화면은 사본 위에 겹쳐 그린다 —
 *   `./replica` 의 `overlay`), 답이 오면 걷는다. 답을 받기 전에 끊긴 op 가운데 만들기는 다시 보내고(같은 id 라 두 번
 *   생기지 않는다), 나머지는 적용됐는지 알 수 없으니 오류로 돌려준다.
 * - 반쯤 죽은 소켓(닫힘이 오지 않는다)은 `ping` 으로 가린다 — 30초 동안 아무 프레임이 없거나 화면이 다시 보이면 묻고,
 *   5초 안에 답이 없으면 닫고 다시 붙는다.
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
  type SyncTables,
  TABLE_NAMES,
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
/** 이만큼 붙어 있던 연결이 끊겼을 때만 다시 1초부터 붙는다 — 붙자마자 닫는 프록시 앞에서 1초마다 돌지 않게. */
export const STABLE_MS = 10_000;
/**
 * 데몬이 밀기를 멈췄을 때(`id` 없는 `err`) 다시 `sync` 를 보내는 최소 간격 — 같은 실패가 되풀이돼도 쏟아내지 않게.
 * 그 안에 또 오면 버리지 않고 간격을 채운 뒤에 보낸다(버리면 그 연결은 끊길 때까지 행을 못 받는다).
 */
export const RESYNC_MIN_MS = 5_000;
/** 저장한 사본을 읽는 데 이만큼만 기다린다 — 저장소가 멈춰도 소켓은 붙는다. */
export const LOAD_TIMEOUT_MS = 1_000;
/** 이만큼 아무 프레임이 없으면 `ping` 으로 살아 있는지 묻는다. */
export const PING_IDLE_MS = 30_000;
/** `ping` 뒤 이만큼 아무 프레임이 없으면 죽은 소켓으로 보고 닫는다. */
export const PING_DEADLINE_MS = 5_000;
/** `sync` 요청의 id — 그 id 로 온 `rows` 가 첫 사본이고, 그 id 로 온 `err` 는 읽기 실패다. */
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
  /** 지금 연결로 `rows` 를 받았다 — 사본이 데몬을 따라가고 있고, 쓰기를 `tx` 로 보낼 수 있다. */
  live: boolean;
}

export interface SyncClientOptions {
  url: string;
  /** 쓰는 사람 — 소켓 주소의 `actor` 와 같다. 겹쳐 그릴 때 doing 의 주인이 된다. */
  actor: string;
  store: ReplicaStore;
  onChange(snapshot: SyncSnapshot): void;
  WebSocketImpl?: WebSocketCtor;
  /** 타이머 — `kind` 는 테스트가 어느 타이머인지 가리는 이름이다. */
  setTimer?: (fn: () => void, ms: number, kind: TimerKind) => unknown;
  clearTimer?: (handle: unknown) => void;
  now?: () => number;
}

export type TimerKind = 'load' | 'retry' | 'resync' | 'ping' | 'deadline';

/** 쓰기가 거절됐다 — 데몬이 준 이유 그대로. */
export class SyncRejected extends Error {}

/** 사본 전체를 통째 프레임으로 — 저장이 한 번 실패하면 다음 저장은 이것을 쓴다(빠진 행이 머리 뒤에 숨지 않게). */
function fullFrameOf(replica: Replica): RowsFrame {
  const rows: Partial<SyncTables> = {};
  for (const table of TABLE_NAMES) {
    (rows as Record<string, unknown[]>)[table] = [...replica.tables[table].values()];
  }
  return { epoch: replica.epoch ?? '', seq: replica.seq, full: true, rows };
}

export class SyncClient {
  private replica: Replica = emptyReplica();
  private socket: WebSocketLike | null = null;
  private live = false;
  private stopped = false;
  /** 지금 소켓이 열린 시각 — 열려 있지 않으면 null. */
  private openedAt: number | null = null;
  private lastFrameAt = 0;
  private retryMs = 1_000;
  private retryTimer: unknown = null;
  private lastResync = Number.NEGATIVE_INFINITY;
  private resyncTimer: unknown = null;
  private pingTimer: unknown = null;
  private deadlineTimer: unknown = null;
  private nextTx = 1;
  private pending: Pending[] = [];
  private saving: Promise<void> = Promise.resolve();
  private saveDirty = false;
  private readonly WebSocketImpl: WebSocketCtor;
  private readonly setTimer: (fn: () => void, ms: number, kind: TimerKind) => unknown;
  private readonly clearTimer: (handle: unknown) => void;
  private readonly now: () => number;

  constructor(private readonly options: SyncClientOptions) {
    this.WebSocketImpl = options.WebSocketImpl ?? (WebSocket as unknown as WebSocketCtor);
    this.setTimer = options.setTimer ?? ((fn, ms) => setTimeout(fn, ms));
    this.clearTimer =
      options.clearTimer ?? ((handle) => clearTimeout(handle as ReturnType<typeof setTimeout>));
    this.now = options.now ?? Date.now;
  }

  /**
   * 저장한 사본을 읽어 알리고 소켓을 연다. 사본 읽기는 `LOAD_TIMEOUT_MS` 까지만 기다린다 — 그보다 늦게 온 사본은 쓰지
   * 않는다(그사이 화면은 REST 로 그렸다). 저장소를 못 읽으면 빈 사본에서 시작한다.
   */
  async start(): Promise<void> {
    const load = this.options.store.load().catch(() => null);
    const saved = await Promise.race([
      load,
      new Promise<null>((resolve) => this.setTimer(() => resolve(null), LOAD_TIMEOUT_MS, 'load')),
    ]);
    if (this.stopped) {
      return;
    }
    if (saved) {
      this.replica = saved;
      this.emit();
    }
    this.connect();
  }

  /** 걸려 있는 저장이 다 끝날 때까지 — 사본을 지우기 전에 기다린다. */
  idle(): Promise<void> {
    return this.saving;
  }

  /**
   * 화면이 다시 보일 때 — 끊겨 있으면 기다리지 않고 다시 붙고, 붙어 있으면 살아 있는지 묻는다(잠든 사이 끊긴 소켓은
   * 닫힘이 오지 않는다).
   */
  wake(): void {
    if (this.stopped) {
      return;
    }
    if (this.socket) {
      this.ping();
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
      return Promise.reject(new Error('보드 싱크가 멈춰 있어요'));
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
    for (const timer of [this.retryTimer, this.resyncTimer, this.pingTimer, this.deadlineTimer]) {
      this.clearTimer(timer);
    }
    const socket = this.socket;
    this.socket = null;
    this.live = false;
    socket?.close();
    this.failPending(() => true, new Error('보드 싱크를 멈췄어요'));
  }

  snapshot(): SyncSnapshot {
    return {
      replica: this.replica,
      pending: this.pending.map(({ op, actor, at }) => ({ op, actor, at })),
      live: this.live,
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
      this.scheduleRetry();
      return;
    }
    this.socket = socket;
    socket.onopen = () => {
      this.openedAt = this.now();
      this.lastFrameAt = this.now();
      this.sendSync(socket);
      this.flush();
      this.armPing();
    };
    socket.onmessage = (e) => {
      if (this.socket === socket) {
        this.receive(String(e.data));
      }
    };
    socket.onerror = () => {};
    socket.onclose = () => {
      if (this.socket !== socket) {
        return; // 이미 버린 소켓
      }
      this.socket = null;
      for (const timer of [this.resyncTimer, this.pingTimer, this.deadlineTimer]) {
        this.clearTimer(timer);
      }
      this.resyncTimer = null; // 다시 붙으면 처음부터 `sync` 한다
      this.live = false;
      if (this.openedAt !== null && this.now() - this.openedAt >= STABLE_MS) {
        this.retryMs = 1_000; // 오래 붙어 있던 연결 — 처음부터 다시
      }
      this.openedAt = null;
      for (const tx of this.pending) {
        if (tx.sent && isRetrySafe(tx.op)) {
          tx.sent = false; // 다시 붙으면 같은 id 로 다시 보낸다
        }
      }
      this.failPending(
        (tx) => tx.sent,
        new Error('연결이 끊겨 적용됐는지 확인하지 못했어요 — 화면은 데몬의 상태로 돌아가요'),
      );
      this.emit();
      this.scheduleRetry();
    };
  }

  private scheduleRetry(): void {
    if (this.stopped) {
      return;
    }
    this.retryTimer = this.setTimer(() => this.connect(), this.retryMs, 'retry');
    this.retryMs = Math.min(this.retryMs * 2, RETRY_MAX_MS);
  }

  /** 다음 `ping` 을 건다 — 프레임이 오면 미뤄진다(`receive`). */
  private armPing(): void {
    this.clearTimer(this.pingTimer);
    this.pingTimer = this.setTimer(
      () => {
        if (this.now() - this.lastFrameAt >= PING_IDLE_MS) {
          this.ping();
        } else {
          this.armPing();
        }
      },
      PING_IDLE_MS,
      'ping',
    );
  }

  /** 살아 있는지 묻는다 — `PING_DEADLINE_MS` 안에 아무 프레임이 없으면 닫고 다시 붙는다. */
  private ping(): void {
    const socket = this.socket;
    if (!socket || socket.readyState !== WS_OPEN || this.deadlineTimer !== null) {
      return;
    }
    const asked = this.now();
    socket.send(JSON.stringify({ t: 'ping' }));
    this.deadlineTimer = this.setTimer(
      () => {
        this.deadlineTimer = null;
        if (this.socket === socket && this.lastFrameAt < asked) {
          socket.close(); // 답이 없다 — 닫힘이 다시 붙기를 시작한다
          socket.onclose?.({} as CloseEvent);
        }
      },
      PING_DEADLINE_MS,
      'deadline',
    );
  }

  /** 커서부터 다시 받는다 — `RESYNC_MIN_MS` 안이면 그 간격을 채운 뒤에(한 번만 걸어 둔다). */
  private resync(): void {
    if (!this.socket || this.resyncTimer !== null) {
      return;
    }
    const wait = this.lastResync + RESYNC_MIN_MS - this.now();
    const send = () => {
      this.resyncTimer = null;
      const socket = this.socket;
      if (socket && socket.readyState === WS_OPEN) {
        this.lastResync = this.now();
        this.sendSync(socket);
      }
    };
    if (wait <= 0) {
      send();
    } else {
      this.resyncTimer = this.setTimer(send, wait, 'resync');
    }
  }

  /** 아직 안 보낸 op 를 보낸다(붙어 있을 때만). 한동안 조용했으면 살아 있는지도 묻는다. */
  private flush(): void {
    const socket = this.socket;
    if (!socket || socket.readyState !== WS_OPEN) {
      return;
    }
    let sentAny = false;
    for (const tx of this.pending) {
      if (!tx.sent) {
        tx.sent = true;
        sentAny = true;
        socket.send(JSON.stringify({ t: 'tx', id: tx.id, ops: [tx.op] }));
      }
    }
    if (sentAny && this.now() - this.lastFrameAt >= PING_DEADLINE_MS * 3) {
      this.ping();
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

  /**
   * 사본을 차례로 저장한다. 한 번 실패하면 다음 저장은 사본 전체를 통째로 쓴다 — 델타만 쓰면 실패한 프레임의 행이
   * 머리(`seq`) 뒤에 숨어 다음 부팅이 그 행을 다시 받지 못한다.
   */
  private persist(frame: RowsFrame): void {
    const replica = this.replica;
    const toSave = this.saveDirty ? fullFrameOf(replica) : frame;
    this.saveDirty = false;
    this.saving = this.saving
      .then(() => (this.stopped ? undefined : this.options.store.save(toSave, replica)))
      .catch(() => {
        this.saveDirty = true;
      });
  }

  private receive(raw: string): void {
    this.lastFrameAt = this.now();
    this.armPing();
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
        if (!this.live) {
          this.live = true;
          this.flush(); // 끊긴 동안 쌓인 op 를 보낸다
        }
        this.persist(rows);
        this.emit();
        return;
      }
      case 'ack':
        this.settle(frame.id);
        return;
      case 'reject':
        this.settle(frame.id, new SyncRejected(frame.error ?? 'rejected'));
        return;
      case 'err':
        // `sync` 의 읽기가 실패했거나(`id` 가 `sync`), 밀다가 읽기가 실패해 밀기를 멈췄다(`id` 없음) — 커서부터 다시
        // 받는다. tx 의 실패는 `reject` 로 온다.
        if (frame.id === SYNC_ID || frame.id === undefined) {
          this.resync();
        }
        return;
    }
  }
}
