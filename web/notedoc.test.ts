import { describe, expect, test } from 'bun:test';
import * as Y from 'yjs';
import {
  activeActors,
  diffText,
  fromB64,
  mergePresence,
  type NoteEvent,
  NoteSync,
  REMOTE,
  shiftCursor,
  TEXT_KEY,
  toB64,
} from './notedoc';

describe('diffText', () => {
  test('finds the one changed span', () => {
    expect(diffText('hello world', 'hello brave world')).toEqual({
      index: 6,
      remove: 0,
      insert: 'brave ',
    });
    expect(diffText('가나다라', '가마다라')).toEqual({ index: 1, remove: 1, insert: '마' });
    expect(diffText('abc', 'abc')).toEqual({ index: 3, remove: 0, insert: '' });
    expect(diffText('abc', '')).toEqual({ index: 0, remove: 3, insert: '' });
  });

  test('does not split a surrogate pair', () => {
    // 🙂 (D83D DE42) → 🙃 (D83D DE43): 앞 코드 유닛이 같아도 쌍 전체를 바꾼다.
    expect(diffText('a🙂b', 'a🙃b')).toEqual({ index: 1, remove: 2, insert: '🙃' });
  });
});

describe('shiftCursor', () => {
  test('inserts before the cursor push it, at or after do not', () => {
    expect(shiftCursor([{ insert: 'xx' }], 3)).toBe(5);
    expect(shiftCursor([{ retain: 3 }, { insert: 'xx' }], 3)).toBe(3);
    expect(shiftCursor([{ retain: 5 }, { insert: 'xx' }], 3)).toBe(3);
  });

  test('deletes before the cursor pull it, capped at the cursor', () => {
    expect(shiftCursor([{ retain: 1 }, { delete: 2 }], 5)).toBe(3);
    expect(shiftCursor([{ retain: 1 }, { delete: 10 }], 5)).toBe(1);
    expect(shiftCursor([{ retain: 6 }, { delete: 2 }], 5)).toBe(5);
  });

  test('positions are tracked in the pre-edit document, so a long insert does not hide a later delete', () => {
    // "abcde", 커서 3 (c|d). 남이 앞에 "xxxx" 를 넣고 b 를 지웠다 → "xxxxacde", 커서는 c 뒤 = 6.
    expect(shiftCursor([{ insert: 'xxxx' }, { retain: 1 }, { delete: 1 }], 3)).toBe(6);
    // 삭제 뒤의 삽입도 순서대로 — "abcde" 커서 4: a 삭제, 그 뒤에 "yy" 삽입 → "yybcd|e" = 5.
    expect(shiftCursor([{ delete: 1 }, { insert: 'yy' }], 4)).toBe(5);
  });
});

describe('presence', () => {
  test('merge replaces the same client and drops expired ones', () => {
    const list = mergePresence([], { client: 'a', actor: 'logan', at: 0 }, 0);
    const next = mergePresence(list, { client: 'a', actor: 'logan', at: 10 }, 10);
    expect(next).toEqual([{ client: 'a', actor: 'logan', at: 10 }]);
    const later = mergePresence(next, { client: 'b', actor: 'codex', at: 100_000 }, 100_000);
    expect(later.map((p) => p.client)).toEqual(['b']);
  });

  test('activeActors excludes me, dedupes names and skips expired', () => {
    const list = [
      { client: 'me', actor: 'logan', at: 0 },
      { client: 'a', actor: 'codex', at: 0 },
      { client: 'b', actor: 'codex', at: 0 },
      { client: 'c', actor: 'old', at: -100_000 },
    ];
    expect(activeActors(list, 'me', 0)).toEqual(['codex']);
  });
});

test('base64 round-trips binary', () => {
  const bytes = new Uint8Array(70_000).map((_, i) => i % 251);
  expect(fromB64(toB64(bytes))).toEqual(bytes);
});

// ── NoteSync — 가짜 fetch/EventSource 로 왕복 ───────────────────────────────

class FakeSource {
  static instances: FakeSource[] = [];
  onopen: ((e: Event) => void) | null = null;
  onmessage: ((e: MessageEvent) => void) | null = null;
  onerror: ((e: Event) => void) | null = null;
  closed = false;
  constructor(public url: string) {
    FakeSource.instances.push(this);
  }
  close() {
    this.closed = true;
  }
  emit(event: NoteEvent) {
    this.onmessage?.({ data: JSON.stringify(event) } as MessageEvent);
  }
}

