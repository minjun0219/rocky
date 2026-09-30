import { Archive, History } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import type { NoteView } from '../types';
import {
  type MountedEditor,
  mountNoteEditor,
  type NoteEditorKind,
  readEditorPref,
  writeEditorPref,
} from '../codemirror-editor';
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
 * 노트 보기 — 머리의 "할 일 | 노트" 전환으로 들어오는 화면 전체. 예전엔 목록 아래 접힌 레일이라
 * 좁은 패널에서 스크롤 1,000px 너머에 묻혔다(`web/DESIGN.md` "Notes").
 *
 * 편집기 스위치(textarea / CodeMirror)는 둘을 번갈아 써 보고 하나를 지우기 위한 임시 것이다
 * (설계 2026-09-28-note-crdt-design, 결정 6). 결정 뒤 지운다.
 */
export function NotesRail() {
  const notes = useUiStore((s) => s.notes);
  const selected = useUiStore((s) => s.selected);
  const addNote = useUiStore((s) => s.addNote);
  const [editor, setEditor] = useState<NoteEditorKind>(() => readEditorPref(localStorage));

  const switchEditor = (kind: NoteEditorKind) => {
    if (kind === editor) {
      return;
    }
    logUsage('web:note-editor', { kind });
    writeEditorPref(localStorage, kind);
    setEditor(kind);
  };

  return (
    <section className="notes-view flex flex-col gap-3 px-4 py-3" aria-label="노트">
      <div className="notes-head flex items-center justify-between gap-2">
        <button
          type="button"
          className="notes-add min-h-8 rounded-md border border-line bg-surface px-3 text-sm text-text hover:border-mine"
          onClick={() =>
            void addNote({
              board: selected === 'all' ? undefined : selected,
              title: '새 메모',
            })
          }
        >
          + 새 노트
        </button>
        <span
          className="note-editor-switch font-mono text-chip text-faint"
          title="편집기 — 둘 다 써 보고 하나만 남긴다"
        >
          편집기{' '}
          {(['textarea', 'codemirror'] as const).map((kind) => (
            <button
              key={kind}
              type="button"
              className={`px-1 ${editor === kind ? 'text-text underline' : 'hover:text-text'}`}
              aria-pressed={editor === kind}
              onClick={() => switchEditor(kind)}
            >
              {kind === 'textarea' ? '기본' : 'CodeMirror'}
            </button>
          ))}
        </span>
      </div>
      {notes.length === 0 ? (
        <div className="empty-state px-1 py-[18px] text-sm text-muted">
          노트가 없다. 사람과 에이전트가 같이 쓰는 스크래치 패드다 — "+ 새 노트" 로 시작.
        </div>
      ) : (
        notes.map((note) => <NoteCard key={`${note.id}:${editor}`} note={note} editor={editor} />)
      )}
    </section>
  );
}

function NoteCard({ note, editor }: { note: NoteView; editor: NoteEditorKind }) {
  const saveNote = useUiStore((s) => s.saveNote);
  const archiveNote = useUiStore((s) => s.archiveNote);
  const refetch = useUiStore((s) => s.refetch);
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
  const cmHostRef = useRef<HTMLDivElement>(null);
  const syncRef = useRef<NoteSync | null>(null);
  const unbindRef = useRef<(() => void) | null>(null);
  const mountedRef = useRef<MountedEditor | null>(null);
  const lingerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pingRef = useRef<ReturnType<typeof setInterval> | null>(null);

  useEffect(() => {
    setTitle(note.title);
  }, [note.title]);

  // 다른 경로(에이전트·CLI)의 편집이 SSE refetch 로 들어오면, 실시간 세션이 없을 때만 그린다 —
  // 세션이 있으면 그 편집은 이미 문서 스트림으로 들어와 편집기에 그려졌다.
  useEffect(() => {
    if (live === 'idle' && textareaRef.current && textareaRef.current.value !== note.content) {
      textareaRef.current.value = note.content;
      setLines(note.content.split('\n').length);
    }
  }, [note.content, live]);

  const stopLive = async () => {
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
    mountedRef.current?.destroy();
    mountedRef.current = null;
    const sync = syncRef.current;
    syncRef.current = null;
    setOthers([]);
    if (sync) {
      // idle 로 돌아가면 위의 effect 가 textarea 를 note.content 로 덮는다. 마지막 편집이
      // 서버의 2초 이벤트 조절 창 안에 끝났으면 스토어의 note.content 는 그 전 POST 에
      // 머물러 있어, 저장된 글이 되돌아간 것처럼 보인다. 남은 편집을 보내고 목록을 다시
      // 읽어 스토어를 맞춘 뒤에야 idle 이 된다 — 그동안 textarea 는 문서 값을 그대로 든다.
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
    logUsage('web:note-live', { editor });
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
    if (editor === 'codemirror') {
      const host = cmHostRef.current;
      if (!host) {
        stopLive();
        return;
      }
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
      return;
    }
    const el = textareaRef.current;
    if (!el) {
      stopLive();
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
    void sync.ping();
    pingRef.current = setInterval(() => void sync.ping(), PRESENCE_PING_MS);
    setLive('on');
  };

  // 카드가 사라지면 세션도 닫는다.
  // biome-ignore lint/correctness/useExhaustiveDependencies: 언마운트에 한 번 — stopLive 는 ref 만 만져 최신 값이 필요 없다
  useEffect(
    () => () => {
      void stopLive();
    },
    [],
  );

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
      {editor === 'codemirror' ? (
        // 세션 전엔 미리보기, 클릭하면 그 자리에 CodeMirror 가 뜬다(문서를 받은 뒤라야 편집기를
        // 만들 수 있다 — yCollab 은 만들 때의 Y.Text 에 묶인다).
        <div className="note-cm">
          {live !== 'on' && (
            <button
              type="button"
              className="note-cm-preview block w-full border-none bg-transparent p-0 text-left text-sm leading-[1.55] text-muted"
              aria-label={`${note.title} 본문 (클릭해 편집)`}
              onClick={() => void startLive()}
              onFocus={() => void startLive()}
            >
              {note.content || '\u00a0'}
              {live === 'opening' && <span className="text-faint"> …</span>}
            </button>
          )}
          <div ref={cmHostRef} />
        </div>
      ) : (
        <textarea
          ref={textareaRef}
          className="note-content mt-1 w-full resize-y border-none bg-transparent text-sm leading-[1.55] text-muted focus:text-text focus:outline-none"
          defaultValue={note.content}
          // 내용만큼 자란다(상한 없음) — CodeMirror 와 같이 스크롤은 페이지 하나로.
          rows={Math.max(6, lines + 1)}
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
      )}
      <div className="mt-1 font-mono text-micro text-faint">{status}</div>
    </div>
  );
}
