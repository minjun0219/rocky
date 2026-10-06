import {
  ChevronDown,
  ChevronRight,
  Circle,
  CircleAlert,
  CircleDot,
  CircleHelp,
  Moon,
  Server,
} from 'lucide-react';
import { useEffect, useState } from 'react';
import { formatStamp, rcVisible } from '../lib';
import { useUiStore } from '../store';
import type { RcServerRow, RcStatus, RcStrayRow } from '../types';

/** 화면이 보일 때만 이 간격으로 다시 읽는다 — 데몬이 5초 캐시하고 `ps` 를 돌리는 비용이 있다. */
const POLL_MS = 30_000;

/** 떠 있은 시간 → `12분` · `3시간` · `2일`(1분 미만은 `방금`). `web/DESIGN.md` "Time Display" 의 단위. */
export function rcUptime(secs: number | undefined): string {
  if (secs === undefined) {
    return '';
  }
  if (secs < 60) {
    return '방금';
  }
  if (secs < 3600) {
    return `${Math.floor(secs / 60)}분`;
  }
  if (secs < 86_400) {
    return `${Math.floor(secs / 3600)}시간`;
  }
  return `${Math.floor(secs / 86_400)}일`;
}

/**
 * 요약 줄의 숫자 — 대상 중 실행 수, 꺼진 고정 수, 열린 세션 수(대상 밖 포함). 프로브가 실패했으면(`probeError`)
 * "꺼짐" 은 모르는 것이라 꺼진 고정으로 세지 않는다.
 */
export function rcCounts(rc: RcStatus): {
  running: number;
  total: number;
  pinnedOff: number;
  sessions: number;
} {
  return {
    running: rc.servers.filter((s) => s.running).length,
    total: rc.servers.length,
    pinnedOff: rc.probeError ? 0 : rc.servers.filter((s) => s.pinned && !s.running).length,
    sessions: [...rc.servers, ...rc.strays].reduce((sum, s) => sum + s.sessions, 0),
  };
}

/** 띄우기 · 재시작이 진행 중일 때만 이 간격 — 결과가 곧 바뀐다. */
const BUSY_POLL_MS = 3_000;

/** 마운트돼 있는 동안 rc 현황을 읽는다(처음 한 번 + 30초마다, 진행 중이면 3초마다). */
function useRcPolling(): RcStatus | null {
  const rc = useUiStore((s) => s.rc);
  const loadRc = useUiStore((s) => s.loadRc);
  const busy = rc?.servers.some((s) => s.action !== undefined) ?? false;
  useEffect(() => {
    void loadRc();
    const id = setInterval(() => void loadRc(), busy ? BUSY_POLL_MS : POLL_MS);
    return () => clearInterval(id);
  }, [loadRc, busy]);
  return rc;
}

const ACTION_TEXT: Record<NonNullable<RcServerRow['action']>, string> = {
  starting: '띄우는 중…',
  restarting: '재시작 중…',
  retrying: '다시 시도 중…',
};

/**
 * 행의 주 액션 — 꺼졌으면 띄우기, 떠 있으면 재시작. 재시작은 붙은 원격 세션을 끊고 되돌릴 수 없어 한 번 더 묻는다
 * (같은 자리 아래 한 줄로 — 좁은 패널에 모달을 띄우지 않는다). 띄우기는 바로. 진행 중이면 그리지 않는다.
 */
