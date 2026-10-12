/**
 * 화면의 줄 목록 — 순수 함수. 위에서부터 "지금"(전 보드의 내 차례·돌고 있음, 웹 피드와 같은 `nowRows`) → 보고 있는
 * 보드의 섹션별 트리. 커서는 할 일을 가리키는 줄만 밟는다.
 */
import { nowRows, type NowGlyph, type NowRow } from '../web/lib';
import type { HandoffView, Section, TodoView } from '../web/types';

export type Line =
  | { kind: 'heading'; key: string; title: string; count?: number }
  | { kind: 'now'; key: string; row: NowRow }
  | { kind: 'todo'; key: string; todo: TodoView; depth: number }
  | { kind: 'empty'; key: string; text: string };

export interface LinesInput {
  /** 전 보드의 할 일(보관 제외). */
  todos: TodoView[];
  handoffs: HandoffView[];
  /** 보고 있는 보드의 id — 없으면 트리를 그리지 않는다. */
  boardId?: string;
  sections: Section[];
  /** `sections` 가 보고 있는 보드 것인가 — 보드를 바꾼 직후 새 섹션이 오기 전엔 아니다(트리 대신 "읽는 중"). */
  sectionsReady?: boolean;
  now?: number;
}

/** 줄이 가리키는 할 일 — 커서가 밟는 줄이면 있다. */
export function todoIdOf(line: Line): string | undefined {
  if (line.kind === 'todo') {
    return line.todo.id;
  }
  if (line.kind === 'now') {
    return line.row.todoId;
  }
  return undefined;
}

export function buildLines({
  todos,
  handoffs,
  boardId,
  sections,
  sectionsReady = true,
  now = Date.now(),
}: LinesInput): Line[] {
  const lines: Line[] = [];

  // 지금 — 읽음 표시는 웹 브라우저에만 있으니 TUI 는 댓글을 전부 읽은 것으로 넘긴다(아니면 모든 댓글이 "새 댓글" 행이 된다).
  const seen = Object.fromEntries(
    todos.filter((t) => t.lastCommentAt).map((t) => [t.id, t.lastCommentAt as string]),
  );
  // 다 펼친다 — TUI 에는 "N개 더" 를 펼칠 동작이 없어 접으면 그 안쪽 행에 닿을 길이 없다(목록은 어차피 스크롤된다).
  const rows = nowRows({ todos, handoffs, seen, expanded: true, expandUnread: true }, now);
  lines.push({ kind: 'heading', key: 'h:now', title: '지금', count: rows.length });
  if (rows.length === 0) {
    lines.push({ kind: 'empty', key: 'e:now', text: '돌고 있는 일도, 내 차례도 없어요' });
  }
  for (const row of rows) {
    lines.push({ kind: 'now', key: `n:${row.key}`, row });
  }

  if (!boardId) {
    return lines;
  }
  if (!sectionsReady) {
    // 섹션 없이 그리면 섹션에 든 할 일이 다 빠져 "열린 할 일이 없어요" 로 보인다 — 섹션이 올 때까지 가린다.
    lines.push({ kind: 'heading', key: 'h:loading', title: '할 일' });
    lines.push({ kind: 'empty', key: 'e:loading', text: '읽는 중이에요…' });
    return lines;
  }

  // 보드 트리 — 웹 TodoPane 과 같은 규칙: 부모가 이 목록에 있으면 그 아래로, 끝난 것만 남은 가지는 숨긴다.
  const mine = todos.filter((t) => t.boardId === boardId);
  const byId = new Set(mine.map((t) => t.id));
  const childrenOf = new Map<string, TodoView[]>();
  const roots: TodoView[] = [];
  for (const todo of mine) {
    if (todo.parentId && byId.has(todo.parentId)) {
      const siblings = childrenOf.get(todo.parentId) ?? [];
      siblings.push(todo);
      childrenOf.set(todo.parentId, siblings);
    } else {
      roots.push(todo);
    }
  }
  const open = (todo: TodoView): boolean =>
    todo.status !== 'done' || (childrenOf.get(todo.id) ?? []).some(open);
  const walk = (items: TodoView[], depth: number) => {
    for (const todo of items.filter(open)) {
      lines.push({ kind: 'todo', key: `t:${todo.id}`, todo, depth });
      walk(childrenOf.get(todo.id) ?? [], depth + 1);
    }
  };

  const groups: { key: string; title: string; items: TodoView[] }[] = [];
  const loose = roots.filter((t) => !t.sectionId);
  if (loose.length > 0) {
    groups.push({ key: '__none', title: '일반', items: loose });
  }
  for (const section of sections) {
    const items = roots.filter((t) => t.sectionId === section.id);
    if (items.length > 0) {
      groups.push({ key: section.id, title: section.title, items });
    }
  }
  const visible = groups.filter((g) => g.items.some(open));
  if (visible.length === 0) {
    lines.push({ kind: 'heading', key: 'h:empty', title: '할 일' });
    lines.push({ kind: 'empty', key: 'e:board', text: '열린 할 일이 없어요' });
  }
  for (const group of visible) {
    lines.push({
      kind: 'heading',
      key: `h:${group.key}`,
      title: group.title,
      count: group.items.filter(open).length,
    });
    walk(group.items, 0);
  }
  return lines;
}

