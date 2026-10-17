/**
 * 클라우드 중계소. Durable Object 하나 안에서 데몬 연결 하나와 폰 연결 여럿 사이를 잇는다. 설계는
 * `docs/design/specs/2026-10-15-dual-structure-sync-design.md`의 3번 조각.
 *
 * - 데몬은 전송으로는 소켓 클라이언트지만 프로토콜로는 `/api/ws`와 같은 서버다. 중계소가 붙자마자 `sync`(사본의 머리)를
 *   보내고, 데몬은 `rows`로 답한 뒤 행이 바뀔 때마다 민다. 중계소는 받은 행을 사본에 두고 폰마다 커서 이후를 민다.
 * - 폰은 웹이 데몬과 쓰는 프레임 그대로다(`sync` → `rows`, `tx` → `ack`·`reject`, `ping` → `pong`).
 * - 폰의 `tx`는 변경함에 넣고 데몬에 하나씩 보낸다(`{t:"tx", id, actor, ops}`). 답을 받아야 다음을 보낸다. 도착 순서가
 *   충돌 규칙이고, 끊겼을 때 답을 모르는 쓰기가 하나뿐이어야 해서다. 데몬이 결과 행을 먼저 밀고 답하므로 폰도 행을
 *   먼저 받는다.
 * - 노트 문서 프레임은 아직 받지 않는다.
 *
 * 상태는 SQLite(`./storage`)와 소켓 첨부(`Peer.state`)에만 둔다. DO가 잠들었다 깨면 이 객체는 새로 만들어진다.
 * 소켓·저장소·시계를 주입받아 테스트가 가짜로 갈아끼운다.
 */
import { isAgentActor } from '../../web/actors';
import type { SyncOp } from '../../web/sync/replica';
import { type Cursor, type DaemonRows, Mirror, type MirrorHead } from './mirror';
import { OUTBOX_LIMIT, Outbox, type OutboxEntry } from './outbox';
import { migrate, type RelayStorage, readMeta, writeMeta } from './storage';

/** 소켓 첨부. DO에서는 `serializeAttachment`로 남아 잠들었다 깨도 그대로다. */
export type PeerState =
  | { role: 'daemon'; conn: string }
  | {
      role: 'phone';
      conn: string;
      /** 쓰는 사람. 폰이 소켓 주소의 `actor`로 준 이름. */
      actor: string;
      /** `sync`를 보냈다. 이 연결에 행을 민다. */
      synced: boolean;
      /** 이 연결에 마지막으로 보낸 머리. `synced`인데 null이면 다음에 통째로 보낸다. */
      cursor: Cursor | null;
    };

export interface Peer {
  send(text: string): void;
  close(code: number, reason: string): void;
  state(): PeerState | null;
  setState(state: PeerState): void;
}

export interface RelayOptions {
  /** 지금 붙어 있는 소켓 전부. DO에서는 `ctx.getWebSockets()`. */
  peers(): Peer[];
  now?: () => number;
  /** 연결·변경함 id. */
  newId?: () => string;
}

/** 새 데몬 연결이 와서 옛 연결을 닫는다. */
export const CLOSE_REPLACED = 4000;
/** 데몬이 행을 읽지 못해 밀기를 멈췄다(`err`). 연결기가 간격을 두고 다시 붙는다. */
export const CLOSE_DAEMON_ERR = 4001;

/** 웹에 보이는 문구는 웹과 같은 말투로. */
export const MESSAGES = {
  badFrame: '프레임을 읽지 못했어요',
  noOps: '보낼 쓰기가 없어요',
  notes: '노트 문서는 아직 중계소에서 열 수 없어요',
  outboxFull: `맥에 보낼 변경이 ${OUTBOX_LIMIT.toLocaleString('en-US')}개 쌓여 더 받지 못해요. 맥이 깨어나 변경을 받은 뒤 다시 보내 주세요`,
  agent: (actor: string) => `에이전트 이름(${actor})으로는 중계소에 쓸 수 없어요`,
  unknownOp: (name: string) => `중계소가 모르는 쓰기예요: ${name}`,
  unknownFrame: (t: string) => `중계소가 모르는 프레임이에요: ${t}`,
};

