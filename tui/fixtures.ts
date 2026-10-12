/**
 * TUI 테스트 픽스처 — 웹 `web/test-support.tsx` 는 브라우저 스토어까지 끌고 와서 쓰지 않는다. 같은 기본값을 쓴다.
 */
import type { Api } from './api';
import type { Board, HandoffView, Section, TodoView } from '../web/types';

export function todoFixture(over: Partial<TodoView> = {}): TodoView {
  return {
    id: 'todo1',
    number: 1,
    boardId: 'board1',
    title: '원래 제목',
    description: '',
    status: 'todo',
    priority: 'p4',
    labels: [],
    links: [],
    position: 0,
    createdAt: '2026-07-27T00:00:00.000Z',
    updatedAt: '2026-07-27T00:00:00.000Z',
    ref: 'rocky-1',
    commentCount: 0,
    ...over,
  };
}

export function boardFixture(over: Partial<Board> = {}): Board {
  return {
    id: 'board1',
    key: 'rocky',
    title: 'rocky',
    createdAt: '2026-07-27T00:00:00.000Z',
    ...over,
  };
}

export function sectionFixture(over: Partial<Section> = {}): Section {
  return { id: 'sec1', boardId: 'board1', title: '검증', position: 0, ...over };
}

/** 데몬 대신 쓰는 가짜 API — 응답은 고정, 쓰기 호출은 `calls` 에 남는다. */
export function fakeApi(data: {
  boards?: Board[];
  todos?: TodoView[];
  sections?: Record<string, Section[]>;
  handoffs?: HandoffView[];
}): Api & { calls: string[] } {
  const calls: string[] = [];
  const api = {
    calls,
    boards: async () => data.boards ?? [],
    todos: async () => data.todos ?? [],
    sections: async (board: string) => {
      calls.push(`sections ${board}`);
      return data.sections?.[board] ?? [];
    },
    handoffs: async () => data.handoffs ?? [],
    todo: async (id: string) => {
      calls.push(`todo ${id}`);
      const todo = (data.todos ?? []).find((t) => t.id === id) ?? todoFixture({ id });
      return { todo, history: [], comments: [] };
    },
    setStatus: async (id: string, action: string) => {
      calls.push(`status ${id} ${action}`);
      return todoFixture({ id });
    },
    add: async (board: string, title: string) => {
      calls.push(`add ${board} ${title}`);
      return todoFixture({ title });
    },
  };
  return api as Api & { calls: string[] };
}
