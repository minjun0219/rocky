/**
 * 보드 사본 — 데몬의 싱크 프레임(`rows`)을 테이블별 행 맵으로 들고, 아직 답을 받지 않은 쓰기(op)를 그 위에 겹친다.
 *
 * 순수 모듈이다. 소켓·IndexedDB·zustand 는 부른 쪽이 잇는다. 프레임과 규칙은 `docs/features/sync.md`, 설계는
 * `docs/design/specs/2026-10-15-dual-structure-sync-design.md` 의 2번 조각. 화면용 목록과 계산 값은 `./views`.
 */
import type {
  Board,
  Comment,
  Handoff,
  HistoryEntry,
  Note,
  Section,
  StatusAction,
  Todo,
  TodoLink,
  TodoPriority,
} from '../types';

/** 데몬이 싱크하는 일곱 테이블 — `rocky_core::sync::SyncTables` 와 같은 모양. */
export interface SyncTables {
  boards: Board[];
  sections: Section[];
  todos: Todo[];
  comments: Comment[];
  notes: Note[];
  history: HistoryEntry[];
  handoffs: Handoff[];
}

export type TableName = keyof SyncTables;

export const TABLE_NAMES: readonly TableName[] = [
  'boards',
  'sections',
  'todos',
  'comments',
  'notes',
  'history',
  'handoffs',
];

/** 테이블 하나의 행. */
export type RowOf<K extends TableName> = SyncTables[K][number];

/** 테이블마다 id → 행. 히스토리의 숫자 id 도 문자열 키로 둔다. */
export type Tables = { readonly [K in TableName]: ReadonlyMap<string, RowOf<K>> };

/** 데몬이 보내는 `{t:"rows"}` 프레임의 몸통. */
export interface RowsFrame {
  epoch: string;
  seq: number;
  /** 참이면 `rows` 가 DB 의 보드 행 전부다 — 사본을 통째로 바꾼다. */
  full: boolean;
  rows: Partial<SyncTables>;
}

export interface Replica {
  /** 이 사본을 준 DB 의 표식. 데몬이 다시 뜨면 바뀐다. 아직 받은 적이 없으면 null. */
  readonly epoch: string | null;
  /** 받은 마지막 순번 — 다음 `sync` 의 `since`. 받은 적이 없으면 -1. */
  readonly seq: number;
  readonly tables: Tables;
}

/**
 * 사본이 들고 있는 히스토리 줄 수 — 데몬이 통째로 줄 때 싣는 줄 수와 같다. 이어 받으며 늘어난 줄은 이 수로 다시
 * 자른다. 그보다 오래된 줄은 상세 드로어가 REST 로 채운다.
 */
export const HISTORY_LIMIT = 2_000;

function keyOf(row: { id: string | number }): string {
  return String(row.id);
}

function emptyTables(): Tables {
  return {
    boards: new Map(),
    sections: new Map(),
    todos: new Map(),
    comments: new Map(),
    notes: new Map(),
    history: new Map(),
    handoffs: new Map(),
  };
}

export function emptyReplica(): Replica {
  return { epoch: null, seq: -1, tables: emptyTables() };
}

/** 히스토리를 새 줄부터 `HISTORY_LIMIT` 줄만 남긴다. 넘지 않으면 그대로 돌려준다. */
function trimHistory(
  history: ReadonlyMap<string, HistoryEntry>,
): ReadonlyMap<string, HistoryEntry> {
  if (history.size <= HISTORY_LIMIT) {
    return history;
  }
  const kept = [...history.values()].sort((a, b) => b.id - a.id).slice(0, HISTORY_LIMIT);
  return new Map(kept.map((h) => [keyOf(h), h]));
}

/**
 * 받은 `rows` 를 사본에 적용한다. `full` 이면 통째로 바꾸고, 아니면 온 행을 id 로 덮어쓴다(삭제가 없어 지운 행은
 * 오지 않는다). 바뀌지 않은 테이블은 같은 맵을 그대로 둔다 — 부른 쪽이 참조 비교로 다시 그릴지 정한다.
 */
