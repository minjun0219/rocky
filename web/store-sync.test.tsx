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
import type { Board, Handoff, HistoryEntry, Todo } from './types';

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

/** 가짜 싱크 — 보낸 op 를 모으고, 답은 테스트가 정한다(기본은 바로 ack). */
class FakeSync implements BoardSync {
  sent: SyncOp[] = [];
  woke = 0;
  hold = false;
  private held: (() => void)[] = [];
  send(op: SyncOp) {
    this.sent.push(op);
    if (!this.hold) {
      return Promise.resolve();
    }
    return new Promise<void>((resolve) => this.held.push(resolve));
  }
  ackAll() {
    for (const resolve of this.held.splice(0)) {
      resolve();
    }
  }
  wake() {
    this.woke += 1;
  }
}

function snapshot(
  rows: Partial<SyncTables> = ROWS,
  pending: PendingOp[] = [],
  options: { live?: boolean; seq?: number } = {},
): SyncSnapshot {
  return {
    replica: applyRows(emptyReplica(), { epoch: 'e1', seq: options.seq ?? 5, full: true, rows }),
    pending,
    live: options.live ?? true,
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
  globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    requests.push(url);
    if (url === '/api/todos/t1') {
      const older: HistoryEntry = {
        id: 2,
        entity: 'todo',
        entityId: 't1',
        actor: 'logan',
        action: 'create',
        at: AT,
      };
      return new Response(
        JSON.stringify({
          todo: { ...todo(), ref: 'rocky-1', commentCount: 0 },
          history: [older],
          comments: [],
        }),
      );
    }
    if (url === '/api/notes' && init?.method === 'POST') {
      return new Response(
        JSON.stringify({ ...ROWS.notes?.[0], id: 'n9', number: 9, ref: 'rocky-9', title: '새' }),
      );
    }
    if (url === '/api/sessions') {
      return new Response(JSON.stringify({ available: true, sessions: [] }));
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

const notUsage = () => requests.filter((r) => r !== '/api/usage');

describe('보드 싱크', () => {
  test('사본을 받기 전에는 REST 로 그린다', () => {
    useUiStore.getState().applySync({ replica: emptyReplica(), pending: [], live: false });
    expect(useUiStore.getState().syncing).toBe(false);
  });

  test('사본을 받으면 고른 보드의 묶음을 사본에서 만든다', () => {
    useUiStore.getState().applySync(snapshot());
    const state = useUiStore.getState();
    expect(state.syncing).toBe(true);
    expect(document.documentElement.dataset.sync).toBe('replica');
    expect(state.boards.map((b) => b.key)).toEqual(['acorn', 'rocky']);
    expect(state.todos.map((t) => t.ref)).toEqual(['rocky-1']);
    expect(state.nowTodos.map((t) => t.ref).sort()).toEqual(['acorn-1', 'rocky-1']);
    expect(state.sections.map((s) => s.title)).toEqual(['진행']);
    expect(state.notes.map((n) => n.ref)).toEqual(['rocky-1']);
  });

  test('바뀌지 않은 묶음은 같은 배열이다 — 그 화면이 다시 그려지지 않는다', () => {
    useUiStore.getState().applySync(snapshot());
    const before = useUiStore.getState();
    useUiStore.getState().deriveBoard();
    expect(useUiStore.getState().todos).toBe(before.todos);
    expect(useUiStore.getState().notes).toBe(before.notes);
  });

  test('보드를 바꾸거나 보관 보기를 켜도 요청을 보내지 않는다', async () => {
    useUiStore.getState().applySync(snapshot());
    useUiStore.getState().setSelected('acorn');
    expect(useUiStore.getState().todos.map((t) => t.ref)).toEqual(['acorn-1']);
    useUiStore.getState().setShowArchived(true);
    await useUiStore.getState().applyRoute({ board: 'rocky' });
    expect(useUiStore.getState().todos.map((t) => t.ref)).toEqual(['rocky-1']);
    expect(notUsage()).toEqual([]);
    // 같은 보드를 다시 누르면(새로고침) 끊긴 싱크를 깨운다.
    const woke = sync.woke;
    useUiStore.getState().setSelected('rocky');
    expect(sync.woke).toBe(woke + 1);
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

  test('저장 사본만 그리고 아직 데몬을 따라잡지 못했으면 쓰기는 REST 로', async () => {
    useUiStore.getState().applySync(snapshot(ROWS, [], { live: false }));
    expect(useUiStore.getState().syncing).toBe(true); // 기다려 주는 동안은 사본으로 그린다
    await useUiStore.getState().setTodoStatus('t1', 'start');
    expect(sync.sent).toEqual([]);
    expect(requests).toContain('/api/todos/t1/status');
  });

  test('기다려 줄 시간이 지나면 REST 로 넘어가고, 다시 따라잡으면 사본으로 돌아온다', () => {
    useUiStore.getState().applySync(snapshot(ROWS, [], { live: false }));
    useUiStore.getState().leaveReplica();
    expect(useUiStore.getState().syncing).toBe(false);
    expect(document.documentElement.dataset.sync).toBeUndefined();
    expect(requests.some((r) => r.startsWith('/api/todos'))).toBe(true);
    useUiStore.getState().applySync(snapshot());
    expect(useUiStore.getState().syncing).toBe(true);
  });

  test('어휘에 없는 칸이 섞인 고치기는 REST 로', async () => {
    useUiStore.getState().applySync(snapshot());
    await useUiStore.getState().patchTodo('t1', { status: 'done' });
    expect(sync.sent).toEqual([]);
    expect(requests).toContain('/api/todos/t1');
  });

  test('막 만든 할 일에 REST 로만 하는 동작은 데몬이 알 때까지 기다린다', async () => {
    useUiStore.getState().applySync(snapshot());
    sync.hold = true;
    const adding = useUiStore.getState().addTodo({ board: 'rocky', title: '새 일' });
    const id = (sync.sent[0] as { id: string }).id;
    const handing = useUiStore.getState().sendHandoff(id, { note: '' });
    await Bun.sleep(0);
    expect(requests.some((r) => r.includes('/handoff'))).toBe(false);
    sync.ackAll();
    await adding;
    await handing;
    expect(requests).toContain(`/api/todos/${id}/handoff`);
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

  test('사본을 받기 전에 REST 로 연 상세의 히스토리는 사본으로 넘어가도 남는다', async () => {
    useUiStore.getState().detachSync();
    await useUiStore.getState().openTodoDetail('t1');
    useUiStore.getState().attachSync(sync);
    useUiStore.getState().applySync(snapshot());
    expect(useUiStore.getState().detail?.history.map((h) => h.id)).toEqual([10, 2]);
  });

  test('상세는 그 할 일의 핸드오프도 사본에서 — 드로어의 "세션" 이 REST 로 묻지 않는다', async () => {
    useUiStore.getState().applySync(
      snapshot({
        ...ROWS,
        handoffs: [
          {
            id: 'h1',
            todoId: 't1',
            sessionId: 's1',
            note: '',
            actor: 'logan',
            status: 'pending',
            createdAt: AT,
          },
          {
            id: 'h2',
            todoId: 't2',
            sessionId: 's1',
            note: '',
            actor: 'logan',
            status: 'pending',
            createdAt: AT,
          },
        ],
      }),
    );
    await useUiStore.getState().openTodoDetail('t1');
    expect(useUiStore.getState().detail?.handoffs?.map((h) => h.id)).toEqual(['h1']);
  });

  test('세션 목록보다 새 핸드오프는 새 목록이 올 때까지 판정하지 않는다', async () => {
    useUiStore.getState().applySync(snapshot(ROWS, [], { seq: 5 }));
    await useUiStore.getState().loadAgents();
    const delivered: Handoff = {
      id: 'h1',
      todoId: 't1',
      sessionId: 's1',
      note: '',
      actor: 'logan',
      status: 'delivered',
      createdAt: AT,
    };
    useUiStore
      .getState()
      .applySync(
        snapshot({ ...ROWS, handoffs: [{ ...delivered, seq: 6 } as Handoff] }, [], { seq: 6 }),
      );
    // 목록(순번 5까지)에 이 세션이 없어도 "착수 안 함" 으로 그리지 않는다.
    expect(useUiStore.getState().handoffs[0]?.unstarted).toBe(false);
    requests = [];
    await Bun.sleep(250);
    expect(requests).toContain('/api/sessions'); // 바뀌자마자 목록을 다시 받는다
  });

  test('번호를 아직 못 받은 노트는 열어도 주소를 만들지 않는다', () => {
    const pending: PendingOp[] = [
      {
        op: { op: 'note.create', id: 'newnote1', board: 'rocky', title: '새 노트' },
        actor: 'logan',
        at: AT,
      },
    ];
    useUiStore.getState().applySync(snapshot(ROWS, pending));
    const before = window.history.length;
    useUiStore.getState().openNote('newnote1');
    expect(useUiStore.getState().openNoteId).toBe('newnote1');
    expect(window.history.length).toBe(before);
  });

  test('에이전트 actor 면 tx 대신 REST 로 쓴다 — 데몬이 에이전트의 tx 를 거절한다', async () => {
    useUiStore.setState({ actor: 'claude-code' });
    useUiStore.getState().applySync(snapshot());
    await useUiStore.getState().setTodoStatus('t1', 'start');
    expect(sync.sent).toEqual([]);
    expect(requests).toContain('/api/todos/t1/status');
  });

  test('에이전트 actor 가 만든 노트는 응답으로 바로 연다 — 행은 아직 밀려오지 않았다', async () => {
    useUiStore.setState({ actor: 'claude-code' });
    useUiStore.getState().applySync(snapshot());
    await useUiStore.getState().addNote({ board: 'rocky', title: '새' });
    expect(useUiStore.getState().openNoteId).toBe('n9');
  });

  test('싱크를 떼면 REST 로 돌아가 다시 읽는다', () => {
    useUiStore.getState().applySync(snapshot());
    useUiStore.getState().detachSync();
    expect(useUiStore.getState().syncing).toBe(false);
    expect(requests.some((r) => r.startsWith('/api/todos'))).toBe(true);
  });

  test('화면이 다시 보이면 싱크를 깨운다', () => {
    useUiStore.getState().wakeSync();
    expect(sync.woke).toBe(1);
  });
});
