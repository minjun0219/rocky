import { afterEach, describe, expect, mock, test } from 'bun:test';
import { cleanup, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { renderWithStore } from '../test-support';
import { ViewSwitch } from './ViewSwitch';

afterEach(cleanup);

describe('ViewSwitch', () => {
  test('누른 쪽으로 바꾸고, 지금 보기는 눌린 상태로 보인다', async () => {
    const setView = mock(() => {});
    renderWithStore(<ViewSwitch />, { view: 'worklog', showGithub: false, setView });
    expect(screen.getByRole('button', { name: '작업로그' }).getAttribute('aria-pressed')).toBe(
      'true',
    );
    await userEvent.click(screen.getByRole('button', { name: /피드/ }));
    expect(setView).toHaveBeenCalledWith('feed');
  });

  test('할 일 · 노트 탭은 없다 — 보드 화면을 걷어냈다', () => {
    renderWithStore(<ViewSwitch />, { view: 'feed', showGithub: true, showAgents: true });
    expect(screen.queryByRole('button', { name: '할 일' })).toBeNull();
    expect(screen.queryByRole('button', { name: /노트/ })).toBeNull();
    expect(screen.getByRole('button', { name: 'GitHub' })).toBeTruthy();
    expect(screen.getByRole('button', { name: '에이전트' })).toBeTruthy();
  });
});
