import type { BoardView } from './types';

/**
 * URL ↔ 화면 상태 변환 — 웹 UI 퍼머링크의 단일 소유자.
 *
 * 라우터 라이브러리를 쓰지 않는다(레포 원칙: 신규 런타임 dep 은 별도 논의). 대신
 * History API 호출은 `src/ui/store.ts` 가 하고, 이 파일은 **순수 변환만** 맡아
 * 단위 테스트된다 — `src/ui/lib.ts` 가 `mdTokens`/`formatElapsed` 를 두는 것과 같은 이유다.
 *
 * URL 문법: `/`(전체) · `/{board}` · `/{board}/{number}` · `/{board}?todo={board}-{number}` ·
 * `/?todo={board}-{number}` · 노트 상세 `/{board}/notes/{number}` · `/{board}?note={ref}`
 *
 * 뒤의 둘은 **보고 있는 보드와 열린 todo 의 보드가 다른** 경우다 — 전체 보기에서 상세를
 * 열거나, "지금" 표에서 다른 보드의 항목을 연 경우. 예전엔 이때 선택을 그 todo 의 보드로
 * 옮겼는데(주소 `/rocky/12` 가 되읽히면 그 보드가 되므로) 상세를 여는 동작이 뒤 화면을
 * 갈아치우는 부작용이 됐다. 주소가 둘을 따로 실으면 선택은 그대로 둘 수 있다.
 */

/**
 * 보드 선택 — `'all'`(전체 보기) 또는 board key.
 *
 * 이 타입이 `src/ui/store.ts` 가 아니라 여기 있는 이유: store 가 route 를 import 하므로
 * 반대 방향 import 는 순환이 된다. store 는 기존 import 경로 보존용으로 재수출한다.
 */
export type BoardSelection = 'all' | string;

/** 상세가 열린 todo — 보드 key + 보드 안 번호(`rocky-12` 의 두 조각). */
export interface TodoRef {
  board: string;
  number: number;
}

/**
 * URL 이 담는 화면 상태. `todo` 가 있으면 그 todo 의 상세가 열린 상태, `note` 가 있으면 노트
 * 화면에서 그 노트(ref — `rocky-3` · 전역 `note-3`)의 상세가 열린 상태다. 둘은 같이 싣지 않는다.
 */
export interface Route {
  board: BoardSelection;
  todo?: TodoRef;
  note?: string;
  /**
   * 보던 탭(`?view=todos`). 피드는 싣지 않는다 — 맨 주소가 첫 화면(피드)이다. 노트 상세(`/rocky/notes/3`·
   * `?note=`)는 그 자체가 노트 탭이라 싣지 않는다. 경로 세그먼트(`/rocky/todos`)가 아니라 쿼리인 이유: 둘째
   * 세그먼트는 이미 todo 번호가, 첫 세그먼트는 board key(`all` 포함)가 쓴다 — 탭 이름과 부딪힐 수 있다.
   */
  view?: Exclude<BoardView, 'feed'>;
}

const VIEWS: readonly BoardView[] = ['feed', 'todos', 'notes', 'worklog', 'github', 'agents', 'rc'];

/** `?view=todos` → `'todos'`. 모르는 값·피드는 undefined(첫 화면). */
function parseViewParam(search: string): Route['view'] {
  let raw: string | null;
  try {
    raw = new URLSearchParams(search).get('view');
  } catch {
    return undefined;
  }
  return raw !== null && raw !== 'feed' && VIEWS.includes(raw as BoardView)
    ? (raw as Route['view'])
    : undefined;
}

/**
 * 경로 첫 세그먼트로 **가리킬 수 없는** board key — 데몬의 REST/MCP 라우트와 충돌한다.
 * 이 키의 보드도 `ensureBoard`(`src/store.ts`)로 정상 생성되고 동작한다 — 다만 URL 로
 * 가리킬 방법이 없어(`/api` 는 `/api/*` 라우트에 먹힌다) `buildPath` 가 이 키를 만나면
 * 전체 보기와 같은 `/` 를 낸다.
 */
