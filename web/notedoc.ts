/**
 * 노트 본문의 실시간 동기화 클라이언트 — 데몬의 노트 문서 라우트(`/api/notes/:ref/doc`)와
 * Yjs 문서를 잇는다. 설계는 `docs/design/specs/2026-09-28-note-crdt-design.md`.
 *
 * 흐름: `open()` 이 전체 상태를 받아 문서에 넣고 노트별 SSE 를 구독한다. 이후 로컬 편집
 * (원점이 `REMOTE` 가 아닌 update)은 150ms 로 묶어 POST 하고, SSE 로 온 남의 update 는
 * 문서에 적용한다. 내 update 의 메아리(`client` 가 나)는 버린다. SSE 가 다시 붙으면 자기
 * state vector 로 차분을 받아 빠진 것을 메운다.
 *
 * 순수 함수(diff·커서 이동·프레즌스 병합)는 여기 두고 단위 테스트한다. `NoteSync` 는
 * `fetch`/`EventSource` 를 주입받아 테스트가 가짜로 갈아끼운다.
 */
import * as Y from 'yjs';

/** 루트 텍스트 이름 — 데몬 `rocky_core::note_doc::TEXT_KEY` 와 같아야 한다. */
export const TEXT_KEY = 'content';
/** 서버에서 온 update 의 트랜잭션 원점 — 이 원점의 update 는 되돌려 보내지 않는다. */
export const REMOTE = 'remote';
/** 로컬 편집을 묶어 보내는 간격. */
export const FLUSH_MS = 150;
/** 보낸 update 가 실패했을 때 다시 시도하는 간격. */
export const RETRY_MS = 2_000;
/** 프레즌스 유효 시간 — 이 안에 신호가 없으면 떠난 것으로 본다. */
export const PRESENCE_TTL_MS = 45_000;
/** 내가 보고 있다는 신호를 보내는 간격. */
export const PRESENCE_PING_MS = 20_000;

export function toB64(bytes: Uint8Array): string {
  let binary = '';
  // 8KB 씩 — `String.fromCharCode(...bytes)` 는 큰 배열에서 인자 한도에 걸린다.
  for (let i = 0; i < bytes.length; i += 8192) {
    binary += String.fromCharCode(...bytes.subarray(i, i + 8192));
  }
  return btoa(binary);
}

export function fromB64(text: string): Uint8Array {
  const binary = atob(text);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes;
}

/** 한 번의 편집 — `index` 에서 `remove` 개(UTF-16 단위)를 지우고 `insert` 를 넣는다. */
export interface TextEdit {
  index: number;
  remove: number;
  insert: string;
}

function isHighSurrogate(s: string, i: number): boolean {
  const c = s.charCodeAt(i);
  return c >= 0xd800 && c <= 0xdbff;
}

/**
 * `prev` → `next` 의 최소 편집(공통 접두·접미를 뺀 한 구간). 인덱스는 UTF-16 코드 유닛 —
 * `Y.Text` 가 쓰는 단위와 같다. 서로게이트 쌍(이모지) 가운데서는 가르지 않는다: 반쪽만
 * 문서에 넣으면 상대 화면에 깨진 글자가 잠깐 뜬다.
 */
export function diffText(prev: string, next: string): TextEdit {
  if (prev === next) {
    return { index: prev.length, remove: 0, insert: '' };
  }
  let prefix = 0;
  const max = Math.min(prev.length, next.length);
  while (prefix < max && prev.charCodeAt(prefix) === next.charCodeAt(prefix)) {
    prefix++;
  }
  if (prefix > 0 && isHighSurrogate(prev, prefix - 1)) {
    prefix--;
  }
  let suffix = 0;
  while (
    suffix < max - prefix &&
    prev.charCodeAt(prev.length - 1 - suffix) === next.charCodeAt(next.length - 1 - suffix)
  ) {
    suffix++;
  }
  // 접미가 서로게이트 쌍의 뒤쪽(low)에서 시작하면 한 칸 줄인다.
  if (suffix > 0 && isHighSurrogate(prev, prev.length - 1 - suffix)) {
    suffix--;
  }
  return {
    index: prefix,
    remove: prev.length - prefix - suffix,
    insert: next.slice(prefix, next.length - suffix),
  };
}

/** `Y.Text` observe 이벤트의 delta 한 조각. */
export type DeltaOp = { insert?: string | object; delete?: number; retain?: number };

/**
 * 남의 편집(delta)이 들어온 뒤 내 커서가 있어야 할 자리. 커서 앞의 삽입은 밀고 삭제는 당긴다.
 * 커서 **바로 그 자리**의 삽입은 밀지 않는다 — 내가 치던 자리는 내 것이다.
 */
