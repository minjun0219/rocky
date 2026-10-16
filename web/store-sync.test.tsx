import { afterEach, beforeEach, describe, expect, test } from 'bun:test';
import { type BoardSync, useUiStore } from './store';
import type { SyncSnapshot } from './sync/client';
import {
  applyRows,
  emptyReplica,
  type PendingOp,
  type SyncOp,
  type SyncTables,
} from './sync/replica';
import type { Board, HistoryEntry, Todo } from './types';

/**
 * 보드 싱크가 붙었을 때의 스토어 — 사본으로 그리고, 쓰기를 `tx` 로 보내고, 답을 기다리는 동안 겹쳐 그린다.
 * (`.tsx` 인 이유: happy-dom preload 가 붙어야 `window.history`·`localStorage` 가 있다.)
 */

const AT = '2026-10-16T00:00:00.000Z';
const board = (over: Partial<Board> = {}): Board => ({
  id: 'b1',
  key: 'rocky',
  title: 'rocky',
  createdAt: AT,
  ...over,
});
const todo = (over: Partial<Todo> = {}): Todo => ({
  id: 't1',
  number: 1,
  boardId: 'b1',
  title: '할 일',
  description: '',
  status: 'todo',
  priority: 'p4',
  labels: [],
  links: [],
  position: 1,
  createdAt: AT,
  updatedAt: AT,
  ...over,
});

const ROWS: Partial<SyncTables> = {
  boards: [board(), board({ id: 'b2', key: 'acorn' })],
  todos: [todo(), todo({ id: 't2', number: 1, boardId: 'b2' })],
  sections: [{ id: 's1', boardId: 'b1', title: '진행', position: 1 }],
  notes: [
    {
      id: 'n1',
      number: 1,
      boardId: 'b1',
      title: '노트',
      content: '',
      position: 1,
      createdAt: AT,
      updatedAt: AT,
    },
  ],
  history: [{ id: 10, entity: 'todo', entityId: 't1', actor: 'logan', action: 'update', at: AT }],
};

class FakeSync implements BoardSync {
  sent: SyncOp[] = [];
  woke = 0;
  async send(op: SyncOp) {
    this.sent.push(op);
  }
  reconnectNow() {
    this.woke += 1;
  }
}

function snapshot(rows: Partial<SyncTables> = ROWS, pending: PendingOp[] = []): SyncSnapshot {
  return {
    replica: applyRows(emptyReplica(), { epoch: 'e1', seq: 5, full: true, rows }),
    pending,
    connected: true,
  };
}

const pristine = useUiStore.getState();
const realFetch = globalThis.fetch;
let requests: string[] = [];
let sync: FakeSync;

beforeEach(() => {
  (window as unknown as { happyDOM: { setURL: (url: string) => void } }).happyDOM.setURL(
    'http://localhost/rocky',
  );
  window.history.replaceState(null, '', '/rocky');
  requests = [];
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const url = String(input);
    requests.push(url);
    if (url.startsWith('/api/todos/')) {
      const older: HistoryEntry = {
        id: 2,
        entity: 'todo',
        entityId: 't1',
        actor: 'logan',
        action: 'create',
        at: AT,
      };
      return new Response(JSON.stringify({ history: [older] }));
    }
    return new Response(url.startsWith('/api/prs') ? '[]' : '{}');
  }) as unknown as typeof fetch;
  useUiStore.setState({ ...pristine, selected: 'rocky', actor: 'logan' }, true);
  sync = new FakeSync();
  useUiStore.getState().attachSync(sync);
});

afterEach(() => {
  // 상태부터 되돌린다 — 싱크 중에 떼면 REST 로 다시 읽고(열린 상세까지), 그 응답이 진짜 fetch 로 새어 나간다.
  useUiStore.setState({ ...pristine }, true);
  useUiStore.getState().detachSync();
  globalThis.fetch = realFetch;
});

