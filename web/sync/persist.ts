/**
 * 보드 사본을 브라우저에 남긴다 — IndexedDB 데이터베이스 하나에 테이블마다 저장소, `meta` 에 머리(`epoch`·`seq`).
 * 다음 부팅이 네트워크를 기다리지 않고 이것으로 먼저 그린다.
 *
 * 실패는 부른 쪽(`SyncClient`)이 삼킨다 — 저장소를 못 여는 브라우저(비공개 창 등)는 메모리 사본만으로 돈다.
 * 실제 브라우저에서의 동작은 E2E(`e2e/`)가 본다. bun·happy-dom 에는 IndexedDB 가 없다.
 */
import type { ReplicaStore } from './client';
import {
  HISTORY_LIMIT,
  type Replica,
  type RowOf,
  TABLE_NAMES,
  type TableName,
  type Tables,
} from './replica';

export const DB_NAME = 'rocky-sync';
/** 저장소 모양을 바꾸면 올린다 — 올라가면 옛 사본을 버리고 통째로 다시 받는다. */
const DB_VERSION = 1;
const META = 'meta';
const HEAD = 'head';

function request<T>(req: IDBRequest<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

function finished(tx: IDBTransaction): Promise<void> {
  return new Promise((resolve, reject) => {
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error);
    tx.onabort = () => reject(tx.error ?? new Error('IndexedDB transaction aborted'));
  });
}

/** 사본이 히스토리를 잘라 냈으면 남은 가장 오래된 id — 그보다 작은 줄은 저장소에서도 지운다. */
function historyFloor(replica: Replica): number | undefined {
  if (replica.tables.history.size < HISTORY_LIMIT) {
    return undefined;
  }
  let floor = Number.POSITIVE_INFINITY;
  for (const entry of replica.tables.history.values()) {
    floor = Math.min(floor, entry.id);
  }
  return Number.isFinite(floor) ? floor : undefined;
}

export function idbReplicaStore(factory: IDBFactory = indexedDB, name = DB_NAME): ReplicaStore {
  let opening: Promise<IDBDatabase> | null = null;
  const open = (): Promise<IDBDatabase> => {
    opening ??= new Promise<IDBDatabase>((resolve, reject) => {
      const req = factory.open(name, DB_VERSION);
      req.onupgradeneeded = () => {
        const db = req.result;
        // 옛 모양의 저장소는 버린다 — 다음 `sync` 가 통째로 다시 채운다.
        for (const store of [...db.objectStoreNames]) {
          db.deleteObjectStore(store);
        }
        for (const table of TABLE_NAMES) {
          db.createObjectStore(table, { keyPath: 'id' });
        }
        db.createObjectStore(META);
      };
      req.onsuccess = () => {
        const db = req.result;
        // 다른 탭이 새 모양(`DB_VERSION`)으로 열려 하면 놓아준다 — 쥐고 있으면 그 탭의 열기가 끝나지 않는다.
        db.onversionchange = () => {
          db.close();
          opening = null;
        };
        resolve(db);
      };
      req.onerror = () => reject(req.error);
      req.onblocked = () => reject(new Error('IndexedDB open blocked'));
    }).catch((error: unknown) => {
      opening = null; // 다음 호출이 다시 열어 본다
      throw error;
    });
    return opening;
  };

  return {
    async load() {
      const db = await open();
      const tx = db.transaction([...TABLE_NAMES, META], 'readonly');
      // 요청을 먼저 다 걸고 기다린다 — 기다리는 사이 트랜잭션이 끝나지 않게.
      const head = request(tx.objectStore(META).get(HEAD)) as Promise<
        { epoch: string; seq: number } | undefined
      >;
      const rows = TABLE_NAMES.map((table) => request(tx.objectStore(table).getAll()));
      const [meta, ...all] = await Promise.all([head, ...rows]);
      if (!meta || typeof meta.epoch !== 'string' || typeof meta.seq !== 'number') {
        return null;
      }
      const tables = {} as Record<TableName, Map<string, RowOf<TableName>>>;
      TABLE_NAMES.forEach((table, index) => {
        const list = (all[index] ?? []) as RowOf<TableName>[];
        tables[table] = new Map(list.map((row) => [String(row.id), row]));
      });
      return { epoch: meta.epoch, seq: meta.seq, tables: tables as unknown as Tables };
    },

    async save(frame, replica) {
      const db = await open();
      const tx = db.transaction([...TABLE_NAMES, META], 'readwrite');
      for (const table of TABLE_NAMES) {
        const store = tx.objectStore(table);
        if (frame.full) {
          store.clear();
        }
        for (const row of frame.rows[table] ?? []) {
          store.put(row);
        }
      }
      const floor = historyFloor(replica);
      if (floor !== undefined) {
        tx.objectStore('history').delete(IDBKeyRange.upperBound(floor, true));
      }
      tx.objectStore(META).put({ epoch: replica.epoch, seq: replica.seq }, HEAD);
      await finished(tx);
    },

    async clear() {
      const db = await open();
      const tx = db.transaction([...TABLE_NAMES, META], 'readwrite');
      for (const store of [...TABLE_NAMES, META]) {
        tx.objectStore(store).clear();
      }
      await finished(tx);
    },
  };
}

/** 메모리 판 — 테스트와 IndexedDB 가 없는 환경. */
export function memoryReplicaStore(): ReplicaStore {
  let saved: Replica | null = null;
  return {
    load: async () => saved,
    save: async (_frame, replica) => {
      saved = replica;
    },
    clear: async () => {
      saved = null;
    },
  };
}
