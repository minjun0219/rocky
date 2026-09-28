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

/** happy-dom 은 matchMedia 가 항상 불일치라 좁은 화면을 흉내 낸다. 반환값으로 복원. */
function stubNarrow(matches: boolean) {
  const original = window.matchMedia;
  window.matchMedia = ((query: string) =>
    ({
      matches,
      media: query,
      addEventListener: () => {},
      removeEventListener: () => {},
    }) as unknown as MediaQueryList) as typeof window.matchMedia;
  return () => {
    window.matchMedia = original;
  };
}

describe('NotesRail 접힘 토글', () => {
  // 접힘은 모든 폭에서 같다 — 여기서는 상태와 신호(is-open 클래스·캐럿·개수)가 올바르게
  // 오가는지만 고정한다.
  test('토글이 is-open 과 캐럿을 뒤집고, 개수를 보여준다', async () => {
    const restore = stubNarrow(true);
    renderWithStore(<NotesRail />, { notes: [NOTE] });
    const toggle = screen.getByRole('button', { name: /NOTES/ });
    expect(toggle.textContent).toContain('· 1');
    expect(toggle.getAttribute('aria-expanded')).toBe('false');
    expect(document.querySelector('.notes-rail.is-open')).toBeNull();

    await userEvent.click(toggle);
    expect(toggle.getAttribute('aria-expanded')).toBe('true');
    expect(document.querySelector('.notes-rail.is-open')).not.toBeNull();
    restore();
  });

  test('+ 메모는 접힘을 강제로 편다 — 접힌 채 추가하면 새 메모가 안 보인다', async () => {
    const restore = stubNarrow(true);
    const addNote = async () => {};
    renderWithStore(<NotesRail />, { notes: [], addNote });
    await userEvent.click(screen.getByRole('button', { name: '+ 메모' }));
    expect(document.querySelector('.notes-rail.is-open')).not.toBeNull();
    restore();
  });
});
