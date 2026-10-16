/**
 * 보드 사본 — 데몬의 싱크 프레임(`rows`)을 테이블별 행 맵으로 들고, 아직 답을 받지 않은 쓰기(op)를 그 위에 겹친다.
 *
 * 순수 모듈이다. 소켓·IndexedDB·zustand 는 부른 쪽이 잇는다. 프레임과 규칙은 `docs/features/sync.md`, 설계는
 * `docs/design/specs/2026-10-15-dual-structure-sync-design.md` 의 2번 조각. 화면용 목록과 계산 값은 `./views`.
 */
import type { Board, Comment, Handoff, HistoryEntry, Note, Section, Todo } from '../types';

/** 데몬이 싱크하는 일곱 테이블 — `rocky_core::sync::SyncTables` 와 같은 모양. */
export interface SyncTables {
  boards: Board[];
  sections: Section[];
  todos: Todo[];
  comments: Comment[];
  notes: Note[];
  history: HistoryEntry[];
  handoffs: Handoff[];
}

export type TableName = keyof SyncTables;

export const TABLE_NAMES: readonly TableName[] = [
  'boards',
  'sections',
  'todos',
  'comments',
  'notes',
  'history',
  'handoffs',
];

/** 테이블 하나의 행. */
export type RowOf<K extends TableName> = SyncTables[K][number];

/** 테이블마다 id → 행. 히스토리의 숫자 id 도 문자열 키로 둔다. */
export type Tables = { readonly [K in TableName]: ReadonlyMap<string, RowOf<K>> };

/** 데몬이 보내는 `{t:"rows"}` 프레임의 몸통. */
export interface RowsFrame {
  epoch: string;
  seq: number;
  /** 참이면 `rows` 가 DB 의 보드 행 전부다 — 사본을 통째로 바꾼다. */
  full: boolean;
  rows: Partial<SyncTables>;
}

export interface Replica {
  /** 이 사본을 준 DB 의 표식. 데몬이 다시 뜨면 바뀐다. 아직 받은 적이 없으면 null. */
  readonly epoch: string | null;
  /** 받은 마지막 순번 — 다음 `sync` 의 `since`. 받은 적이 없으면 -1. */
  readonly seq: number;
  readonly tables: Tables;
}

/**
 * 사본이 들고 있는 히스토리 줄 수 — 데몬이 통째로 줄 때 싣는 줄 수와 같다. 이어 받으며 늘어난 줄은 이 수로 다시
 * 자른다. 그보다 오래된 줄은 상세 드로어가 REST 로 채운다.
 */
export const HISTORY_LIMIT = 2_000;

function keyOf(row: { id: string | number }): string {
  return String(row.id);
}

function emptyTables(): Tables {
  return {
    boards: new Map(),
    sections: new Map(),
    todos: new Map(),
    comments: new Map(),
    notes: new Map(),
    history: new Map(),
    handoffs: new Map(),
  };
}

export function emptyReplica(): Replica {
  return { epoch: null, seq: -1, tables: emptyTables() };
}

/** 히스토리를 새 줄부터 `HISTORY_LIMIT` 줄만 남긴다. 넘지 않으면 그대로 돌려준다. */
function trimHistory(
  history: ReadonlyMap<string, HistoryEntry>,
): ReadonlyMap<string, HistoryEntry> {
  if (history.size <= HISTORY_LIMIT) {
    return history;
  }
  const kept = [...history.values()].sort((a, b) => b.id - a.id).slice(0, HISTORY_LIMIT);
  return new Map(kept.map((h) => [keyOf(h), h]));
}

/**
 * 받은 `rows` 를 사본에 적용한다. `full` 이면 통째로 바꾸고, 아니면 온 행을 id 로 덮어쓴다(삭제가 없어 지운 행은
 * 오지 않는다). 바뀌지 않은 테이블은 같은 맵을 그대로 둔다 — 부른 쪽이 참조 비교로 다시 그릴지 정한다.
 */
export function applyRows(replica: Replica, frame: RowsFrame): Replica {
  if (frame.full) {
    const tables = emptyTables() as { [K in TableName]: Map<string, RowOf<K>> };
    for (const name of TABLE_NAMES) {
      const into = tables[name] as Map<string, RowOf<TableName>>;
      for (const row of frame.rows[name] ?? []) {
        into.set(keyOf(row), row);
      }
    }
    return {
      epoch: frame.epoch,
      seq: frame.seq,
      tables: { ...tables, history: trimHistory(tables.history) },
    };
  }
  const tables: { -readonly [K in TableName]: ReadonlyMap<string, RowOf<K>> } = {
    ...replica.tables,
  };
  for (const name of TABLE_NAMES) {
    const rows = frame.rows[name];
    if (!rows || rows.length === 0) {
      continue;
    }
    const next = new Map(replica.tables[name] as ReadonlyMap<string, RowOf<TableName>>);
    for (const row of rows) {
      next.set(keyOf(row), row);
    }
    (tables as Record<TableName, ReadonlyMap<string, RowOf<TableName>>>)[name] = next;
  }
  tables.history = trimHistory(tables.history);
  return { epoch: frame.epoch, seq: Math.max(replica.seq, frame.seq), tables };
}
