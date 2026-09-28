import { Archive, ChevronDown, ChevronRight, History } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import type { NoteView } from '../types';
import { boardCommand, copyRefWithFeedback, formatElapsed } from '../lib';
import { NoteSync, PRESENCE_PING_MS } from '../notedoc';
import { useUiStore } from '../store';
import { bindTextarea } from '../textarea-binding';
import { logUsage } from '../usage';

/** 로컬 편집의 트랜잭션 원점 — 바인딩이 자기 편집을 다시 그리지 않게 가른다. */
const LOCAL_ORIGIN = Symbol('note-textarea');
/** 포커스가 빠진 뒤 동기화 세션을 얼마나 더 살려 두나 — 잠깐 다른 곳을 눌렀다 돌아오는 경우. */
const LIVE_LINGER_MS = 20_000;

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
  const actor = useUiStore((s) => s.actor);
  const [title, setTitle] = useState(note.title);
  const [lines, setLines] = useState(note.content.split('\n').length);
  const [copied, setCopied] = useState(false);
  // 본문의 실시간 세션 — 포커스가 들어오면 열고, 빠진 뒤 잠시 있다 닫는다. idle 일 땐 textarea 가
  // SSE refetch 로 온 note.content 를 그대로 보여 준다(uncontrolled: 값은 ref 로 만진다).
  const [live, setLive] = useState<'idle' | 'opening' | 'on'>('idle');
  const [others, setOthers] = useState<string[]>([]);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const syncRef = useRef<NoteSync | null>(null);
  const unbindRef = useRef<(() => void) | null>(null);
  const lingerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pingRef = useRef<ReturnType<typeof setInterval> | null>(null);

  useEffect(() => {
    setTitle(note.title);
  }, [note.title]);

  // 다른 경로(에이전트·CLI)의 편집이 SSE refetch 로 들어오면, 실시간 세션이 없을 때만 그린다 —
  // 세션이 있으면 그 편집은 이미 문서 스트림으로 들어와 textarea 에 그려졌다.
  useEffect(() => {
    if (live === 'idle' && textareaRef.current && textareaRef.current.value !== note.content) {
      textareaRef.current.value = note.content;
      setLines(note.content.split('\n').length);
    }
  }, [note.content, live]);

  const stopLive = () => {
    if (lingerRef.current) {
      clearTimeout(lingerRef.current);
      lingerRef.current = null;
    }
    if (pingRef.current) {
      clearInterval(pingRef.current);
      pingRef.current = null;
    }
    unbindRef.current?.();
    unbindRef.current = null;
    syncRef.current?.close();
    syncRef.current = null;
    setOthers([]);
    setLive('idle');
  };

  const startLive = async () => {
    if (lingerRef.current) {
      clearTimeout(lingerRef.current);
      lingerRef.current = null;
    }
    if (syncRef.current) {
      return;
    }
    const el = textareaRef.current;
    if (!el) {
      return;
    }
    logUsage('web:note-live');
    setLive('opening');
    const sync = new NoteSync(note.id, actor);
    syncRef.current = sync;
    try {
      await sync.open();
    } catch (err) {
      console.warn('[rocky] 노트 실시간 세션을 열지 못했다', err);
      if (syncRef.current === sync) {
        syncRef.current = null;
        setLive('idle');
      }
      return;
    }
    if (syncRef.current !== sync) {
      sync.close(); // 여는 사이 닫혔다
      return;
    }
    unbindRef.current = bindTextarea(el, sync.text, {
      origin: LOCAL_ORIGIN,
      // 세션을 여는 사이 친 글자는 화면값과 이 기준값의 차이다 — 문서에 먼저 넣는다.
      baseline: note.content,
      pauseRemote: () => sync.pauseRemote(),
      resumeRemote: () => sync.resumeRemote(),
      onChange: (value) => setLines(value.split('\n').length),
    });
    sync.onPresence = setOthers;
    void sync.ping();
    pingRef.current = setInterval(() => void sync.ping(), PRESENCE_PING_MS);
    setLive('on');
  };

  const scheduleStop = () => {
    if (!syncRef.current || lingerRef.current) {
      return;
    }
    lingerRef.current = setTimeout(stopLive, LIVE_LINGER_MS);
  };

  // 카드가 사라지면 세션도 닫는다.
  // biome-ignore lint/correctness/useExhaustiveDependencies: 언마운트에 한 번 — stopLive 는 ref 만 만져 최신 값이 필요 없다
  useEffect(() => stopLive, []);

  const titleDirty = title !== note.title;
  const saveTitle = () => {
    if (titleDirty) {
      void saveNote(note.id, { title });
    }
  };

  // 글로벌 메모는 note.ref 가 `note-3` 으로 오고 보드 메모는 `rocky-3` 으로 온다 —
  // 어느 쪽이든 boardCommand 가 그대로 감싸므로 별도 분기가 없다.
  const handleCopyRef = () => copyRefWithFeedback(boardCommand(note.ref), setCopied);

  const status =
    live === 'on'
      ? `실시간${others.length > 0 ? ` · 같이 보는 중: ${others.join(', ')}` : ''}`
      : live === 'opening'
        ? '여는 중…'
        : `갱신 ${formatElapsed(note.updatedAt)} 전`;

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
          onBlur={saveTitle}
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
        ref={textareaRef}
        className="note-content mt-1 w-full resize-y border-none bg-transparent text-sm leading-[1.55] text-muted focus:text-text focus:outline-none"
        defaultValue={note.content}
        rows={Math.min(12, Math.max(3, lines + 1))}
        readOnly={live === 'opening'}
        aria-label={`${note.title} 본문`}
        onFocus={() => void startLive()}
        onBlur={scheduleStop}
        onInput={(e) => {
          if (live === 'idle') {
            // 세션이 열리기 전(readOnly 가 걸리기 전 한 틱)의 입력은 세션이 열리면 문서 값으로
            // 덮인다 — 그 한 틱을 잃지 않게 세션을 연다.
            void startLive();
          }
          setLines(e.currentTarget.value.split('\n').length);
        }}
      />
      <div className="mt-1 font-mono text-micro text-faint">{status}</div>
    </div>
  );
}