/** 중계소가 받는 쓰기. 웹의 `SyncOp`(곧 `rocky_core::sync::SyncOp`)와 같은 이름이고, 어휘가 늘면 여기서 타입이 깨진다. */
const OP_NAMES: Record<SyncOp['op'], true> = {
  'todo.create': true,
  'todo.update': true,
  'todo.status': true,
  'todo.move': true,
  'comment.create': true,
  'comment.update': true,
  'comment.archive': true,
  'comment.unarchive': true,
  'section.create': true,
  'section.archive': true,
  'note.create': true,
  'note.update': true,
  'note.archive': true,
  'note.unarchive': true,
  'note.pin': true,
  'note.unpin': true,
  'board.update': true,
};

/** 노트 문서 프레임. 아직 중계소에서 받지 않는다(노트 PR에서 연다). */
const NOTE_FRAMES = new Set(['doc', 'update', 'presence', 'sub']);

type Frame = { t?: unknown; id?: unknown; [key: string]: unknown };

function opName(op: unknown): string | null {
  const name = (op as { op?: unknown } | null)?.op;
  return typeof name === 'string' ? name : null;
}

function isKnownOp(op: unknown): op is SyncOp {
  const name = opName(op);
  return name !== null && Object.hasOwn(OP_NAMES, name);
}

/** 보내다 실패해도(닫히는 소켓) 다른 연결의 처리는 이어 간다. */
function trySend(peer: Peer, text: string): void {
  try {
    peer.send(text);
  } catch {
    // 닫힘이 따로 온다
  }
}

/** 데몬에게 보내는 `sync`. 사본의 머리로 이어 받고, 사본이 없으면 통째로 받는다. */
function syncText(head: MirrorHead | null): string {
  return JSON.stringify(
    head === null
      ? { t: 'sync', id: 'sync' }
      : { t: 'sync', id: 'sync', since: head.seq, epoch: head.daemonEpoch },
  );
}

function txText(entry: OutboxEntry): string {
  return JSON.stringify({ t: 'tx', id: entry.id, actor: entry.actor, ops: entry.ops });
}

function withId(frame: Record<string, unknown>, id: unknown): string {
  return JSON.stringify(id === undefined || id === null ? frame : { ...frame, id });
}

export class Relay {
  private readonly mirror: Mirror;
  private readonly outbox: Outbox;
  private readonly now: () => number;
  private readonly newId: () => string;

  constructor(
    private readonly storage: RelayStorage,
    private readonly options: RelayOptions,
  ) {
    migrate(storage);
    this.mirror = new Mirror(storage);
    this.outbox = new Outbox(storage);
    this.now = options.now ?? Date.now;
    this.newId = options.newId ?? (() => crypto.randomUUID());
  }

  /**
   * 소켓이 붙었다. 데몬이면 옛 데몬 연결을 닫고 `sync`와 변경함 맨 앞의 쓰기를 보낸다. 폰은 `sync`를 기다린다.
   * `actor`는 Worker가 소켓 주소에서 읽은 이름이다.
   */
  open(peer: Peer, who: { role: 'daemon' } | { role: 'phone'; actor: string }): void {
    const conn = this.newId();
    if (who.role === 'phone') {
      peer.setState({ role: 'phone', conn, actor: who.actor, synced: false, cursor: null });
      return;
    }
    peer.setState({ role: 'daemon', conn });
    for (const other of this.options.peers()) {
      const state = other.state();
      if (state?.role === 'daemon' && state.conn !== conn) {
        other.close(CLOSE_REPLACED, 'replaced by a newer daemon connection');
      }
    }
    writeMeta(this.storage, 'daemon', conn);
    trySend(peer, syncText(this.mirror.head()));
    this.pump();
  }

