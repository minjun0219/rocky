/**
 * 변경함. 폰이 보낸 쓰기(`tx`)를 도착 순서로 두었다가 데몬에 하나씩 넘긴다. 데몬이 답하면 지우고, 거절이면
 * `failed`에 남긴다.
 *
 * 칸마다 그 쓰기를 보낸 폰 연결과 폰의 `tx` id를 두어 데몬의 답을 그 폰에 그대로 전한다. 웹은 끊겼다 붙으면 만들기를
 * 같은 id로 다시 보내는데, 같은 만들기가 이미 변경함에 있으면 새 칸을 만들지 않고 답을 받을 연결만 바꾼다.
 */
import { isRetrySafe, type SyncOp } from '../../web/sync/replica';
import type { RelayStorage } from './storage';

/** 변경함이 이만큼 차면 폰의 쓰기를 더 받지 않는다. 맥이 오래 잠든 동안 끝없이 쌓이지 않게 하려는 것이다. */
export const OUTBOX_LIMIT = 1_000;
/** 거절된 쓰기를 이만큼만 남긴다(최근 것부터). */
export const FAILED_LIMIT = 100;

export interface OutboxEntry {
  /** 변경함 id. 데몬에 보내는 `tx`의 id로, 중계소가 정한다. */
  id: string;
  actor: string;
  ops: SyncOp[];
  /** 중계소가 받은 시각(ISO). */
  at: string;
  /** 답을 받을 폰 연결. */
  conn: string | null;
  /** 그 폰이 보낸 `tx`의 id. */
  tx: unknown;
  /** 이 쓰기를 보내고 답을 기다리는 데몬 연결. */
  sentTo: string | null;
}

export interface FailedEntry {
  id: string;
  actor: string;
  ops: SyncOp[];
  at: string;
  /** 데몬이 멈춘 op의 자리(`reject`의 `at`). */
  opIndex: number | null;
  error: string;
  failedAt: string;
}

export type EnqueueResult =
  | { kind: 'queued'; entry: OutboxEntry }
  /** 같은 만들기가 이미 있어 답을 받을 연결만 바꿨다. */
  | { kind: 'merged'; entry: OutboxEntry }
  | { kind: 'full' };

type OutboxRow = {
  id: string;
  actor: string;
  ops: string;
  at: string;
  conn: string | null;
  tx: string;
  sent_to: string | null;
};

const COLUMNS = 'id, actor, ops, at, conn, tx, sent_to';

function entryOf(row: OutboxRow): OutboxEntry {
  return {
    id: row.id,
    actor: row.actor,
    ops: JSON.parse(row.ops) as SyncOp[],
    at: row.at,
    conn: row.conn,
    tx: JSON.parse(row.tx) as unknown,
    sentTo: row.sent_to,
  };
}

/**
 * 같은 만들기를 가리는 열쇠. op 하나짜리 만들기에만 있다. 할 일·노트·댓글은 클라이언트가 정한 id로, 섹션은 보드와
 * 이름으로 가린다(데몬이 이름으로 만든다).
 */
export function createKey(ops: readonly SyncOp[]): string | null {
  const [op] = ops;
  if (ops.length !== 1 || !op || !isRetrySafe(op)) {
    return null;
  }
  return op.op === 'section.create'
    ? `${op.op}:${JSON.stringify([op.board, op.title])}`
    : `${op.op}:${(op as { id: string }).id}`;
}

export class Outbox {
  constructor(private readonly storage: RelayStorage) {}