export function applyRows(replica: Replica, frame: RowsFrame): Replica {
  if (frame.full) {
    const tables = emptyTables() as { [K in TableName]: Map<string, RowOf<K>> };
    for (const name of TABLE_NAMES) {
      const into = tables[name] as Map<string, RowOf<TableName>>;
      for (const row of frame.rows[name] ?? []) {
        into.set(keyOf(row), row);
      }
    }
    return {
      epoch: frame.epoch,
      seq: frame.seq,
      tables: { ...tables, history: trimHistory(tables.history) },
    };
  }
  const tables: { -readonly [K in TableName]: ReadonlyMap<string, RowOf<K>> } = {
    ...replica.tables,
  };
  for (const name of TABLE_NAMES) {
    const rows = frame.rows[name];
    if (!rows || rows.length === 0) {
      continue;
    }
    const next = new Map(replica.tables[name] as ReadonlyMap<string, RowOf<TableName>>);
    for (const row of rows) {
      next.set(keyOf(row), row);
    }
    (tables as Record<TableName, ReadonlyMap<string, RowOf<TableName>>>)[name] = next;
  }
  tables.history = trimHistory(tables.history);
  return { epoch: frame.epoch, seq: Math.max(replica.seq, frame.seq), tables };
}

// ── 쓰기(op) ─────────────────────────────────────────────────────────────────

/**
 * 싱크 트랜잭션의 쓰기 하나 — `rocky_core::sync::SyncOp` 의 JSON 모양. 지우기가 되는 칸은 `null` 이면 지우고, 빠지면
 * 그대로 둔다. 할 일의 `section` 은 섹션 **이름**이다(없으면 데몬이 만든다).
 */
export type SyncOp =
  | {
      op: 'todo.create';
      id: string;
      board: string;
      title: string;
      description?: string;
      section?: string;
      parentId?: string;
      priority?: TodoPriority;
      due?: string;
      labels?: string[];
      links?: TodoLink[];
    }
  | {
      op: 'todo.update';
      id: string;
      title?: string;
      description?: string;
      priority?: TodoPriority;
      due?: string | null;
      labels?: string[];
      links?: TodoLink[];
      section?: string | null;
      parentId?: string | null;
    }
  | { op: 'todo.status'; id: string; action: StatusAction }
  /** 같은 보드 안에서 `before` 앞으로. `null` 이면 맨 뒤. */
  | { op: 'todo.move'; id: string; before: string | null }
  | { op: 'comment.create'; id: string; todoId: string; body: string }
  | { op: 'comment.update'; id: string; body: string }
  | { op: 'comment.archive'; id: string }
  | { op: 'comment.unarchive'; id: string }
  | { op: 'section.create'; board: string; title: string }
  | { op: 'section.archive'; id: string }
  | { op: 'note.create'; id: string; board?: string; title: string; content?: string }
  | { op: 'note.update'; id: string; title: string }
  | { op: 'note.archive'; id: string }
  | { op: 'note.unarchive'; id: string }
  | { op: 'note.pin'; id: string }
  | { op: 'note.unpin'; id: string }
  | { op: 'board.update'; key: string; title?: string; description?: string | null };

/** 겹쳐 그릴 때의 맥락 — 쓰는 사람과 그 시각. */
export interface OpContext {
  actor: string;
  now: string;
}

/** 답을 기다리는 op 하나 — 보낸 사람과 보낸 시각을 같이 들고 있어 다시 그려도 같은 그림이 나온다. */
export interface PendingOp {
  op: SyncOp;
  actor: string;
  at: string;
}

/**
 * 만들기에 쓸 id — 데몬의 `new_id` 와 같은 모양(8자 base36). 낙관적으로 그린 행과 확정된 행이 이 id 를 같이 쓴다.
 */
