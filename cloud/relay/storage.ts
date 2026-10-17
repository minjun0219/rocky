/**
 * 중계소가 쓰는 저장소. Durable Object의 `ctx.storage`에서 쓰는 부분만 옮긴 모양이다. DO에서는 `ctx.storage`를
 * 그대로 넘기고, 테스트는 `bun:sqlite`로 흉내 낸다(`./test-support`).
 *
 * 테이블(설계는 `docs/design/specs/2026-10-15-dual-structure-sync-design.md`의 3번 조각 "중계소가 드는 것"):
 * - `mirror`: 데몬이 보낸 행을 JSON 그대로. 중계소는 행을 해석하지 않는다.
 * - `meta`: 사본의 머리(데몬 `epoch`, 세대, 순번)와 지금 데몬 연결.
 * - `outbox`: 폰이 보낸 쓰기를 도착 순서로. 데몬이 답하면 지운다.
 * - `failed`: 데몬이 거절한 변경함 쓰기의 최근 것.
 */

/** DO SQLite가 받고 돌려주는 값. */
export type SqlValue = string | number | null | ArrayBuffer;

export interface SqlCursor<T> {
  toArray(): T[];
}

export interface Sql {
  exec<T extends Record<string, SqlValue>>(query: string, ...bindings: SqlValue[]): SqlCursor<T>;
}

export interface RelayStorage {
  readonly sql: Sql;
  /** 안의 쓰기를 한 트랜잭션으로 묶는다. 던지면 되돌린다. 겹쳐 부르지 않는다. */
  transactionSync<T>(fn: () => T): T;
}

/** 한 번에 문장 하나씩. DO의 `exec`와 `bun:sqlite`가 여러 문장을 다르게 다룬다. */
const SCHEMA = [
  `CREATE TABLE IF NOT EXISTS mirror (
    tbl TEXT NOT NULL,
    id TEXT NOT NULL,
    seq INTEGER NOT NULL,
    body TEXT NOT NULL,
    PRIMARY KEY (tbl, id)
  )`,
  'CREATE INDEX IF NOT EXISTS mirror_seq ON mirror (seq)',
  'CREATE TABLE IF NOT EXISTS meta (k TEXT PRIMARY KEY, v TEXT NOT NULL)',
  `CREATE TABLE IF NOT EXISTS outbox (
    n INTEGER PRIMARY KEY AUTOINCREMENT,
    id TEXT NOT NULL UNIQUE,
    actor TEXT NOT NULL,
    ops TEXT NOT NULL,
    at TEXT NOT NULL,
    create_key TEXT,
    conn TEXT,
    tx TEXT NOT NULL,
    sent_to TEXT
  )`,
  'CREATE INDEX IF NOT EXISTS outbox_create ON outbox (create_key)',
  `CREATE TABLE IF NOT EXISTS failed (
    n INTEGER PRIMARY KEY AUTOINCREMENT,
    id TEXT NOT NULL,
    actor TEXT NOT NULL,
    ops TEXT NOT NULL,
    at TEXT NOT NULL,
    op_index INTEGER,
    error TEXT NOT NULL,
    failed_at TEXT NOT NULL
  )`,
];

/** 테이블을 만든다. 이미 있으면 그대로 둔다. DO가 깰 때마다 부른다. */
export function migrate(storage: RelayStorage): void {
  for (const statement of SCHEMA) {
    storage.sql.exec(statement);
  }
}

export function readMeta(storage: RelayStorage, key: string): string | null {
  const [row] = storage.sql.exec<{ v: string }>('SELECT v FROM meta WHERE k = ?', key).toArray();
  return row?.v ?? null;
}

export function writeMeta(storage: RelayStorage, key: string, value: string | null): void {
  if (value === null) {
    storage.sql.exec('DELETE FROM meta WHERE k = ?', key);
    return;
  }
  storage.sql.exec(
    'INSERT INTO meta (k, v) VALUES (?, ?) ON CONFLICT (k) DO UPDATE SET v = excluded.v',
    key,
    value,
  );
}
