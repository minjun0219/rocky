import { useEffect, useState } from 'react';
import { formatAge, githubHideKey, type PrRow, prRows } from '../lib';
import { api, useUiStore } from '../store';
import type { OpenPr } from '../types';
import { logUsage } from '../usage';
import { BoardInbox } from './BoardInbox';
import { HideButton } from './HideButton';
import { SessionDelivery } from './SessionDelivery';
import { PR_ICON, StateIcon, useNow } from './NowTable';

/**
 * GitHub 화면 — 구독한 PR 의 상태판(레포별)과 수집함(이슈 등). 할 일 화면에 섞이면 어지러워 탭으로 뺐다
 * (2026-09-30 오너). 데몬은 **구독한 PR 만** 보므로 상태판도 구독한 PR 이다; 레포마다 "그 밖의 열린 PR" 을
 * 펼치면 그때만 그 레포의 열린 PR 을 묻고 "지켜보기" 로 구독한다. 행마다 숨기기(×): PR 은 상태가 바뀌면 다시
 * 보이고(`githubHideKey`), 숨긴 게 있으면 아래에 "숨긴 N건 · 다시 보이기". 숨김은 이 브라우저에만 남는다.
 */
export function GithubPane() {
  const prs = useUiStore((s) => s.prs);
  const selected = useUiStore((s) => s.selected);
  const boards = useUiStore((s) => s.boards);
  const hidden = useUiStore((s) => s.githubHidden);
  const hide = useUiStore((s) => s.hideGithub);
  const unhideAll = useUiStore((s) => s.unhideAllGithub);
  const board = selected === 'all' ? undefined : boards.find((b) => b.key === selected);
  // 전체 보기면 전 보드의 레포 + 구독한 PR 의 레포, 보드면 그 보드의 레포. 레포가 없는 보드는 PR 이 없다.
  const repo = selected === 'all' ? null : (board?.repo ?? undefined);
  const all = repo === undefined ? [] : prRows(prs, repo);
  const keyOf = (pr: PrRow) => githubHideKey({ kind: 'pr', key: pr.key, status: pr.status });
  const visible = all.filter((pr) => !hidden.includes(keyOf(pr)));
  const now = useNow([], visible.length);
  const repos =
    repo === undefined
      ? []
      : repo !== null
        ? [repo]
        : [
            ...new Set([
              ...boards.map((b) => b.repo).filter((r): r is string => Boolean(r)),
              ...all.map((p) => p.repo),
            ]),
          ].sort();

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
            이 보드에 GitHub 레포가 없다. 보드 편집에서 GitHub 칸을 채우면 PR이 보인다.
          </p>
        ) : repos.length === 0 ? (
          <p className="m-0 text-meta text-faint">
            구독한 PR이 없다. 보드에 GitHub 레포를 연결하면 열린 PR을 여기서 지켜볼 수 있다.
          </p>
        ) : (
          <div className="flex flex-col gap-3">
            {repos.map((r) => {
              const rows = visible.filter((pr) => pr.repo === r);
              return (
                <RepoPrs key={r} repo={r} count={rows.length}>
                  {rows.length > 0 ? (
                    <ul className="m-0 list-none overflow-hidden rounded-[10px] border border-line bg-surface p-0">
                      {rows.map((pr) => (
                        <PrItem
                          key={pr.key}
                          pr={pr}
                          now={now}
                          showRepo={false}
                          onHide={() => hide(keyOf(pr))}
                        />
                      ))}
                    </ul>
                  ) : null}
                </RepoPrs>
              );
            })}
          </div>
        )}
      </section>
      {board ? (
        <BoardInbox key={`inbox:${board.key}`} board={board.key} />
      ) : (
        <p className="m-0 mb-[26px] text-meta text-faint">
          수집함은 보드를 고르면 보인다(보드마다 등록한다).
        </p>
      )}
      <SessionDelivery />
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
 * 레포 하나 — 머리줄(이름 · 구독 수 · 오른쪽에 "열린 PR" 펼치기), 구독한 PR, 펼치면 그 밖의 열린 PR. 구독이 없는
 * 레포는 머리줄 한 줄로 접힌다 — 전체 보기에 레포가 많아도 어수선하지 않게.
 */