export const RESERVED_BOARD_KEYS: readonly string[] = ['api', 'mcp'];

/**
 * 이 board key 를 주소 첫 세그먼트로 **되읽을 수 있게** 실어 보낼 수 있는가.
 *
 * 두 부류가 실패한다:
 * - `RESERVED_BOARD_KEYS` — 데몬의 `/api/*`·`/mcp` 라우트가 먼저 먹는다.
 * - 점 세그먼트(`.` / `..`) — `encodeURIComponent` 가 점을 이스케이프하지 않아 `/.`·`/..`
 *   가 그대로 나가고, 브라우저 URL 파서가 이를 `/` 로 정규화해 버린다. 주소가 만들어진
 *   순간 다른 화면을 가리키게 되므로 실을 수 없는 것으로 본다.
 *
 * `ensureBoard`(`src/store.ts`)는 이 키들을 거부하지 않는다 — board key 는 레포 이름에서
 * 유추되는 값이라 웹 UI 사정으로 조용히 망글링하거나 생성을 막지 않는다는 것이 그쪽 원칙이다.
 * 대신 주소만 전체 보기와 같은 `/` 로 접는다.
 */
export function isAddressableBoardKey(key: string): boolean {
  if (RESERVED_BOARD_KEYS.includes(key)) {
    return false;
  }
  const encoded = encodeURIComponent(key);
  return encoded !== '.' && encoded !== '..';
}

/**
 * `/rocky/12` → `{ board: 'rocky', todo: { board: 'rocky', number: 12 } }`,
 * `/?todo=rocky-12` → `{ board: 'all', todo: { board: 'rocky', number: 12 } }`.
 *
 * 둘째 세그먼트는 **양의 정수일 때만** 번호로 읽는다 — 번호는 `MAX(number)+1` 로 발급되어
 * 1부터 시작하므로 `0`/음수/`12abc` 는 번호가 아니다. 셋째 이후 세그먼트는 무시한다.
 * 경로에 번호가 있으면 `?todo=` 는 보지 않는다(`buildPath` 는 둘을 같이 내지 않는다).
 * `?todo=` 는 `{board}-{number}` 를 **가장 오른쪽** `-` 에서 가른다 — 보드 key 에 `-` 가
 * 들어갈 수 있어서다(`rocky-todo-12`). CLI/MCP 의 ref 표기와 같은 규칙.
 *
 * 두 세그먼트 모두 퍼센트 디코딩한다. 숫자 쪽은 `buildPath` 가 그런 주소를 내보내지 않지만
 * (`${number}` 는 늘 맨숫자다) 손으로 친 `/rocky/%31%32` 도 같은 화면을 뜻하는 게 맞고,
 * 디코딩 후에 정수 검사를 하므로 `%2F` 같은 게 통과할 여지도 없다. 디코딩이 실패하면
 * — 보드 쪽은 전체 보기로, 숫자 쪽은 보드 화면으로 — 한 칸씩 떨어진다. 주소창에 손으로
 * 친 문자열이 앱을 죽이면 안 된다.
 */
export function parseRoute(pathname: string, search = ''): Route {
  const route = parseRouteBase(pathname, search);
  const view = route.note === undefined ? parseViewParam(search) : undefined;
  return view === undefined ? route : { ...route, view };
}

