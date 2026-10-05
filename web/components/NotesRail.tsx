import {
  Archive,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  History,
  Maximize2,
  Pin,
} from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import { type MountedEditor, mountNoteEditor } from '../codemirror-editor';
import { boardCommand, copyRefWithFeedback, formatElapsed } from '../lib';
import { NoteSync, PRESENCE_PING_MS } from '../notedoc';
import { useUiStore } from '../store';
import type { NoteView } from '../types';
import { logUsage } from '../usage';
import { FormatToolbar } from './FormatToolbar';
import { Markdown, markdownExcerpt } from './Markdown';

/** 포커스가 빠진 뒤 동기화 세션을 얼마나 더 살려 두나 — 잠깐 다른 곳을 눌렀다 돌아오는 경우. */
const LIVE_LINGER_MS = 20_000;
/** 접어 둔 고정 카드 — 보는 사람마다 다르니 브라우저에만 둔다. */
export const COLLAPSED_KEY = 'rocky:notes-collapsed';

function readCollapsed(): Set<string> {
  try {
    const raw = JSON.parse(localStorage.getItem(COLLAPSED_KEY) ?? '[]');
    return new Set(Array.isArray(raw) ? raw.filter((v) => typeof v === 'string') : []);
  } catch {
    return new Set();
  }
}

function writeCollapsed(ids: Set<string>): void {
  try {
    localStorage.setItem(COLLAPSED_KEY, JSON.stringify([...ids]));
  } catch {
    // 저장 못 해도 이번 화면에서는 접힌다.
  }
}

/**
 * 노트 보기 — 머리의 "할 일 | 노트" 전환으로 들어오는 화면 전체(`web/DESIGN.md` "Notes").
 *
 * 게시판처럼 목록 한 줄 → 누르면 상세(전체 높이 편집기). 늘 곁에 둘 노트만 고정하면 목록 위에 카드로
 * 펼쳐 두고, 카드는 접을 수 있다.
 */
export function NotesRail() {
  const notes = useUiStore((s) => s.notes);
  const selected = useUiStore((s) => s.selected);
  const addNote = useUiStore((s) => s.addNote);
  const openNoteId = useUiStore((s) => s.openNoteId);
  const [collapsed, setCollapsed] = useState<Set<string>>(readCollapsed);

  const open = openNoteId === null ? undefined : notes.find((n) => n.id === openNoteId);
  if (open) {
    return <NoteDetail key={open.id} note={open} />;
  }

  const pinned = notes
    .filter((n) => n.pinnedAt)
    .sort((a, b) => (a.pinnedAt ?? '').localeCompare(b.pinnedAt ?? ''));
  const rest = notes
    .filter((n) => !n.pinnedAt)
    .sort((a, b) => b.updatedAt.localeCompare(a.updatedAt));

  const toggle = (id: string) => {
    // 고정을 풀었거나 보관한 노트의 흔적은 이때 같이 걷는다.
    const next = new Set([...collapsed].filter((c) => pinned.some((n) => n.id === c)));
    if (next.has(id)) {
      next.delete(id);
    } else {
      next.add(id);
    }
    writeCollapsed(next);
    setCollapsed(next);
  };

  return (
    <section
      className="notes-view flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto px-4 py-3"
      aria-label="노트"
    >
      <div className="notes-head flex items-center gap-2">
        <button
          type="button"
          className="notes-add min-h-8 rounded-md border border-line bg-surface px-3 text-sm text-text hover:border-mine"
          onClick={() =>
            void addNote({
              board: selected === 'all' ? undefined : selected,
              title: '새 노트',
            })
          }
        >
          + 새 노트
        </button>
      </div>
      {notes.length === 0 ? (
        <div className="empty-state px-1 py-[18px] text-sm text-muted">
          노트가 없다. 사람과 에이전트가 같이 쓰는 스크래치 패드다. "+ 새 노트"를 눌러 시작한다.
        </div>
      ) : (
        <>
          {pinned.length > 0 && (
            <section className="flex flex-col gap-2" aria-label="고정한 노트">
              {pinned.map((note) => (
                <PinnedNoteCard
                  key={note.id}
                  note={note}
                  collapsed={collapsed.has(note.id)}
                  onToggle={() => toggle(note.id)}
                />
              ))}
            </section>
          )}
          {rest.length > 0 && (
            <ul
              className="m-0 flex list-none flex-col divide-y divide-line border-y border-line p-0"
              aria-label="노트 목록"
            >
              {rest.map((note) => (
                <NoteRow key={note.id} note={note} />
              ))}
            </ul>
          )}
        </>
      )}
    </section>
  );
}