/** 서버 흉내 — 자기 Y.Doc 을 들고 GET/POST 를 받는다. */
function fakeServer(initial: string) {
  const doc = new Y.Doc();
  doc.getText(TEXT_KEY).insert(0, initial);
  const calls: { method: string; path: string; body?: unknown }[] = [];
  const fetch = async (path: string, init?: RequestInit): Promise<Response> => {
    const method = init?.method ?? 'GET';
    const body = init?.body ? JSON.parse(init.body as string) : undefined;
    calls.push({ method, path, body });
    if (method === 'GET' && path.startsWith('/api/notes/n1/doc')) {
      const sv = new URL(`http://x${path}`).searchParams.get('sv');
      const update = sv
        ? Y.encodeStateAsUpdate(doc, fromB64(decodeURIComponent(sv)))
        : Y.encodeStateAsUpdate(doc);
      return Response.json({ update: toB64(update), sv: toB64(Y.encodeStateVector(doc)) });
    }
    if (method === 'POST' && path === '/api/notes/n1/doc') {
      Y.applyUpdate(doc, fromB64(body.update));
      return Response.json({ ok: true, changed: true });
    }
    if (method === 'POST' && path === '/api/notes/n1/presence') {
      return Response.json({ ok: true });
    }
    return new Response('nope', { status: 404 });
  };
  return { doc, calls, fetch, text: () => doc.getText(TEXT_KEY).toString() };
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

describe('NoteSync', () => {
  test('opens with the server state, batches local edits into one POST, ignores its own echo', async () => {
    FakeSource.instances = [];
    const server = fakeServer('hello');
    const sync = new NoteSync('n1', 'logan', {
      fetch: server.fetch,
      EventSource: FakeSource as never,
      client: 'me',
    });
    await sync.open();
    expect(sync.text.toString()).toBe('hello');
    expect(FakeSource.instances[0]?.url).toBe('/api/notes/n1/doc/events');

    sync.doc.transact(() => sync.text.insert(5, ' a'), 'local');
    sync.doc.transact(() => sync.text.insert(7, 'b'), 'local');
    await sleep(220);
    const posts = server.calls.filter((c) => c.method === 'POST');
    expect(posts.length).toBe(1);
    const posted = posts[0]?.body as { client: string; update: string };
    expect(posted.client).toBe('me');
    expect(server.text()).toBe('hello ab');

    // 서버가 내 update 를 되쏜 것 — 무시(적용해도 무해하지만 프레즌스에 나를 넣지 않는다).
    const seen: string[][] = [];
    sync.onPresence = (a) => seen.push(a);
    FakeSource.instances[0]?.emit({
      kind: 'update',
      update: posted.update,
      client: 'me',
      actor: 'logan',
    });
    expect(seen).toEqual([]);
    sync.close();
    expect(FakeSource.instances[0]?.closed).toBe(true);
  });

  test('applies remote updates without sending them back, and reports who is editing', async () => {
    FakeSource.instances = [];
    const server = fakeServer('x');
    const sync = new NoteSync('n1', 'logan', {
      fetch: server.fetch,
      EventSource: FakeSource as never,
      client: 'me',
    });
    await sync.open();
    const seen: string[][] = [];
    sync.onPresence = (a) => seen.push(a);

    const other = new Y.Doc();
    Y.applyUpdate(other, Y.encodeStateAsUpdate(server.doc));
    other.getText(TEXT_KEY).insert(1, 'y');
    const update = Y.encodeStateAsUpdate(other, Y.encodeStateVector(server.doc));
    FakeSource.instances[0]?.emit({
      kind: 'update',
      update: toB64(update),
      client: 'them',
      actor: 'codex',
    });
    expect(sync.text.toString()).toBe('xy');
    expect(seen.at(-1)).toEqual(['codex']);
    await sleep(220);
    expect(server.calls.filter((c) => c.method === 'POST').length).toBe(0);
    FakeSource.instances[0]?.emit({ kind: 'presence', client: 'p1', actor: 'logan-phone' });
    expect(seen.at(-1)).toEqual(['codex', 'logan-phone']);
    sync.close();
  });

  test('pauseRemote buffers remote updates until resumeRemote', async () => {
    FakeSource.instances = [];
    const server = fakeServer('x');
    const sync = new NoteSync('n1', 'logan', {
      fetch: server.fetch,
      EventSource: FakeSource as never,
      client: 'me',
    });
    await sync.open();
    const other = new Y.Doc();
    Y.applyUpdate(other, Y.encodeStateAsUpdate(server.doc));
    other.getText(TEXT_KEY).insert(1, 'y');
    const update = toB64(Y.encodeStateAsUpdate(other, Y.encodeStateVector(server.doc)));
    sync.pauseRemote();
    FakeSource.instances[0]?.emit({ kind: 'update', update, client: 'them', actor: 'codex' });
    expect(sync.text.toString()).toBe('x');
    sync.resumeRemote();
    expect(sync.text.toString()).toBe('xy');
    sync.close();
  });

  test('a reconnect pulls the diff the client missed', async () => {
    FakeSource.instances = [];
    const server = fakeServer('x');
    const sync = new NoteSync('n1', 'logan', {
      fetch: server.fetch,
      EventSource: FakeSource as never,
      client: 'me',
    });
    await sync.open();
    const source = FakeSource.instances[0]!;
    // 첫 open 도 차분을 받는다 — GET 스냅숏과 구독 사이에 온 update 는 듣는 이가 없었다.
    server.doc.getText(TEXT_KEY).insert(1, 'q');
    source.onopen?.(new Event('open'));
    await sleep(10);
    expect(sync.text.toString()).toBe('xq');
    server.doc.getText(TEXT_KEY).insert(2, 'z'); // 끊긴 사이의 변경
    source.onopen?.(new Event('open')); // 재접속
    await sleep(10);
    expect(server.calls.filter((c) => c.path.includes('?sv=')).length).toBe(2);
    expect(sync.text.toString()).toBe('xqz');
    sync.close();
  });

  test('a failed POST is retried, and REMOTE-origin updates are never sent', async () => {
    FakeSource.instances = [];
    const server = fakeServer('x');
    let fail = true;
    const flaky = async (path: string, init?: RequestInit) => {
      if (fail && init?.method === 'POST') {
        return new Response('down', { status: 503 });
      }
      return server.fetch(path, init);
    };
    const sync = new NoteSync('n1', 'logan', {
      fetch: flaky,
      EventSource: FakeSource as never,
      client: 'me',
    });
    await sync.open();
    sync.doc.transact(() => sync.text.insert(1, 'q'), 'local');
    await sleep(200);
    expect(server.text()).toBe('x');
    fail = false;
    await sync.flush();
    expect(server.text()).toBe('xq');
    const before = server.calls.length;
    Y.applyUpdate(sync.doc, Y.encodeStateAsUpdate(server.doc), REMOTE);
    await sleep(200);
    expect(server.calls.length).toBe(before);
    sync.close();
  });
});
