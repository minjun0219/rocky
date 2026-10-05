import { afterEach, describe, expect, mock, test } from 'bun:test';
import { cleanup, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { renderWithStore } from '../test-support';
import type { RcStatus } from '../types';
import { RcPane, RcSummary, rcCounts, rcUptime } from './RcPane';
import { ViewSwitch } from './ViewSwitch';

afterEach(cleanup);

const loadRc = mock(async () => {});

const status = (over: Partial<RcStatus> = {}): RcStatus => ({
  configured: true,
  servers: [
    {
      label: 'repo-a',
      dir: '/w/repo-a',
      pinned: true,
      running: true,
      pid: 1,
      uptimeSecs: 7200,
      sessions: 2,
    },
    { label: 'repo-b', dir: '/w/repo-b', pinned: true, running: false, sessions: 0 },
    { label: 'repo-c', dir: '/w/repo-c', pinned: false, running: false, sessions: 0 },
  ],
  strays: [{ label: 'old', dir: '/Users/u/w/old', pid: 9, uptimeSecs: 90_000, sessions: 1 }],
  auth: 'in',
  antigravity: { state: 'running', instance: 'mac-1' },
  ...over,
});

describe('rc 숫자', () => {
  test('떠 있은 시간은 단위 하나', () => {
    expect(rcUptime(undefined)).toBe('');
    expect(rcUptime(30)).toBe('방금');
    expect(rcUptime(600)).toBe('10분');
    expect(rcUptime(7200)).toBe('2시간');
    expect(rcUptime(90_000)).toBe('1일');
  });

  test('요약 수는 대상 기준, 세션은 대상 밖까지', () => {
    expect(rcCounts(status())).toEqual({ running: 1, total: 3, pinnedOff: 1, sessions: 3 });
  });
});

describe('RcSummary', () => {
  test('꺼진 기기에서는 아무것도 그리지 않는다', () => {
    const { container } = renderWithStore(<RcSummary />, {
      rc: status({ configured: false, servers: [], strays: [] }),
      loadRc,
    });
    expect(container.textContent).toBe('');
  });

  test('한 줄에 실행 수 · 꺼진 고정 · 세션, 누르면 고정만 펼치고 탭으로 보낸다', async () => {
    const setView = mock(() => {});
    renderWithStore(<RcSummary />, { rc: status(), loadRc, setView, showRc: true });
    const toggle = screen.getByRole('button', { expanded: false });
    expect(toggle.textContent).toContain('1/3');
    expect(toggle.textContent).toContain('고정 1 꺼짐');
    expect(toggle.textContent).toContain('세션 3');
    await userEvent.click(toggle);
    expect(screen.getByText('repo-b')).toBeTruthy();
    expect(screen.queryByText('repo-c')).toBeNull();
    await userEvent.click(screen.getByRole('button', { name: /전체 4개 보기/ }));
    expect(setView).toHaveBeenCalledWith('rc');
  });
});

describe('RcPane', () => {
  test('고정 → 부를 수 있음 → 대상 밖 → 환경 순서, 꺼진 고정은 경고로', () => {
    renderWithStore(<RcPane />, { rc: status(), loadRc });
    const heads = [...document.querySelectorAll('h2')].map((h) => h.textContent);
    expect(heads).toEqual(['고정1/2', '부를 수 있음0/1', '대상 밖1', '환경']);
    expect(screen.getByRole('img', { name: '고정인데 꺼짐' })).toBeTruthy();
    // 아직 없는 감시를 약속하지 않는다.
    expect(document.body.textContent).not.toContain('감시');
    expect(screen.getByText(/~\/w\/old/)).toBeTruthy();
    expect(screen.getByText('로그인됨')).toBeTruthy();
    expect(screen.getByText('running · mac-1')).toBeTruthy();
  });

  test('프로브가 실패하면 떠 있지 않은 행은 꺼짐이 아니라 모름 — 꺼진 고정 경고도 없다', () => {
    const rc = status({ probeError: 'ps 실패: x' });
    expect(rcCounts(rc).pinnedOff).toBe(0);
    renderWithStore(<RcPane />, { rc, loadRc });
    expect(screen.queryByRole('img', { name: '고정인데 꺼짐' })).toBeNull();
    expect(screen.getAllByRole('img', { name: '모름' }).length).toBe(2);
  });

  test('Antigravity 는 running 일 때만 실행 표시 — 꺼진 데몬도 객체는 온다', () => {
    renderWithStore(<RcPane />, {
      rc: status({ antigravity: { state: 'stopped', instance: 'mac-1' } }),
      loadRc,
    });
    const row = screen.getByText('Antigravity').closest('li');
    expect(row?.querySelector('.text-run')).toBeNull();
    expect(row?.textContent).toContain('stopped · mac-1');
  });

  test('프로브가 실패하면 꺼짐을 믿지 말라고 맨 위에 적는다', () => {
    renderWithStore(<RcPane />, { rc: status({ probeError: 'ps 실패: x' }), loadRc });
    expect(screen.getByText(/ps 실패: x — 꺼짐 표시는 모르는 것이다/)).toBeTruthy();
  });
});

describe('원격 제어 탭', () => {
  test('rc 가 켜진 기기에서만, 메뉴에서 끄지 않았을 때만 보인다', () => {
    renderWithStore(<ViewSwitch />, { view: 'feed', notes: [], rc: status(), showRc: true });
    expect(screen.getByRole('button', { name: '원격 제어' })).toBeTruthy();
    cleanup();
    renderWithStore(<ViewSwitch />, {
      view: 'feed',
      notes: [],
      rc: status({ configured: false }),
      showRc: true,
    });
    expect(screen.queryByRole('button', { name: '원격 제어' })).toBeNull();
    cleanup();
    renderWithStore(<ViewSwitch />, { view: 'feed', notes: [], rc: status(), showRc: false });
    expect(screen.queryByRole('button', { name: '원격 제어' })).toBeNull();
  });
});