function parseRouteBase(pathname: string, search: string): Route {
  const segments = pathname.split('/').filter((s) => s !== '');
  const rawBoard = segments[0];
  const fromQuery = (board: BoardSelection): Route => {
    const note = parseNoteParam(search);
    if (note !== undefined) {
      return { board, note };
    }
    const todo = parseTodoParam(search);
    return todo === undefined ? { board } : { board, todo };
  };
  if (rawBoard === undefined) {
    return fromQuery('all');
  }
  let board: string;
  try {
    board = decodeURIComponent(rawBoard);
  } catch {
    return fromQuery('all');
  }
  if (segments[1] === 'notes') {
    const n = segments[2];
    if (n !== undefined && /^[1-9]\d*$/.test(n) && board !== 'all') {
      return { board, note: `${board}-${n}` };
    }
    return fromQuery(board);
  }
  const rawNumber = segments[1];
  if (rawNumber === undefined) {
    return fromQuery(board);
  }
  let number: string;
  try {
    number = decodeURIComponent(rawNumber);
  } catch {
    return fromQuery(board);
  }
  if (!/^[1-9]\d*$/.test(number) || board === 'all') {
    return fromQuery(board);
  }
  return { board, todo: { board, number: Number(number) } };
}

/** `?note=rocky-3` → `'rocky-3'`. ref 는 서버가 푸니 모양만 본다(빈 값·공백 거절). */
function parseNoteParam(search: string): string | undefined {
  let raw: string | null;
  try {
    raw = new URLSearchParams(search).get('note');
  } catch {
    return undefined;
  }
  return raw !== null && /^[^\s/]+$/.test(raw) ? raw : undefined;
}

/** `?todo=rocky-12` → `{ board: 'rocky', number: 12 }`. 모양이 아니면 undefined. */
function parseTodoParam(search: string): TodoRef | undefined {
  let raw: string | null;
  try {
    raw = new URLSearchParams(search).get('todo');
  } catch {
    return undefined;
  }
  if (raw === null) {
    return undefined;
  }
  const cut = raw.lastIndexOf('-');
  if (cut <= 0) {
    return undefined;
  }
  const board = raw.slice(0, cut);
  const number = raw.slice(cut + 1);
  if (!/^[1-9]\d*$/.test(number) || board === 'all') {
    return undefined;
  }
  return { board, number: Number(number) };
}

/**
 * `{ board: 'rocky', todo: { board: 'rocky', number: 12 } }` → `/rocky/12`,
 * `{ board: 'all', todo: { board: 'rocky', number: 12 } }` → `/?todo=rocky-12`.
 *
 * 전체 보기와 `isAddressableBoardKey` 가 거부하는 board key 는 `/` 를 낸다. 그런 보드도
 * 정상적으로 존재하고 선택도 되지만, 되읽을 수 없는 주소를 내보내느니 덜 정확한 `/` 를
 * 택한다. 이 폴백에 기대는 쪽은 히스토리 항목도 만들지 않아야 한다 — `src/ui/store.ts` 의
 * "주소가 그대로면 push 하지 않는다" 규칙이 그 짝이다. 열린 todo 의 보드가 실을 수 없는
 * key 여도 같은 이유로 todo 를 뺀다.
 */
export function buildPath(route: Route): string {
  const path = buildPathBase(route);
  // 노트 상세 주소는 그 자체가 노트 탭이다 — 탭을 또 싣지 않는다.
  if (route.view === undefined || route.note !== undefined) {
    return path;
  }
  return `${path}${path.includes('?') ? '&' : '?'}view=${route.view}`;
}

function buildPathBase(route: Route): string {
  const base =
    route.board === 'all' || !isAddressableBoardKey(route.board)
      ? '/'
      : `/${encodeURIComponent(route.board)}`;
  if (route.note !== undefined) {
    // 보고 있는 보드의 번호 ref 면 경로로, 아니면(전체 보기·전역 메모·raw id) 쿼리로.
    const prefix = `${route.board}-`;
    const n = route.note.startsWith(prefix) ? route.note.slice(prefix.length) : '';
    if (base !== '/' && /^[1-9]\d*$/.test(n)) {
      return `${base}/notes/${n}`;
    }
    return `${base}?${new URLSearchParams({ note: route.note }).toString()}`;
  }
  const todo = route.todo;
  if (todo === undefined || !isAddressableBoardKey(todo.board)) {
    return base;
  }
  if (base !== '/' && todo.board === route.board) {
    return `${base}/${todo.number}`;
  }
  const query = new URLSearchParams({ todo: `${todo.board}-${todo.number}` });
  return `${base}?${query.toString()}`;
}

