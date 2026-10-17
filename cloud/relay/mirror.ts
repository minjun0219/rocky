/**
 * 중계소 사본. 데몬이 확정해 보낸 행을 테이블과 id로 들고, 폰에게 커서 이후의 행을 `rows` 프레임으로 준다.
 *
 * 행은 받은 JSON 그대로 두고 `seq`만 꺼내 쓴다. 보드 규칙은 데몬에만 있다. 폰에게 주는 `epoch`는
 * `<데몬 epoch>.<세대>`이고, 데몬에게 통째 사본을 받을 때마다 세대를 올린다. 데몬이 거르기 때문에 사본에서 빠지는
 * 행이 생기는데 삭제 프레임이 없어서, 폰도 통째로 다시 받아야 그 행이 사라진다.
 */
import { HISTORY_LIMIT, TABLE_NAMES, type TableName } from '../../web/sync/replica';
import { type RelayStorage, readMeta, writeMeta } from './storage';

/** 데몬이 보낸 `rows` 프레임. 행은 해석하지 않는다. */
export interface DaemonRows {
  epoch: string;
  seq: number;
  full: boolean;
  rows: Partial<Record<TableName, unknown[]>>;
}

export interface MirrorHead {
  daemonEpoch: string;
  /** 데몬에게 통째 사본을 받은 횟수. */
  generation: number;
  /** 데몬에게 받은 마지막 순번. */
  seq: number;
}

/** 폰이 마지막으로 받은 머리. 폰의 `sync`에 실려 오는 `epoch`·`since`다. */
export interface Cursor {
  epoch: string;
  seq: number;
}

/** 폰에게 보낼 `rows` 프레임과 그 뒤의 커서. */
export interface RowsOut {
  text: string;
  cursor: Cursor;
  full: boolean;
  /** 실은 행 수. */
  count: number;
}

/** 폰에게 주는 `epoch`. */
export function phoneEpoch(head: MirrorHead): string {
  return `${head.daemonEpoch}.${head.generation}`;
}

/** 폰이 사본을 통째로 다시 받아야 하나. `rocky_core::sync::needs_full`과 같은 규칙이다. */
export function needsFull(cursor: Cursor | null, head: MirrorHead): boolean {
  return (
    cursor === null || cursor.epoch !== phoneEpoch(head) || cursor.seq < 0 || cursor.seq > head.seq
  );
}

function idOf(row: unknown): string | null {
  const id = (row as { id?: unknown } | null)?.id;
  return typeof id === 'string' || typeof id === 'number' ? String(id) : null;
}

function seqOf(row: unknown): number | null {
  const seq = (row as { seq?: unknown } | null)?.seq;
  return typeof seq === 'number' ? seq : null;
}

export class Mirror {
  constructor(private readonly storage: RelayStorage) {}

  /** 사본의 머리. 데몬에게 아직 받은 적이 없으면 null. */
  head(): MirrorHead | null {
    const daemonEpoch = readMeta(this.storage, 'epoch');
    if (daemonEpoch === null) {
      return null;
    }
    return {
      daemonEpoch,
      generation: Number(readMeta(this.storage, 'generation') ?? 0),
      seq: Number(readMeta(this.storage, 'seq') ?? -1),
    };
  }

  /**
   * 데몬의 `rows`를 적용한다. 통째면 사본을 비우고 다시 쓰며 세대를 올린다. 이어 받기는 온 행을 id로 덮어쓴다.
   * 이어 받기인데 사본이 없거나 `epoch`가 다르면 적용하지 않고 false를 돌려준다. 부른 쪽이 통째로 다시 묻는다.
   */
  apply(frame: DaemonRows): boolean {
    const head = this.head();
    if (!frame.full && (head === null || head.daemonEpoch !== frame.epoch)) {
      return false;
    }
    const { sql } = this.storage;
    this.storage.transactionSync(() => {
      if (frame.full) {
        sql.exec('DELETE FROM mirror');
      }
      for (const table of TABLE_NAMES) {
        for (const row of frame.rows[table] ?? []) {
          const id = idOf(row);
          if (id === null) {
            continue;
          }
          sql.exec(
            `INSERT INTO mirror (tbl, id, seq, body) VALUES (?, ?, ?, ?)
             ON CONFLICT (tbl, id) DO UPDATE SET seq = excluded.seq, body = excluded.body`,
            table,
            id,
            seqOf(row) ?? frame.seq,
            JSON.stringify(row),
          );
        }
      }
      this.trimHistory();
      const generation = frame.full ? (head?.generation ?? 0) + 1 : head!.generation;
      const seq = frame.full ? frame.seq : Math.max(head!.seq, frame.seq);
      writeMeta(this.storage, 'epoch', frame.epoch);
      writeMeta(this.storage, 'generation', String(generation));
      writeMeta(this.storage, 'seq', String(seq));
    });
    return true;
  }

  /**
   * 폰에게 보낼 `rows`. 커서가 이 사본의 것이면 그 뒤에 바뀐 행만, 아니면 통째로 싣는다. `id`가 있으면 같은 `id`로
   * 답한다(`sync`의 답). 사본이 비어 있으면 null.
   */
  rowsFor(cursor: Cursor | null, id?: unknown): RowsOut | null {
    const head = this.head();
    if (head === null) {
      return null;
    }
    const full = needsFull(cursor, head);
    const found = full
      ? this.storage.sql
          .exec<{ tbl: string; body: string }>('SELECT tbl, body FROM mirror')
          .toArray()
      : this.storage.sql
          .exec<{ tbl: string; body: string }>(
            'SELECT tbl, body FROM mirror WHERE seq > ? ORDER BY seq',
            cursor!.seq,
          )
          .toArray();
    const tables = new Map<string, string[]>();
    for (const { tbl, body } of found) {
      const bodies = tables.get(tbl);
      if (bodies) {
        bodies.push(body);
      } else {
        tables.set(tbl, [body]);
      }
    }
    const epoch = phoneEpoch(head);
    const rows = TABLE_NAMES.map((t) => `"${t}":[${(tables.get(t) ?? []).join(',')}]`).join(',');
    const idPart = id === undefined || id === null ? '' : `"id":${JSON.stringify(id)},`;
    return {
      text: `{"t":"rows",${idPart}"epoch":${JSON.stringify(epoch)},"seq":${head.seq},"full":${full},"rows":{${rows}}}`,
      cursor: { epoch, seq: head.seq },
      full,
      count: found.length,
    };
  }

  /** 히스토리를 새 줄부터 `HISTORY_LIMIT`줄만 남긴다. 데몬이 통째로 줄 때와 같은 수다. */
  private trimHistory(): void {
    const [counted] = this.storage.sql
      .exec<{ n: number }>("SELECT COUNT(*) AS n FROM mirror WHERE tbl = 'history'")
      .toArray();
    if ((counted?.n ?? 0) <= HISTORY_LIMIT) {
      return;
    }
    this.storage.sql.exec(
      `DELETE FROM mirror WHERE tbl = 'history' AND id NOT IN (
         SELECT id FROM mirror WHERE tbl = 'history' ORDER BY CAST(id AS INTEGER) DESC LIMIT ?
       )`,
      HISTORY_LIMIT,
    );
  }
}
