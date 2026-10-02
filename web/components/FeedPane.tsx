import {
  ALERT_LABEL,
  type AlertKind,
  type AlertRow,
  alertRows,
  formatAge,
  mineCount,
} from '../lib';
import { useUiStore } from '../store';
import { logUsage } from '../usage';
import { HideButton } from './HideButton';
import { MineSection, PR_ICON, StateIcon, useNow, useNowRows } from './NowTable';

const ICON: Record<AlertKind, Parameters<typeof StateIcon>[0]> = {
  decide: PR_ICON.decide,
  merge: PR_ICON.ready,
  conflict: PR_ICON.conflict,
  ci: PR_ICON.failing,
};

/**
 * 피드 — 첫 화면. 오너가 손댈 것을 한곳에 모은다(2026-10-02 오너, 알림 탭을 피드로 바꿈):
 * 위는 PR(결정 필요·머지 후보는 바로, 충돌·CI 실패는 세션이 먼저 풀 틈을 주고 30분 뒤에 — `alertRows`),
 * 아래는 내 차례(넘김·멈춘 진행·읽지 않은 댓글·수집함 — 예전 할 일 화면의 "지금" 표에서 옮겼다).
 * PR 은 보드를 고르면 그 보드의 레포 것만, 내 차례는 늘 전 보드. PR 의 × 는 GitHub 탭과 같은 숨김 목록을
 * 쓴다 — 종류별이라 상태가 바뀌면 다시 뜬다.
 */
export function FeedPane() {
  const rows = useAlertRows();
  const now = useNow([], 1);
  const hide = useUiStore((s) => s.hideGithub);
  const kinds = [...new Set(rows.map((r) => r.kind))];

  return (
    <main className="min-w-0 flex-1 overflow-y-auto px-4 py-3" aria-label="피드">
      {kinds.map((kind) => {
        const group = rows.filter((r) => r.kind === kind);
        return (
          <section key={kind} className="mb-[26px]" aria-label={ALERT_LABEL[kind]}>
            <h2 className="m-0 mb-1.5 flex items-baseline gap-2 font-mono text-chip font-medium text-faint">
              {ALERT_LABEL[kind]}
              <span className="tabular-nums text-run">{group.length}</span>
            </h2>
            <ul className="m-0 list-none overflow-hidden rounded-[10px] border border-line bg-surface p-0">
              {group.map((row) => (
                <AlertItem key={row.key} row={row} now={now} onHide={() => hide(row.hideKey)} />
              ))}
            </ul>
          </section>
        );
      })}
      <MineSection />
    </main>
  );
}

/** 지금 보드의 PR 알림 — 탭 배지와 화면이 같은 값을 본다. */
export function useAlertRows(): AlertRow[] {
  const prs = useUiStore((s) => s.prs);
  const boards = useUiStore((s) => s.boards);
  const selected = useUiStore((s) => s.selected);
  const hidden = useUiStore((s) => s.githubHidden);
  // 늘 1분마다 — 충돌·CI 실패가 30분을 넘기는 순간 올라와야 한다.
  const now = useNow([], 1);
  const board = selected === 'all' ? null : boards.find((b) => b.key === selected);
  const scoped = board ? prs.filter((p) => p.repo === board.repo) : prs;
  return alertRows(scoped, hidden, now);
}

/** 피드 탭 옆 숫자 — PR 알림 + 내 차례. */
export function useFeedCount(): number {
  return useAlertRows().length + mineCount(useNowRows());
}

function AlertItem(props: { row: AlertRow; now: number; onHide: () => void }) {
  const { row, now, onHide } = props;
  return (
    <li className="flex items-start border-t border-line first:border-t-0">
      <a
        className="now-item flex min-w-0 flex-1 items-start gap-2 px-3 py-2 text-left no-underline hover:bg-surface-2 focus-visible:bg-surface-2"
        href={row.url}
        target="_blank"
        rel="noreferrer"
        onClick={() => logUsage('web:alert-open', { kind: row.kind })}
      >
        <StateIcon {...ICON[row.kind]} />
        <span className="min-w-0 flex-1">
          <span className="now-title block text-sm leading-[1.45] text-text">{row.title}</span>
          <span className="mt-0.5 block truncate font-mono text-chip tabular-nums text-muted">
            {[row.detail, formatAge(row.at, now)].join(' · ')}
          </span>
        </span>
      </a>
      <HideButton label={`${row.detail} 숨기기`} onClick={onHide} />
    </li>
  );
}