/** 목록 한 줄 — 번호 · 제목 · 첫 줄 요약 · 갱신 시각. 누르면 상세. */
function NoteRow({ note }: { note: NoteView }) {
  const openNote = useUiStore((s) => s.openNote);
  const excerpt = markdownExcerpt(note.content);
  return (
    <li className="flex items-center gap-1">
      <button
        type="button"
        className="note-row flex min-w-0 flex-1 items-baseline gap-2 py-2 text-left hover:bg-surface-2"
        onClick={() => openNote(note.id)}
      >
        <span className="w-6 shrink-0 text-right font-mono text-chip text-faint">
          {note.number}
        </span>
        <span className="min-w-0 flex-1">
          <span className="block truncate text-sm font-semibold text-text">{note.title}</span>
          {excerpt && <span className="block truncate text-meta text-muted">{excerpt}</span>}
        </span>
        <span className="shrink-0 font-mono text-micro text-faint">
          {formatElapsed(note.updatedAt)} 전
        </span>
      </button>
      <PinButton note={note} />
    </li>
  );
}

/** 고정 카드 — 지금처럼 펼쳐 두고 그 자리에서 편집한다. 접으면 머리줄만 남는다. */
function PinnedNoteCard({
  note,
  collapsed,
  onToggle,
}: {
  note: NoteView;
  collapsed: boolean;
  onToggle: () => void;
}) {
  const openNote = useUiStore((s) => s.openNote);
  return (
    <div className="note-card rounded-[10px] border border-line bg-surface px-3 py-2.5">
      <div className="note-card-head flex items-center gap-1">
        <button
          type="button"
          className="note-action px-0.5 text-faint hover:text-text"
          aria-expanded={!collapsed}
          aria-label={collapsed ? `${note.title} 펼치기` : `${note.title} 접기`}
          onClick={onToggle}
        >
          {collapsed ? (
            <ChevronRight size={14} aria-hidden />
          ) : (
            <ChevronDown size={14} aria-hidden />
          )}
        </button>
        <NoteHead note={note} />
        <button
          type="button"
          className="note-action px-1 py-0.5 text-meta text-faint hover:text-text"
          title="크게 열기"
          aria-label="크게 열기"
          onClick={() => openNote(note.id)}
        >
          <Maximize2 size={13} aria-hidden />
        </button>
      </div>
      {collapsed ? (
        markdownExcerpt(note.content) && (
          <div className="truncate pl-5 text-meta text-muted">{markdownExcerpt(note.content)}</div>
        )
      ) : (
        <NoteEditor note={note} />
      )}
    </div>
  );
}

/** 상세 — 목록을 밀어내고 편집기가 화면 전체 높이를 쓴다. 뒤로 가면 목록. */
function NoteDetail({ note }: { note: NoteView }) {
  const closeNote = useUiStore((s) => s.closeNote);
  return (
    <section
      className="notes-view flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto px-4 py-3"
      aria-label="노트 상세"
    >
      <div className="flex items-center gap-1">
        <button
          type="button"
          className="flex min-h-8 items-center gap-0.5 pr-2 text-sm text-muted hover:text-text"
          onClick={closeNote}
        >
          <ChevronLeft size={16} aria-hidden />
          노트
        </button>
      </div>
      <div className="note-card-head flex items-center gap-1">
        <NoteHead note={note} large />
      </div>
      <NoteEditor note={note} fill autoStart />
    </section>
  );
}

