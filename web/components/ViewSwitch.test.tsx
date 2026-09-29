import { afterEach, describe, expect, mock, test } from 'bun:test';
import { cleanup, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import type { NoteView } from '../types';
import { renderWithStore } from '../test-support';
import { ViewSwitch } from './ViewSwitch';

afterEach(cleanup);

const note = (updatedAt: string): NoteView => ({
  id: updatedAt,
  number: 1,
  ref: 'note-1',
  title: 'n',
  content: '',
  position: 0,
  createdAt: '2026-09-01T00:00:00.000Z',
  updatedAt,
});

describe('ViewSwitch', () => {
  test('누른 쪽으로 바꾸고, 지금 보기는 눌린 상태로 보인다', async () => {
    const setView = mock(() => {});
    renderWithStore(<ViewSwitch />, { view: 'todos', notes: [], setView });
    expect(screen.getByRole('button', { name: '할 일' }).getAttribute('aria-pressed')).toBe('true');
    await userEvent.click(screen.getByRole('button', { name: /노트/ }));
    expect(setView).toHaveBeenCalledWith('notes');
  });

  test('노트 보기를 떠난 뒤 고쳐진 노트가 있으면 점 — 에이전트의 편집을 할 일 보기에서 안다', () => {
    renderWithStore(<ViewSwitch />, {
      view: 'todos',
      notesSeenAt: '2026-09-28T00:00:00.000Z',
      notes: [note('2026-09-27T00:00:00.000Z'), note('2026-09-28T01:00:00.000Z')],
    });
    expect(screen.getByRole('img', { name: '새 편집 있음' })).toBeTruthy();
    expect(screen.getByRole('button', { name: /노트/ }).textContent).toContain('2');
  });

  test('다 본 뒤거나 노트 보기 안에서는 점이 없다', () => {
    renderWithStore(<ViewSwitch />, {
      view: 'todos',
      notesSeenAt: '2026-09-29T00:00:00.000Z',
      notes: [note('2026-09-28T01:00:00.000Z')],
    });
    expect(screen.queryByRole('img', { name: '새 편집 있음' })).toBeNull();
    cleanup();
    renderWithStore(<ViewSwitch />, {
      view: 'notes',
      notesSeenAt: '2026-09-01T00:00:00.000Z',
      notes: [note('2026-09-28T01:00:00.000Z')],
    });
    expect(screen.queryByRole('img', { name: '새 편집 있음' })).toBeNull();
  });
});
