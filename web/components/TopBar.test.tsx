import { afterEach, describe, expect, mock, test } from 'bun:test';
import { cleanup, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { renderWithStore } from '../test-support';
import { TopBar } from './TopBar';

afterEach(cleanup);

describe('TopBar', () => {
  test('연결돼 있으면 연결 표시가 없다 — 끊겼을 때만 말한다', () => {
    renderWithStore(<TopBar />, { connected: true, boards: [] });
    expect(screen.queryByText('연결 끊김')).toBeNull();
    cleanup();
    renderWithStore(<TopBar />, { connected: false, boards: [] });
    expect(screen.getByRole('status').textContent).toContain('연결 끊김');
  });

  test('드물게 쓰는 설정은 ⋯ 메뉴 안 — 테마·보관된 항목·편집자 이름', async () => {
    const setThemePref = mock(() => {});
    const setShowArchived = mock(() => {});
    const setActor = mock(() => {});
    renderWithStore(<TopBar />, {
      connected: true,
      boards: [],
      themePref: 'auto',
      actor: 'logan',
      setThemePref,
      setShowArchived,
      setActor,
    });
    // 머리줄에는 설명 없는 컨트롤이 늘어서지 않는다.
    expect(screen.queryByRole('checkbox')).toBeNull();
    await userEvent.click(screen.getByRole('button', { name: '메뉴' }));
    await userEvent.click(screen.getByRole('menuitemradio', { name: '다크' }));
    expect(setThemePref).toHaveBeenCalledWith('dark');
    await userEvent.click(screen.getByRole('checkbox', { name: '보관된 항목도 보기' }));
    expect(setShowArchived).toHaveBeenCalledWith(true);
    const name = screen.getByRole('textbox', { name: '편집자 이름' });
    await userEvent.clear(name);
    await userEvent.type(name, 'minjun{Enter}');
    expect(setActor).toHaveBeenCalledWith('minjun');
  });
});
