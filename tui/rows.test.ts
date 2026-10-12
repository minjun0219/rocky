import { describe, expect, test } from 'bun:test';
import { sectionFixture, todoFixture } from './fixtures';
import { buildLines, cursorAt, GLYPH, moveCursor, selectable, todoGlyph, todoIdOf } from './rows';

const NOW = Date.parse('2026-10-12T03:00:00Z');

describe('buildLines', () => {
  test('지금 → 섹션 없는 것("일반") → 섹션 순서, 하위는 한 칸 들여서', () => {
    const lines = buildLines({
      todos: [
        todoFixture({ id: 'a', number: 1, title: '느슨한' }),
        todoFixture({ id: 'b', number: 2, title: '검증 항목', sectionId: 'sec1' }),
        todoFixture({ id: 'c', number: 3, title: '하위', parentId: 'b', sectionId: 'sec1' }),
      ],
      handoffs: [],
      boardId: 'board1',
      sections: [sectionFixture()],
      now: NOW,
    });
    expect(lines.map((l) => l.key)).toEqual([
      'h:now',
      'e:now',
      'h:__none',
      't:a',
      'h:sec1',
      't:b',
      't:c',
    ]);
    const child = lines.find((l) => l.key === 't:c');
    expect(child?.kind === 'todo' && child.depth).toBe(1);
  });

  test('끝난 것만 남은 가지는 숨기고, 열린 자손이 있으면 끝난 부모도 남긴다', () => {
    const lines = buildLines({
      todos: [
        todoFixture({ id: 'done', status: 'done' }),
        todoFixture({ id: 'parent', status: 'done' }),
        todoFixture({ id: 'kid', parentId: 'parent' }),
      ],
      handoffs: [],
      boardId: 'board1',
      sections: [],
      now: NOW,
    });
    const keys = lines.map((l) => l.key);
    expect(keys).not.toContain('t:done');
    expect(keys).toContain('t:parent');
    expect(keys).toContain('t:kid');
  });

  test('진행 중인 할 일은 지금에 나오고, 다른 보드 것도 지금에는 나온다', () => {
    const lines = buildLines({
      todos: [
        todoFixture({
          id: 'run',
          boardId: 'other',
          status: 'doing',
          doingBy: 'claude-code',
          doingSince: '2026-10-12T02:50:00Z',
          doingState: 'live',
        }),
      ],
      handoffs: [],
      boardId: 'board1',
      sections: [],
      now: NOW,
    });
    const now = lines.filter((l) => l.kind === 'now');
    expect(now.map(todoIdOf)).toEqual(['run']);
    // 보고 있는 보드에는 없다 — 트리는 비었다
    expect(lines.map((l) => l.key)).toContain('e:board');
  });

  test('댓글은 읽은 것으로 넘긴다 — TUI 에는 읽음 표시가 없어 모든 댓글이 행이 되면 안 된다', () => {
    const lines = buildLines({
      todos: [todoFixture({ commentCount: 2, lastCommentAt: '2026-10-12T02:00:00Z' })],
      handoffs: [],
      sections: [],
      now: NOW,
    });
    expect(lines.filter((l) => l.kind === 'now')).toEqual([]);
  });
});

describe('커서', () => {
  const lines = buildLines({
    todos: [todoFixture({ id: 'a' }), todoFixture({ id: 'b' })],
    handoffs: [],
    boardId: 'board1',
    sections: [],
    now: NOW,
  });

  test('할 일 줄만 밟고 끝에서 멈춘다', () => {
    const [first, second] = selectable(lines);
    expect(moveCursor(lines, first as number, 1)).toBe(second as number);
    expect(moveCursor(lines, second as number, 1)).toBe(second as number);
    expect(moveCursor(lines, first as number, -1)).toBe(first as number);
  });

  test('밟을 수 없는 자리에서 시작하면 가까운 다음 줄로', () => {
    expect(moveCursor(lines, 0, 1)).toBe(selectable(lines)[0] as number);
  });
});

test('트리 글리프는 세션 판정을 따르고, 판정이 없으면 그냥 진행 중이다', () => {
  expect(todoGlyph(todoFixture())).toBe(GLYPH.open);
  expect(todoGlyph(todoFixture({ status: 'done' }))).toBe(GLYPH.done);
  expect(todoGlyph(todoFixture({ status: 'doing' }))).toBe(GLYPH.run);
  expect(todoGlyph(todoFixture({ status: 'doing', doingState: 'idle' }))).toBe(GLYPH.mine);
  expect(todoGlyph(todoFixture({ status: 'doing', doingState: 'gone' }))).toBe(GLYPH.dead);
  expect(todoGlyph(todoFixture({ status: 'doing', doingState: 'unknown' }))).toBe(GLYPH.unknown);
});

describe('cursorAt', () => {
  const lines = buildLines({
    todos: [todoFixture({ id: 'a' }), todoFixture({ id: 'b' }), todoFixture({ id: 'c' })],
    handoffs: [],
    boardId: 'board1',
    sections: [],
    now: NOW,
  });
  const [a, b, c] = selectable(lines) as [number, number, number];

  test('키의 줄이 있으면 그 줄', () => {
    expect(cursorAt(lines, 't:b', a)).toBe(b);
  });

  test('줄이 사라지면 맨 위가 아니라 마지막 위치의 다음 줄로 — 끝이면 마지막 줄', () => {
    expect(cursorAt(lines, 't:gone', b)).toBe(b);
    expect(cursorAt(lines, 't:gone', c + 5)).toBe(c);
    expect(cursorAt(lines, undefined)).toBe(a);
  });
});

test('보드를 바꾼 직후 섹션이 오기 전엔 트리 대신 읽는 중', () => {
  const lines = buildLines({
    todos: [todoFixture({ sectionId: 'sec1' })],
    handoffs: [],
    boardId: 'board1',
    sections: [],
    sectionsReady: false,
    now: NOW,
  });
  expect(lines.map((l) => l.key)).toEqual(['h:now', 'e:now', 'h:loading', 'e:loading']);
});

test('지금은 다 펼친다 — 내 차례가 많아도 "N개 더" 로 접지 않는다', () => {
  const stuck = Array.from({ length: 7 }, (_, i) =>
    todoFixture({
      id: `s${i}`,
      status: 'doing',
      doingBy: 'claude-code',
      doingSince: '2026-10-12T01:00:00Z',
      doingState: 'gone',
    }),
  );
  const lines = buildLines({ todos: stuck, handoffs: [], sections: [], now: NOW });
  const now = lines.filter((l) => l.kind === 'now');
  expect(now).toHaveLength(7);
  expect(now.every((l) => l.kind === 'now' && l.row.group !== 'more')).toBe(true);
});
