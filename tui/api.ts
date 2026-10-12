/**
 * 데몬 REST·SSE — TUI 가 쓰는 것만. `fetch` 를 주입받아 테스트가 가짜로 바꾼다. 모든 요청에 CLI 와 같은 actor 와
 * `x-rocky-client: tui` 를 붙인다 — 사용 로그가 TUI 를 따로 센다(`rocky_core::usage::client_of` 가 헤더를 먼저 본다).
 */
import type {
  Board,
  Comment,
  HandoffView,
  HistoryEntry,
  Section,
  StatusAction,
  TodoView,
} from '../web/types';

export interface ApiOptions {
  /** `http://127.0.0.1:8636` — 끝 슬래시 없이. */
  base: string;
  actor: string;
  fetch?: typeof fetch;
}

export interface TodoDetail {
  todo: TodoView;
  history: HistoryEntry[];
  comments: Comment[];
}

export type Api = ReturnType<typeof createApi>;

export function createApi({ base, actor, fetch: send = fetch }: ApiOptions) {
  const headers = (json: boolean) => ({
    ...(json ? { 'content-type': 'application/json' } : {}),
    'x-rocky-actor': actor,
    'x-rocky-client': 'tui',
  });
  const call = async <T>(method: string, path: string, body?: unknown): Promise<T> => {
    const res = await send(`${base}${path}`, {
      method,
      headers: headers(body !== undefined),
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    if (!res.ok) {
      const payload = (await res.json().catch(() => ({}))) as { error?: string };
      // 상태 줄에 그대로 뜬다 — 무엇을 어디로 보냈다가 무엇을 받았는지.
      throw new Error(
        `${method} ${path} → ${res.status}${payload.error ? ` ${payload.error}` : ''}`,
      );
    }
    return (await res.json()) as T;
  };
  return {
    boards: () => call<Board[]>('GET', '/api/boards'),
    /** 전 보드 — "지금" 의 재료이자, 보고 있는 보드의 목록은 여기서 거른다(요청 하나로). */
    todos: () => call<TodoView[]>('GET', '/api/todos'),
    sections: (board: string) =>
      call<Section[]>('GET', `/api/sections?board=${encodeURIComponent(board)}`),
    /** 대기 중 + 배달됐는데 아직 안 끝난 넘김 — "지금" 의 넘김 행. */
    handoffs: () => call<HandoffView[]>('GET', '/api/handoffs?open=true'),
    todo: (id: string) => call<TodoDetail>('GET', `/api/todos/${encodeURIComponent(id)}`),
    setStatus: (id: string, action: StatusAction) =>
      call<TodoView>('POST', `/api/todos/${encodeURIComponent(id)}/status`, { action }),
    add: (board: string, title: string) => call<TodoView>('POST', '/api/todos', { board, title }),
  };
}

/**
 * SSE 청크를 프레임으로 — `data:` 줄을 모아 JSON 으로(모르는 모양은 null). 다 오지 않은 끝 조각은 `rest` 로 돌려
 * 다음 청크와 잇는다. 주석 프레임(`: connected`)은 버린다.
 */
export function parseSse(buffer: string): { events: unknown[]; rest: string } {
  const events: unknown[] = [];
  let rest = buffer.replace(/\r\n/g, '\n');
  for (let end = rest.indexOf('\n\n'); end >= 0; end = rest.indexOf('\n\n')) {
    const frame = rest.slice(0, end);
    rest = rest.slice(end + 2);
    const data = frame
      .split('\n')
      .filter((line) => line.startsWith('data:'))
      .map((line) => line.slice(5).replace(/^ /, ''))
      .join('\n');
    if (data === '') {
      continue;
    }
    try {
      events.push(JSON.parse(data));
    } catch {
      events.push(null);
    }
  }
  return { events, rest };
}

/** 이만큼 붙어 있었으면 다시 붙기 간격을 처음(1s)으로 되돌린다 — 받자마자 닫히는 서버에 1초마다 붙지 않게. */
export const STABLE_MS = 10_000;

/**
 * 다음 대기에 쓸 시도 번호 — 한동안(`STABLE_MS`) 붙어 있다가 끊긴 것만 처음으로 되돌린다. 붙자마자 끊기면(200 을 주고
 * 바로 닫는 서버·프록시) 번호가 계속 늘어 간격이 커진다 — 아니면 1초마다 붙으며 전부 다시 읽는다.
 */
export function nextAttempt(attempt: number, openedForMs: number | undefined): number {
  return openedForMs !== undefined && openedForMs >= STABLE_MS ? 0 : attempt;
}

/** 다시 붙기까지 기다리는 시간 — 1s 에서 두 배씩, 30s 까지. */
export function backoffMs(attempt: number): number {
  return Math.min(30_000, 1000 * 2 ** Math.max(0, attempt));
}

export interface StreamHandlers {
  /** 붙었다 — 끊긴 동안의 변경은 오지 않으므로 전부 다시 받는다. */
  onOpen: () => void;
  onEvents: (events: unknown[]) => void;
  /** 끊겼다 — `retryInMs` 뒤에 다시 붙는다. */
  onClose: (retryInMs: number) => void;
}

/**
 * `/api/events` 를 연다 — Bun 에는 전역 `EventSource` 가 없어서 fetch 스트림을 직접 읽는다. 끊기면 `backoffMs` 로
 * 다시 붙는다. 반환값을 부르면 닫는다.
 */
export function openStream(
  { base, actor, fetch: send = fetch }: ApiOptions,
  handlers: StreamHandlers,
): () => void {
  const abort = new AbortController();
  let attempt = 0;
  let openedAt = 0;
  const loop = async () => {
    while (!abort.signal.aborted) {
      try {
        const res = await send(`${base}/api/events`, {
          headers: { 'x-rocky-actor': actor, 'x-rocky-client': 'tui' },
          signal: abort.signal,
        });
        if (!res.ok || !res.body) {
          await res.body?.cancel();
          throw new Error(`GET /api/events → ${res.status}`);
        }
        openedAt = Date.now();
        handlers.onOpen();
        const reader = res.body.pipeThrough(new TextDecoderStream()).getReader();
        let buffer = '';
        for (;;) {
          const { value, done } = await reader.read();
          if (done) {
            break;
          }
          const parsed = parseSse(buffer + value);
          buffer = parsed.rest;
          if (parsed.events.length > 0) {
            handlers.onEvents(parsed.events);
          }
        }
      } catch {
        // 아래에서 다시 붙는다 — 닫힌 것(abort)이면 루프가 끝난다.
      }
      if (abort.signal.aborted) {
        return;
      }
      attempt = nextAttempt(attempt, openedAt > 0 ? Date.now() - openedAt : undefined);
      openedAt = 0;
      const wait = backoffMs(attempt++);
      handlers.onClose(wait);
      await new Promise((resolve) => setTimeout(resolve, wait));
    }
  };
  void loop();
  return () => abort.abort();
}
