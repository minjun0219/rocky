import { Archive, ChevronDown, ChevronRight, History } from 'lucide-react';
import { useEffect, useState } from 'react';
import type { NoteView } from '../types';
import { boardCommand, copyRefWithFeedback, formatElapsed } from '../lib';
import { useUiStore } from '../store';
import { logUsage } from '../usage';

/**
 * 메모 레일 — 목록 아래, 기본 접힘. 헤더(개수)가 토글이다. 넓은 화면에서 옆 열로 늘 펼쳐
 * 있던 시절엔 대개 빈 열이었다 — 관제판에선 필요할 때만 편다.
 */
export function NotesRail() {
  const notes = useUiStore((s) => s.notes);
  const selected = useUiStore((s) => s.selected);
  const addNote = useUiStore((s) => s.addNote);
  const [open, setOpen] = useState(false);

  return (
    <aside
      className={`notes-rail flex shrink-0 flex-col gap-3 border-t border-line px-[22px] py-2 ${open ? 'is-open' : ''}`}
    >
      <div className="notes-head flex items-center justify-between">
        <button
          type="button"
          className="notes-toggle"
          aria-expanded={open}
          onClick={() => {
            logUsage('web:notes-toggle');
            setOpen((v) => !v);
          }}
        >
          <span className="sidebar-label">
            NOTES
            {notes.length > 0 ? ` · ${notes.length}` : ''}
            <span className="notes-caret">
              {open ? (
                <ChevronDown size={11} aria-hidden className="inline align-[-1px]" />
              ) : (
                <ChevronRight size={11} aria-hidden className="inline align-[-1px]" />
              )}
            </span>
          </span>
        </button>
        <button
          type="button"
          className="notes-add text-meta text-warm"
          onClick={() => {
            setOpen(true); // 접힌 채 추가하면 새 메모가 안 보인다
            void addNote({
              board: selected === 'all' ? undefined : selected,
              title: '새 메모',
            });
          }}
        >
          + 메모
        </button>
      </div>
      <div className="notes-body">
        {notes.length === 0 && (
          <div className="empty-state px-1 py-[18px] text-sm text-faint">
            메모가 없다. 스크래치패드로 쓰자.
          </div>
        )}
        {notes.map((note) => (
          <NoteCard key={note.id} note={note} />
        ))}
      </div>
    </aside>
  );
}

function NoteCard({ note }: { note: NoteView }) {
  const saveNote = useUiStore((s) => s.saveNote);
  const archiveNote = useUiStore((s) => s.archiveNote);
  const openNoteDetail = useUiStore((s) => s.openNoteDetail);
  const [title, setTitle] = useState(note.title);
  const [content, setContent] = useState(note.content);
  const [copied, setCopied] = useState(false);

  // 다른 경로(에이전트)의 편집이 SSE refetch 로 들어오면, 내가 수정중이 아닐 때만 동기화
  useEffect(() => {
    setTitle(note.title);
    setContent(note.content);
  }, [note.title, note.content]);

  const dirty = title !== note.title || content !== note.content;

  const save = () => {
    if (!dirty) {
      return;
    }
    void saveNote(note.id, { title, content });
  };

  // 글로벌 메모는 note.ref 가 `note-3` 으로 오고 보드 메모는 `rocky-3` 으로 온다 —
  // 어느 쪽이든 boardCommand 가 그대로 감싸므로 별도 분기가 없다.
  const handleCopyRef = () => copyRefWithFeedback(boardCommand(note.ref), setCopied);

  return (
    <div
      className={`note-card rounded-[10px] border border-line bg-surface px-3 py-2.5 ${note.archivedAt ? 'is-archived' : ''}`}
    >
      <div className="note-card-head flex items-center gap-1">
        <button
          type="button"
          className="todo-ref"
          onClick={() => void handleCopyRef()}
          title={copied ? '복사됨' : `${note.ref} 복사`}
          aria-label={copied ? '복사됨' : `${note.ref} 복사`}
        >
          {copied ? '✓' : note.number}
        </button>
        <input
          className="note-title min-w-0 flex-1 border-none bg-transparent py-0.5 text-sm font-semibold"
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          onBlur={save}
        />
        <button
          type="button"
          className="note-action px-1 py-0.5 text-meta text-faint hover:text-text"
          title="히스토리"
          aria-label="히스토리"
          onClick={() => void openNoteDetail(note.id)}
        >
          <History size={13} aria-hidden />
        </button>
        <button
          type="button"
          className="note-action px-1 py-0.5 text-meta text-faint hover:text-text"
          title="보관 (삭제는 없다)"
          aria-label="보관"
          onClick={() => void archiveNote(note.id)}
        >
          <Archive size={13} aria-hidden />
        </button>
      </div>
      <textarea
        className="note-content mt-1 w-full resize-y border-none bg-transparent text-sm leading-[1.55] text-muted focus:text-text focus:outline-none"
        value={content}
        rows={Math.min(12, Math.max(3, content.split('\n').length + 1))}
        onChange={(e) => setContent(e.target.value)}
        onBlur={save}
        onKeyDown={(e) => {
          if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
            save();
          }
        }}
      />
      <div className="mt-1 font-mono text-micro text-faint">
        {dirty ? '수정중… (blur 로 저장)' : `갱신 ${formatElapsed(note.updatedAt)} 전`}
      </div>
    </div>
  );
}
