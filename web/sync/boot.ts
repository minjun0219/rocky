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

export interface BoardSyncHandle {
  /** 저장한 사본을 읽어 봤다 — 있으면 이미 그렸다. */
  cached: Promise<void>;
  /**
   * 화면이 데몬을 따라잡았다 — 소켓으로 첫 `rows` 를 받았거나, 저장 사본이 없어 REST 로 그리거나, 기다려 줄 시간이 지나
   * REST 로 넘어갔다. 주소(`/rocky/57`)는 이 뒤에 푼다 — 낡은 사본에 없는 새 항목의 링크를 지우지 않게.
   */
  caughtUp: Promise<void>;
  stop(): void;
}

/** 지금 도는 클라이언트 — 로그아웃이 사본을 지우기 전에 멈춘다. */
let active: SyncClient | null = null;

/** 싱크를 시작한다. */
export function startBoardSync(): BoardSyncHandle {
  if (typeof WebSocket === 'undefined' || typeof location === 'undefined') {
    return { cached: Promise.resolve(), caughtUp: Promise.resolve(), stop: () => {} };
  }
  const store = typeof indexedDB === 'undefined' ? memoryReplicaStore() : idbReplicaStore();
  let caughtUpNow: () => void = () => {};
  const caughtUp = new Promise<void>((resolve) => {
    caughtUpNow = resolve;
  });
  const begin = (actor: string): Promise<void> => {
    const next = new SyncClient({
      url: socketUrl(actor),
      actor,
      store,
      onChange: (snapshot) => {
        useUiStore.getState().applySync(snapshot);
        if (snapshot.live) {
          caughtUpNow();
        }
      },
    });
    active = next;
    useUiStore.getState().attachSync(next);
    return next.start();
  };
  const cached = begin(useUiStore.getState().actor);
  // 저장 사본이 없었으면 기다릴 것이 없다 — REST 로 그린다.
  void cached.then(() => {
    if (!useUiStore.getState().syncing) {
      caughtUpNow();
    }
  });
  const unsubscribe = useUiStore.subscribe((state, prev) => {
    if (state.actor !== prev.actor) {
      active?.stop();
      void begin(state.actor);
    }
    if (prev.syncing && !state.syncing) {
      caughtUpNow(); // 기다려 줄 시간이 지나 REST 로 넘어갔다
    }
  });
  return {
    cached,
    caughtUp,
    stop: () => {
      unsubscribe();
      active?.stop();
      active = null;
      useUiStore.getState().detachSync();
    },
  };
}

/**
 * 이 브라우저에 남은 보드 사본을 지운다 — Access 로그아웃 앞에서. 싱크부터 멈추고 걸려 있던 저장이 끝난 뒤에 지운다(지운
 * 뒤에 온 행이 다시 쓰이지 않게). 길어야 `timeoutMs` 기다린다.
 */
export async function clearBoardReplica(timeoutMs = 1_000): Promise<void> {
  const client = active;
  active = null;
  client?.stop();
  if (typeof indexedDB === 'undefined') {
    return;
  }
  const clear = async () => {
    await client?.idle();
    await idbReplicaStore().clear();
  };
  await Promise.race([
    clear().catch(() => {}),
    new Promise<void>((resolve) => setTimeout(resolve, timeoutMs)),
  ]);
}
