// 보드 패널(register.tsx)의 순수 판정 — 데몬 응답을 패널이 그릴 모양으로 바꾼다. 엔진(`claude-code`)을 부르지 않아
// bun 테스트(data.test.ts, CI)로 돈다.

/** 패널 한 줄 — 진행 중이거나 할 일인 보드 항목. */
export type BoardItem = { ref: string; title: string; status: 'doing' | 'todo' };

/**
 * 패널이 그리는 것 — 데몬에서 읽은 결과와 마지막 실패 사유. 상태 계약(plugin/types/index.d.ts 의 `BoardPanelView`)에
 * 같은 모양을 옮겨 적었다 — 필드를 바꾸면 거기도(data.test.ts 가 필드 이름을 대조한다).
 */
export type BoardView = {
  /** 세션 폴더(cwd)로 풀린 보드 key — 붙은 보드가 없으면 null. */
  board: string | null;
  items: BoardItem[];
  /** 수집함에 아직 안 올린 외부 항목 수 — 데몬이 캐시로만 답해 모르면 null. */
  collect: number | null;
  /** 마지막으로 읽지 못한 사유 — 읽으면 비운다. */
  error: string | null;
  /** 마지막으로 읽은 시각(ISO) — 한 번도 못 읽었으면 null. */
  at: string | null;
};

export const EMPTY_VIEW: BoardView = {
  board: null,
  items: [],
  collect: null,
  error: null,
  at: null,
};

/** 데몬 GET 하나 — 경로(`/api/…`)를 받아 상태와 본문을 돌려준다. */
export type Get = (path: string) => Promise<{ ok: boolean; status: number; text: string }>;

/** 읽기 결과 — 실패면 패널에 보일 사유(어느 요청이 왜). */
export type Read<T> = { ok: true; value: T } | { ok: false; error: string };

type Todo = { ref?: string; title?: string; status?: string };

/** 진행 중을 먼저, 그다음 할 일. 끝낸 것과 모양이 틀린 행은 뺀다. 같은 상태 안의 순서는 데몬이 준 대로. */
export function boardItems(todos: readonly Todo[]): BoardItem[] {
  const pick = (status: BoardItem['status']): BoardItem[] =>
    todos.flatMap((t) =>
      t.status === status && typeof t.ref === 'string' && typeof t.title === 'string'
        ? [{ ref: t.ref, title: t.title, status }]
        : [],
    );
  return [...pick('doing'), ...pick('todo')];
}

/** GET 한 번을 JSON 으로 — 상태 코드나 본문이 틀리면 그 요청 이름과 함께 사유로. */
async function getJson(get: Get, path: string): Promise<Read<unknown>> {
  const name = path.split('?')[0];
  const res = await get(path);
  if (!res.ok) {
    return { ok: false, error: `${name} 응답 ${res.status}` };
  }
  try {
    return { ok: true, value: JSON.parse(res.text) };
  } catch {
    return { ok: false, error: `${name} 응답이 JSON 이 아니다(${res.text.slice(0, 40)})` };
  }
}

/**
 * 세션 폴더의 보드 key 와 수집함 수. `cached=true` 로 부른다 — 그냥 부르면 캐시가 만료된 수집함 어댑터(외부 명령)를 데몬이
 * 실행하고 기다린다. 값은 `"true"` 여야 켜진다(`cached=1` 은 꺼진 것으로 읽힌다).
 */
export async function readSummary(
  get: Get,
  cwd: string,
): Promise<Read<{ board: string | null; collect: number | null }>> {
  const read = await getJson(get, `/api/summary?cached=true&cwd=${encodeURIComponent(cwd)}`);
  if (!read.ok) {
    return { ok: false, error: read.error };
  }
  const body = (read.value ?? {}) as { board?: unknown; collect?: unknown };
  return {
    ok: true,
    value: {
      board: typeof body.board === 'string' && body.board !== '' ? body.board : null,
      collect: typeof body.collect === 'number' ? body.collect : null,
    },
  };
}

/** 그 보드의 진행 중·할 일. */
export async function readItems(get: Get, board: string): Promise<Read<BoardItem[]>> {
  const read = await getJson(get, `/api/todos?board=${encodeURIComponent(board)}`);
  if (!read.ok) {
    return { ok: false, error: read.error };
  }
  if (!Array.isArray(read.value)) {
    return { ok: false, error: '/api/todos 응답이 목록이 아니다' };
  }
  return { ok: true, value: boardItems(read.value as Todo[]) };
}

/** `GET /api/changes` 한 번 — 히스토리의 마지막 id 와, `sinceId` 뒤의 새 행들이 건드린 보드 key(모르면 null). */
export type Changes = { lastId: number; boards: (string | null)[] };

/** `/api/changes` 응답을 읽는다. 모양이 틀리면 null. */
export function parseChanges(text: string): Changes | null {
  try {
    const raw = JSON.parse(text) as { lastId?: unknown; entries?: unknown };
    if (typeof raw.lastId !== 'number' || !Number.isFinite(raw.lastId)) {
      return null;
    }
    const entries: unknown[] = Array.isArray(raw.entries) ? raw.entries : [];
    const boards = entries.map((e) => {
      const key = (e as { boardKey?: unknown } | null)?.boardKey;
      return typeof key === 'string' ? key : null;
    });
    return { lastId: raw.lastId, boards };
  } catch {
    return null;
  }
}

/**
 * 새 히스토리가 이 보드를 건드렸나 — `lastId` 는 전 보드의 것이라 그것만 보면 다른 보드의 변화에도 모든 세션의 패널이
 * 다시 읽는다. 받은 행이 `limit` 만큼 꽉 찼으면 그 뒤에 더 있을 수 있어 건드린 것으로 본다.
 */
export function boardTouched(changes: Changes, board: string, limit: number): boolean {
  return changes.boards.length >= limit || changes.boards.includes(board);
}
