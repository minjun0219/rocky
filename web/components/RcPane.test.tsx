import { afterEach, describe, expect, mock, test } from 'bun:test';
import { cleanup, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { renderWithStore } from '../test-support';
import type { RcServerRow, RcStatus } from '../types';
import { RcPane, RcSummary, rcCounts, rcNightlyText, rcUptime } from './RcPane';
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

  test('rc 블록이 없는 기기 — Antigravity 줄만, claude 로그인 줄은 없다', () => {
    renderWithStore(<RcPane />, {
      rc: status({ configured: false, servers: [], strays: [], auth: 'unknown' }),
      loadRc,
    });
    expect(screen.getByText('Antigravity')).toBeTruthy();
    expect(screen.queryByText('claude 로그인')).toBeNull();
    expect(screen.getByText(/rc 블록을 두면/)).toBeTruthy();
  });

  test('로컬이면 켜기·끄기 버튼 — 도는 중이면 끄기를 부른다', async () => {
    const controlAgy = mock(async (_action: 'start' | 'stop') => {});
    renderWithStore(<RcPane />, { rc: status(), loadRc, controlAgy, spawnAllowed: true });
    await userEvent.click(screen.getByRole('button', { name: '끄기' }));
    expect(controlAgy).toHaveBeenCalledWith('stop');
    cleanup();
    renderWithStore(<RcPane />, {
      rc: status({ antigravity: { state: 'stopped', instance: 'mac-1' } }),
      loadRc,
      controlAgy,
      spawnAllowed: true,
    });
    await userEvent.click(screen.getByRole('button', { name: '켜기' }));
    expect(controlAgy).toHaveBeenLastCalledWith('start');
  });

  test('노출된 화면에는 버튼이 없고, 실패하면 사유를 줄 아래에 남긴다', async () => {
    renderWithStore(<RcPane />, { rc: status(), loadRc, spawnAllowed: false });
    expect(screen.queryByRole('button', { name: '끄기' })).toBeNull();
    cleanup();
    const controlAgy = mock(async () => {
      throw new Error('agy remote-control stop 실패(종료 코드 1): x');
    });
    renderWithStore(<RcPane />, { rc: status(), loadRc, controlAgy, spawnAllowed: true });
    await userEvent.click(screen.getByRole('button', { name: '끄기' }));
    expect(await screen.findByText(/stop 실패\(종료 코드 1\): x/)).toBeTruthy();
  });

  test('프로브가 실패하면 꺼짐을 믿지 말라고 맨 위에 적는다', () => {
    renderWithStore(<RcPane />, { rc: status({ probeError: 'ps 실패: x' }), loadRc });
    expect(screen.getByText(/ps 실패: x — 꺼짐 표시는 모르는 것이다/)).toBeTruthy();
  });
});

describe('원격 제어 탭', () => {
  test('rc 가 켜졌거나 agy 가 있는 기기에서만, 메뉴에서 끄지 않았을 때만 보인다', () => {
    renderWithStore(<ViewSwitch />, { view: 'feed', notes: [], rc: status(), showRc: true });
    expect(screen.getByRole('button', { name: '원격 제어' })).toBeTruthy();
    cleanup();
    renderWithStore(<ViewSwitch />, {
      view: 'feed',
      notes: [],
      rc: status({ configured: false, antigravity: null }),
      showRc: true,
    });
    expect(screen.queryByRole('button', { name: '원격 제어' })).toBeNull();
    cleanup();
    // rc 블록이 없어도 agy 가 있으면 탭이 있다 — Antigravity 를 켜고 끄는 자리다.
    renderWithStore(<ViewSwitch />, {
      view: 'feed',
      notes: [],
      rc: status({ configured: false, servers: [], strays: [] }),
      showRc: true,
    });
    expect(screen.getByRole('button', { name: '원격 제어' })).toBeTruthy();
    cleanup();
    renderWithStore(<ViewSwitch />, { view: 'feed', notes: [], rc: status(), showRc: false });
    expect(screen.queryByRole('button', { name: '원격 제어' })).toBeNull();
  });
});