/**
 * 번호(복사) · 제목 입력 · 고정 — 카드와 상세가 같이 쓴다. `large`(상세)면 히스토리·보관까지 — 좁은
 * 카드 머리에서는 제목이 잘려서 뺐다(상세로 가면 있다).
 */
function NoteHead({ note, large = false }: { note: NoteView; large?: boolean }) {
  const saveNote = useUiStore((s) => s.saveNote);
  const archiveNote = useUiStore((s) => s.archiveNote);
  const openNoteDetail = useUiStore((s) => s.openNoteDetail);
  const [title, setTitle] = useState(note.title);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    setTitle(note.title);
  }, [note.title]);

  const saveTitle = () => {
    if (title !== note.title) {
      void saveNote(note.id, { title });
    }
  };
  // 글로벌 메모는 note.ref 가 `note-3` 으로 오고 보드 메모는 `rocky-3` 으로 온다 —
  // 어느 쪽이든 boardCommand 가 그대로 감싸므로 별도 분기가 없다.
  const handleCopyRef = () => copyRefWithFeedback(boardCommand(note.ref), setCopied);

  return (
    <>
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
        className={`note-title min-w-0 flex-1 border-none bg-transparent py-0.5 font-semibold ${large ? 'text-title' : 'text-sm'}`}
        value={title}
        aria-label="노트 제목"
        onChange={(e) => setTitle(e.target.value)}
        onBlur={saveTitle}
        onKeyDown={(e) => {
          if (e.key === 'Enter') {
            e.currentTarget.blur();
          }
        }}
      />
      <PinButton note={note} />
      {large && (
        <>
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
        </>
      )}
    </>
  );
}

function PinButton({ note }: { note: NoteView }) {
  const pinNote = useUiStore((s) => s.pinNote);
  const pinned = Boolean(note.pinnedAt);
  const label = pinned ? '고정 해제' : '위에 고정';
  return (
    <button
      type="button"
      className={`note-action px-1 py-0.5 text-meta hover:text-text ${pinned ? 'text-mine' : 'text-faint'}`}
      title={label}
      aria-label={label}
      aria-pressed={pinned}
      onClick={() => void pinNote(note.id, !pinned)}
    >
      <Pin size={13} aria-hidden fill={pinned ? 'currentColor' : 'none'} />
    </button>
  );
}

/**
 * 본문 — 쉴 때는 렌더된 마크다운, 누르면 그 자리에 CodeMirror(실시간 세션)가 뜨고 서식 툴바가 붙는다.
 * `fill` 이면 남은 높이를 전부 쓴다(상세). `autoStart` 면 열자마자 편집기를 띄운다.
 */
