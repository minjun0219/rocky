import { rcVisible } from '../lib';
import { useUiStore } from '../store';
import type { AppView } from '../types';
import { useFeedCount } from './FeedPane';

const LABEL: Record<AppView, string> = {
  feed: '피드',
  worklog: '작업로그',
  github: 'GitHub',
  agents: '에이전트',
  rc: '원격 제어',
};

/**
 * 피드 / 작업로그 / GitHub / 에이전트 / 원격 제어 전환. 피드 옆 숫자는 오너가 손댈 것의 수(PR 알림 + 내 차례, `FeedPane`).
 * GitHub · 에이전트 · 원격 제어 탭은 ⋯ 메뉴에서 끄면 안 보인다.
 */
export function ViewSwitch() {
  const view = useUiStore((s) => s.view);
  const setView = useUiStore((s) => s.setView);
  const showGithub = useUiStore((s) => s.showGithub);
  const feed = useFeedCount();
  // 피드가 맨 앞이자 첫 화면이다(2026-10-02 오너).
  const showRc = useUiStore((s) => s.showRc && rcVisible(s.rc));
  const showAgents = useUiStore((s) => s.showAgents);
  const kinds: AppView[] = ['feed', 'worklog'];
  if (showGithub) {
    kinds.push('github');
  }
  if (showAgents) {
    kinds.push('agents');
  }
  if (showRc) {
    kinds.push('rc');
  }

  return (
    <nav
      className="view-switch flex w-full sm:w-auto max-w-full items-center justify-between sm:justify-center overflow-x-auto rounded-lg border border-line/60 bg-surface-2/60 p-0.5"
      aria-label="보기"
    >
      {kinds.map((kind) => {
        const isActive = view === kind;
        return (
          <button
            key={kind}
            type="button"
            aria-pressed={isActive}
            className={`inline-flex min-h-7 flex-auto sm:flex-initial min-w-0 items-center justify-center gap-1 sm:gap-1.5 rounded-md px-1 sm:px-2.5 py-1 text-chip whitespace-nowrap transition-all duration-150 ${
              isActive
                ? 'bg-surface font-semibold text-text shadow-xs'
                : 'text-muted hover:text-text'
            }`}
            onClick={() => setView(kind)}
            aria-label={kind === 'rc' ? LABEL.rc : undefined}
          >
            {kind === 'rc' ? (
              // 400px 미만에선 탭 다섯이 붙는다 — 이름을 줄인다(읽는 이름은 aria-label 로 그대로).
              <>
                <span className="hidden min-[400px]:inline">{LABEL.rc}</span>
                <span className="min-[400px]:hidden">원격</span>
              </>
            ) : (
              LABEL[kind]
            )}
            {kind === 'feed' && feed > 0 ? (
              <span className="rounded bg-mine-soft px-1.5 py-0.2 font-mono text-chip font-semibold tabular-nums text-mine">
                {feed}
              </span>
            ) : null}
          </button>
        );
      })}
    </nav>
  );
}
