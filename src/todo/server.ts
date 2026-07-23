import type { ListTodosFilter, StatusAction, TodoLink, TodoPriority, TodoStore } from './store';

/**
 * rocky-todo REST + SSE 표면 — CLI / 웹 UI 가 공유한다.
 *
 * `buildTodoServer` 는 fetch 핸들러만 반환한다 (Bun.serve 바인딩은 daemon.ts 몫)
 * — 테스트에서 Request 를 직접 넣어 계약을 검증할 수 있게 DI 형태를 유지한다.
 * actor 는 `x-rocky-actor` 헤더로 전달된다 (웹 UI 는 localStorage 설정값을 보낸다).
 */

export interface TodoServerOptions {
  store: TodoStore;
}

export interface TodoServer {
  fetch: (req: Request) => Promise<Response>;
}

const STATUS_ACTIONS: ReadonlySet<string> = new Set([
  'start',
  'stop',
  'done',
  'reopen',
  'archive',
  'unarchive',
]);

function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'content-type': 'application/json' },
  });
}

function errorResponse(message: string, status: number): Response {
  return json({ error: message }, status);
}

async function readBody(req: Request): Promise<Record<string, unknown>> {
  try {
    const body = (await req.json()) as unknown;
    if (typeof body !== 'object' || body === null) {
      throw new Error('body must be a JSON object');
    }
    return body as Record<string, unknown>;
  } catch {
    throw new Error('invalid JSON body');
  }
}

const PRIORITIES: ReadonlySet<string> = new Set(['p1', 'p2', 'p3', 'p4']);

/**
 * `limit` 쿼리 파싱 — 없으면 undefined(스토어 기본값), 있으면 1..500 정수만 허용한다.
 * NaN / 음수 / 과도한 값이 SQLite 쿼리로 새어 들어가지 않게 막는다 (lan 노출 대비).
 */
function parseLimitParam(raw: string | null): number | undefined {
  if (raw === null) {
    return undefined;
  }
  const n = Number(raw);
  if (!Number.isInteger(n) || n < 1 || n > 500) {
    throw new Error(`limit must be an integer between 1 and 500: ${raw}`);
  }
  return n;
}

/** priority 는 있으면 p1–p4 만 허용 (DB CHECK 이전에 400 으로 거른다). */
function validatePriority(value: unknown): TodoPriority | undefined {
  if (value === undefined) {
    return undefined;
  }
  if (typeof value !== 'string' || !PRIORITIES.has(value)) {
    throw new Error(`invalid priority (expected p1-p4): ${String(value)}`);
  }
  return value as TodoPriority;
}

/** labels 는 있으면 string[] 만 허용. */
function validateLabels(value: unknown): string[] | undefined {
  if (value === undefined) {
    return undefined;
  }
  if (!Array.isArray(value) || value.some((v) => typeof v !== 'string')) {
    throw new Error('invalid labels (expected string[])');
  }
  return value as string[];
}

/** links 는 있으면 `{ url: string, title?: string }[]` 만 허용. */
function validateLinks(value: unknown): TodoLink[] | undefined {
  if (value === undefined) {
    return undefined;
  }
  const ok =
    Array.isArray(value) &&
    value.every((v) => {
      if (typeof v !== 'object' || v === null) {
        return false;
      }
      const link = v as { url?: unknown; title?: unknown };
      return (
        typeof link.url === 'string' && (link.title === undefined || typeof link.title === 'string')
      );
    });
  if (!ok) {
    throw new Error('invalid links (expected { url: string, title?: string }[])');
  }
  return value as TodoLink[];
}

/** not found 류 스토어 에러를 HTTP status 로 번역한다. */
function toHttpError(error: unknown): Response {
  const message = error instanceof Error ? error.message : String(error);
  if (/not found/i.test(message)) {
    return errorResponse(message, 404);
  }
  return errorResponse(message, 400);
}