function useServerAction(row: RcServerRow) {
  const rcCommand = useUiStore((s) => s.rcCommand);
  const [confirming, setConfirming] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const run = async (verb: 'start' | 'restart') => {
    setConfirming(false);
    setError(null);
    try {
      await rcCommand(row.label, verb);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };
  const button = row.action ? null : (
    <button
      type="button"
      className="min-h-8 shrink-0 rounded-md border border-line px-2.5 text-chip text-text hover:bg-surface-2"
      aria-expanded={row.running ? confirming : undefined}
      onClick={() => (row.running ? setConfirming(!confirming) : void run('start'))}
    >
      {row.running ? '재시작' : '띄우기'}
    </button>
  );
  const below =
    confirming && !row.action ? (
      <div className="flex flex-wrap items-center gap-2 px-3.5 pb-2.5 pl-[42px]">
        <span className="text-chip text-muted">
          {row.sessions > 0
            ? `세션 ${row.sessions}개가 끊기고 이어받기(-c)로 다시 뜬다`
            : '다시 띄운다 — 붙은 원격 연결이 끊긴다'}
        </span>
        <button
          type="button"
          className="min-h-8 rounded-md bg-mine-soft px-2.5 text-chip font-semibold text-mine"
          onClick={() => void run('restart')}
        >
          끊고 재시작
        </button>
        <button
          type="button"
          className="tap text-chip text-faint hover:text-text"
          onClick={() => setConfirming(false)}
        >
          취소
        </button>
      </div>
    ) : error ? (
      <p className="mb-0 mt-0 px-3.5 pb-2.5 pl-[42px] text-chip text-mine">{error}</p>
    ) : null;
  return { button, below };
}

function ServerItem(props: {
  row: RcServerRow | RcStrayRow;
  stray?: boolean;
  unknown?: boolean;
  actionable?: boolean;
}) {
  if (props.actionable && 'running' in props.row) {
    return <ActionableItem {...props} row={props.row} />;
  }
  return <ServerItemView {...props} />;
}

function ActionableItem(props: { row: RcServerRow; unknown?: boolean }) {
  const { button, below } = useServerAction(props.row);
  return <ServerItemView {...props} button={button} below={below} />;
}

function ServerItemView({
  row,
  stray,
  unknown,
  button,
  below,
}: {
  row: RcServerRow | RcStrayRow;
  stray?: boolean;
  /** 프로브가 실패했다 — 떠 있지 않은 것으로 나온 행은 "꺼짐" 이 아니라 "모름" 이다. */
  unknown?: boolean;
  /** 행 오른쪽 주 액션과 그 아래 확인 줄 — 로컬 화면의 설정 대상만(대상 밖 서버는 화면이 움직이지 않는다). */
  button?: React.ReactNode;
  below?: React.ReactNode;
}) {
  const action = 'action' in row ? row.action : undefined;
  const failed =
    'lastResult' in row && row.lastResult && !row.lastResult.ok ? row.lastResult : null;
  const pinned = 'pinned' in row && row.pinned;
  const running = !('running' in row) || row.running;
  const unsure = unknown === true && !running;
  const down = pinned && !running && !unsure;
  const meta = [
    pinned ? '고정' : null,
    running ? null : unsure ? '모름' : '꺼짐',
    row.sessions > 0 ? `세션 ${row.sessions}` : null,
    rcUptime(row.uptimeSecs) || null,
    'stale' in row && row.stale ? '구버전' : null,
    stray ? row.dir.replace(/^\/(?:Users|home)\/[^/]+/, '~') : null,
    action ? ACTION_TEXT[action] : null,
  ]
    .filter(Boolean)
    .join(' · ');
  const Icon = unsure ? CircleHelp : down ? CircleAlert : running ? CircleDot : Circle;
  const tone = down ? 'text-mine' : running ? 'text-run' : 'text-faint';
  const label = unsure ? '모름' : down ? '고정인데 꺼짐' : running ? '실행 중' : '꺼짐';
  return (
    <li className="border-t border-line/70 first:border-t-0">
      <div className="flex w-full items-center gap-2.5 px-3.5 py-2.5" title={row.dir}>
        <span
          className={`mt-0.5 flex w-4 shrink-0 justify-center ${tone}`}
          role="img"
          aria-label={label}
        >
          <Icon size={14} aria-hidden="true" />
        </span>
        <span className="min-w-0 flex-1">
          <span className={`block text-sm leading-[1.45] text-text ${down ? 'font-semibold' : ''}`}>
            {row.label}
          </span>
          <span
            className={`mt-0.5 block truncate font-mono text-chip tabular-nums ${down ? 'text-mine' : 'text-muted'}`}
          >
            {meta}
          </span>
          {failed && !action ? (
            <span className="mt-0.5 block text-chip text-mine">✗ {failed.message}</span>
          ) : null}
          {'authSuspect' in row && row.authSuspect && !action ? (
            <span className="mt-0.5 block text-chip text-mine">
              자격 의심 — 끊기기 전에 떴다, 다시 띄우기를 권한다
            </span>
          ) : null}
        </span>
        {button}
      </div>
      {below}
    </li>
  );
}

function Card({ children }: { children: React.ReactNode }) {
  return (
    <ul className="m-0 list-none overflow-hidden rounded-lg border border-line bg-surface p-0 shadow-xs">
      {children}
    </ul>
  );
}

function Head({ name, count, tone }: { name: string; count?: string; tone?: 'run' | 'mine' }) {
  const badge =
    tone === 'run'
      ? 'bg-run-soft text-run'
      : tone === 'mine'
        ? 'bg-mine-soft text-mine'
        : 'text-faint';
  return (
    <h2 className="m-0 mb-2 mt-[18px] flex items-baseline gap-2 font-mono text-chip font-medium text-faint">
      {name}
      {count ? (
        <span
          className={`rounded-[4px] px-1.5 py-0.2 font-mono text-chip font-semibold tabular-nums ${badge}`}
        >
          {count}
        </span>
      ) : null}
    </h2>
  );
}

/**
 * 피드 위 요약 한 줄 — `원격 제어 7/13 · 세션 4`. 고정 서버가 꺼졌으면 그 수를 `mine` 색으로 붙인다. 누르면 고정
 * 서버만 펼치고 전체는 원격 제어 탭으로 보낸다(늘 보이는 머리라 길어지지 않게). rc 가 꺼진 기기면 아무것도 없다.
 */
export function RcSummary() {
  const rc = useRcPolling();
  const showRc = useUiStore((s) => s.showRc);
  const setView = useUiStore((s) => s.setView);
  const [open, setOpen] = useState(false);
  if (!rc?.configured) {
    return null;
  }
  const { running, total, pinnedOff, sessions } = rcCounts(rc);
  const Chevron = open ? ChevronDown : ChevronRight;
  return (
    <section className="border-b border-line px-4 py-2" aria-label="원격 제어">
      <button
        type="button"
        className="flex min-h-8 w-full items-center gap-2 rounded-md text-left"
        aria-expanded={open}
        onClick={() => setOpen(!open)}
      >
        <Server size={14} className="text-faint" aria-hidden="true" />
        <span className="font-mono text-chip font-medium text-faint">원격 제어</span>
        <span className="rounded-[4px] bg-run-soft px-1.5 py-0.2 font-mono text-chip font-semibold tabular-nums text-run">
          {running}/{total}
        </span>
        {pinnedOff > 0 ? (
          <span className="rounded-[4px] bg-mine-soft px-1.5 py-0.2 font-mono text-chip font-semibold tabular-nums text-mine">
            고정 {pinnedOff} 꺼짐
          </span>
        ) : null}
        <span className="font-mono text-chip tabular-nums text-muted">세션 {sessions}</span>
        {rc.probeError ? <span className="font-mono text-chip text-mine">⚠ 모름</span> : null}
        {rc.supervise?.loggedOut ? (
          // 감시가 되살리지 못하는 상태 — 셸은 로그인돼 있어도 데몬 맥락(키체인)은 따로다.
          <span className="rounded-[4px] bg-mine-soft px-1.5 py-0.2 font-mono text-chip font-semibold text-mine">
            자격 끊김
          </span>
        ) : rc.supervise ? (
          <span className="font-mono text-chip text-faint">감시 중</span>
        ) : null}
        <Chevron size={14} className="ml-auto text-faint" aria-hidden="true" />
      </button>
      {open ? (
        <div className="mb-1 mt-1.5">
          <Card>
            {rc.servers
              .filter((s) => s.pinned)
              .map((s) => (
                <ServerItem key={s.dir} row={s} unknown={Boolean(rc.probeError)} />
              ))}
            {showRc ? (
              <li className="border-t border-line/70 first:border-t-0">
                <button
                  type="button"
                  className="w-full px-3.5 py-2.5 text-left text-chip text-link"
                  onClick={() => setView('rc')}
                >
                  원격 제어 탭에서 전체 {rc.servers.length + rc.strays.length}개 보기 ›
                </button>
              </li>
            ) : null}
          </Card>
        </div>
      ) : null}
    </section>
  );
}

/**
 * 야간 재시작 요약 — `마지막 10-07 04:31 · 재시작 3 · 건너뜀 2 · 못 띄움 1`. 도는 중이거나 아직 안 돌았으면 그 말.
 * 못 띄운 서버는 감시가 되살리지만 사람이 알아야 하니 수를 따로 센다.
 */
export function rcNightlyText(
  nightly: NonNullable<RcStatus['nightly']>,
  now = new Date(),
): {
  text: string;
  down: number;
} {
  if (nightly.running) {
    return { text: '도는 중', down: 0 };
  }
  const last = nightly.last;
  if (!last) {
    return { text: '아직 안 돌았다', down: 0 };
  }
  const when = last.finishedAt ? formatStamp(last.finishedAt, now) : '?';
  if (last.blocked) {
    return { text: `마지막 ${when} · 전부 건너뜀(${last.blocked})`, down: 0 };
  }
  const count = (o: string) => last.items.filter((i) => i.outcome === o).length;
  const down = count('down');
  const parts = [`마지막 ${when}`, `재시작 ${count('restarted')}`, `건너뜀 ${count('skipped')}`];
  if (down > 0) {
    parts.push(`못 띄움 ${down}`);
  }
  return { text: parts.join(' · '), down };
}

function NightlyItem({ nightly }: { nightly: NonNullable<RcStatus['nightly']> }) {
  const { text, down } = rcNightlyText(nightly);
  return (
    <li className="flex items-center gap-2.5 border-t border-line/70 px-3.5 py-2.5">
      <span className={down > 0 ? 'text-mine' : 'text-faint'}>
        <Moon size={14} aria-hidden="true" />
      </span>
      <span className="min-w-0 flex-1">
        <span className="block text-sm text-text">
          야간 재시작{nightly.at ? ` ${nightly.at}` : ''}
        </span>
        <span
          className={`mt-0.5 block truncate font-mono text-chip tabular-nums ${down > 0 ? 'text-mine' : 'text-muted'}`}
        >
          {text}
        </span>
      </span>
    </li>
  );
}

/**
 * Antigravity 줄 — 상태와, 로컬 요청이면 켜기·끄기 버튼 하나. 끄면 이 기계로 들어오던 원격 세션이 끊기므로
 * 노출된 화면(폰·테일넷)에는 버튼이 없다(데몬도 403). 실패하면 사유를 줄 아래에 남긴다.
 */
function AgyItem({ agy, first }: { agy: NonNullable<RcStatus['antigravity']>; first: boolean }) {
  const local = useUiStore((s) => s.spawnAllowed);
  const controlAgy = useUiStore((s) => s.controlAgy);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // 설치돼 응답했다는 것과 도는 것은 다르다 — 데몬이 멈춰 있어도 객체는 온다.
  const running = agy.state === 'running';
  const toggle = async () => {
    setBusy(true);
    setError(null);
    try {
      await controlAgy(running ? 'stop' : 'start');
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <li className={`px-3.5 py-2.5 ${first ? '' : 'border-t border-line/70'}`}>
      <div className="flex min-w-0 items-center gap-2.5">
        <span className={running ? 'text-run' : 'text-faint'}>
          {running ? (
            <CircleDot size={14} aria-hidden="true" />
          ) : (
            <Circle size={14} aria-hidden="true" />
          )}
        </span>
        <span className="shrink-0 text-sm text-text">Antigravity</span>
        <span className="ml-auto min-w-0 truncate font-mono text-chip text-muted">
          {[agy.state ?? '꺼짐', agy.instance].filter(Boolean).join(' · ')}
        </span>
        {local ? (
          <button
            type="button"
            className="shrink-0 rounded-md border border-line px-2 py-1 text-meta text-muted hover:border-mine hover:text-text disabled:opacity-60"
            disabled={busy}
            onClick={toggle}
          >
            {busy ? '바꾸는 중' : running ? '끄기' : '켜기'}
          </button>
        ) : null}
      </div>
      {error ? <p className="mb-0 mt-1.5 font-mono text-chip text-mine">⚠ {error}</p> : null}
    </li>
  );
}

/**
 * 원격 제어 탭 — 고정 → 부를 수 있음 → 대상 밖 → 환경(로그인 · Antigravity). claude rc 는 보기만 한다: 목록은
 * `rocky.json` 의 `rc` 블록에서 고치고(설정 화면을 패널에 두지 않는다), 띄우기·재시작은 다음 조각이다. rc 블록이
 * 없는 기기에서도 agy 가 있으면 Antigravity 줄만 보인다.
 */
export function RcPane() {
  const rc = useRcPolling();
  const spawnAllowed = useUiStore((s) => s.spawnAllowed);
  if (!rc) {
    return <p className="px-4 py-6 text-chip text-faint">원격 제어 현황을 읽는 중…</p>;
  }
  if (!rcVisible(rc)) {
    return null;
  }
  if (!rc.configured) {
    return (
      <main className="min-w-0 flex-1 overflow-y-auto px-4 pb-6 pt-1" aria-label="원격 제어">
        <Head name="환경" />
        <Card>{rc.antigravity ? <AgyItem agy={rc.antigravity} first /> : null}</Card>
        <p className="mt-3 text-chip text-faint">
          claude rc 서버는 rocky.json 에 rc 블록을 두면 여기 보인다
        </p>
      </main>
    );
  }
  // 프로브가 실패한 현황으로는 띄우지 않는다 — 떠 있는 서버를 하나 더 띄운다(데몬도 같은 이유로 거절한다).
  const actionable = spawnAllowed && !rc.probeError;
  const pinned = rc.servers.filter((s) => s.pinned);
  const others = rc.servers
    .filter((s) => !s.pinned)
    .sort((a, b) => Number(b.running) - Number(a.running));
  const pinnedUp = pinned.filter((s) => s.running).length;
  const auth = rc.auth === 'in' ? '로그인됨' : rc.auth === 'out' ? '로그아웃' : '모름';
  return (
    <main className="min-w-0 flex-1 overflow-y-auto px-4 pb-6 pt-1" aria-label="원격 제어">
      {rc.probeError ? (
        <p className="mb-0 mt-3 font-mono text-chip text-mine">
          ⚠ {rc.probeError} — 꺼짐 표시는 모르는 것이다
        </p>
      ) : null}
      {pinned.length > 0 ? (
        <>
          <Head
            name="고정"
            count={`${pinnedUp}/${pinned.length}`}
            tone={pinnedUp < pinned.length && !rc.probeError ? 'mine' : 'run'}
          />
          <Card>
            {pinned.map((s) => (
              <ServerItem
                key={s.dir}
                row={s}
                unknown={Boolean(rc.probeError)}
                actionable={actionable}
              />
            ))}
          </Card>
        </>
      ) : null}
      {others.length > 0 ? (
        <>
          <Head
            name="부를 수 있음"
            count={`${others.filter((s) => s.running).length}/${others.length}`}
          />
          <Card>
            {others.map((s) => (
              <ServerItem
                key={s.dir}
                row={s}
                unknown={Boolean(rc.probeError)}
                actionable={actionable}
              />
            ))}
          </Card>
        </>
      ) : null}
      {rc.strays.length > 0 ? (
        <>
          <Head name="대상 밖" count={String(rc.strays.length)} />
          <Card>
            {rc.strays.map((s) => (
              <ServerItem key={s.pid} row={s} stray />
            ))}
          </Card>
        </>
      ) : null}
      <Head name="환경" />
      <Card>
        <li className="flex items-center gap-2.5 px-3.5 py-2.5">
          <span
            className={
              rc.auth === 'out' ? 'text-mine' : rc.auth === 'in' ? 'text-run' : 'text-faint'
            }
          >
            {rc.auth === 'out' ? (
              <CircleAlert size={14} aria-hidden="true" />
            ) : (
              <CircleDot size={14} aria-hidden="true" />
            )}
          </span>
          <span className="text-sm text-text">claude 로그인</span>
          <span className="ml-auto font-mono text-chip text-muted">{auth}</span>
        </li>
        {rc.antigravity ? <AgyItem agy={rc.antigravity} first={false} /> : null}
        {rc.nightly ? <NightlyItem nightly={rc.nightly} /> : null}
      </Card>
      <p className="mt-3 text-chip text-faint">
        목록은 rocky.json 의 rc 블록에서 고친다
        {spawnAllowed ? '' : ' · 띄우기·재시작은 로컬(루프백) 주소로 연 화면에서만'}
      </p>
    </main>
  );
}