export function shiftCursor(delta: readonly DeltaOp[], cursor: number): number {
  // `pos` 는 **편집 전 문서**의 위치다 — 커서도 그 좌표에 있다. 삽입은 원본 글자를 먹지 않으니
  // pos 를 움직이지 않고, 삭제·retain 은 먹는다. 삽입 길이로 pos 를 올리면 앞의 긴 삽입이
  // 뒤의 삭제를 못 보게 끊어 커서가 오른쪽으로 밀린다.
  let pos = 0;
  let out = cursor;
  for (const op of delta) {
    if (pos >= cursor) {
      break;
    }
    if (op.retain !== undefined) {
      pos += op.retain;
    } else if (op.insert !== undefined) {
      out += typeof op.insert === 'string' ? op.insert.length : 1;
    } else if (op.delete !== undefined) {
      out -= Math.min(op.delete, cursor - pos);
      pos += op.delete;
    }
  }
  return Math.max(0, out);
}

export interface Presence {
  client: string;
  actor: string;
  at: number;
}

/** 같은 client 는 갈아끼우고, 만료된 것은 걷는다. */
export function mergePresence(
  list: readonly Presence[],
  incoming: Presence,
  now: number,
  ttl = PRESENCE_TTL_MS,
): Presence[] {
  const kept = list.filter((p) => p.client !== incoming.client && now - p.at < ttl);
  kept.push(incoming);
  return kept;
}

/** 지금 같이 보고 있는 다른 사람·에이전트의 이름(중복 없이, 나 제외). */
export function activeActors(
  list: readonly Presence[],
  me: string,
  now: number,
  ttl = PRESENCE_TTL_MS,
): string[] {
  const names: string[] = [];
  for (const p of list) {
    if (p.client === me || now - p.at >= ttl || names.includes(p.actor)) {
      continue;
    }
    names.push(p.actor);
  }
  return names;
}

/** 노트별 SSE 한 건. */
export type NoteEvent =
  | { kind: 'update'; update: string; client?: string | null; actor?: string }
  | { kind: 'presence'; client?: string | null; actor?: string; state?: unknown; at?: string };

type FetchLike = (input: string, init?: RequestInit) => Promise<Response>;
type EventSourceLike = {
  onopen: ((e: Event) => void) | null;
  onmessage: ((e: MessageEvent) => void) | null;
  onerror: ((e: Event) => void) | null;
  close(): void;
};
type EventSourceCtor = new (url: string) => EventSourceLike;

export interface NoteSyncDeps {
  fetch?: FetchLike;
  EventSource?: EventSourceCtor;
  /** 테스트용 client id. */
  client?: string;
}

export class NoteSync {
  readonly doc = new Y.Doc();
  readonly text: Y.Text;
  readonly client: string;
  /** 같이 보고 있는 사람이 바뀔 때. */
  onPresence: ((actors: string[]) => void) | null = null;
  presence: Presence[] = [];

  private readonly fetchImpl: FetchLike;
  private readonly EventSourceImpl: EventSourceCtor;
  private source: EventSourceLike | null = null;
  private pending: Uint8Array[] = [];
  private flushTimer: ReturnType<typeof setTimeout> | null = null;
  private retryTimer: ReturnType<typeof setTimeout> | null = null;
  private paused: Uint8Array[] | null = null;
  private opened = false;
  private closed = false;

  constructor(
    private readonly noteId: string,
    private readonly actor: string,
    deps: NoteSyncDeps = {},
  ) {
    this.text = this.doc.getText(TEXT_KEY);
    this.client = deps.client ?? Math.random().toString(36).slice(2, 10);
    this.fetchImpl = deps.fetch ?? ((input, init) => fetch(input, init));
    this.EventSourceImpl = deps.EventSource ?? (EventSource as unknown as EventSourceCtor);
  }

  /** 전체 상태를 받아 문서에 넣고 SSE 를 구독한다. 실패하면 던진다 — 호출자가 idle 로 돌린다. */
  async open(): Promise<void> {
    const body = await this.request<{ update: string; sv: string }>(
      'GET',
      `/api/notes/${this.noteId}/doc`,
    );
    Y.applyUpdate(this.doc, fromB64(body.update), REMOTE);
    this.doc.on('update', (update: Uint8Array, origin: unknown) => {
      if (origin === REMOTE || this.closed) {
        return;
      }
      this.pending.push(update);
      this.scheduleFlush();
    });
    this.subscribe();
    this.opened = true;
  }

  /**
   * IME 조합 중에는 남의 update 를 문서에 넣지 않고 모아 둔다 — 조합 중인 글자 밑에서 본문이
   * 바뀌면 브라우저가 조합을 깬다. `resumeRemote` 가 모아 둔 것을 한 번에 넣는다(Yjs 는 순서
   * 가 섞인 update 도 받아 준다).
   */
  pauseRemote(): void {
    if (!this.paused) {
      this.paused = [];
    }
  }