  /** 소켓이 닫혔다. */
  close(peer: Peer): void {
    const state = peer.state();
    if (state?.role === 'daemon' && readMeta(this.storage, 'daemon') === state.conn) {
      writeMeta(this.storage, 'daemon', null);
    }
  }

  /** 텍스트 프레임 하나. */
  message(peer: Peer, raw: string): void {
    const state = peer.state();
    if (state === null) {
      return;
    }
    let frame: Frame;
    try {
      frame = JSON.parse(raw) as Frame;
    } catch {
      trySend(peer, JSON.stringify({ t: 'err', error: MESSAGES.badFrame }));
      return;
    }
    if (typeof frame !== 'object' || frame === null || Array.isArray(frame)) {
      trySend(peer, JSON.stringify({ t: 'err', error: MESSAGES.badFrame }));
      return;
    }
    if (frame.t === 'ping') {
      // 웹의 `{"t":"ping"}`은 DO의 자동 응답이 받는다. 다른 모양(`id`를 단 것)만 여기까지 온다.
      trySend(peer, withId({ t: 'pong' }, frame.id));
      return;
    }
    if (state.role === 'daemon') {
      this.fromDaemon(peer, state.conn, frame);
    } else {
      this.fromPhone(peer, state, frame);
    }
  }

  private daemon(): { peer: Peer; conn: string } | null {
    const conn = readMeta(this.storage, 'daemon');
    if (conn === null) {
      return null;
    }
    for (const peer of this.options.peers()) {
      const state = peer.state();
      if (state?.role === 'daemon' && state.conn === conn) {
        return { peer, conn };
      }
    }
    return null;
  }

  /** 데몬이 붙어 있고 답을 기다리는 쓰기가 없으면 변경함 맨 앞을 보낸다. */
  private pump(): void {
    const daemon = this.daemon();
    const head = this.outbox.head();
    if (!daemon || !head || head.sentTo === daemon.conn) {
      return;
    }
    this.outbox.markSent(head.id, daemon.conn);
    trySend(daemon.peer, txText(head));
  }

  private fromDaemon(peer: Peer, conn: string, frame: Frame): void {
    if (readMeta(this.storage, 'daemon') !== conn) {
      return; // 닫히는 중인 옛 연결
    }
    switch (frame.t) {
      case 'rows':
        this.daemonRows(peer, frame);
        return;
      case 'ack':
      case 'reject':
        this.daemonAnswer(frame);
        return;
      case 'err':
        // 데몬이 행을 읽지 못해 밀기를 멈췄다. 바로 다시 물으면 같은 실패가 쉬지 않고 되풀이될 수 있어 닫는다.
        // 연결기가 간격을 두고 다시 붙는다.
        peer.close(CLOSE_DAEMON_ERR, 'daemon failed to read rows');
        return;
    }
  }

  private daemonRows(peer: Peer, frame: Frame): void {
    const { epoch, seq, rows } = frame;
    if (typeof epoch !== 'string' || typeof seq !== 'number' || typeof rows !== 'object' || !rows) {
      return;
    }
    const applied = this.mirror.apply({
      epoch,
      seq,
      full: frame.full === true,
      rows: rows as DaemonRows['rows'],
    });
    if (!applied) {
      // 사본과 이어지지 않는 이어 받기다. 버리고 통째로 다시 받는다.
      trySend(peer, syncText(null));
      return;
    }
    this.pushRows();
  }

  /** 행을 받은 폰 연결마다 커서 이후를 민다. 보낼 것이 없으면 보내지 않는다. */
  private pushRows(): void {
    for (const peer of this.options.peers()) {
      const state = peer.state();
      if (state?.role !== 'phone' || !state.synced) {
        continue;
      }
      const out = this.mirror.rowsFor(state.cursor);
      if (!out) {
        continue;
      }
      if (out.full || out.count > 0) {
        trySend(peer, out.text);
      }
      peer.setState({ ...state, cursor: out.cursor });
    }
  }

