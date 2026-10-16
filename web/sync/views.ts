/**
 * 사본에서 화면용 목록을 만든다 — 데몬이 REST 응답에 얹던 값(ref·댓글 수·doing 판정·핸드오프 단계)을 같은 규칙으로
 * 계산한다. 규칙의 정본은 Rust 다(`rocky_core::refs`, `rocky_core::doing`, `rocky_core::sessions::match_board`). 갈라지면
 * 화면과 CLI 가 다른 말을 하니 그쪽을 고치면 여기도 고친다.
 */
import { isAgentActor } from '../actors';
import type { BoardSelection } from '../route';
import {
  type AgentSession,
  type Board,
  type Comment,
  DETAIL_HISTORY_EXCLUDED,
  type DoingState,
  type Handoff,
  type HandoffPhase,
  type HandoffView,
  type HistoryEntry,
  type NoteView,
  type Section,
  type Todo,
  type TodoView,
} from '../types';
import type { Tables } from './replica';

/** 보드 없는 노트의 참조 접두사 — `rocky_core::refs::GLOBAL_NOTE_PREFIX`. */
export const GLOBAL_NOTE_PREFIX = 'note';
/** 상세 드로어가 보이는 히스토리 줄 수 — `GET /api/todos/:ref` 의 기본값. */
export const DETAIL_HISTORY_LIMIT = 50;

/** 세션 목록 — `GET /api/sessions` 의 `available`·`sessions`. */
export interface SessionsView {
  available: boolean;
  sessions: readonly AgentSession[];
  /**
   * 목록을 물을 때의 사본 순번. 이보다 나중에 바뀐 행(새 핸드오프, 방금 doing 이 된 할 일)은 이 목록으로 판정하지
   * 않는다 — 방금 뜬 세션이 낡은 목록에 없다고 "세션 없음"·"착수 안 함" 으로 그리지 않게. 없으면 늘 판정한다.
   */
  seq?: number;
}

/** 이 행이 세션 목록보다 나중에 바뀌었나 — 그러면 판정하지 않는다(모름). 싱크 행만 `seq` 를 싣는다. */
function newerThanSessions(row: object, sessions: SessionsView): boolean {
  const seq = (row as { seq?: unknown }).seq;
  return sessions.seq !== undefined && typeof seq === 'number' && seq > sessions.seq;
}

