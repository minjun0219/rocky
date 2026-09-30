import { afterEach, beforeEach, describe, expect, test } from 'bun:test';
import { cleanup, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { renderWithStore } from '../test-support';
import type { NoteView } from '../types';
import { COLLAPSED_KEY, NotesRail } from './NotesRail';

function noteFixture(over: Partial<NoteView> = {}): NoteView {
  return {
    id: 'n1',
    number: 1,
    ref: 'note-1',
    title: '메모',
    content: '',
    position: 0,
    createdAt: '2026-08-16T00:00:00.000Z',
    updatedAt: '2026-08-16T00:00:00.000Z',
    ...over,
  };
}

beforeEach(() => localStorage.removeItem(COLLAPSED_KEY));
afterEach(cleanup);

describe('NotesRail — 목록', () => {
  test('고정한 노트는 위에 카드로 펼치고, 나머지는 한 줄씩', () => {
    renderWithStore(<NotesRail />, {
      notes: [
        noteFixture({ id: 'a', title: '회의록', content: '## 안건\n- 배포' }),
        noteFixture({ id: 'b', title: '늘 보는 것', pinnedAt: '2026-09-30T00:00:00.000Z' }),
      ],
    });
    const pinned = screen.getByRole('region', { name: '고정한 노트' });
    expect(pinned.textContent).not.toContain('회의록');
    expect(screen.getByDisplayValue('늘 보는 것')).toBeTruthy();
    // 목록 행은 제목과 첫 내용 줄(기호를 걷은 것)을 싣는다.
    const row = screen.getByRole('button', { name: /회의록/ });
    expect(row.textContent).toContain('안건');
    expect(row.textContent).not.toContain('##');
  });

  test('행을 누르면 상세를 연다', async () => {
    const opened: string[] = [];
    renderWithStore(<NotesRail />, {
      notes: [noteFixture({ id: 'a', title: '회의록' })],
      openNote: (id: string) => void opened.push(id),
    });
    await userEvent.click(screen.getByRole('button', { name: /회의록/ }));
    expect(opened).toEqual(['a']);
  });

  test('고정 카드를 접으면 본문이 걷히고, 접힌 것은 기억된다', async () => {
    const note = noteFixture({
      title: '고정',
      content: '본문',
      pinnedAt: '2026-09-30T00:00:00.000Z',
    });
    renderWithStore(<NotesRail />, { notes: [note] });
    expect(screen.getByRole('button', { name: /고정 본문/ })).toBeTruthy();
    await userEvent.click(screen.getByRole('button', { name: '고정 접기' }));
    expect(screen.queryByRole('button', { name: /고정 본문/ })).toBeNull();
    expect(JSON.parse(localStorage.getItem(COLLAPSED_KEY) ?? '[]')).toEqual(['n1']);

    cleanup();
    renderWithStore(<NotesRail />, { notes: [note] });
    expect(screen.getByRole('button', { name: '고정 펼치기' })).toBeTruthy();
  });

  test('고정 버튼은 상태를 뒤집는다', async () => {
    const calls: [string, boolean][] = [];
    renderWithStore(<NotesRail />, {
      notes: [noteFixture({ id: 'a', title: '회의록' })],
      pinNote: async (id: string, pinned: boolean) => void calls.push([id, pinned]),
    });
    await userEvent.click(screen.getByRole('button', { name: '위에 고정' }));
    expect(calls).toEqual([['a', true]]);
  });

  test('노트가 없으면 무엇인지와 시작하는 법을 말한다', () => {
    renderWithStore(<NotesRail />, { notes: [] });
    expect(screen.getByText(/같이 쓰는 스크래치 패드/)).toBeTruthy();
  });

  test('"+ 새 노트" 는 보고 있는 보드에 만든다', async () => {
    const calls: unknown[] = [];
    const addNote = async (input: unknown) => {
      calls.push(input);
    };
    renderWithStore(<NotesRail />, { notes: [], selected: 'rocky', addNote });
    await userEvent.click(screen.getByRole('button', { name: '+ 새 노트' }));
    expect(calls).toEqual([{ board: 'rocky', title: '새 메모' }]);
  });
});

describe('NotesRail — 상세', () => {
  // 상세는 열자마자 실시간 세션을 연다 — 여기서는 서버가 없으니 여는 데 실패하고 미리보기로 남는다.
  const realFetch = globalThis.fetch;
  const hadEventSource = 'EventSource' in globalThis;
  beforeEach(() => {
    globalThis.fetch = (async () => {
      throw new Error('offline');
    }) as unknown as typeof fetch;
    if (!hadEventSource) {
      (globalThis as { EventSource?: unknown }).EventSource = class {
        close() {}
      };
    }
  });
  afterEach(() => {
    globalThis.fetch = realFetch;
    if (!hadEventSource) {
      delete (globalThis as { EventSource?: unknown }).EventSource;
    }
  });

  test('열린 노트는 목록 대신 상세로, 뒤로 가면 닫는다', async () => {
    let closed = 0;
    renderWithStore(<NotesRail />, {
      notes: [
        noteFixture({ id: 'a', title: '회의록' }),
        noteFixture({ id: 'b', title: '다른 것' }),
      ],
      openNoteId: 'b',
      closeNote: () => {
        closed++;
      },
    });
    expect(screen.getByRole('region', { name: '노트 상세' })).toBeTruthy();
    expect(screen.getByDisplayValue('다른 것')).toBeTruthy();
    expect(screen.queryByText('회의록')).toBeNull();
    await userEvent.click(screen.getByRole('button', { name: '노트' }));
    expect(closed).toBe(1);
  });

  test('열린 노트가 목록에 없으면(보관됨) 목록을 보여 준다', () => {
    renderWithStore(<NotesRail />, {
      notes: [noteFixture({ id: 'a', title: '회의록' })],
      openNoteId: 'gone',
    });
    expect(screen.getByRole('button', { name: /회의록/ })).toBeTruthy();
  });
});

describe('NotesRail — 본문 미리보기', () => {
  test('마크다운을 렌더해 보여 준다 — 기호 대신 모양으로', () => {
    renderWithStore(<NotesRail />, {
      notes: [
        noteFixture({
          title: '할 일',
          content: '# 오늘\n- [x] 끝낸 것\n- [ ] 남은 것',
          pinnedAt: '2026-09-30T00:00:00.000Z',
        }),
      ],
    });
    const body = screen.getByRole('button', { name: /할 일 본문/ });
    expect(body.textContent).toContain('오늘');
    expect(body.textContent).not.toContain('# 오늘');
    expect(body.textContent).toContain('☑');
    expect(body.textContent).toContain('☐');
  });
});