export function newClientId(
  random: (bytes: Uint8Array) => Uint8Array = (b) => crypto.getRandomValues(b),
): string {
  const bytes = random(new Uint8Array(8));
  let id = '';
  for (const byte of bytes) {
    id += (byte % 36).toString(36);
  }
  return id;
}

/** 아직 데몬에 없는 행의 id — 확정된 행과 겹치지 않게 base36 밖의 글자로 시작한다. */
function pendingId(kind: string, ...parts: string[]): string {
  return `~${kind}:${parts.join(':')}`;
}

/** 표 하나를 고칠 때만 복사하는 초안. */
class Draft {
  private readonly copies = new Map<TableName, Map<string, unknown>>();

  constructor(private readonly base: Tables) {}

  get<K extends TableName>(name: K, id: string): RowOf<K> | undefined {
    return this.table(name).get(id);
  }

  values<K extends TableName>(name: K): RowOf<K>[] {
    return [...this.table(name).values()];
  }

  put<K extends TableName>(name: K, row: RowOf<K>): void {
    let copy = this.copies.get(name) as Map<string, RowOf<K>> | undefined;
    if (!copy) {
      copy = new Map(this.base[name] as ReadonlyMap<string, RowOf<K>>);
      this.copies.set(name, copy as Map<string, unknown>);
    }
    copy.set(keyOf(row), row);
  }

  done(): Tables {
    const out: Record<string, unknown> = { ...this.base };
    for (const [name, copy] of this.copies) {
      out[name] = copy;
    }
    return out as unknown as Tables;
  }

  private table<K extends TableName>(name: K): ReadonlyMap<string, RowOf<K>> {
    return (this.copies.get(name) ?? this.base[name]) as ReadonlyMap<string, RowOf<K>>;
  }
}

function boardByKey(draft: Draft, key: string): Board | undefined {
  const boards = draft.values('boards');
  return (
    boards.find((b) => b.key === key) ?? boards.find((b) => b.previousKeys?.includes(key) === true)
  );
}

/** 보드를 key 로 찾고, 없으면(데몬이 처음 쓸 때 만든다) 임시 보드를 둔다. */
function ensureBoard(draft: Draft, key: string, now: string): Board {
  const found = boardByKey(draft, key);
  if (found) {
    return found;
  }
  const board: Board = { id: pendingId('board', key), key, title: key, createdAt: now };
  draft.put('boards', board);
  return board;
}

/** 섹션을 이름으로 찾고(보관하지 않은 것), 없으면 임시 섹션을 둔다 — 데몬도 이름으로 만든다. */
function ensureSection(draft: Draft, boardId: string, title: string): Section {
  const trimmed = title.trim();
  const sections = draft.values('sections').filter((s) => s.boardId === boardId);
  const found = sections.find((s) => s.title === trimmed && s.archivedAt === undefined);
  if (found) {
    return found;
  }
  const section: Section = {
    id: pendingId('section', boardId, trimmed),
    boardId,
    title: trimmed,
    position: Math.max(0, ...sections.map((s) => s.position)) + 1,
  };
  draft.put('sections', section);
  return section;
}

function nextPosition(rows: readonly { position: number }[]): number {
  return Math.max(0, ...rows.map((r) => r.position)) + 1;
}

/** 빠진 칸은 그대로, `null` 은 지우기, 값은 덮기 — 고친 행을 돌려준다. */
function withOptional<T extends object, K extends keyof T>(
  row: T,
  key: K,
  value: T[K] | null | undefined,
): T {
  if (value === undefined) {
    return row;
  }
  const next = { ...row };
  if (value === null) {
    delete next[key];
  } else {
    next[key] = value;
  }
  return next;
}

