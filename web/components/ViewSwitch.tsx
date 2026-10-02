import { hasNoteNews } from '../lib';
import { useUiStore } from '../store';
import type { BoardView } from '../types';
import { useFeedCount } from './FeedPane';

const LABEL: Record<BoardView, string> = {
  feed: '피드',
  todos: '할 일',
  notes: '노트',
  worklog: '작업로그',
  github: 'GitHub',
};

/**
 * 피드 / 할 일 / 노트 / GitHub 전환. 피드 옆 숫자는 오너가 손댈 것의 수(PR 알림 + 내 차례, `FeedPane`).
 * GitHub 탭은 ⋯ 메뉴에서 끄면 안 보인다.
 *
 * 할 일 / 노트 전환 — 노트가 목록 아래 스크롤 너머에 묻히지 않게 화면 맨 위에서 고른다.
 * 노트 보기를 떠난 뒤 누가(에이전트 포함) 노트를 고쳤으면 "노트" 옆에 점을 찍는다.
 */
export function ViewSwitch() {
  const view = useUiStore((s) => s.view);
  const setView = useUiStore((s) => s.setView);
  const notes = useUiStore((s) => s.notes);
  const notesSeenAt = useUiStore((s) => s.notesSeenAt);
  const news = view === 'todos' && hasNoteNews(notes, notesSeenAt);
  const showGithub = useUiStore((s) => s.showGithub);
  const feed = useFeedCount();
  // 피드가 맨 앞이자 첫 화면이다(2026-10-02 오너).
  const kinds: BoardView[] = showGithub
    ? ['feed', 'todos', 'notes', 'worklog', 'github']
    : ['feed', 'todos', 'notes', 'worklog'];

  return (
    <nav className="view-switch flex gap-1 border-b border-line px-4 py-1.5" aria-label="보기">
      {kinds.map((kind) => (
        <button
          key={kind}
          type="button"
          aria-pressed={view === kind}
          className={`min-h-8 rounded-md px-3 text-sm ${
            view === kind ? 'bg-surface font-semibold text-text' : 'text-muted hover:text-text'
          }`}
          onClick={() => setView(kind)}
        >
          {LABEL[kind]}
          {kind === 'notes' && news ? (
            <span className="ml-1 text-mine" role="img" aria-label="새 편집 있음">
              •
            </span>
          ) : null}
          {kind === 'feed' && feed > 0 ? (
            <span className="ml-1 font-mono text-chip font-semibold tabular-nums text-mine">
              {feed}
            </span>
          ) : null}
          {kind === 'notes' && notes.length > 0 ? (
            <span className="ml-1 font-mono text-chip tabular-nums text-faint">{notes.length}</span>
          ) : null}
        </button>
      ))}
    </nav>
  );
}
