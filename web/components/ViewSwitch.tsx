import { githubAttention, hasNoteNews, prRows } from '../lib';
import { useUiStore } from '../store';
import type { BoardView } from '../types';

const LABEL: Record<BoardView, string> = { todos: '할 일', notes: '노트', github: 'GitHub' };

/**
 * 할 일 / 노트 / GitHub 전환. GitHub 탭 옆 숫자는 사람이 움직일 PR(충돌·머지 가능·CI 실패·결정 필요) —
 * GitHub 줄을 할 일 화면에서 뺐으니 놓치지 않게 여기서 한 번 말한다. ⋯ 메뉴에서 탭을 끄면 안 보인다.
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
  const prs = useUiStore((s) => s.prs);
  const hidden = useUiStore((s) => s.githubHidden);
  const selected = useUiStore((s) => s.selected);
  const boards = useUiStore((s) => s.boards);
  const repo =
    selected === 'all' ? null : (boards.find((b) => b.key === selected)?.repo ?? undefined);
  const attention = repo === undefined ? 0 : githubAttention(prRows(prs, repo), hidden);
  const kinds: BoardView[] = showGithub ? ['todos', 'notes', 'github'] : ['todos', 'notes'];

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
          {kind === 'github' && attention > 0 ? (
            <span className="ml-1 font-mono text-chip font-semibold tabular-nums text-mine">
              {attention}
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