  private daemonAnswer(frame: Frame): void {
    const rejected = frame.t === 'reject';
    const entry = this.outbox.settle(
      String(frame.id),
      rejected
        ? {
            opIndex: typeof frame.at === 'number' ? frame.at : null,
            error: typeof frame.error === 'string' ? frame.error : 'rejected',
            failedAt: new Date(this.now()).toISOString(),
          }
        : undefined,
    );
    if (entry?.conn) {
      const phone = this.options.peers().find((p) => p.state()?.conn === entry.conn);
      if (phone) {
        trySend(
          phone,
          JSON.stringify(
            rejected
              ? {
                  t: 'reject',
                  id: entry.tx,
                  at: frame.at ?? null,
                  error: frame.error ?? 'rejected',
                }
              : { t: 'ack', id: entry.tx, seq: frame.seq },
          ),
        );
      }
    }
    this.pump();
  }

  private fromPhone(peer: Peer, state: Extract<PeerState, { role: 'phone' }>, frame: Frame): void {
    switch (frame.t) {
      case 'sync':
        this.phoneSync(peer, state, frame);
        return;
      case 'tx':
        this.phoneTx(peer, state, frame);
        return;
      case 'unsub':
        return;
      default:
        if (typeof frame.t === 'string' && NOTE_FRAMES.has(frame.t)) {
          trySend(peer, withId({ t: 'err', error: MESSAGES.notes }, frame.id));
          return;
        }
        trySend(
          peer,
          withId({ t: 'err', error: MESSAGES.unknownFrame(String(frame.t)) }, frame.id),
        );
    }
  }

  /**
   * 폰의 `sync`. 커서가 이 사본의 것이면 그 뒤를, 아니면 통째로 답한다. 이어 받을 것이 없어도 답한다(데몬과 같다).
   * 사본이 비어 있으면(데몬이 아직 붙은 적이 없다) 답하지 않고 기다렸다가, 데몬의 첫 행이 오면 통째로 민다.
   */
  private phoneSync(peer: Peer, state: Extract<PeerState, { role: 'phone' }>, frame: Frame): void {
    const cursor =
      typeof frame.since === 'number' && typeof frame.epoch === 'string'
        ? { epoch: frame.epoch, seq: frame.since }
        : null;
    const out = this.mirror.rowsFor(cursor, frame.id);
    if (!out) {
      peer.setState({ ...state, synced: true, cursor: null });
      return;
    }
    peer.setState({ ...state, synced: true, cursor: out.cursor });
    trySend(peer, out.text);
  }

  private phoneTx(peer: Peer, state: Extract<PeerState, { role: 'phone' }>, frame: Frame): void {
    const id = frame.id ?? null;
    const reject = (at: number | null, error: string) =>
      trySend(peer, JSON.stringify({ t: 'reject', id, at, error }));
    // 데몬과 같은 규칙이다. 트랜잭션은 사람의 쓰기라 에이전트의 쓰기는 변경함에 쌓지 않는다.
    if (isAgentActor(state.actor)) {
      reject(null, MESSAGES.agent(state.actor));
      return;
    }
    const { ops } = frame;
    if (!Array.isArray(ops) || ops.length === 0) {
      reject(null, MESSAGES.noOps);
      return;
    }
    const unknown = ops.findIndex((op) => !isKnownOp(op));
    if (unknown !== -1) {
      reject(unknown, MESSAGES.unknownOp(opName(ops[unknown]) ?? '(op 없음)'));
      return;
    }
    const result = this.outbox.enqueue(
      {
        actor: state.actor,
        ops: ops as SyncOp[],
        at: new Date(this.now()).toISOString(),
        conn: state.conn,
        tx: id,
      },
      this.newId,
    );
    if (result.kind === 'full') {
      reject(null, MESSAGES.outboxFull);
      return;
    }
    this.pump();
  }
}
