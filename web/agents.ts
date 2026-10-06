/**
 * 에이전트 탭 — `claude agents` 의 세션 목록(`GET /api/sessions`)을 화면 묶음으로 나누는 순수 판정.
 */
import type { BoardSelection } from './route';
import type { AgentSession, Board, SessionRow, TodoView } from './types';

/**
 * 세션 한 줄의 상태. `blocked` 는 background 세션이 사람 답을 기다리는 것(내 차례), `done` 은 끝난 background
 * 세션이 잠시 목록에 남은 것이다. interactive 세션의 `idle` 은 사람이 칠 차례지만 터미널(cmux)이 이미 알리므로
 * 내 차례로 올리지 않는다.
 */
export type AgentPhase = 'blocked' | 'run' | 'idle' | 'done';

export function agentPhase(session: AgentSession): AgentPhase {
  if (session.state === 'blocked') {
    return 'blocked';
  }
  if (session.state === 'done') {
    return 'done';
  }
  return session.status === 'busy' || session.state === 'working' ? 'run' : 'idle';
}

/**
 * `claude attach/stop` 에 넘길 수 있는 짧은 id 인가 — CLI 출력에서 온 값이라 셸 명령으로 복사하기 전에 영숫자 64자 이하만
 * 받는다(데몬 `rocky_core::sessions::is_safe_short_id` 와 같은 규칙).
 */
export function isSafeShortId(id: string): boolean {
  return /^[A-Za-z0-9]{1,64}$/.test(id);
}

function isUnder(cwd: string, root: string): boolean {
  const base = root.replace(/\/+$/, '');
  return base !== '' && (cwd === base || cwd.startsWith(`${base}/`));
}

/**
 * 세션이 붙은 보드 — 데몬의 `board_key_for_cwd` 와 같은 순서: cwd 가 보드 `path` 아래인 것 중 가장 긴 경로, 없으면
 * key(옛 key 포함)가 cwd 의 경로 세그먼트인 것 중 가장 긴 key. 처음 맞는 것을 고르면 보드 생성 순서에 따라
 * 바깥 보드나 우연히 같은 이름의 상위 폴더로 갈 수 있다(basename 만 보면 워크트리를 놓친다).
 */
export function boardOfSession(cwd: string, boards: Board[]): Board | undefined {
  const longest = <T>(items: [T, number][]) =>
    items.reduce<[T, number] | undefined>((a, b) => (a && a[1] >= b[1] ? a : b), undefined)?.[0];
  const byPath = longest(
    boards.flatMap((b): [Board, number][] =>
      b.path !== undefined && isUnder(cwd, b.path) ? [[b, b.path.replace(/\/+$/, '').length]] : [],
    ),
  );
  if (byPath) {
    return byPath;
  }
  const segments = cwd.split('/');
  return longest(
    boards.flatMap((b): [Board, number][] =>
      [b.key, ...(b.previousKeys ?? [])]
        .filter((k) => k !== '' && segments.includes(k))
        .map((k): [Board, number] => [b, k.length]),
    ),
  );
}

/** 보드가 없는 세션의 이름표 — 레포 폴더 이름. 워크트리(`<레포>/.claude/worktrees/<이름>`)는 레포로 접는다. */
export function repoLabel(cwd: string): string {
  const root = cwd.split('/.claude/worktrees/')[0] ?? cwd;
  return root.split('/').filter(Boolean).pop() ?? cwd;
}

export interface AgentRow {
  session: SessionRow;
  phase: AgentPhase;
  /** 어디서 도나 — 보드 key, 없으면 레포 폴더 이름. */
  place: string;
  /** 이 세션이 든 진행 중 할 일 — 핸드오프로 띄운 세션은 짧은 id 로 귀속된다. */
  todo?: TodoView;
}

export interface AgentSection {
  group: 'mine' | 'run' | 'rest';
  title: string;
  rows: AgentRow[];
}

/** 시각 하나로 — 내 차례는 기다리기 시작한 때(요약을 고친 때), 나머지는 시작한 때. */
export function agentSince(row: AgentRow): string {
  const updated = row.phase === 'blocked' ? row.session.job?.updatedAt : undefined;
  return updated ?? new Date(row.session.startedAt).toISOString();
}

/**
 * 탭의 묶음 — 내 차례(blocked) → 실행 중 → 쉬는 중(idle·done). 보드를 고르면 그 보드의 세션만, 묶음 안은 최근 것이
 * 위다. 빈 묶음은 빠진다.
 */
export function agentSections(input: {
  sessions: SessionRow[];
  boards: Board[];
  selected: BoardSelection;
  /** 진행 중 할 일(전 보드) — 세션 귀속(`doingSessionId`)으로 행에 붙인다. */
  doing: TodoView[];
}): AgentSection[] {
  const rows: AgentRow[] = [];
  for (const session of input.sessions) {
    const board = boardOfSession(session.cwd, input.boards);
    if (input.selected !== 'all' && board?.key !== input.selected) {
      continue;
    }
    const todo = input.doing.find(
      (t) =>
        t.status === 'doing' &&
        t.doingSessionId !== undefined &&
        (t.doingSessionId === session.sessionId || t.doingSessionId === session.id),
    );
    rows.push({
      session,
      phase: agentPhase(session),
      place: board?.key ?? repoLabel(session.cwd),
      ...(todo ? { todo } : {}),
    });
  }
  const byRecent = (a: AgentRow, b: AgentRow) =>
    Date.parse(agentSince(b)) - Date.parse(agentSince(a));
  const sections: AgentSection[] = [
    { group: 'mine', title: '내 차례', rows: rows.filter((r) => r.phase === 'blocked') },
    { group: 'run', title: '실행 중', rows: rows.filter((r) => r.phase === 'run') },
    {
      group: 'rest',
      title: '쉬는 중',
      rows: rows.filter((r) => r.phase === 'idle' || r.phase === 'done'),
    },
  ];
  return sections
    .map((s) => ({ ...s, rows: [...s.rows].sort(byRecent) }))
    .filter((s) => s.rows.length > 0);
}
