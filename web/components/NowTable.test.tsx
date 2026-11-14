import { afterEach, describe, expect, test } from 'bun:test';
import { cleanup, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { renderWithStore } from '../test-support';
import type { SessionRow } from '../types';
import { MineSection } from './NowTable';

afterEach(cleanup);

const waiting = (i: number): SessionRow => ({
  cwd: '/w/rocky',
  kind: 'background',
  id: `aaaa000${i}`,
  sessionId: `sess-${i}`,
  name: `rocky-${i}`,
  status: 'idle',
  state: 'blocked',
  startedAt: Date.parse('2026-10-06T00:00:00Z'),
  matched: false,
  job: { needs: `정할 것 ${i}`, updatedAt: `2026-10-0${i + 1}T00:00:00Z` },
});

describe('MineSection — 피드의 내 차례', () => {
  test('아무것도 없으면 "내 차례 없음" 한 줄', () => {
    renderWithStore(<MineSection />, { showAgents: true, agents: null });
    expect(screen.getByText('내 차례 없음')).toBeTruthy();
  });

  test('답을 기다리는 에이전트만 — 일하는 세션은 싣지 않는다', () => {
    const busy: SessionRow = { ...waiting(0), sessionId: 'busy', state: 'working', name: 'busy' };
    renderWithStore(<MineSection />, {
      showAgents: true,
      agents: { available: true, list: [waiting(1), busy] },
    });
    const rows = [...document.querySelectorAll('li')];
    expect(rows).toHaveLength(1);
    expect(rows[0]?.textContent).toContain('정할 것 1');
    expect(rows[0]?.textContent).toContain('rocky-1');
    expect(screen.getByRole('heading', { name: /내 차례\s*1/ })).toBeTruthy();
  });

  test('내 차례가 5행을 넘으면 "N개 더" — 누르면 펼친다', async () => {
    const list = Array.from({ length: 7 }, (_, i) => waiting(i));
    renderWithStore(<MineSection />, { showAgents: true, agents: { available: true, list } });
    // 오래 기다린 것이 위 — 가장 최근(6)이 접힌다.
    expect(screen.queryByText('정할 것 6')).toBeNull();
    // 머리의 개수는 접힘과 무관하게 전체 — 펼치기 전에도 7.
    expect(screen.getByRole('heading', { name: /내 차례\s*7/ })).toBeTruthy();
    await userEvent.click(screen.getByRole('button', { name: '내 차례 2개 더' }));
    expect(screen.getByText('정할 것 6')).toBeTruthy();
    expect(screen.getByRole('heading', { name: /내 차례\s*7/ })).toBeTruthy();
  });

  test('열린 PR 이 있어도 내 차례에는 PR 이 없다 — PR 은 위의 알림이 맡는다', () => {
    renderWithStore(<MineSection />, {
      showAgents: true,
      agents: null,
      prs: [
        {
          repo: 'o/rocky',
          number: 8,
          title: '머지 가능 PR',
          url: 'https://github.com/o/rocky/pull/8',
          state: 'OPEN',
          isDraft: false,
          base: 'main',
          head: 'abc',
          mergeState: 'CLEAN',
          ci: 'pass',
          unhandled: 0,
          decision: 0,
          ready: true,
          updatedAt: '2026-09-28T10:00:00Z',
        },
      ] as never,
    });
    expect(screen.queryByText('머지 가능 PR')).toBeNull();
  });
});
