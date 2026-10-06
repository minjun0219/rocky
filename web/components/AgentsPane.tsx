import { Circle, CircleAlert, CircleCheck, CircleDot, type LucideIcon } from 'lucide-react';
import { useEffect } from 'react';
import { type AgentPhase, type AgentRow, agentSections, agentSince } from '../agents';
import { formatAge } from '../lib';
import { useUiStore } from '../store';
import { logUsage } from '../usage';
import { StateIcon, useNow } from './NowTable';

/** 다시 읽는 간격 — 탭을 보는 동안은 짧게, 아니면 피드의 "내 차례" 몫으로 길게. 데몬이 몇 초 캐시한다. */
const POLL_MS = 15_000;
const IDLE_POLL_MS = 60_000;

/** 상태 아이콘 — `web/DESIGN.md` "State Vocabulary". 사람 답을 기다리는 것만 `mine` 이다. */
const PHASE_ICON: Record<AgentPhase, { Icon: LucideIcon; className: string; label: string }> = {
  blocked: { Icon: CircleAlert, className: 'text-mine', label: '내 차례' },
  run: { Icon: CircleDot, className: 'text-run', label: '실행 중' },
  idle: { Icon: Circle, className: 'text-faint', label: '쉬는 중' },
  done: { Icon: CircleCheck, className: 'text-faint', label: '끝남' },
};

/**
 * 세션 목록 폴링 — 앱 한 곳(`main.tsx`)에서 돈다. 탭을 끄면 돌지 않는다(피드의 에이전트 행도 같이 사라진다).
 * `claude agents --json` 은 데몬이 돌리는 CLI 라 탭 밖에서는 1분에 한 번이면 된다.
 */
export function useAgentsPolling() {
  const enabled = useUiStore((s) => s.showAgents);
  const watching = useUiStore((s) => s.view === 'agents');
  const loadAgents = useUiStore((s) => s.loadAgents);
  useEffect(() => {
    if (!enabled) {
      return;
    }
    void loadAgents();
    const id = setInterval(() => void loadAgents(), watching ? POLL_MS : IDLE_POLL_MS);
    return () => clearInterval(id);
  }, [enabled, watching, loadAgents]);
}

/**
 * 에이전트 — `claude agents` 가 보는 세션(interactive · background)을 내 차례 → 실행 중 → 쉬는 중으로. 보드를 고르면
 * 그 보드의 세션만. 출력·로그는 두지 않고 Claude 가 남긴 요약 한 줄만(`web/DESIGN.md` "Not in the Panel").
 */
export function AgentsPane() {
  const agents = useUiStore((s) => s.agents);
  const boards = useUiStore((s) => s.boards);
  const selected = useUiStore((s) => s.selected);
  const doing = useUiStore((s) => s.nowTodos);
  const sections = agents?.available
    ? agentSections({ sessions: agents.list, boards, selected, doing })
    : [];
  const now = useNow([], sections.length);
  return (
    <main className="min-w-0 flex-1 overflow-y-auto px-4 pb-6 pt-1" aria-label="에이전트">
      {agents === null ? null : !agents.available ? (
        <p className="mt-3 text-meta text-muted" title={agents.reason}>
          세션 목록을 읽지 못했어요{agents.reason ? ` — ${agents.reason}` : ''}
        </p>
      ) : sections.length === 0 ? (
        <p className="mt-3 text-meta text-muted">
          {selected === 'all' ? '떠 있는 에이전트가 없어요' : '이 보드에서 도는 에이전트가 없어요'}
        </p>
      ) : (
        sections.map((section) => (
          <section key={section.group} aria-label={section.title}>
            <h2 className="m-0 mb-2 mt-[18px] flex items-baseline gap-2 font-mono text-chip font-medium text-faint">
              {section.title}
              <span
                className={`rounded-[4px] px-1.5 py-0.2 font-mono text-chip font-semibold tabular-nums ${
                  section.group === 'mine'
                    ? 'bg-mine-soft text-mine'
                    : section.group === 'run'
                      ? 'bg-run-soft text-run'
                      : 'text-faint'
                }`}
              >
                {section.rows.length}
              </span>
            </h2>
            <ul className="m-0 list-none overflow-hidden rounded-lg border border-line bg-surface p-0 shadow-xs">
              {section.rows.map((row) => (
                <AgentItem key={row.session.sessionId} row={row} now={now} />
              ))}
            </ul>
          </section>
        ))
      )}
    </main>
  );
}

/**
 * 한 행 — 첫 줄: 아이콘 + 세션 이름, 둘째 줄: 어디서 · 백그라운드 · 시각 · 든 할 일, 셋째 줄: 기다리는 것(내 차례)
 * 또는 지금 하는 일. 할 일을 든 세션이면 행을 눌러 그 할 일을 연다.
 */
function AgentItem({ row, now }: { row: AgentRow; now: number }) {
  const openTodoDetail = useUiStore((s) => s.openTodoDetail);
  const { session, todo } = row;
  const job = session.job;
  const summary = row.phase === 'blocked' ? (job?.needs ?? job?.detail) : job?.detail;
  const meta = [
    row.place,
    session.kind === 'background' ? '백그라운드' : null,
    formatAge(agentSince(row), now, { since: row.phase !== 'blocked' }),
    row.phase === 'done' ? '끝남' : null,
    todo?.ref,
  ].filter(Boolean);
  const body = (
    <>
      <StateIcon {...PHASE_ICON[row.phase]} />
      <span className="min-w-0 flex-1">
        <span className="now-title block text-sm leading-[1.45] text-text">{session.name}</span>
        <span className="mt-0.5 block truncate font-mono text-chip tabular-nums text-muted">
          {meta.join(' · ')}
        </span>
        {summary ? (
          <span className="mt-1 line-clamp-2 block text-meta text-muted">{summary}</span>
        ) : null}
      </span>
    </>
  );
  const className = 'flex w-full items-start gap-2.5 px-3.5 py-2.5 text-left';
  return (
    <li className="border-t border-line/70 first:border-t-0" title={session.cwd}>
      {todo ? (
        <button
          type="button"
          className={`${className} transition-colors duration-150 hover:bg-surface-2 focus-visible:bg-surface-2`}
          onClick={() => {
            logUsage('web:agent-row');
            void openTodoDetail(todo.id);
          }}
        >
          {body}
        </button>
      ) : (
        <div className={className}>{body}</div>
      )}
    </li>
  );
}
