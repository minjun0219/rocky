import { CircleOff } from 'lucide-react';
import { ALERT_LABEL, type AlertKind, type AlertRow, alertRows, formatAge } from '../lib';
import { useUiStore } from '../store';
import { logUsage } from '../usage';
import { HideButton } from './HideButton';
import { PR_ICON, StateIcon, useNow } from './NowTable';

const ICON: Record<AlertKind, Parameters<typeof StateIcon>[0]> = {
  decide: PR_ICON.decide,
  merge: PR_ICON.ready,
  conflict: PR_ICON.conflict,
  ci: PR_ICON.failing,
  abandoned: { Icon: CircleOff, className: 'text-dead', label: '세션 없음' },
};

/**
 * 알림 — 오너가 손댈 것만 모은다(2026-10-01 오너). 결정 필요·머지 후보는 바로, 충돌·CI 실패는 세션이 먼저
 * 풀 틈을 주고 30분 뒤에, 세션이 사라진 진행 중 할 일. 판정은 `alertRows`(`lib.ts`). 보드를 고르면 그
 * 보드(레포)의 것만, "전체" 면 전부. × 는 GitHub 탭과 같은 숨김 목록을 쓴다 — 종류별이라 상태가 바뀌면 다시 뜬다.
 */
export function AlertsPane() {
  const rows = useAlertRows();
  const now = useNow([], 1);
  const hide = useUiStore((s) => s.hideGithub);
  const openTodoDetail = useUiStore((s) => s.openTodoDetail);
  const kinds = [...new Set(rows.map((r) => r.kind))];

  return (
    <main className="min-w-0 flex-1 overflow-y-auto px-4 py-3" aria-label="알림">
      {rows.length === 0 ? (
        <p className="m-0 text-meta text-faint">손댈 것 없음</p>
      ) : (
        kinds.map((kind) => {
          const group = rows.filter((r) => r.kind === kind);
          return (
            <section key={kind} className="mb-[26px]" aria-label={ALERT_LABEL[kind]}>
              <h2 className="m-0 mb-1.5 flex items-baseline gap-2 font-mono text-chip font-medium text-faint">
                {ALERT_LABEL[kind]}
                <span className="tabular-nums text-run">{group.length}</span>
              </h2>
              <ul className="m-0 list-none overflow-hidden rounded-[10px] border border-line bg-surface p-0">
                {group.map((row) => (
                  <AlertItem
                    key={row.key}
                    row={row}
                    now={now}
                    onOpen={() => {
                      logUsage('web:alert-open', { kind: row.kind });
                      if (row.todoId) {
                        void openTodoDetail(row.todoId);
                      }
                    }}
                    onHide={() => hide(row.hideKey)}
                  />
                ))}
              </ul>
            </section>
          );
        })
      )}
    </main>
  );
}

/** 지금 보드의 알림 — 탭 배지와 화면이 같은 값을 본다. */
export function useAlertRows(): AlertRow[] {
  const prs = useUiStore((s) => s.prs);
  const todos = useUiStore((s) => s.nowTodos);
  const boards = useUiStore((s) => s.boards);
  const selected = useUiStore((s) => s.selected);
  const hidden = useUiStore((s) => s.githubHidden);
  // 늘 1분마다 — 충돌·CI 실패가 30분을 넘기는 순간 올라와야 한다.
  const now = useNow([], 1);
  const board = selected === 'all' ? null : boards.find((b) => b.key === selected);
  const scopedPrs = board ? prs.filter((p) => p.repo === board.repo) : prs;
  const scopedTodos = board ? todos.filter((t) => t.boardId === board.id) : todos;
  return alertRows(scopedPrs, scopedTodos, boards, hidden, now);
}

function AlertItem(props: { row: AlertRow; now: number; onOpen: () => void; onHide: () => void }) {
  const { row, now, onOpen, onHide } = props;
  const body = (
    <>
      <StateIcon {...ICON[row.kind]} />
      <span className="min-w-0 flex-1">
        <span className="now-title block text-sm leading-[1.45] text-text">{row.title}</span>
        <span className="mt-0.5 block truncate font-mono text-chip tabular-nums text-muted">
          {[row.detail, formatAge(row.at, now)].join(' · ')}
        </span>
      </span>
    </>
  );
  const cls =
    'now-item flex min-w-0 flex-1 items-start gap-2 px-3 py-2 text-left no-underline hover:bg-surface-2 focus-visible:bg-surface-2';
  return (
    <li className="flex items-start border-t border-line first:border-t-0">
      {row.url ? (
        <a className={cls} href={row.url} target="_blank" rel="noreferrer" onClick={onOpen}>
          {body}
        </a>
      ) : (
        <button type="button" className={`${cls} border-0 bg-transparent`} onClick={onOpen}>
          {body}
        </button>
      )}
      <HideButton label={`${row.detail} 숨기기`} onClick={onHide} />
    </li>
  );
}
