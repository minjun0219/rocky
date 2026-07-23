import { useEffect, useRef, useState } from 'react';
import type { Note } from '../../store';
import { formatElapsed } from '../lib';
import { useUiStore } from '../store';

/** 우측 메모 레일 — 스티커 카드. 인라인 편집, 저장/보관은 서버 확정 후 반영. */
export function NotesRail() {
  const notes = useUiStore((s) => s.notes);
  const selected = useUiStore((s) => s.selected);
  const addNote = useUiStore((s) => s.addNote);

  return (
    <aside className="notes-rail">
      <div className="notes-head">
        <span className="sidebar-label">NOTES</span>
        <button
          type="button"
          className="notes-add"
          onClick={() =>
            void addNote({
              board: selected === 'all' ? undefined : selected,
              title: '새 메모',
            })
          }
        >
          + 메모
        </button>
      </div>
      {notes.length === 0 && <div className="empty-state">메모가 없다. 스크래치패드로 쓰자.</div>}
      {notes.map((note) => (
        <NoteCard key={note.id} note={note} />
      ))}
    </aside>
  );
}

function NoteCard({ note }: { note: Note }) {
  const saveNote = useUiStore((s) => s.saveNote);
  const archiveNote = useUiStore((s) => s.archiveNote);
  const openNoteDetail = useUiStore((s) => s.openNoteDetail);
  const [title, setTitle] = useState(note.title);
  const [content, setContent] = useState(note.content);
  // 직전에 반영한 서버 값 — "사용자가 편집했는지" 를 새 서버 값과 구분하기 위해 추적한다.
  const syncedRef = useRef({ title: note.title, content: note.content });

  // 다른 경로(에이전트)의 편집이 SSE refetch 로 들어와도, 해당 필드가 직전 서버 값
  // 그대로면(= 사용자가 편집 중이 아니면) 새 값으로 동기화하고, 편집 중이면 입력을 보존한다.
  // 필드별로 판단하므로 title 만 편집 중이어도 content 는 계속 동기화된다.
  useEffect(() => {
    setTitle((prev) => (prev === syncedRef.current.title ? note.title : prev));
    setContent((prev) => (prev === syncedRef.current.content ? note.content : prev));
    syncedRef.current = { title: note.title, content: note.content };
  }, [note.title, note.content]);

  const dirty = title !== note.title || content !== note.content;

  const save = () => {
    if (!dirty) {
      return;
    }
    void saveNote(note.id, { title, content });
  };

  return (
    <div className={`note-card ${note.archivedAt ? 'is-archived' : ''}`}>
      <div className="note-card-head">
        <input
          className="note-title"
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          onBlur={save}
        />
        <button
          type="button"
          className="note-action"
          title="히스토리"
          onClick={() => void openNoteDetail(note.id)}
        >
          ⌚
        </button>
        <button
          type="button"
          className="note-action"
          title="보관 (삭제는 없다)"
          onClick={() => void archiveNote(note.id)}
        >
          ▣
        </button>
      </div>
      <textarea
        className="note-content"
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
      <div className="note-meta">
        {dirty ? '수정중… (blur 로 저장)' : `갱신 ${formatElapsed(note.updatedAt)} 전`}
      </div>
    </div>
  );
}