  resumeRemote(): void {
    const buffered = this.paused;
    this.paused = null;
    if (buffered && buffered.length > 0) {
      Y.applyUpdate(this.doc, Y.mergeUpdates(buffered), REMOTE);
    }
  }

  /** 내가 보고 있다는 신호. `state` 는 남에게 그대로 전달된다(커서 등 — 편집기가 정한다). */
  async ping(state?: unknown): Promise<void> {
    try {
      await this.request('POST', `/api/notes/${this.noteId}/presence`, {
        client: this.client,
        state: state ?? null,
      });
    } catch {
      // 프레즌스는 있으면 좋은 것 — 실패해도 편집은 계속된다.
    }
  }

  /** 아직 안 보낸 편집을 지금 보낸다. 실패하면 되돌려 두고 잠시 뒤 다시 시도한다. */
  async flush(): Promise<void> {
    if (this.flushTimer) {
      clearTimeout(this.flushTimer);
      this.flushTimer = null;
    }
    if (this.pending.length === 0) {
      return;
    }
    const merged = Y.mergeUpdates(this.pending);
    this.pending = [];
    try {
      await this.request('POST', `/api/notes/${this.noteId}/doc`, {
        update: toB64(merged),
        client: this.client,
      });
    } catch {
      this.pending.unshift(merged);
      if (!this.closed && !this.retryTimer) {
        this.retryTimer = setTimeout(() => {
          this.retryTimer = null;
          void this.flush();
        }, RETRY_MS);
      }
    }
  }

  /** SSE 로 온 한 건 — 테스트가 직접 부르기도 한다. */
  handle(event: NoteEvent): void {
    if (event.kind === 'update') {
      if (event.client === this.client) {
        return;
      }
      this.applyRemote(fromB64(event.update));
      if (event.actor) {
        this.notePresence({
          client: event.client ?? `?${event.actor}`,
          actor: event.actor,
          at: Date.now(),
        });
      }
      return;
    }
    if (event.kind === 'presence' && event.actor) {
      this.notePresence({
        client: event.client ?? `?${event.actor}`,
        actor: event.actor,
        at: Date.now(),
      });
    }
  }

  /** 구독을 끊고 남은 편집을 보낸다. 이후의 로컬 편집은 보내지 않는다. */
  close(): void {
    this.closed = true;
    this.source?.close();
    this.source = null;
    if (this.retryTimer) {
      clearTimeout(this.retryTimer);
      this.retryTimer = null;
    }
    void this.flush();
  }

  private applyRemote(update: Uint8Array): void {
    if (this.paused) {
      this.paused.push(update);
      return;
    }
    Y.applyUpdate(this.doc, update, REMOTE);
  }

  private notePresence(p: Presence): void {
    this.presence = mergePresence(this.presence, p, p.at);
    this.onPresence?.(activeActors(this.presence, this.client, p.at));
  }

  private scheduleFlush(): void {
    if (this.flushTimer) {
      return;
    }
    this.flushTimer = setTimeout(() => {
      this.flushTimer = null;
      void this.flush();
    }, FLUSH_MS);
  }

  private subscribe(): void {
    const source = new this.EventSourceImpl(`/api/notes/${this.noteId}/doc/events`);
    source.onopen = () => {
      // 붙을 때마다 내 state vector 로 차분을 받는다 — 첫 접속도 예외가 아니다: GET 스냅숏과
      // 구독 등록 사이에 남이 보낸 update 는 듣는 이가 없어 사라진다. 재접속이면 끊긴 사이 것.
      void this.resync();
    };
    source.onmessage = (e) => {
      try {
        this.handle(JSON.parse(e.data as string) as NoteEvent);
      } catch {
        // 깨진 한 건은 버린다 — 다음 재접속의 차분이 메운다.
      }
    };
    source.onerror = () => {};
    this.source = source;
  }

  private async resync(): Promise<void> {
    if (this.closed || !this.opened) {
      return;
    }
    try {
      const sv = encodeURIComponent(toB64(Y.encodeStateVector(this.doc)));
      const body = await this.request<{ update: string }>(
        'GET',
        `/api/notes/${this.noteId}/doc?sv=${sv}`,
      );
      this.applyRemote(fromB64(body.update));
    } catch {
      // 다음 재접속에 다시.
    }
  }

  private async request<T>(method: string, path: string, body?: unknown): Promise<T> {
    const res = await this.fetchImpl(path, {
      method,
      headers: {
        ...(body !== undefined ? { 'content-type': 'application/json' } : {}),
        'x-rocky-actor': this.actor,
        'x-rocky-client': 'web',
      },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    if (!res.ok) {
      throw new Error(`${method} ${path} → ${res.status}`);
    }
    return (await res.json()) as T;
  }
}