describe('보드 싱크', () => {
  test('사본을 받기 전에는 REST 로 그린다', () => {
    useUiStore.getState().applySync({ replica: emptyReplica(), pending: [], connected: false });
    expect(useUiStore.getState().syncing).toBe(false);
  });

  test('사본을 받으면 고른 보드의 묶음을 사본에서 만든다', () => {
    useUiStore.getState().applySync(snapshot());
    const state = useUiStore.getState();
    expect(state.syncing).toBe(true);
    expect(state.boards.map((b) => b.key)).toEqual(['acorn', 'rocky']);
    expect(state.todos.map((t) => t.ref)).toEqual(['rocky-1']);
    expect(state.nowTodos.map((t) => t.ref).sort()).toEqual(['acorn-1', 'rocky-1']);
    expect(state.sections.map((s) => s.title)).toEqual(['진행']);
    expect(state.notes.map((n) => n.ref)).toEqual(['rocky-1']);
  });

  test('보드를 바꾸면 요청을 기다리지 않고 다시 그린다', () => {
    useUiStore.getState().applySync(snapshot());
    useUiStore.getState().setSelected('acorn');
    expect(useUiStore.getState().todos.map((t) => t.ref)).toEqual(['acorn-1']);
    expect(requests.some((r) => r.startsWith('/api/todos'))).toBe(false);
  });

  test('쓰기는 tx 로 보내고 목록을 다시 읽지 않는다', async () => {
    useUiStore.getState().applySync(snapshot());
    await useUiStore.getState().addTodo({ board: 'rocky', title: '새 일' });
    await useUiStore.getState().setTodoStatus('t1', 'start');
    await useUiStore.getState().patchTodo('t1', { title: '고침', section: null });
    expect(sync.sent[0]).toMatchObject({ op: 'todo.create', board: 'rocky', title: '새 일' });
    expect((sync.sent[0] as { id: string }).id).toMatch(/^[0-9a-z]{8}$/);
    expect(sync.sent[1]).toEqual({ op: 'todo.status', id: 't1', action: 'start' });
    expect(sync.sent[2]).toEqual({ op: 'todo.update', id: 't1', title: '고침', section: null });
    expect(requests.filter((r) => r.startsWith('/api/todos'))).toEqual([]);
  });

  test('어휘에 없는 칸이 섞인 고치기는 REST 로', async () => {
    useUiStore.getState().applySync(snapshot());
    await useUiStore.getState().patchTodo('t1', { status: 'done' });
    expect(sync.sent).toEqual([]);
    expect(requests).toContain('/api/todos/t1');
  });

  test('답을 기다리는 op 는 사본 위에 겹쳐 그린다 — 번호 없는 새 할 일', () => {
    const pending: PendingOp[] = [
      {
        op: { op: 'todo.create', id: 'newtodo1', board: 'rocky', title: '새 일' },
        actor: 'logan',
        at: AT,
      },
    ];
    useUiStore.getState().applySync(snapshot(ROWS, pending));
    expect(useUiStore.getState().todos.map((t) => t.ref)).toEqual(['rocky-1', 'rocky-?']);
  });

  test('상세는 사본으로 바로 열고, 오래된 히스토리는 한 번 채운다', async () => {
    useUiStore.getState().applySync(snapshot());
    await useUiStore.getState().openTodoDetail('t1');
    expect(useUiStore.getState().detail?.history.map((h) => h.id)).toEqual([10]);
    await Bun.sleep(0);
    expect(requests).toContain('/api/todos/t1');
    expect(useUiStore.getState().detail?.history.map((h) => h.id)).toEqual([10, 2]);
  });

  test('에이전트 actor 면 tx 대신 REST 로 쓴다 — 데몬이 에이전트의 tx 를 거절한다', async () => {
    useUiStore.setState({ actor: 'claude-code' });
    useUiStore.getState().applySync(snapshot());
    await useUiStore.getState().setTodoStatus('t1', 'start');
    expect(sync.sent).toEqual([]);
    expect(requests).toContain('/api/todos/t1/status');
  });

  test('싱크를 떼면 REST 로 돌아가 다시 읽는다', () => {
    useUiStore.getState().applySync(snapshot());
    useUiStore.getState().detachSync();
    expect(useUiStore.getState().syncing).toBe(false);
    expect(requests.some((r) => r.startsWith('/api/todos'))).toBe(true);
  });

  test('화면이 다시 보이면 끊긴 싱크를 깨운다', () => {
    useUiStore.getState().wakeSync();
    expect(sync.woke).toBe(1);
  });
});