/**
 * 주소의 보드 세그먼트를 **현재** board key 로 푼다 — 옛 key(별칭)로 들어온 링크까지.
 *
 * 보드 이름은 바뀔 수 있고(`updateBoard`), 그 전에 복사해 둔 퍼머링크(`/gotgan/12`)는
 * 계속 살아 있어야 한다. 서버는 REST·MCP·CLI 에서 별칭을 풀어주지만 이 판정만은
 * 클라이언트에 있으므로(주소 → 화면) 여기서 따로 본다. 돌려주는 값은 언제나 **새 key** 다
 * — 별칭은 입력 전용이라, 호출부는 이 값으로 주소를 정규화한다.
 *
 * @returns 못 찾으면 undefined — 호출부가 전체 보기로 떨어뜨린다(낡은 링크에 에러 화면을
 *   띄우지 않는다).
 */
export function resolveBoardKey(
  boards: readonly { key: string; previousKeys?: string[] }[],
  key: string,
): string | undefined {
  // 현재 이름이 먼저다 — 어떤 보드의 옛 이름이 다른 보드의 현재 이름과 같아질 수는
  // 없지만(`updateBoard` 가 막는다), 순서를 명시해 두면 그 불변식이 코드에도 남는다.
  const live = boards.find((board) => board.key === key);
  if (live) {
    return live.key;
  }
  return boards.find((board) => board.previousKeys?.includes(key))?.key;
}

/** todo 하나의 ref(보드 key + 번호). 보드를 못 찾으면(FK 가 깨진 상태) undefined. */
export function todoRefFor(
  todo: { boardId: string; number: number },
  boards: readonly { id: string; key: string }[],
): TodoRef | undefined {
  const board = boards.find((b) => b.id === todo.boardId);
  return board === undefined ? undefined : { board: board.key, number: todo.number };
}

/**
 * **보고 있는 보드는 그대로 두고** todo 의 상세만 연 라우트. 보드를 못 찾으면 상세 없이
 * 지금 선택만 남는다.
 */
export function routeForTodo(
  todo: { boardId: string; number: number },
  boards: readonly { id: string; key: string }[],
  selected: BoardSelection,
): Route {
  const ref = todoRefFor(todo, boards);
  return ref === undefined ? { board: selected } : { board: selected, todo: ref };
}

/**
 * URL 의 번호를 todo id 로 되돌린다 — 이미 로드된 목록에서 찾으므로 새 REST 호출이 없다.
 *
 * 번호는 보드 안에서만 유일하므로 board 스코프가 반드시 필요하다. 전체 보기(`'all'`)에는
 * 스코프가 없어 항상 `undefined` 다 — `buildPath` 는 전체 보기에 번호를 싣지 않지만,
 * `parseRoute('/all/12')` 처럼 손으로 친 주소는 `{ board: 'all', todoNumber: 12 }` 를
 * 만들어낼 수 있어 이 조합이 URL 에서 실제로 나올 수 있다. 그때 `undefined` 를 돌려주는
 * 것이 올바른 처리다 — 스코프 없이 번호만으로 todo 를 특정할 수 없기 때문이다.
 */
export function findTodoIdByNumber(
  todos: readonly { id: string; boardId: string; number: number }[],
  boards: readonly { id: string; key: string }[],
  board: BoardSelection,
  todoNumber: number,
): string | undefined {
  if (board === 'all') {
    return undefined;
  }
  const boardId = boards.find((b) => b.key === board)?.id;
  if (boardId === undefined) {
    return undefined;
  }
  return todos.find((t) => t.boardId === boardId && t.number === todoNumber)?.id;
}