describe('원격 제어 — 띄우기 · 재시작', () => {
  test('꺼진 대상은 바로 띄우고, 떠 있는 대상은 한 번 더 묻고 재시작한다', async () => {
    const rcCommand = mock(async () => {});
    renderWithStore(<RcPane />, { rc: status(), loadRc, rcCommand, spawnAllowed: true });
    await userEvent.click(screen.getAllByRole('button', { name: '띄우기' })[0] as HTMLElement);
    expect(rcCommand).toHaveBeenCalledWith('repo-b', 'start');
    await userEvent.click(screen.getByRole('button', { name: '재시작' }));
    expect(screen.getByText('세션 2개가 끊기고 이어받기(-c)로 다시 뜬다')).toBeTruthy();
    expect(rcCommand).toHaveBeenCalledTimes(1);
    await userEvent.click(screen.getByRole('button', { name: '끊고 재시작' }));
    expect(rcCommand).toHaveBeenLastCalledWith('repo-a', 'restart');
  });

  test('대상 밖 서버 · 원격 화면 · 프로브 실패에는 버튼이 없다', () => {
    renderWithStore(<RcPane />, { rc: status(), loadRc, spawnAllowed: false });
    expect(screen.queryByRole('button', { name: /띄우기|재시작/ })).toBeNull();
    expect(screen.getByText(/로컬\(루프백\) 주소로 연 화면에서만/)).toBeTruthy();
    cleanup();
    renderWithStore(<RcPane />, {
      rc: status({ probeError: 'ps 실패' }),
      loadRc,
      spawnAllowed: true,
    });
    expect(screen.queryByRole('button', { name: /띄우기|재시작/ })).toBeNull();
    cleanup();
    renderWithStore(<RcPane />, {
      rc: status({ servers: [] }),
      loadRc,
      spawnAllowed: true,
    });
    // 대상 밖(old) 행만 남았다.
    expect(screen.getByText('old')).toBeTruthy();
    expect(screen.queryByRole('button', { name: /띄우기|재시작/ })).toBeNull();
  });

  test('진행 중이면 버튼 대신 상태, 실패한 결과는 이유를 남긴다', () => {
    const rc = status();
    rc.servers[0] = { ...(rc.servers[0] as RcServerRow), action: 'restarting' };
    rc.servers[2] = {
      ...(rc.servers[2] as RcServerRow),
      lastResult: { ok: false, message: '뜨자마자 내려갔다 — x', at: 't' },
    };
    renderWithStore(<RcPane />, { rc, loadRc, spawnAllowed: true });
    expect(screen.getByText(/재시작 중…/)).toBeTruthy();
    expect(screen.queryByRole('button', { name: '재시작' })).toBeNull();
    expect(screen.getByText('✗ 뜨자마자 내려갔다 — x')).toBeTruthy();
  });
});

describe('원격 제어 — 감시', () => {
  test('감시가 켜져 있으면 요약 줄에 감시 중, 데몬 자격이 끊기면 그 사실을 배지로', () => {
    renderWithStore(<RcSummary />, {
      rc: status({ supervise: { lastTick: 't', loggedOut: false } }),
      loadRc,
    });
    expect(screen.getByText('감시 중')).toBeTruthy();
    cleanup();
    renderWithStore(<RcSummary />, {
      rc: status({ supervise: { loggedOut: true } }),
      loadRc,
    });
    expect(screen.getByText('자격 끊김')).toBeTruthy();
    expect(screen.queryByText('감시 중')).toBeNull();
    cleanup();
    renderWithStore(<RcSummary />, { rc: status(), loadRc });
    expect(screen.queryByText(/감시 중|자격 끊김/)).toBeNull();
  });

  test('자격 의심 서버에는 다시 띄우기를 권하는 줄', () => {
    const rc = status();
    rc.servers[0] = { ...(rc.servers[0] as RcServerRow), authSuspect: true };
    renderWithStore(<RcPane />, { rc, loadRc, spawnAllowed: true });
    expect(screen.getByText(/자격 의심 — 끊기기 전에 떴다/)).toBeTruthy();
  });
});

