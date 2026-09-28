import { describe, expect, test } from 'bun:test';
import {
  buildPath,
  findTodoIdByNumber,
  isAddressableBoardKey,
  parseRoute,
  RESERVED_BOARD_KEYS,
  resolveBoardKey,
  routeForTodo,
  todoRefFor,
} from './route';

const BOARDS = [
  { id: 'b1', key: 'rocky' },
  { id: 'b2', key: 'rocky-todo' },
];

const TODOS = [
  { id: 't1', boardId: 'b1', number: 12 },
  { id: 't2', boardId: 'b2', number: 12 },
  { id: 't3', boardId: 'b1', number: 3 },
];

describe('parseRoute', () => {
  test('root and empty mean the all-boards view', () => {
    expect(parseRoute('/')).toEqual({ board: 'all' });
    expect(parseRoute('')).toEqual({ board: 'all' });
  });

  test('one segment is a board', () => {
    expect(parseRoute('/rocky')).toEqual({ board: 'rocky' });
  });

  test('a trailing slash changes nothing', () => {
    expect(parseRoute('/rocky/')).toEqual({ board: 'rocky' });
    expect(parseRoute('/rocky/12/')).toEqual({
      board: 'rocky',
      todo: { board: 'rocky', number: 12 },
    });
  });

  test('a numeric second segment is the todo number', () => {
    expect(parseRoute('/rocky/12')).toEqual({
      board: 'rocky',
      todo: { board: 'rocky', number: 12 },
    });
  });

  test('a non-numeric second segment is ignored', () => {
    expect(parseRoute('/rocky/abc')).toEqual({ board: 'rocky' });
    expect(parseRoute('/rocky/12abc')).toEqual({ board: 'rocky' });
    expect(parseRoute('/rocky/-1')).toEqual({ board: 'rocky' });
    expect(parseRoute('/rocky/0')).toEqual({ board: 'rocky' });
  });

  test('extra segments are ignored', () => {
    expect(parseRoute('/rocky/12/anything/else')).toEqual({
      board: 'rocky',
      todo: { board: 'rocky', number: 12 },
    });
  });

  test('segments are percent-decoded', () => {
    expect(parseRoute('/my%20board')).toEqual({ board: 'my board' });
    // buildPath 는 맨숫자만 내보내지만, 손으로 친 인코딩 숫자도 같은 화면을 뜻한다.
    expect(parseRoute('/rocky/%31%32')).toEqual({
      board: 'rocky',
      todo: { board: 'rocky', number: 12 },
    });
  });

  test('the number is checked after decoding, so an encoded slash is still not a number', () => {
    expect(parseRoute('/rocky/%2F12')).toEqual({ board: 'rocky' });
  });

  test('a malformed percent escape falls back to the all view rather than throwing', () => {
    expect(parseRoute('/%E0%A4%A')).toEqual({ board: 'all' });
  });

  test('a malformed escape in the number segment falls back to the board, not the all view', () => {
    expect(parseRoute('/rocky/%E0%A4%A')).toEqual({ board: 'rocky' });
  });
});

describe('parseRoute ?todo=', () => {
  test('the all view can carry an open todo of any board', () => {
    expect(parseRoute('/', '?todo=rocky-12')).toEqual({
      board: 'all',
      todo: { board: 'rocky', number: 12 },
    });
  });

  test('a board view can carry an open todo of another board', () => {
    expect(parseRoute('/tally', '?todo=rocky-12')).toEqual({
      board: 'tally',
      todo: { board: 'rocky', number: 12 },
    });
  });

  test('the ref is split at the rightmost dash, so board keys may contain dashes', () => {
    expect(parseRoute('/', '?todo=rocky-todo-3')).toEqual({
      board: 'all',
      todo: { board: 'rocky-todo', number: 3 },
    });
  });

  test('a malformed ref is ignored', () => {
    for (const q of [
      '?todo=rocky',
      '?todo=-12',
      '?todo=rocky-0',
      '?todo=rocky-1a',
      '?todo=all-3',
    ]) {
      expect(parseRoute('/', q)).toEqual({ board: 'all' });
    }
  });

  test('a number in the path wins over the query', () => {
    expect(parseRoute('/rocky/12', '?todo=tally-3')).toEqual({
      board: 'rocky',
      todo: { board: 'rocky', number: 12 },
    });
  });

  test('the board in the query is percent-decoded', () => {
    expect(parseRoute('/', '?todo=my+board-3')).toEqual({
      board: 'all',
      todo: { board: 'my board', number: 3 },
    });
  });
});