function RepoPrs(props: { repo: string; count: number; children: React.ReactNode }) {
  const { repo, count, children } = props;
  const [open, setOpen] = useState(false);
  return (
    <section aria-label={repo} className="flex flex-col gap-1.5">
      <h3 className="m-0 flex items-baseline gap-2 font-mono text-chip font-medium text-muted">
        <a
          href={`https://github.com/${repo}/pulls`}
          target="_blank"
          rel="noreferrer"
          className="tap min-w-0 truncate text-muted no-underline hover:text-text hover:underline"
        >
          {repo}
        </a>
        {count > 0 ? <span className="tabular-nums text-faint">구독 {count}</span> : null}
        <button
          type="button"
          className="tap ml-auto shrink-0 font-sans text-meta font-normal text-muted underline underline-offset-2 hover:text-text"
          aria-expanded={open}
          aria-label={open ? `${repo} 그 밖의 열린 PR 접기` : `${repo} 그 밖의 열린 PR 보기`}
          onClick={() => setOpen((o) => !o)}
        >
          {open ? '접기' : '열린 PR'}
        </button>
      </h3>
      {children}
      {open ? <OtherOpenPrs repo={repo} /> : null}
    </section>
  );
}

/**
 * 그 레포에서 구독하지 않은 열린 PR — 펼칠 때만 데몬에 묻는다(`/api/prs/open`, GitHub 1포인트·60초 캐시). "지켜보기"
 * 는 세션 없이 구독한다(알림 탭에는 뜨고 세션은 깨우지 않는다). 구독은 로컬에서 연 화면에서만 된다.
 */
function OtherOpenPrs({ repo }: { repo: string }) {
  const actor = useUiStore((s) => s.actor);
  const [prs, setPrs] = useState<OpenPr[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [watched, setWatched] = useState<number[]>([]);
  const now = useNow([], prs?.length ?? 0);

  const load = async () => {
    setError(null);
    try {
      setPrs(await api<OpenPr[]>(`/api/prs/open?repo=${encodeURIComponent(repo)}`, actor));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };
  // 펼쳐서 마운트될 때 한 번 — 접었다 다시 펴면 다시 묻는다(데몬이 60초 캐시한다).
  // biome-ignore lint/correctness/useExhaustiveDependencies: 마운트에 한 번
  useEffect(() => {
    logUsage('web:open-prs');
    void load();
  }, []);
  const watch = async (number: number) => {
    setError(null);
    try {
      await api('/api/prs/subscriptions', actor, {
        method: 'POST',
        body: JSON.stringify({ repo, number }),
      });
      logUsage('web:watch-pr');
      setWatched((w) => [...w, number]);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const others = (prs ?? []).filter((p) => !p.subscribed);
  return (
    <div className="flex flex-col gap-1">
      {error ? (
        <p className="m-0 text-meta text-dead">{error}</p>
      ) : prs === null ? (
        <p className="m-0 text-meta text-faint">불러오는 중…</p>
      ) : others.length === 0 ? (
        <p className="m-0 text-meta text-faint">구독하지 않은 열린 PR 없음</p>
      ) : (
        <ul className="m-0 list-none overflow-hidden rounded-[10px] border border-line p-0">
          {others.map((p) => {
            const done = watched.includes(p.number);
            const meta = [
              `#${p.number}`,
              p.author,
              p.isDraft ? 'draft' : null,
              formatAge(p.updatedAt, now),
            ]
              .filter(Boolean)
              .join(' · ');
            return (
              <li
                key={p.number}
                className="flex items-center gap-2 border-t border-line px-3 py-2 first:border-t-0"
              >
                <a
                  className="min-w-0 flex-1 no-underline"
                  href={p.url}
                  target="_blank"
                  rel="noreferrer"
                >
                  <span className="block text-sm leading-[1.45] text-muted">{p.title}</span>
                  <span className="block truncate font-mono text-chip tabular-nums text-faint">
                    {meta}
                  </span>
                </a>
                <button
                  type="button"
                  className="shrink-0 rounded-md border border-line px-2 py-1 text-meta text-muted hover:border-mine hover:text-text disabled:opacity-60"
                  disabled={done}
                  onClick={() => void watch(p.number)}
                >
                  {done ? '지켜보는 중' : '지켜보기'}
                </button>
              </li>
            );
          })}
        </ul>
      )}
      {watched.length > 0 ? (
        <p className="m-0 text-meta text-faint">
          지켜보는 PR은 다음 확인(3분 안)부터 위 목록에 보인다. 세션은 깨우지 않는다.
        </p>
      ) : null}
    </div>
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