/** board key 가 `<key>-<number>` 로 되읽히는 모양인가 — `refs::is_ref_safe_board_key`. */
export function isRefSafeBoardKey(key: string): boolean {
  return key !== '' && !/[#\s]/.test(key) && key !== GLOBAL_NOTE_PREFIX;
}

/**
 * 사람이 읽는 참조 — `refs::ref_of`. 보드가 없으면 `note-N`, key 가 참조에 못 쓰는 모양이면 id. 데몬이 아직 번호를
 * 주지 않은 행(번호 0)은 `rocky-?` 로 그린다.
 */
export function refOf(boardKey: string | undefined, number: number, id: string): string {
  const shown = number > 0 ? String(number) : '?';
  if (boardKey === undefined) {
    return `${GLOBAL_NOTE_PREFIX}-${shown}`;
  }
  return isRefSafeBoardKey(boardKey) ? `${boardKey}-${shown}` : id;
}

// ── 세션 판정 (`rocky_core::doing`) ──────────────────────────────────────────

/** `sessionId` 와 짧은 8자 `id` 를 둘 다 본다 — spawn 세션은 짧은 id 로 저장된다. */
function findSession(
  sessions: readonly AgentSession[],
  identifier: string,
): AgentSession | undefined {
  return sessions.find((s) => s.sessionId === identifier || s.id === identifier);
}

function stateOfSession(session: AgentSession): DoingState {
  if (session.state === 'done') {
    return 'gone'; // background 세션은 끝나도 잠시 목록에 남는다
  }
  return session.status === 'busy' ? 'live' : 'idle';
}

/** cwd 의 경로 조각 하나가 board key 와 같은 세션 — `sessions::match_board`. */
export function matchBoard(sessions: readonly AgentSession[], boardKey: string): AgentSession[] {
  if (boardKey === '') {
    return [];
  }
  return sessions.filter((s) => s.cwd.split('/').some((segment) => segment === boardKey));
}

/** doing 인 할 일의 생존 상태 — `doing::resolve_doing_state`. */
export function resolveDoingState(
  todo: Todo,
  boardKey: string,
  sessions: SessionsView,
): DoingState {
  if (todo.status !== 'doing' || !sessions.available) {
    return 'unknown';
  }
  if (todo.doingSessionId !== undefined && !todo.doingSessionClaimed) {
    const session = findSession(sessions.sessions, todo.doingSessionId);
    return session ? stateOfSession(session) : 'gone';
  }
  if (todo.doingBy !== undefined && isAgentActor(todo.doingBy)) {
    return matchBoard(sessions.sessions, boardKey).length === 0 ? 'gone' : 'unknown';
  }
  return 'unknown';
}

/** 저장된 상태·시각을 한 단계로 접는다 — `doing::handoff_phase`. */
export function handoffPhase(handoff: Handoff): HandoffPhase {
  if (handoff.status !== 'delivered') {
    return handoff.status;
  }
  if (handoff.completedAt !== undefined) {
    return 'completed';
  }
  return handoff.acceptedAt !== undefined ? 'accepted' : 'delivered';
}

/** 배달됐는데 그 세션이 아무것도 안 했다 — `doing::is_unstarted`. 판별할 수 없으면 false. */
export function isUnstarted(handoff: Handoff, sessions: SessionsView): boolean {
  if (handoff.status !== 'delivered' || handoff.acceptedAt !== undefined || !sessions.available) {
    return false;
  }
  const session = findSession(sessions.sessions, handoff.sessionId);
  return session === undefined || stateOfSession(session) !== 'live';
}

/** 대기 중인데 대상 세션이 목록에 없다 — `GET /api/handoffs` 의 `stale`(전체 `sessionId` 로만 본다). */
export function isStaleHandoff(handoff: Handoff, sessions: SessionsView): boolean {
  return (
    handoff.status === 'pending' &&
    sessions.available &&
    !sessions.sessions.some((s) => s.sessionId === handoff.sessionId)
  );
}

// ── 목록 ─────────────────────────────────────────────────────────────────────

export interface ListOptions {
  /** 보드 key(현재 이름) 또는 전체. */
  board: BoardSelection;
  includeArchived: boolean;
  /** 세션 목록. 없으면 doing·핸드오프를 판정하지 않는다(모름). */
  sessions?: SessionsView | null;
}

/** 보드 목록 — `GET /api/boards`(보관하지 않은 것, key 순). */
export function boardsOf(tables: Tables): Board[] {
  return [...tables.boards.values()]
    .filter((b) => b.archivedAt === undefined)
    .sort((a, b) => (a.key < b.key ? -1 : a.key > b.key ? 1 : 0));
}

/** 보드 key 를 id 로 — 옛 key(별칭)도 푼다. 없으면 undefined. */
export function boardIdOf(tables: Tables, key: string): string | undefined {
  const boards = [...tables.boards.values()];
  return (boards.find((b) => b.key === key) ?? boards.find((b) => b.previousKeys?.includes(key)))
    ?.id;
}

function byPosition<T extends { position: number; createdAt?: string }>(a: T, b: T): number {
  return a.position - b.position || (a.createdAt ?? '').localeCompare(b.createdAt ?? '');
}

/** 할 일마다 보관하지 않은 댓글 수와 가장 늦은 시각 — `store.comment_stats_of`. */
function commentStats(tables: Tables): Map<string, { count: number; lastAt: string }> {
  const stats = new Map<string, { count: number; lastAt: string }>();
  for (const comment of tables.comments.values()) {
    if (comment.archivedAt !== undefined) {
      continue;
    }
    const seen = stats.get(comment.todoId);
    if (!seen) {
      stats.set(comment.todoId, { count: 1, lastAt: comment.createdAt });
    } else {
      seen.count += 1;
      if (comment.createdAt > seen.lastAt) {
        seen.lastAt = comment.createdAt;
      }
    }
  }
  return stats;
}

function toTodoView(
  todo: Todo,
  tables: Tables,
  stats: ReadonlyMap<string, { count: number; lastAt: string }>,
  sessions: SessionsView | null | undefined,
): TodoView {
  const boardKey = tables.boards.get(todo.boardId)?.key;
  const stat = stats.get(todo.id);
  return {
    ...todo,
    ref: refOf(boardKey ?? '', todo.number, todo.id),
    commentCount: stat?.count ?? 0,
    ...(stat ? { lastCommentAt: stat.lastAt } : {}),
    ...(sessions && todo.status === 'doing' && !newerThanSessions(todo, sessions)
      ? { doingState: resolveDoingState(todo, boardKey ?? '', sessions) }
      : {}),
  };
}

/** 할 일 목록 — `GET /api/todos?board=&includeArchived=`(위치 순). 보드를 못 찾으면 빈 목록. */
export function todoViews(tables: Tables, options: ListOptions): TodoView[] {
  const boardId = options.board === 'all' ? undefined : boardIdOf(tables, options.board);
  if (options.board !== 'all' && boardId === undefined) {
    return [];
  }
  const stats = commentStats(tables);
  return [...tables.todos.values()]
    .filter(
      (t) =>
        (boardId === undefined || t.boardId === boardId) &&
        (options.includeArchived || t.archivedAt === undefined),
    )
    .sort(byPosition)
    .map((t) => toTodoView(t, tables, stats, options.sessions));
}

/** 노트 목록 — `GET /api/notes?board=&includeArchived=`. 보드를 고르면 그 보드의 노트만(보드 없는 노트는 전체 보기에서). */
export function noteViews(tables: Tables, options: ListOptions): NoteView[] {
  const boardId = options.board === 'all' ? undefined : boardIdOf(tables, options.board);
  if (options.board !== 'all' && boardId === undefined) {
    return [];
  }
  return [...tables.notes.values()]
    .filter(
      (n) =>
        (boardId === undefined || n.boardId === boardId) &&
        (options.includeArchived || n.archivedAt === undefined),
    )
    .sort(byPosition)
    .map((note) => ({
      ...note,
      ref: refOf(
        note.boardId === undefined ? undefined : (tables.boards.get(note.boardId)?.key ?? ''),
        note.number,
        note.id,
      ),
    }));
}

/** 섹션 목록 — `GET /api/sections?board=`(보관하지 않은 것, 위치 순). 전체 보기는 빈 목록. */
export function sectionsOf(tables: Tables, board: BoardSelection): Section[] {
  const boardId = board === 'all' ? undefined : boardIdOf(tables, board);
  if (boardId === undefined) {
    return [];
  }
  return [...tables.sections.values()]
    .filter((s) => s.boardId === boardId && s.archivedAt === undefined)
    .sort((a, b) => a.position - b.position);
}

/**
 * 열린 핸드오프 — `GET /api/handoffs?open=true&board=`. 대기 중이거나 배달됐는데 안 끝난 것, 보관한 할 일의 것은
 * 뺀다. 새것부터.
 */
export function openHandoffs(
  tables: Tables,
  options: Omit<ListOptions, 'includeArchived'>,
): HandoffView[] {
  const boardId = options.board === 'all' ? undefined : boardIdOf(tables, options.board);
  if (options.board !== 'all' && boardId === undefined) {
    return [];
  }
  const sessions = options.sessions;
  return [...tables.handoffs.values()]
    .filter((h) => {
      const open =
        h.status === 'pending' || (h.status === 'delivered' && h.completedAt === undefined);
      const todo = tables.todos.get(h.todoId);
      return (
        open &&
        todo !== undefined &&
        todo.archivedAt === undefined &&
        (boardId === undefined || todo.boardId === boardId)
      );
    })
    .sort((a, b) => b.createdAt.localeCompare(a.createdAt))
    .map((h) => handoffView(h, sessions));
}

/** 핸드오프에 단계와 세션 판정을 얹는다. 세션 목록이 없으면 판정하지 않는다(false). */
function handoffView(handoff: Handoff, sessions: SessionsView | null | undefined): HandoffView {
  const judged = sessions && !newerThanSessions(handoff, sessions) ? sessions : null;
  return {
    ...handoff,
    phase: handoffPhase(handoff),
    unstarted: judged ? isUnstarted(handoff, judged) : false,
    stale: judged ? isStaleHandoff(handoff, judged) : false,
  };
}

/** 한 할 일의 핸드오프 전부 — `GET /api/handoffs?todo=`(상세 드로어의 "세션"). 끝난 것·취소한 것까지, 새것부터. */
export function todoHandoffs(
  tables: Tables,
  todoId: string,
  sessions?: SessionsView | null,
): HandoffView[] {
  return [...tables.handoffs.values()]
    .filter((h) => h.todoId === todoId)
    .sort((a, b) => b.createdAt.localeCompare(a.createdAt))
    .map((h) => handoffView(h, sessions));
}

/**
 * 상세 드로어 — `GET /api/todos/:ref` 의 `todo`·`history`·`comments`. 히스토리는 그 할 일의 줄 가운데 댓글 줄을 빼고
 * 새것부터 `DETAIL_HISTORY_LIMIT` 줄, 댓글은 오래된 것부터. 사본에 없는 할 일이면 undefined.
 */
export function todoDetail(
  tables: Tables,
  id: string,
  options: { includeArchived: boolean; sessions?: SessionsView | null },
): { todo: TodoView; history: HistoryEntry[]; comments: Comment[] } | undefined {
  const todo = tables.todos.get(id);
  if (!todo) {
    return undefined;
  }
  const history = [...tables.history.values()]
    .filter((h) => h.entityId === id && !DETAIL_HISTORY_EXCLUDED.has(h.action))
    .sort((a, b) => b.id - a.id)
    .slice(0, DETAIL_HISTORY_LIMIT);
  const comments = [...tables.comments.values()]
    .filter((c) => c.todoId === id && (options.includeArchived || c.archivedAt === undefined))
    .sort((a, b) => a.createdAt.localeCompare(b.createdAt));
  return {
    todo: toTodoView(todo, tables, commentStats(tables), options.sessions),
    history,
    comments,
  };
}

/**
 * 노트 히스토리 드로어 — `GET /api/notes/:ref` 의 `note`·`history`. 히스토리는 그 노트의 줄을 새것부터
 * `DETAIL_HISTORY_LIMIT` 줄. 사본에 없는 노트면 undefined.
 */
export function noteDetail(
  tables: Tables,
  id: string,
): { note: NoteView; history: HistoryEntry[] } | undefined {
  const note = tables.notes.get(id);
  if (!note) {
    return undefined;
  }
  const history = [...tables.history.values()]
    .filter((h) => h.entityId === id)
    .sort((a, b) => b.id - a.id)
    .slice(0, DETAIL_HISTORY_LIMIT);
  const boardKey =
    note.boardId === undefined ? undefined : (tables.boards.get(note.boardId)?.key ?? '');
  return { note: { ...note, ref: refOf(boardKey, note.number, note.id) }, history };
}
