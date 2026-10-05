import { ChevronDown } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import { useUiStore } from '../store';

/**
 * 보드 스위처 — 머리줄의 버튼(지금 보드 이름 또는 "전체"). 누르면 보드 목록이 패널 폭 전체로
 * 펼쳐진다. 가로 탭 줄은 보드 10개가 360px 에 안 들어가 잘리고 넘겨야 했다(`web/DESIGN.md`
 * "Components" 보드 스위처). 보드 추가도 여기서.
 */
export function BoardSwitcher() {
  const boards = useUiStore((s) => s.boards);
  const nowTodos = useUiStore((s) => s.nowTodos);
  const selected = useUiStore((s) => s.selected);
  const setSelected = useUiStore((s) => s.setSelected);
  const createBoard = useUiStore((s) => s.createBoard);

  const [open, setOpen] = useState(false);
  const [adding, setAdding] = useState(false);
  const [key, setKey] = useState('');
  const [error, setError] = useState<string | null>(null);
  const rootRef = useRef<HTMLDivElement>(null);

  // 보드마다 진행중 개수 — 전 보드 기준(`nowTodos`)이라 보고 있는 보드와 무관하다.
  const doing = new Map<string, number>();
  for (const t of nowTodos) {
    if (t.status === 'doing' && !t.archivedAt) {
      doing.set(t.boardId, (doing.get(t.boardId) ?? 0) + 1);
    }
  }
  const current =
    selected === 'all' ? '전체' : (boards.find((b) => b.key === selected)?.title ?? selected);

  const closeAdd = () => {
    setAdding(false);
    setKey('');
    setError(null);
  };
  const close = () => {
    setOpen(false);
    closeAdd();
  };

  useEffect(() => {
    if (!open) {
      return;
    }
    // 바깥을 누르거나 Esc — 목록과 추가 입력을 함께 닫는다(setter 는 안정적이라 의존성이 없다).
    const dismiss = () => {
      setOpen(false);
      setAdding(false);
      setKey('');
      setError(null);
    };
    const onDown = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) {
        dismiss();
      }
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        dismiss();
      }
    };
    document.addEventListener('mousedown', onDown);
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('mousedown', onDown);
      document.removeEventListener('keydown', onKey);
    };
  }, [open]);

  const pick = (board: string) => {
    close();
    setSelected(board);
  };

  /**
   * 서버가 key 를 거절하면(공백·`#` 은 참조로 쓸 수 없다) 그 이유를 그대로 보여주고
   * 입력을 유지한다 — 조용히 닫으면 왜 안 만들어졌는지 알 수 없다.
   */
  const submit = async () => {
    const next = key.trim();
    if (next === '') {
      closeAdd();
      return;
    }
    try {
      await createBoard(next);
      close();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const itemClass = (active: boolean) =>
    `flex min-h-10 w-full items-center justify-between gap-2 px-3 text-left text-sm ${
      active ? 'bg-surface-2 font-semibold text-text' : 'text-text hover:bg-surface-2'
    }`;

  return (
    <div className="board-switcher" ref={rootRef}>
      <button
        type="button"
        className="inline-flex min-h-8 max-w-[60vw] items-center gap-1 rounded-md px-2 text-sm font-semibold text-text hover:bg-surface-2"
        aria-haspopup="true"
        aria-expanded={open}
        aria-label={`보드: 지금 ${current}. 눌러서 바꾸기`}
        onClick={() => (open ? close() : setOpen(true))}
      >
        <span className="truncate">{current}</span>
        <ChevronDown size={14} aria-hidden className="shrink-0 text-muted" />
      </button>
      {open ? (
        <div
          className="board-switcher-sheet absolute inset-x-0 top-full z-30 max-h-[70vh] overflow-y-auto border-b border-line bg-surface"
          role="menu"
          aria-label="보드"
        >
          <button
            type="button"
            role="menuitemradio"
            aria-checked={selected === 'all'}
            className={itemClass(selected === 'all')}
            onClick={() => pick('all')}
          >
            전체
          </button>
          {boards.map((board) => {
            const n = doing.get(board.id) ?? 0;
            return (
              <button
                key={board.id}
                type="button"
                role="menuitemradio"
                aria-checked={selected === board.key}
                className={`${itemClass(selected === board.key)} border-t border-line`}
                onClick={() => pick(board.key)}
              >
                <span className="truncate">{board.title}</span>
                {n > 0 ? (
                  <span className="shrink-0 font-mono text-chip tabular-nums text-run">
                    진행 {n}
                  </span>
                ) : null}
              </button>
            );
          })}
          <div className="border-t border-line px-3 py-2">
            {adding ? (
              <>
                <input
                  className="min-h-8 w-full rounded-md border border-line bg-bg px-2.5 text-sm text-text"
                  value={key}
                  placeholder="보드 이름 (레포 이름 권장)"
                  aria-label="새 보드 이름"
                  // biome-ignore lint/a11y/noAutofocus: 버튼을 눌러 진입한 입력이라 즉시 타이핑이 기대 동작
                  autoFocus
                  onChange={(e) => {
                    setKey(e.target.value);
                    setError(null);
                  }}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') {
                      void submit();
                    } else if (e.key === 'Escape') {
                      e.stopPropagation();
                      closeAdd();
                    }
                  }}
                />
                {/* 생성 실패 사유는 즉시 읽혀야 한다 — 보이기만 하면 스크린리더가 놓친다. */}
                {error && (
                  <div className="px-0.5 pt-1 text-meta leading-[1.4] text-p1" role="alert">
                    {error}
                  </div>
                )}
              </>
            ) : (
              <button
                type="button"
                className="min-h-8 text-sm text-muted hover:text-text"
                onClick={() => setAdding(true)}
              >
                + 새 보드
              </button>
            )}
          </div>
        </div>
      ) : null}
    </div>
  );
}