describe('원격 제어 — 야간 재시작', () => {
  test('구버전 서버는 메타에, 야간 요약은 환경 카드 맨 아래에', () => {
    const rc = status({
      nightly: {
        at: '04:30',
        running: false,
        last: {
          startedAt: '2026-10-07T04:30:00+09:00',
          finishedAt: '2026-10-07T04:31:00+09:00',
          update: '변화 없음',
          items: [
            { label: 'repo-a', outcome: 'restarted', note: '' },
            { label: 'repo-c', outcome: 'down', note: '' },
          ],
        },
      },
    });
    rc.servers[0] = { ...(rc.servers[0] as RcServerRow), stale: true };
    renderWithStore(<RcPane />, { rc, loadRc });
    expect(screen.getByText(/구버전/)).toBeTruthy();
    expect(screen.getByText('야간 재시작 04:30')).toBeTruthy();
    expect(screen.getByText(/재시작 1 · 건너뜀 0 · 못 띄움 1/)).toBeTruthy();
  });

  test('요약 문구 — 도는 중 · 아직 · 전부 건너뜀', () => {
    expect(rcNightlyText({ running: true })).toEqual({ text: '도는 중', down: 0 });
    expect(rcNightlyText({ at: '04:30', running: false }).text).toBe('아직 안 돌았다');
    const now = new Date('2026-10-07T12:00:00');
    expect(
      rcNightlyText(
        {
          running: false,
          last: {
            startedAt: 'x',
            finishedAt: '2026-10-07T04:31:00',
            update: 'u',
            blocked: 'logged-out',
            items: [],
          },
        },
        now,
      ).text,
    ).toBe('마지막 04:31 · 전부 건너뜀(logged-out)');
  });

  test('야간을 켜지 않은 기기에는 그 줄이 없다', () => {
    renderWithStore(<RcPane />, { rc: status(), loadRc });
    expect(screen.queryByText(/야간 재시작/)).toBeNull();
  });
});

describe('RcPane 핸드오프 서버', () => {
  const handoff = {
    label: 'handoff-rocky-41',
    name: 'rocky-41: 핸드오프 작업',
    todoRef: 'rocky-41',
    dir: '/w/rocky/.claude/worktrees/todo-41',
    pid: 500,
    uptimeSecs: 600,
    sessions: 1,
  };

  test('대상 밖 앞에 따로 — 닫기는 한 번 더 묻고 라벨로 닫는다', async () => {
    const closeHandoff = mock(async () => {});
    renderWithStore(<RcPane />, {
      rc: status({ handoffs: [handoff] }),
      loadRc,
      closeHandoff,
      spawnAllowed: true,
    });
    const heads = [...document.querySelectorAll('h2')].map((h) => h.textContent);
    expect(heads).toEqual(['고정1/2', '부를 수 있음0/1', '핸드오프1', '대상 밖1', '환경']);
    expect(screen.getByText('rocky-41: 핸드오프 작업')).toBeTruthy();
    await userEvent.click(screen.getByRole('button', { name: 'rocky-41: 핸드오프 작업 닫기' }));
    expect(closeHandoff).not.toHaveBeenCalled();
    expect(screen.getByText(/세션 1개가 끝나요 — 폴더는 그대로 남아요/)).toBeTruthy();
    await userEvent.click(screen.getByRole('button', { name: '끝내고 닫기' }));
    expect(closeHandoff).toHaveBeenCalledWith('handoff-rocky-41');
  });

  test('노출된 화면에서는 닫기가 없다', () => {
    renderWithStore(<RcPane />, {
      rc: status({ handoffs: [handoff] }),
      loadRc,
      spawnAllowed: false,
    });
    expect(screen.queryByRole('button', { name: /닫기/ })).toBeNull();
  });

  test('현황을 못 읽었으면 닫기가 없다', () => {
    renderWithStore(<RcPane />, {
      rc: status({ handoffs: [handoff], probeError: 'ps 실패' }),
      loadRc,
      spawnAllowed: true,
    });
    expect(screen.queryByRole('button', { name: /닫기/ })).toBeNull();
  });

  test('요약의 세션 수에 핸드오프 서버도 든다', () => {
    expect(rcCounts(status({ handoffs: [handoff] })).sessions).toBe(4);
  });
});

describe('RcPane 대상 밖 서버 닫기', () => {
  test('로컬 화면이면 닫기 — 한 번 더 묻고 pid 로 닫는다', async () => {
    const closeStray = mock(async () => {});
    renderWithStore(<RcPane />, { rc: status(), loadRc, closeStray, spawnAllowed: true });
    await userEvent.click(screen.getByRole('button', { name: 'old 닫기' }));
    expect(closeStray).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole('button', { name: '끝내고 닫기' }));
    expect(closeStray).toHaveBeenCalledWith(9);
  });

  test('노출된 화면이나 현황을 못 읽었으면 닫기가 없다', () => {
    renderWithStore(<RcPane />, { rc: status(), loadRc, spawnAllowed: false });
    expect(screen.queryByRole('button', { name: 'old 닫기' })).toBeNull();
    cleanup();
    renderWithStore(<RcPane />, {
      rc: status({ probeError: 'ps 실패' }),
      loadRc,
      spawnAllowed: true,
    });
    expect(screen.queryByRole('button', { name: 'old 닫기' })).toBeNull();
  });
});