function NoteEditor({
  note,
  fill = false,
  autoStart = false,
}: {
  note: NoteView;
  fill?: boolean;
  autoStart?: boolean;
}) {
  const refetch = useUiStore((s) => s.refetch);
  const actor = useUiStore((s) => s.actor);
  // 본문의 실시간 세션 — 포커스가 들어오면 열고, 빠진 뒤 잠시 있다 닫는다. idle 일 땐 SSE refetch 로
  // 온 note.content 를 미리보기로 보여 준다.
  const [live, setLive] = useState<'idle' | 'opening' | 'on'>('idle');
  const [others, setOthers] = useState<string[]>([]);
  const hostRef = useRef<HTMLDivElement>(null);
  const syncRef = useRef<NoteSync | null>(null);
  const mountedRef = useRef<MountedEditor | null>(null);
  const lingerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pingRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const stopLive = async () => {
    if (lingerRef.current) {
      clearTimeout(lingerRef.current);
      lingerRef.current = null;
    }
    if (pingRef.current) {
      clearInterval(pingRef.current);
      pingRef.current = null;
    }
    mountedRef.current?.destroy();
    mountedRef.current = null;
    const sync = syncRef.current;
    syncRef.current = null;
    setOthers([]);
    if (sync) {
      // 마지막 편집이 서버의 2초 이벤트 조절 창 안에 끝났으면 스토어의 note.content 는 그 전 POST 에
      // 머물러 있어, 미리보기가 저장된 글을 되돌린 것처럼 보인다. 남은 편집을 보내고 목록을 다시
      // 읽어 스토어를 맞춘 뒤에야 idle 이 된다.
      await sync.flush();
      sync.close();
      await refetch().catch(() => {});
    }
    setLive('idle');
  };

  const scheduleStop = () => {
    if (!syncRef.current || lingerRef.current) {
      return;
    }
    lingerRef.current = setTimeout(() => void stopLive(), LIVE_LINGER_MS);
  };

  const cancelStop = () => {
    if (lingerRef.current) {
      clearTimeout(lingerRef.current);
      lingerRef.current = null;
    }
  };

  const startLive = async () => {
    cancelStop();
    if (syncRef.current) {
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
    sync.onPresence = setOthers;
    const host = hostRef.current;
    if (!host) {
      void stopLive();
      return;
    }
    // 문서를 받은 뒤라야 편집기를 만들 수 있다 — yCollab 은 만들 때의 Y.Text 에 묶인다.
    const mounted = mountNoteEditor(host, sync, {
      actor,
      onFocus: (focused) => (focused ? cancelStop() : scheduleStop()),
    });
    mountedRef.current = mounted;
    void sync.ping(mounted.pingState());
    pingRef.current = setInterval(() => void sync.ping(mounted.pingState()), PRESENCE_PING_MS);
    setLive('on');
    // 미리보기가 걷힌 다음 프레임에 포커스 — 같은 틱에 부르면 아직 그려지지 않은 편집기라 놓친다.
    requestAnimationFrame(() => mounted.view.focus());
  };

  // 상세는 열자마자 편집기, 사라지면(카드 접기·상세 닫기) 세션도 닫는다.
  // biome-ignore lint/correctness/useExhaustiveDependencies: 마운트·언마운트에 한 번 — 두 함수는 ref 만 만진다
  useEffect(() => {
    if (autoStart) {
      void startLive();
    }
    return () => {
      void stopLive();
    };
  }, []);

  const status =
    live === 'on'
      ? `실시간${others.length > 0 ? ` · 같이 보는 중: ${others.join(', ')}` : ''}`
      : live === 'opening'
        ? '여는 중…'
        : `갱신 ${formatElapsed(note.updatedAt)} 전`;

  return (
    <div className={`note-body mt-1 flex flex-col ${fill ? 'flex-1' : ''}`}>
      {live === 'on' && (
        <FormatToolbar
          view={() => mountedRef.current?.view}
          className={`note-toolbar below-head sticky -top-3 z-10 -mx-1 px-1 py-1 ${fill ? 'bg-bg' : 'bg-surface'}`}
          onFormat={(action) => logUsage('web:note-format', { action })}
        />
      )}
      <div className={`note-cm ${fill ? 'is-fill flex-1' : ''}`}>
        {live !== 'on' && (
          // biome-ignore lint/a11y/useSemanticElements: 안에 목록·링크가 있어 button 으로 감쌀 수 없다
          <div
            role="button"
            tabIndex={0}
            className="note-cm-preview text-sm leading-[1.55] text-muted"
            aria-label={`${note.title} 본문 (눌러서 편집)`}
            onClick={() => void startLive()}
            onKeyDown={(e) => {
              if (e.key === 'Enter' || e.key === ' ') {
                e.preventDefault();
                void startLive();
              }
            }}
          >
            {note.content ? (
              <Markdown text={note.content} />
            ) : (
              <span className="text-faint">눌러서 적는다(마크다운)</span>
            )}
            {live === 'opening' && <span className="text-faint"> …</span>}
          </div>
        )}
        <div ref={hostRef} />
      </div>
      <div className="mt-1 font-mono text-micro text-faint">{status}</div>
    </div>
  );
}
