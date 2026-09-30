import { formatAge, githubHideKey, type PrRow, prRows } from '../lib';
import { useUiStore } from '../store';
import { logUsage } from '../usage';
import { BoardInbox } from './BoardInbox';
import { HideButton } from './HideButton';
import { PR_ICON, StateIcon, useNow } from './NowTable';

/**
 * GitHub 화면 — 열린 PR 상태판과 수집함(이슈 등). 할 일 화면에 섞이면 어지러워 탭으로 뺐다
 * (2026-09-30 오너). 행마다 숨기기(×): PR 은 상태가 바뀌면 다시 보이고(`githubHideKey`), 숨긴 게
 * 있으면 아래에 "숨긴 N건 · 다시 보이기". 숨김은 이 브라우저에만 남는다.
 */
export function GithubPane() {
  const prs = useUiStore((s) => s.prs);
  const selected = useUiStore((s) => s.selected);
  const boards = useUiStore((s) => s.boards);
  const hidden = useUiStore((s) => s.githubHidden);
  const hide = useUiStore((s) => s.hideGithub);
  const unhideAll = useUiStore((s) => s.unhideAllGithub);
  const board = selected === 'all' ? undefined : boards.find((b) => b.key === selected);
  // 전체 보기면 전 보드의 레포, 보드면 그 보드의 레포. 레포가 없는 보드는 PR 이 없다.
  const repo = selected === 'all' ? null : (board?.repo ?? undefined);
  const all = repo === undefined ? [] : prRows(prs, repo);
  const keyOf = (pr: PrRow) => githubHideKey({ kind: 'pr', key: pr.key, status: pr.status });
  const visible = all.filter((pr) => !hidden.includes(keyOf(pr)));
  const now = useNow([], visible.length);

  return (
    <main className="min-w-0 flex-1 overflow-y-auto px-4 py-3" aria-label="GitHub">
      <section className="mb-[26px]" aria-label="PR">
        <h2 className="m-0 mb-1.5 flex items-baseline gap-2 font-mono text-chip font-medium text-faint">
          PR
          {visible.length > 0 ? (
            <span className="tabular-nums text-run">{visible.length}</span>
          ) : null}
        </h2>
        {repo === undefined ? (
          <p className="m-0 text-meta text-faint">
            이 보드에 GitHub 레포가 없다 — 보드 편집에서 GitHub 을 채우면 PR 이 보인다.
          </p>
        ) : visible.length === 0 ? (
          <p className="m-0 text-meta text-faint">열린 PR 없음</p>
        ) : (
          <ul className="m-0 list-none overflow-hidden rounded-[10px] border border-line bg-surface p-0">
            {visible.map((pr) => (
              <PrItem
                key={pr.key}
                pr={pr}
                now={now}
                showRepo={repo === null}
                onHide={() => hide(keyOf(pr))}
              />
            ))}
          </ul>
        )}
      </section>
      {board ? (
        <BoardInbox key={`inbox:${board.key}`} board={board.key} />
      ) : (
        <p className="m-0 mb-[26px] text-meta text-faint">
          수집함은 보드를 고르면 보인다(보드마다 등록한다).
        </p>
      )}
      {hidden.length > 0 ? (
        <p className="m-0 text-meta text-faint">
          숨긴 항목 {hidden.length}건 ·{' '}
          <button
            type="button"
            className="text-muted underline underline-offset-2"
            onClick={unhideAll}
          >
            다시 보이기
          </button>
        </p>
      ) : null}
    </main>
  );
}

/**
 * PR 한 줄 — 상태 아이콘 + 제목(두 줄까지), 둘째 줄: 번호 · CI·스레드 · 갱신. 누르면 GitHub 새 탭.
 * 오른쪽 × 는 숨기기(늘 보인다 — 좁은 패널·터치에는 hover 가 없다, `web/DESIGN.md`).
 */
export function PrItem(props: { pr: PrRow; now: number; showRepo: boolean; onHide?: () => void }) {
  const { pr, now, showRepo, onHide } = props;
  const ref = showRepo ? `${pr.repo.split('/')[1] ?? pr.repo} #${pr.number}` : `#${pr.number}`;
  const meta = [ref, pr.detail, formatAge(pr.updatedAt, now)].filter(Boolean);
  return (
    <li className="flex items-start border-t border-line first:border-t-0">
      <a
        className="now-item flex min-w-0 flex-1 items-start gap-2 px-3 py-2 text-left no-underline hover:bg-surface-2 focus-visible:bg-surface-2"
        href={pr.url}
        target="_blank"
        rel="noreferrer"
        onClick={() => logUsage('web:now-row', { kind: 'pr-status' })}
      >
        <StateIcon {...PR_ICON[pr.status]} />
        <span className="min-w-0 flex-1">
          <span className="now-title block text-sm leading-[1.45] text-text">{pr.title}</span>
          <span className="mt-0.5 block truncate font-mono text-chip tabular-nums text-muted">
            {meta.join(' · ')}
          </span>
        </span>
      </a>
      {onHide ? <HideButton label={`#${pr.number} 숨기기`} onClick={onHide} /> : null}
    </li>
  );
}