function applyTodoStatus(todo: Todo, action: StatusAction, ctx: OpContext): Todo {
  const cleared = { ...todo };
  delete cleared.doingBy;
  delete cleared.doingSince;
  delete cleared.doingSessionId;
  cleared.doingSessionClaimed = false;
  switch (action) {
    case 'start':
      return {
        ...cleared,
        status: 'doing',
        doingBy: ctx.actor,
        doingSince: ctx.now,
      };
    case 'stop':
      return { ...cleared, status: 'todo' };
    case 'done':
      return { ...cleared, status: 'done', completedAt: ctx.now };
    case 'reopen': {
      const reopened = { ...todo, status: 'todo' as const };
      delete reopened.completedAt;
      return reopened;
    }
    case 'archive':
      return { ...todo, archivedAt: ctx.now };
    case 'unarchive': {
      const restored = { ...todo };
      delete restored.archivedAt;
      return restored;
    }
  }
}

/** 같은 보드 안에서 `before` 앞으로(없으면 맨 뒤) 옮기고 1 부터 다시 번호를 매긴다 — 데몬의 재부여와 같은 모양. */
function moveTodo(draft: Draft, id: string, before: string | null): void {
  const moving = draft.get('todos', id);
  if (!moving) {
    return;
  }
  const siblings = draft
    .values('todos')
    .filter((t) => t.boardId === moving.boardId && t.id !== id)
    .sort((a, b) => a.position - b.position);
  const at = before === null ? -1 : siblings.findIndex((t) => t.id === before);
  if (at === -1) {
    siblings.push(moving);
  } else {
    siblings.splice(at, 0, moving);
  }
  siblings.forEach((todo, index) => {
    if (todo.position !== index + 1) {
      draft.put('todos', { ...todo, position: index + 1 });
    }
  });
}