/**
 * 커서가 있는 줄 — 그 키의 줄이 있으면 그 줄, 사라졌으면(완료로 트리에서 빠짐·갱신) 마지막 위치에서 가장 가까운 다음 줄
 * (없으면 마지막 할 일 줄). 맨 위로 튀면 다음 키(`d` 등)가 보지도 않은 할 일에 닿는다.
 */
export function cursorAt(lines: Line[], key: string | undefined, lastIndex = 0): number {
  const found = lines.findIndex((l) => l.key === key);
  const line = found >= 0 ? lines[found] : undefined;
  if (line && todoIdOf(line)) {
    return found;
  }
  return moveCursor(lines, lastIndex, 0);
}

/** 커서가 밟을 수 있는 줄의 위치들. */
export function selectable(lines: Line[]): number[] {
  return lines.flatMap((line, i) => (todoIdOf(line) ? [i] : []));
}

/**
 * 커서를 `delta` 만큼 옮긴다 — 밟을 수 있는 줄 사이에서, 끝에서 멈춘다. 지금 줄이 사라졌으면(갱신으로) 가장 가까운
 * 다음 줄로.
 */
export function moveCursor(lines: Line[], current: number, delta: number): number {
  const stops = selectable(lines);
  if (stops.length === 0) {
    return -1;
  }
  let at = stops.indexOf(current);
  if (at < 0) {
    const next = stops.findIndex((i) => i >= current);
    at = next < 0 ? stops.length - 1 : next;
    if (delta > 0) {
      at -= 1;
    }
  }
  const to = Math.min(stops.length - 1, Math.max(0, at + delta));
  return stops[to] as number;
}

/** 줄 앞 글리프와 색 — 웹 DESIGN.md "State Vocabulary". 색만으로 말하지 않게 모양도 다르다. */
export const GLYPH: Record<NowGlyph | 'open' | 'done', { char: string; color?: string }> = {
  run: { char: '●', color: 'green' },
  mine: { char: '◆', color: 'yellow' },
  dead: { char: '⊘', color: 'red' },
  unknown: { char: '?' },
  open: { char: '○' },
  done: { char: '✓', color: 'gray' },
};

/** 트리 줄의 글리프 — 진행 중이면 세션 판정(`doingState`)을 따른다. */
export function todoGlyph(todo: TodoView): (typeof GLYPH)[keyof typeof GLYPH] {
  if (todo.status === 'done') {
    return GLYPH.done;
  }
  if (todo.status !== 'doing') {
    return GLYPH.open;
  }
  // 판정이 없으면(사람이 든 것·서버가 세션을 안 본 응답) 그냥 진행 중이다 — 모름은 서버가 "못 읽었다" 고 할 때만.
  switch (todo.doingState) {
    case 'idle':
      return GLYPH.mine;
    case 'gone':
      return GLYPH.dead;
    case 'unknown':
      return GLYPH.unknown;
    default:
      return GLYPH.run;
  }
}