export function buildTodoServer(options: TodoServerOptions): TodoServer {
  const { store } = options;

  const fetch = async (req: Request): Promise<Response> => {
    const url = new URL(req.url);
    const path = url.pathname;
    const method = req.method.toUpperCase();
    const actor = req.headers.get('x-rocky-actor') ?? 'unknown';

    try {
      // ── health ──
      if (method === 'GET' && path === '/api/health') {
        return json({ ok: true, name: 'rocky-todo' });
      }

      // ── SSE ──
      if (method === 'GET' && path === '/api/events') {
        return sseResponse(store);
      }

      // ── boards ──
      if (method === 'GET' && path === '/api/boards') {
        return json(store.listBoards(url.searchParams.get('includeArchived') === 'true'));
      }
      if (method === 'POST' && path === '/api/boards') {
        const body = await readBody(req);
        if (typeof body.key !== 'string' || body.key === '') {
          return errorResponse('key is required', 400);
        }
        return json(
          store.ensureBoard(body.key, {
            title: typeof body.title === 'string' ? body.title : undefined,
            actor,
          }),
          201,
        );
      }

      // ── sections ──
      if (method === 'GET' && path === '/api/sections') {
        const boardKey = url.searchParams.get('board');
        if (!boardKey) {
          return errorResponse('board query parameter is required', 400);
        }
        const board = store.listBoards(true).find((b) => b.key === boardKey);
        if (!board) {
          return json([]);
        }
        return json(store.listSections(board.id));
      }

      // ── todos ──
      if (method === 'GET' && path === '/api/todos') {
        const filter: ListTodosFilter = {
          board: url.searchParams.get('board') ?? undefined,
          status: (url.searchParams.get('status') as ListTodosFilter['status']) ?? undefined,
          label: url.searchParams.get('label') ?? undefined,
          includeArchived: url.searchParams.get('includeArchived') === 'true',
        };
        return json(store.listTodos(filter));
      }
      if (method === 'POST' && path === '/api/todos') {
        const body = await readBody(req);
        if (typeof body.title !== 'string' || body.title === '') {
          return errorResponse('title is required', 400);
        }
        if (typeof body.board !== 'string' || body.board === '') {
          return errorResponse('board is required', 400);
        }
        const todo = store.createTodo(
          {
            board: body.board,
            title: body.title,
            description: typeof body.description === 'string' ? body.description : undefined,
            section: typeof body.section === 'string' ? body.section : undefined,
            parentId: typeof body.parentId === 'string' ? body.parentId : undefined,
            priority: validatePriority(body.priority),
            due: typeof body.due === 'string' ? body.due : undefined,
            labels: validateLabels(body.labels),
            links: validateLinks(body.links),
          },
          actor,
        );
        return json(todo, 201);
      }

      const todoDetail = path.match(/^\/api\/todos\/([^/]+)$/);
      if (todoDetail?.[1]) {
        const id = todoDetail[1];
        if (method === 'GET') {
          const todo = store.getTodo(id);
          if (!todo) {
            return errorResponse(`todo not found: ${id}`, 404);
          }
          return json({ todo, history: store.listHistory({ entityId: todo.id }) });
        }
        if (method === 'PATCH') {
          const body = await readBody(req);
          // 위험 필드는 store 로 넘기기 전에 검증 (잘못된 값은 400).
          validatePriority(body.priority);
          validateLabels(body.labels);
          validateLinks(body.links);
          return json(store.updateTodo(id, body as never, actor));
        }
      }

      const todoStatus = path.match(/^\/api\/todos\/([^/]+)\/status$/);
      if (todoStatus?.[1] && method === 'POST') {
        const body = await readBody(req);
        const action = body.action;
        if (typeof action !== 'string' || !STATUS_ACTIONS.has(action)) {
          return errorResponse(`invalid action: ${String(action)}`, 400);
        }
        return json(store.setTodoStatus(todoStatus[1], action as StatusAction, actor));
      }

      // ── notes ──
      if (method === 'GET' && path === '/api/notes') {
        return json(
          store.listNotes({
            board: url.searchParams.get('board') ?? undefined,
            global: url.searchParams.get('global') === 'true',
            includeArchived: url.searchParams.get('includeArchived') === 'true',
          }),
        );
      }
      if (method === 'POST' && path === '/api/notes') {
        const body = await readBody(req);
        if (typeof body.title !== 'string' || body.title === '') {
          return errorResponse('title is required', 400);
        }
        const note = store.createNote(
          {
            board: typeof body.board === 'string' ? body.board : undefined,
            title: body.title,
            content: typeof body.content === 'string' ? body.content : undefined,
          },
          actor,
        );
        return json(note, 201);
      }

      const noteDetail = path.match(/^\/api\/notes\/([^/]+)$/);
      if (noteDetail?.[1]) {
        const id = noteDetail[1];
        if (method === 'GET') {
          const note = store.getNote(id);
          if (!note) {
            return errorResponse(`note not found: ${id}`, 404);
          }
          return json({ note, history: store.listHistory({ entityId: note.id }) });
        }
        if (method === 'PATCH') {
          const body = await readBody(req);
          return json(store.updateNote(id, body as never, actor));
        }
      }

      const noteArchive = path.match(/^\/api\/notes\/([^/]+)\/(archive|unarchive)$/);
      if (noteArchive?.[1] && noteArchive[2] && method === 'POST') {
        const id = noteArchive[1];
        return json(
          noteArchive[2] === 'archive'
            ? store.archiveNote(id, actor)
            : store.unarchiveNote(id, actor),
        );
      }

      // ── changes feed (훅 주입용) ──
      if (method === 'GET' && path === '/api/changes') {
        const sinceId = Number(url.searchParams.get('sinceId') ?? '0');
        if (!Number.isInteger(sinceId) || sinceId < 0) {
          return errorResponse('sinceId must be a non-negative integer', 400);
        }
        const limit = parseLimitParam(url.searchParams.get('limit'));
        return json(store.listChangesSince(sinceId, limit));
      }

      // ── history ──
      if (method === 'GET' && path === '/api/history') {
        return json(
          store.listHistory({
            entityId: url.searchParams.get('entityId') ?? undefined,
            entity: (url.searchParams.get('entity') as never) ?? undefined,
            limit: parseLimitParam(url.searchParams.get('limit')),
          }),
        );
      }

      return errorResponse(`not found: ${method} ${path}`, 404);
    } catch (error) {
      return toHttpError(error);
    }
  };

  return { fetch };
}

/** store change 이벤트를 SSE 로 흘린다 — 웹 UI 실시간 갱신 경로. */
function sseResponse(store: TodoStore): Response {
  let unsubscribe: (() => void) | undefined;
  const encoder = new TextEncoder();
  const stream = new ReadableStream<Uint8Array>({
    start(controller) {
      controller.enqueue(encoder.encode(': connected\n\n'));
      unsubscribe = store.subscribe((event) => {
        try {
          controller.enqueue(encoder.encode(`data: ${JSON.stringify(event)}\n\n`));
        } catch {
          // 스트림이 이미 닫힌 경우 — cancel 경로에서 구독 해제된다.
        }
      });
    },
    cancel() {
      unsubscribe?.();
    },
  });
  return new Response(stream, {
    headers: {
      'content-type': 'text/event-stream',
      'cache-control': 'no-cache',
      connection: 'keep-alive',
    },
  });
}