  /** 폰의 쓰기를 넣는다. 같은 만들기가 있으면 합치고, 꽉 찼으면 넣지 않는다. */
  enqueue(
    input: { actor: string; ops: SyncOp[]; at: string; conn: string; tx: unknown },
    newId: () => string,
  ): EnqueueResult {
    const { sql } = this.storage;
    const tx = JSON.stringify(input.tx ?? null);
    const key = createKey(input.ops);
    if (key !== null) {
      const [same] = sql
        .exec<OutboxRow>(`SELECT ${COLUMNS} FROM outbox WHERE create_key = ? LIMIT 1`, key)
        .toArray();
      if (same) {
        sql.exec('UPDATE outbox SET conn = ?, tx = ? WHERE id = ?', input.conn, tx, same.id);
        return { kind: 'merged', entry: entryOf({ ...same, conn: input.conn, tx }) };
      }
    }
    if (this.size() >= OUTBOX_LIMIT) {
      return { kind: 'full' };
    }
    const id = newId();
    const ops = JSON.stringify(input.ops);
    sql.exec(
      'INSERT INTO outbox (id, actor, ops, at, create_key, conn, tx) VALUES (?, ?, ?, ?, ?, ?, ?)',
      id,
      input.actor,
      ops,
      input.at,
      key,
      input.conn,
      tx,
    );
    return {
      kind: 'queued',
      entry: { ...input, id, tx: input.tx ?? null, sentTo: null },
    };
  }

  /** 맨 앞(가장 먼저 온) 쓰기. 비었으면 null. */
  head(): OutboxEntry | null {
    const [row] = this.storage.sql
      .exec<OutboxRow>(`SELECT ${COLUMNS} FROM outbox ORDER BY n LIMIT 1`)
      .toArray();
    return row ? entryOf(row) : null;
  }

  /** 이 쓰기를 데몬 연결 `conn`에 보냈다. */
  markSent(id: string, conn: string): void {
    this.storage.sql.exec('UPDATE outbox SET sent_to = ? WHERE id = ?', conn, id);
  }

  /**
   * 데몬이 답했다. 칸을 지우고(거절이면 `failed`에 남기고) 그 칸을 돌려준다. 그런 칸이 없으면(이미 답을 받았다) null.
   */
  settle(
    id: string,
    rejected?: { opIndex: number | null; error: string; failedAt: string },
  ): OutboxEntry | null {
    const { sql } = this.storage;
    const [row] = sql.exec<OutboxRow>(`SELECT ${COLUMNS} FROM outbox WHERE id = ?`, id).toArray();
    if (!row) {
      return null;
    }
    this.storage.transactionSync(() => {
      sql.exec('DELETE FROM outbox WHERE id = ?', id);
      if (rejected) {
        sql.exec(
          `INSERT INTO failed (id, actor, ops, at, op_index, error, failed_at)
           VALUES (?, ?, ?, ?, ?, ?, ?)`,
          row.id,
          row.actor,
          row.ops,
          row.at,
          rejected.opIndex,
          rejected.error,
          rejected.failedAt,
        );
        sql.exec(
          'DELETE FROM failed WHERE n NOT IN (SELECT n FROM failed ORDER BY n DESC LIMIT ?)',
          FAILED_LIMIT,
        );
      }
    });
    return entryOf(row);
  }

  size(): number {
    const [row] = this.storage.sql
      .exec<{ n: number }>('SELECT COUNT(*) AS n FROM outbox')
      .toArray();
    return row?.n ?? 0;
  }

  /** 도착 순서대로 전부. */
  entries(): OutboxEntry[] {
    return this.storage.sql
      .exec<OutboxRow>(`SELECT ${COLUMNS} FROM outbox ORDER BY n`)
      .toArray()
      .map(entryOf);
  }

  /** 거절된 쓰기, 최근 것부터. */
  failed(): FailedEntry[] {
    return this.storage.sql
      .exec<{
        id: string;
        actor: string;
        ops: string;
        at: string;
        op_index: number | null;
        error: string;
        failed_at: string;
      }>('SELECT id, actor, ops, at, op_index, error, failed_at FROM failed ORDER BY n DESC')
      .toArray()
      .map((row) => ({
        id: row.id,
        actor: row.actor,
        ops: JSON.parse(row.ops) as SyncOp[],
        at: row.at,
        opIndex: row.op_index,
        error: row.error,
        failedAt: row.failed_at,
      }));
  }
}
