import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

import {
  boardItems,
  boardTouched,
  EMPTY_VIEW,
  type Get,
  parseChanges,
  readItems,
  readSummary,
} from './data';

/** 경로별 가짜 데몬 — 받은 경로를 기록한다. 본문이 문자열이면 그대로 돌려준다. */
function daemon(routes: Record<string, { status: number; body: unknown }>) {
  const seen: string[] = [];
  const get: Get = async (path) => {
    seen.push(path);
    const hit = routes[path.split('?')[0] ?? ''] ?? { status: 404, body: {} };
    const text = typeof hit.body === 'string' ? hit.body : JSON.stringify(hit.body);
    return { ok: hit.status < 300, status: hit.status, text };
  };
  return { get, seen };
}

describe('boardItems', () => {
  test('진행 중을 먼저, 끝낸 것과 모양이 틀린 행은 뺀다', () => {
    expect(
      boardItems([
        { ref: 'r-1', title: 'a', status: 'todo' },
        { ref: 'r-2', title: 'b', status: 'done' },
        { ref: 'r-3', title: 'c', status: 'doing' },
        { title: 'ref 없음', status: 'todo' },
        { ref: 'r-4', title: 'd', status: 'todo' },
      ]).map((i) => `${i.status}:${i.ref}`),
    ).toEqual(['doing:r-3', 'todo:r-1', 'todo:r-4']);
  });
});

describe('readSummary', () => {
  test('캐시로만 묻는다 — 그냥 부르면 데몬이 만료된 수집함 어댑터를 실행하고 기다린다', async () => {
    const { get, seen } = daemon({
      '/api/summary': { status: 200, body: { board: 'rocky', collect: 2, items: [] } },
    });
    expect(await readSummary(get, '/w/my repo')).toEqual({
      ok: true,
      value: { board: 'rocky', collect: 2 },
    });
    expect(seen).toEqual(['/api/summary?cached=true&cwd=%2Fw%2Fmy%20repo']);
  });

  test('보드가 없거나 수집함을 모르면 null', async () => {
    const { get } = daemon({
      '/api/summary': { status: 200, body: { board: null, collect: null } },
    });
    expect(await readSummary(get, '/tmp')).toEqual({
      ok: true,
      value: { board: null, collect: null },
    });
  });

  test('실패는 어느 요청이 왜인지 담는다', async () => {
    const down = daemon({ '/api/summary': { status: 500, body: {} } });
    expect(await readSummary(down.get, '/w')).toEqual({
      ok: false,
      error: '/api/summary 응답 500',
    });
    const html = daemon({ '/api/summary': { status: 200, body: '<html>proxy</html>' } });
    expect(await readSummary(html.get, '/w')).toEqual({
      ok: false,
      error: '/api/summary 응답이 JSON 이 아니다(<html>proxy</html>)',
    });
  });
});

describe('readItems', () => {
  test('그 보드의 할 일을 패널 줄로', async () => {
    const { get, seen } = daemon({
      '/api/todos': {
        status: 200,
        body: [
          { ref: 'rocky-1', title: 'a', status: 'todo' },
          { ref: 'rocky-2', title: 'b', status: 'doing' },
        ],
      },
    });
    const read = await readItems(get, 'rocky');
    expect(read.ok && read.value.map((i) => i.ref)).toEqual(['rocky-2', 'rocky-1']);
    expect(seen).toEqual(['/api/todos?board=rocky']);
  });

  test('목록이 아니거나 실패면 사유', async () => {
    expect(await readItems(daemon({ '/api/todos': { status: 200, body: {} } }).get, 'r')).toEqual({
      ok: false,
      error: '/api/todos 응답이 목록이 아니다',
    });
    expect(await readItems(daemon({ '/api/todos': { status: 503, body: {} } }).get, 'r')).toEqual({
      ok: false,
      error: '/api/todos 응답 503',
    });
  });
});

describe('parseChanges · boardTouched', () => {
  test('마지막 id 와 새 행이 건드린 보드를 읽고, 모양이 틀리면 null', () => {
    expect(
      parseChanges('{"lastId":12,"entries":[{"boardKey":"rocky"},{"boardKey":null},{}]}'),
    ).toEqual({ lastId: 12, boards: ['rocky', null, null] });
    expect(parseChanges('{"lastId":12}')).toEqual({ lastId: 12, boards: [] });
    expect(parseChanges('{"entries":[]}')).toBeNull();
    expect(parseChanges('{"lastId":"7"}')).toBeNull();
    expect(parseChanges('not json')).toBeNull();
  });

  test('다른 보드의 변화로는 다시 읽지 않는다 — 행이 꽉 찼으면 더 있을 수 있어 다시 읽는다', () => {
    expect(boardTouched({ lastId: 9, boards: ['other', null] }, 'rocky', 50)).toBe(false);
    expect(boardTouched({ lastId: 9, boards: ['other', 'rocky'] }, 'rocky', 50)).toBe(true);
    expect(boardTouched({ lastId: 9, boards: ['a', 'b'] }, 'rocky', 2)).toBe(true);
  });
});

// 상태 계약(plugin/types/index.d.ts)은 import 없이 혼자 서야 해서 모양을 옮겨 적었다. 필드가 어긋나면 엔진이 `$.state` 를
// 거절하는데, 그걸 잡는 플러그인 타입 검사는 claude CLI 가 있어야 해서 CI 밖이다 — 필드 이름만이라도 여기서 대조한다.
describe('상태 계약', () => {
  const contract = readFileSync(join(import.meta.dir, '../../types/index.d.ts'), 'utf8');
  const fields = (type: string): string[] => {
    const body = contract.match(new RegExp(`export type ${type} = \\{([^}]*)\\}`))?.[1] ?? '';
    return [...body.matchAll(/(\w+)\??:/g)].map((m) => m[1] ?? '').sort();
  };

  test('BoardPanelView 는 BoardView 와 같은 필드', () => {
    expect(fields('BoardPanelView')).toEqual(Object.keys(EMPTY_VIEW).sort());
  });

  test('BoardPanelItem 은 BoardItem 과 같은 필드', () => {
    const [item] = boardItems([{ ref: 'r-1', title: 'a', status: 'todo' }]);
    expect(fields('BoardPanelItem')).toEqual(Object.keys(item ?? {}).sort());
  });
});
