import {
  CircleAlert,
  CircleX,
  GitMerge,
  GitPullRequest,
  GitPullRequestDraft,
  type LucideIcon,
  TriangleAlert,
} from 'lucide-react';
import { useEffect, useState } from 'react';
import { formatAge, mineCount, type NowRow, nowRows, type PrStatus } from '../lib';
import { useUiStore } from '../store';
import { logUsage } from '../usage';

/**
 * 현재 시각 — 시각을 보여 주는 행이 있으면 1분마다 다시 잰다. 곁눈으로 보는 화면이라 움직이는 숫자는 정말 필요한
 * 자리에만 둔다(`web/DESIGN.md` "Time Display").
 */
export function useNow(rows: readonly unknown[], extra: number): number {
  const [now, setNow] = useState(() => Date.now());
  const visible = rows.length + extra;
  useEffect(() => {
    if (visible === 0) {
      return;
    }
    const id = setInterval(() => setNow(Date.now()), 60_000);
    return () => clearInterval(id);
  }, [visible]);
  return now;
}

/** PR 상태 아이콘 — 머지 가능·충돌은 내 차례 색, 대기·초안은 무채색. */
export const PR_ICON: Record<PrStatus, { Icon: LucideIcon; className: string; label: string }> = {
  conflict: { Icon: TriangleAlert, className: 'text-dead', label: '충돌' },
  ready: { Icon: GitMerge, className: 'text-mine', label: '머지 후보' },
  failing: { Icon: CircleX, className: 'text-dead', label: 'CI 실패' },
  decide: { Icon: CircleAlert, className: 'text-mine', label: '결정 필요' },
  waiting: { Icon: GitPullRequest, className: 'text-muted', label: '대기' },
  draft: { Icon: GitPullRequestDraft, className: 'text-faint', label: '초안' },
};

const MINE_ICON = { Icon: CircleAlert, className: 'text-mine', label: '내 차례' };

/**
 * 행 앞 상태 아이콘 — 색만으로 말하지 않도록 모양이 상태마다 다르다(`web/DESIGN.md` "State
 * Vocabulary"). lucide 아이콘이라 글꼴에 따라 모양이 흔들리지 않는다. 움직이지 않는다.
 */
export function StateIcon(props: { Icon: LucideIcon; className: string; label: string }) {
  const { Icon } = props;
  return (
    <span
      className={`mt-0.5 flex w-4 shrink-0 justify-center ${props.className}`}
      role="img"
      aria-label={props.label}
    >
      <Icon size={14} strokeWidth={2.25} aria-hidden />
    </span>
  );
}

/** 내 차례의 행 — 답을 기다리는 에이전트(에이전트 탭을 켰을 때만). 순서는 `nowRows`(순수)가 정한다. */
export function useNowRows(expanded = false): NowRow[] {
  const agents = useUiStore((s) => (s.showAgents ? s.agents?.list : undefined));
  return nowRows({ agents, expanded });
}

/** 피드의 "내 차례" — 사람 답을 기다리는 에이전트. 누르면 에이전트 탭에서 무엇을 기다리는지와 나머지 세션을 같이 본다. */
export function MineSection() {
  const [expanded, setExpanded] = useState(false);
  const rows = useNowRows(expanded);
  const now = useNow(rows, 0);
  const setView = useUiStore((s) => s.setView);
  const count = mineCount(rows);
  return (
    <section className="mb-6" aria-label="내 차례">
      <h2 className="m-0 mb-2 flex items-baseline gap-2 font-mono text-chip font-medium text-faint">
        내 차례
        {count > 0 ? (
          <span className="rounded-[4px] bg-mine-soft px-1.5 py-0.2 font-mono text-chip font-semibold tabular-nums text-mine">
            {count}
          </span>
        ) : null}
      </h2>
      {rows.length === 0 ? (
        <p className="m-0 text-meta text-muted">내 차례 없음</p>
      ) : (
        <ul className="m-0 list-none overflow-hidden rounded-lg border border-line bg-surface p-0 shadow-xs">
          {rows.map((row) =>
            row.group === 'more' ? (
              <li key={row.key} className="border-t border-line/70 first:border-t-0">
                <button
                  type="button"
                  className="w-full px-3.5 py-2 text-left transition-colors duration-150 hover:bg-surface-2"
                  onClick={() => setExpanded(true)}
                >
                  <span className="font-mono text-chip font-medium text-muted">{row.title}</span>
                </button>
              </li>
            ) : (
              <li key={row.key} className="border-t border-line/70 first:border-t-0">
                <button
                  type="button"
                  className="now-item flex w-full items-start gap-2.5 px-3.5 py-2.5 text-left no-underline transition-colors duration-150 hover:bg-surface-2 focus-visible:bg-surface-2"
                  onClick={() => {
                    logUsage('web:now-row', { kind: row.kind });
                    setView('agents');
                  }}
                >
                  <StateIcon {...MINE_ICON} />
                  <span className="min-w-0 flex-1">
                    <span className="now-title block text-sm leading-[1.45] text-text">
                      {row.title}
                    </span>
                    <span className="mt-0.5 block truncate font-mono text-chip tabular-nums text-muted">
                      {[row.ref, row.since ? formatAge(row.since, now) : '', row.state]
                        .filter(Boolean)
                        .join(' · ')}
                    </span>
                  </span>
                </button>
              </li>
            ),
          )}
        </ul>
      )}
    </section>
  );
}