describe('buildPath', () => {
  test('the all view is the root path', () => {
    expect(buildPath({ board: 'all' })).toBe('/');
  });

  test('a todo of another board than the selection rides in the query', () => {
    expect(buildPath({ board: 'all', todo: { board: 'rocky', number: 12 } })).toBe(
      '/?todo=rocky-12',
    );
    expect(buildPath({ board: 'tally', todo: { board: 'rocky', number: 12 } })).toBe(
      '/tally?todo=rocky-12',
    );
    expect(buildPath({ board: 'all', todo: { board: 'my board', number: 3 } })).toBe(
      '/?todo=my+board-3',
    );
  });

  test('a todo on an unaddressable board is dropped from the address', () => {
    expect(buildPath({ board: 'all', todo: { board: 'api', number: 1 } })).toBe('/');
    expect(buildPath({ board: 'api', todo: { board: 'api', number: 1 } })).toBe('/');
  });

  test('a board becomes one segment', () => {
    expect(buildPath({ board: 'rocky' })).toBe('/rocky');
  });

  test('a board and number become two segments', () => {
    expect(buildPath({ board: 'rocky', todo: { board: 'rocky', number: 12 } })).toBe('/rocky/12');
  });

  test('board keys are percent-encoded', () => {
    expect(buildPath({ board: 'my board' })).toBe('/my%20board');
  });

  test('reserved keys collapse to the root so no link collides with a REST route', () => {
    for (const key of RESERVED_BOARD_KEYS) {
      expect(buildPath({ board: key })).toBe('/');
      expect(buildPath({ board: key, todo: { board: key, number: 12 } })).toBe('/');
    }
  });

  test('dot-segment keys collapse to the root rather than emitting /. or /..', () => {
    // encodeURIComponent 는 점을 이스케이프하지 않는다 — `/.`/`/..` 를 그대로 내보내면
    // 브라우저 URL 파서가 `/` 로 정규화해, 주소가 만들어진 순간 다른 화면을 가리킨다.
    for (const key of ['.', '..']) {
      expect(buildPath({ board: key })).toBe('/');
      expect(buildPath({ board: key, todo: { board: key, number: 12 } })).toBe('/');
    }
  });

  test('a key that merely contains dots is still addressable', () => {
    expect(buildPath({ board: '.github' })).toBe('/.github');
    expect(buildPath({ board: 'a.b' })).toBe('/a.b');
    expect(buildPath({ board: '...' })).toBe('/...');
  });

  test('round-trips with parseRoute', () => {
    for (const route of [
      { board: 'all' as const },
      { board: 'rocky' },
      { board: 'rocky', todo: { board: 'rocky', number: 12 } },
      { board: 'my board', todo: { board: 'my board', number: 3 } },
      { board: 'all' as const, todo: { board: 'rocky', number: 12 } },
      { board: 'tally', todo: { board: 'rocky-todo', number: 7 } },
      { board: 'all' as const, todo: { board: 'my board', number: 3 } },
    ]) {
      const [pathname, search] = buildPath(route).split('?');
      expect(parseRoute(pathname ?? '/', search === undefined ? '' : `?${search}`)).toEqual(route);
    }
  });
});

