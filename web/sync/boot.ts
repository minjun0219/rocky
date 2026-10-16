/**
 * 보드 싱크를 화면에 잇는다 — 부팅 때 한 번. 소켓이 없는 환경(테스트)이면 아무것도 하지 않는다. IndexedDB 가 없으면
 * 메모리 사본만으로 돈다.
 *
 * actor 를 바꾸면 그 이름으로 다시 붙는다 — 쓰기의 주인이 소켓의 `actor` 다.
 */
import { useUiStore } from '../store';
import { SyncClient } from './client';
import { idbReplicaStore, memoryReplicaStore } from './persist';

function socketUrl(actor: string): string {
  const scheme = location.protocol === 'https:' ? 'wss' : 'ws';
  return `${scheme}://${location.host}/api/ws?actor=${encodeURIComponent(actor)}`;
}

/** 저장한 사본을 읽는 데 이만큼만 기다린다 — IndexedDB 가 멈춰도 부팅은 REST 로 이어 간다. */
export const LOAD_WAIT_MS = 500;

export interface BoardSyncHandle {
  /** 저장한 사본을 읽어 봤다(있으면 이미 그렸다) — 부팅이 이걸 기다린 뒤 REST 로 나머지를 읽는다. */
  ready: Promise<void>;
  stop(): void;
}

/** 싱크를 시작한다. */
export function startBoardSync(): BoardSyncHandle {
  if (typeof WebSocket === 'undefined' || typeof location === 'undefined') {
    return { ready: Promise.resolve(), stop: () => {} };
  }
  const store = typeof indexedDB === 'undefined' ? memoryReplicaStore() : idbReplicaStore();
  let client: SyncClient | null = null;
  const begin = (actor: string): Promise<void> => {
    const next = new SyncClient({
      url: socketUrl(actor),
      actor,
      store,
      onChange: (snapshot) => useUiStore.getState().applySync(snapshot),
      onFallback: () => useUiStore.getState().detachSync(),
    });
    client = next;
    useUiStore.getState().attachSync(next);
    return next.start();
  };
  const ready = Promise.race([
    begin(useUiStore.getState().actor),
    new Promise<void>((resolve) => setTimeout(resolve, LOAD_WAIT_MS)),
  ]);
  const unsubscribe = useUiStore.subscribe((state, prev) => {
    if (state.actor !== prev.actor) {
      client?.stop();
      void begin(state.actor);
    }
  });
  return {
    ready,
    stop: () => {
      unsubscribe();
      client?.stop();
      useUiStore.getState().detachSync();
    },
  };
}

/** 이 브라우저에 남은 보드 사본을 지운다 — Access 로그아웃 앞에서. 길어야 `timeoutMs` 기다린다. */
export async function clearBoardReplica(timeoutMs = 1_000): Promise<void> {
  if (typeof indexedDB === 'undefined') {
    return;
  }
  await Promise.race([
    idbReplicaStore()
      .clear()
      .catch(() => {}),
    new Promise<void>((resolve) => setTimeout(resolve, timeoutMs)),
  ]);
}