function applyOp(draft: Draft, op: SyncOp, ctx: OpContext): void {
  switch (op.op) {
    case 'todo.create': {
      if (draft.get('todos', op.id)) {
        return; // 이미 확정됐다 — 결과 행이 먼저 와 있다
      }
      const board = ensureBoard(draft, op.board, ctx.now);
      const section = op.section?.trim() ? ensureSection(draft, board.id, op.section) : undefined;
      const todo: Todo = {
        id: op.id,
        number: 0,
        boardId: board.id,
        title: op.title,
        description: op.description ?? '',
        status: 'todo',
        priority: op.priority ?? 'p4',
        labels: op.labels ?? [],
        links: op.links ?? [],
        position: nextPosition(draft.values('todos').filter((t) => t.boardId === board.id)),
        createdAt: ctx.now,
        updatedAt: ctx.now,
        ...(section ? { sectionId: section.id } : {}),
        ...(op.parentId ? { parentId: op.parentId } : {}),
        ...(op.due ? { due: op.due } : {}),
      };
      draft.put('todos', todo);
      return;
    }
    case 'todo.update': {
      const current = draft.get('todos', op.id);
      if (!current) {
        return;
      }
      let next: Todo = {
        ...current,
        ...(op.title !== undefined ? { title: op.title } : {}),
        ...(op.description !== undefined ? { description: op.description } : {}),
        ...(op.priority !== undefined ? { priority: op.priority } : {}),
        ...(op.labels !== undefined ? { labels: op.labels } : {}),
        ...(op.links !== undefined ? { links: op.links } : {}),
      };
      next = withOptional(next, 'due', op.due);
      next = withOptional(next, 'parentId', op.parentId);
      if (op.section === null) {
        next = withOptional(next, 'sectionId', null);
      } else if (op.section?.trim()) {
        next.sectionId = ensureSection(draft, current.boardId, op.section).id;
      }
      draft.put('todos', next);
      return;
    }
    case 'todo.status': {
      const current = draft.get('todos', op.id);
      if (current) {
        draft.put('todos', applyTodoStatus(current, op.action, ctx));
      }
      return;
    }
    case 'todo.move':
      moveTodo(draft, op.id, op.before);
      return;
    case 'comment.create':
      if (!draft.get('comments', op.id)) {
        draft.put('comments', {
          id: op.id,
          todoId: op.todoId,
          actor: ctx.actor,
          body: op.body,
          createdAt: ctx.now,
          updatedAt: ctx.now,
        });
      }
      return;
    case 'comment.update': {
      const current = draft.get('comments', op.id);
      if (current) {
        draft.put('comments', { ...current, body: op.body });
      }
      return;
    }
    case 'comment.archive':
    case 'comment.unarchive': {
      const current = draft.get('comments', op.id);
      if (current) {
        draft.put(
          'comments',
          withOptional(current, 'archivedAt', op.op === 'comment.archive' ? ctx.now : null),
        );
      }
      return;
    }
    case 'section.create': {
      const board = ensureBoard(draft, op.board, ctx.now);
      if (op.title.trim()) {
        ensureSection(draft, board.id, op.title);
      }
      return;
    }
    case 'section.archive': {
      const current = draft.get('sections', op.id);
      if (!current) {
        return;
      }
      draft.put('sections', { ...current, archivedAt: ctx.now });
      // 데몬은 보관한 섹션의 할 일을 섹션 밖으로 뺀다(보관하지는 않는다).
      for (const todo of draft.values('todos')) {
        if (todo.sectionId === op.id) {
          draft.put('todos', withOptional(todo, 'sectionId', null));
        }
      }
      return;
    }
    case 'note.create': {
      if (draft.get('notes', op.id)) {
        return;
      }
      const board = op.board === undefined ? undefined : ensureBoard(draft, op.board, ctx.now);
      const boardId = board?.id;
      draft.put('notes', {
        id: op.id,
        number: 0,
        title: op.title,
        content: op.content ?? '',
        position: nextPosition(draft.values('notes').filter((n) => n.boardId === boardId)),
        createdAt: ctx.now,
        updatedAt: ctx.now,
        ...(boardId ? { boardId } : {}),
      });
      return;
    }
    case 'note.update':
    case 'note.archive':
    case 'note.unarchive':
    case 'note.pin':
    case 'note.unpin': {
      const current = draft.get('notes', op.id);
      if (!current) {
        return;
      }
      const next =
        op.op === 'note.update'
          ? { ...current, title: op.title }
          : op.op === 'note.archive' || op.op === 'note.unarchive'
            ? withOptional(current, 'archivedAt', op.op === 'note.archive' ? ctx.now : null)
            : withOptional(current, 'pinnedAt', op.op === 'note.pin' ? ctx.now : null);
      draft.put('notes', next);
      return;
    }
    case 'board.update': {
      const current = boardByKey(draft, op.key);
      if (!current) {
        return;
      }
      let next = op.title === undefined ? { ...current } : { ...current, title: op.title };
      next = withOptional(next, 'description', op.description);
      draft.put('boards', next);
      return;
    }
  }
}

/**
 * 아직 답을 받지 않은 op 를 확정된 테이블 위에 차례로 겹친다. 데몬의 규칙을 다 옮기지는 않는다 — 검증은 데몬이
 * 하고, 결과 행이 곧 와서 이 그림을 덮는다. 겹칠 op 가 없으면 받은 테이블을 그대로 돌려준다.
 *
 * 고친 행의 `updatedAt` 은 건드리지 않는다 — 그 값은 서버 시계다(노트의 "본 것" 이 그것으로 앞으로 간다). 새 행에만
 * 보낸 시각(브라우저 시계)을 쓴다.
 */
export function overlay(tables: Tables, pending: readonly PendingOp[]): Tables {
  if (pending.length === 0) {
    return tables;
  }
  const draft = new Draft(tables);
  for (const { op, actor, at } of pending) {
    applyOp(draft, op, { actor, now: at });
  }
  return draft.done();
}

/** 이 op 를 다시 보내도 안전한가 — 만들기는 같은 id 라 두 번 생기지 않는다. */
export function isRetrySafe(op: SyncOp): boolean {
  return (
    op.op === 'todo.create' ||
    op.op === 'comment.create' ||
    op.op === 'note.create' ||
    op.op === 'section.create'
  );
}