describe('isAddressableBoardKey', () => {
  test('ordinary keys are addressable', () => {
    expect(isAddressableBoardKey('rocky')).toBe(true);
    expect(isAddressableBoardKey('my board')).toBe(true);
    expect(isAddressableBoardKey('.github')).toBe(true);
  });

  test('reserved keys are not — the daemon routes eat those paths', () => {
    for (const key of RESERVED_BOARD_KEYS) {
      expect(isAddressableBoardKey(key)).toBe(false);
    }
  });

  test('dot segments are not — the browser normalizes them away', () => {
    expect(isAddressableBoardKey('.')).toBe(false);
    expect(isAddressableBoardKey('..')).toBe(false);
  });
});

describe('todoRefFor / routeForTodo', () => {
  test('resolves the board key from the todo boardId', () => {
    expect(todoRefFor({ boardId: 'b1', number: 12 }, BOARDS)).toEqual({
      board: 'rocky',
      number: 12,
    });
  });

  test('an unknown boardId has no ref', () => {
    expect(todoRefFor({ boardId: 'nope', number: 12 }, BOARDS)).toBeUndefined();
  });

  // 상세를 여는 동작이 뒤 화면(선택한 보드)을 바꾸지 않는다 — 전체 보기에서 열면 전체 보기
  // 위에, 다른 보드를 보다 "지금" 표에서 열면 그 보드 위에 상세가 얹힌다.
  test('keeps the selected board and adds the todo on top', () => {
    expect(routeForTodo({ boardId: 'b1', number: 12 }, BOARDS, 'all')).toEqual({
      board: 'all',
      todo: { board: 'rocky', number: 12 },
    });
    expect(buildPath(routeForTodo({ boardId: 'b1', number: 12 }, BOARDS, 'all'))).toBe(
      '/?todo=rocky-12',
    );
    expect(buildPath(routeForTodo({ boardId: 'b1', number: 12 }, BOARDS, 'rocky-todo'))).toBe(
      '/rocky-todo?todo=rocky-12',
    );
    expect(buildPath(routeForTodo({ boardId: 'b1', number: 12 }, BOARDS, 'rocky'))).toBe(
      '/rocky/12',
    );
  });

  test('an unknown boardId keeps only the selection', () => {
    expect(routeForTodo({ boardId: 'nope', number: 12 }, BOARDS, 'all')).toEqual({ board: 'all' });
  });
});

describe('findTodoIdByNumber', () => {
  test('scopes the number to the selected board', () => {
    expect(findTodoIdByNumber(TODOS, BOARDS, 'rocky', 12)).toBe('t1');
    expect(findTodoIdByNumber(TODOS, BOARDS, 'rocky-todo', 12)).toBe('t2');
  });

  test('returns undefined for a number that is not on that board', () => {
    expect(findTodoIdByNumber(TODOS, BOARDS, 'rocky', 999)).toBeUndefined();
  });

  test('returns undefined for an unknown board', () => {
    expect(findTodoIdByNumber(TODOS, BOARDS, 'nope', 12)).toBeUndefined();
  });

  test('the all view has no board scope, so a bare number is not resolvable', () => {
    expect(findTodoIdByNumber(TODOS, BOARDS, 'all', 12)).toBeUndefined();
  });
});

describe('resolveBoardKey', () => {
  const RENAMED = [{ key: 'tally', previousKeys: ['gotgan'] }, { key: 'rocky' }];

  test('현재 이름은 그대로 돌려준다', () => {
    expect(resolveBoardKey(RENAMED, 'rocky')).toBe('rocky');
  });

  // 이름을 바꾸기 전에 복사해 둔 퍼머링크가 죽으면 안 된다 — 새 key 로 정규화해 준다.
  test('옛 이름은 현재 이름으로 푼다', () => {
    expect(resolveBoardKey(RENAMED, 'gotgan')).toBe('tally');
  });

  test('모르는 이름은 undefined — 호출부가 전체 보기로 떨어뜨린다', () => {
    expect(resolveBoardKey(RENAMED, 'nope')).toBeUndefined();
  });

  test('현재 이름이 별칭보다 먼저다', () => {
    const shadowed = [{ key: 'a', previousKeys: ['b'] }, { key: 'b' }];
    expect(resolveBoardKey(shadowed, 'b')).toBe('b');
  });
});
