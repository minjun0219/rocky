import { describe, expect, test } from 'bun:test';
import { backoffMs, createApi, nextAttempt, parseSse, STABLE_MS } from './api';

describe('parseSse', () => {
  test('주석 프레임은 버리고 data 프레임을 JSON 으로', () => {
    const { events, rest } = parseSse(': connected\n\ndata: {"entity":"todo"}\n\n');
    expect(events).toEqual([{ entity: 'todo' }]);
    expect(rest).toBe('');
  });

  test('덜 온 끝 조각은 rest 로 남겨 다음 청크와 잇는다', () => {
    const first = parseSse('data: {"entity":"no');
    expect(first.events).toEqual([]);
    const second = parseSse(`${first.rest}te"}\n\n`);
    expect(second.events).toEqual([{ entity: 'note' }]);
  });

  test('CRLF 줄바꿈과 모르는 모양(null)', () => {
    const { events } = parseSse('data: {"a":1}\r\n\r\ndata: not json\n\n');
    expect(events).toEqual([{ a: 1 }, null]);
  });
});

test('다시 붙기 간격은 1초에서 두 배씩, 30초까지', () => {
  expect([0, 1, 2, 3, 4, 5, 9].map(backoffMs)).toEqual([
    1000, 2000, 4000, 8000, 16000, 30000, 30000,
  ]);
});

test('한동안 붙어 있다 끊긴 것만 다시 붙기 간격을 처음으로 — 붙자마자 끊기면 간격이 는다', () => {
  expect(nextAttempt(4, STABLE_MS)).toBe(0);
  expect(nextAttempt(4, 50)).toBe(4);
  expect(nextAttempt(4, undefined)).toBe(4);
});

describe('createApi', () => {
  test('actor 와 클라이언트 헤더를 붙이고, 실패는 메서드·경로·상태 코드로', async () => {
    const seen: { url: string; init?: RequestInit }[] = [];
    const api = createApi({
      base: 'http://127.0.0.1:9',
      actor: 'logan',
      fetch: (async (url: string, init?: RequestInit) => {
        seen.push({ url, init });
        return new Response(JSON.stringify({ error: 'invalid action: x' }), { status: 400 });
      }) as unknown as typeof fetch,
    });
    await expect(api.setStatus('t1', 'start')).rejects.toThrow(
      'POST /api/todos/t1/status → 400 invalid action: x',
    );
    const headers = seen[0]?.init?.headers as Record<string, string>;
    expect(headers['x-rocky-actor']).toBe('logan');
    expect(headers['x-rocky-client']).toBe('tui');
    expect(headers['content-type']).toBe('application/json');
    expect(seen[0]?.init?.body).toBe('{"action":"start"}');
  });
});
