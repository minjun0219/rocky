/**
 * TUI 상태 — zustand vanilla 스토어(웹 `web/store.ts` 는 브라우저 주소·localStorage 에 묶여 있어 따로 둔다). 데이터는
 * 데몬에서 받고, 커서는 줄 키(`Line.key`)로 든다 — 위치로 들면 SSE 갱신으로 줄이 늘고 줄 때 다른 항목으로 미끄러진다.
 */
import { createStore } from 'zustand/vanilla';
import { ALL_SLICES, type Slice, slicesFor } from '../web/lib';
import type { Board, HandoffView, Section, TodoView } from '../web/types';
import type { Api } from './api';

/** 실패가 어디서 났나 — 읽기 실패는 다음 읽기 성공이 지우고, 동작 실패는 다음 키가 지운다(관계없는 갱신이 지우지 않게). */
export type ErrorFrom = 'load' | 'action';

export interface TuiState {
  boards: Board[];
  /** 보고 있는 보드의 key. */
  boardKey?: string;
  todos: TodoView[];
  sections: Section[];
  /** `sections` 가 어느 보드 것인가 — 보드를 바꾼 직후 새 섹션이 오기 전엔 `boardKey` 와 다르다(화면은 "읽는 중"). */
  sectionsBoard?: string;
  handoffs: HandoffView[];
  /** SSE 가 붙어 있나. */
  connected: boolean;
  /** 끊겼으면 다시 붙기까지(ms). */
  retryInMs?: number;
  /** 마지막 실패 — 상태 줄에 그대로 뜬다. */
  error?: string;
  errorFrom?: ErrorFrom;
  /** 첫 데이터를 받았나 — 그 전엔 빈 화면 대신 "읽는 중". */
  ready: boolean;
  cursorKey?: string;
  /** 커서가 마지막으로 있던 줄 위치 — 그 줄이 사라지면(완료로 트리에서 빠짐) 맨 위가 아니라 이 근처로 간다. */
  cursorIndex: number;

  load: (slices?: ReadonlySet<Slice>) => Promise<void>;
  /** SSE 이벤트 — 150ms 창으로 모아 이벤트가 건드린 묶음만 다시 받는다(웹 `main.tsx` 와 같은 규칙). */
  queueEvents: (events: unknown[]) => void;
  setConnected: (connected: boolean, retryInMs?: number) => void;
  /** `[`·`]` — 보드를 앞뒤로. 끝에서 돈다. */
  shiftBoard: (delta: number) => void;
  setCursor: (key: string | undefined, index: number) => void;
  setError: (error: string | undefined, from?: ErrorFrom) => void;
}

const DEBOUNCE_MS = 150;

export function createTuiStore(api: Api, initialBoard?: string) {
  let pending: unknown[] = [];
  let timer: ReturnType<typeof setTimeout> | undefined;

  return createStore<TuiState>()((set, get) => {
    const fail = (e: unknown, from: ErrorFrom) =>
      set({ error: e instanceof Error ? e.message : String(e), errorFrom: from });
    const loadSections = async (key: string | undefined) =>
      key ? api.sections(key) : ([] as Section[]);

    return {
      boards: [],
      boardKey: initialBoard,
      todos: [],
      sections: [],
      handoffs: [],
      connected: false,
      ready: false,
      cursorIndex: 0,

      load: async (slices = new Set(ALL_SLICES)) => {
        const want = (slice: Slice) => slices.has(slice);
        const current = get().boardKey;
        try {
          const [boards, todos, handoffs] = await Promise.all([
            want('boards') ? api.boards() : undefined,
            want('todos') ? api.todos() : undefined,
            want('handoffs') ? api.handoffs() : undefined,
          ]);
          // 처음이거나 보던 보드가 없어졌으면(이름 바뀜·보관) 첫 보드로.
          const known = boards ?? get().boards;
          const boardKey =
            boards && (!current || !known.some((b) => b.key === current)) ? known[0]?.key : current;
          const sections =
            want('sections') || boardKey !== get().sectionsBoard
              ? await loadSections(boardKey)
              : undefined;
          // 받는 사이 사람이 보드를 바꿨으면(`[`·`]`) 그 선택을 두고, 옛 보드 기준으로 받은 섹션은 버린다.
          const switched = get().boardKey !== current;
          set({
            ...(boards ? { boards } : {}),
            ...(boards && !switched ? { boardKey } : {}),
            ...(todos ? { todos } : {}),
            ...(handoffs ? { handoffs } : {}),
            ...(sections && !switched ? { sections, sectionsBoard: boardKey } : {}),
            ready: true,
            ...(get().errorFrom === 'load' ? { error: undefined, errorFrom: undefined } : {}),
          });
        } catch (e) {
          fail(e, 'load');
        }
      },

      queueEvents: (events) => {
        pending.push(...events);
        clearTimeout(timer);
        timer = setTimeout(() => {
          const slices = slicesFor(pending);
          pending = [];
          void get().load(slices);
        }, DEBOUNCE_MS);
      },

      setConnected: (connected, retryInMs) => set({ connected, retryInMs }),

      shiftBoard: (delta) => {
        const { boards, boardKey } = get();
        if (boards.length === 0) {
          return;
        }
        const at = Math.max(
          0,
          boards.findIndex((b) => b.key === boardKey),
        );
        const next = boards[(at + delta + boards.length) % boards.length];
        if (!next || next.key === boardKey) {
          return;
        }
        // 섹션은 새 것이 올 때까지 둔다 — `sectionsBoard` 가 다르니 화면은 그동안 "읽는 중" 으로 가린다.
        set({ boardKey: next.key });
        loadSections(next.key)
          .then((sections) => {
            // 받는 사이 또 바꿨으면 늦게 온 것을 버린다.
            if (get().boardKey === next.key) {
              set({ sections, sectionsBoard: next.key });
            }
          })
          .catch((e) => fail(e, 'load'));
      },

      setCursor: (cursorKey, cursorIndex) => set({ cursorKey, cursorIndex }),
      setError: (error, from = 'action') => set({ error, errorFrom: error ? from : undefined }),
    };
  });
}

export type TuiStore = ReturnType<typeof createTuiStore>;
