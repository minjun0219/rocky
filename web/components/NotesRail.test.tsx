import { afterEach, describe, expect, test } from 'bun:test';
import { cleanup, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import type { NoteView } from '../types';
import { renderWithStore } from '../test-support';
import { NotesRail } from './NotesRail';

const NOTE: NoteView = {
  id: 'n1',
  number: 1,
  ref: 'note-1',
  title: '메모',
  content: '',
  position: 0,
  createdAt: '2026-08-16T00:00:00.000Z',
  updatedAt: '2026-08-16T00:00:00.000Z',
};

afterEach(cleanup);

describe('NotesRail — 노트 보기', () => {
  // 접히는 레일이 아니라 화면 전체 — 들어오면 바로 노트가 보여야 한다(좁은 패널에서 목록 아래
  // 접힌 레일은 스크롤 너머에 묻혔다).
  test('토글 없이 바로 노트를 싣는다', () => {
    renderWithStore(<NotesRail />, { notes: [NOTE] });
    expect(screen.queryByRole('button', { name: /NOTES/ })).toBeNull();
    expect(screen.getByDisplayValue('메모')).toBeTruthy();
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
