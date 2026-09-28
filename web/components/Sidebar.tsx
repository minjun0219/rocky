import { useEffect, useRef, useState } from 'react';
import { useUiStore } from '../store';

/** 좌측 보드 목록 — 전체 뷰 + 보드별 뷰 전환, 보드 생성. */
export function Sidebar() {
  const boards = useUiStore((s) => s.boards);
  const todos = useUiStore((s) => s.todos);
  const selected = useUiStore((s) => s.selected);
  const setSelected = useUiStore((s) => s.setSelected);
  const createBoard = useUiStore((s) => s.createBoard);

  const [adding, setAdding] = useState(false);
  const [key, setKey] = useState('');
  const [error, setError] = useState<string | null>(null);

  const doingBoards = new Set(todos.filter((t) => t.status === 'doing').map((t) => t.boardId));

  const closeAdd = () => {
    setAdding(false);
    setKey('');
    setError(null);
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
      closeAdd();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  // 좁은 화면에서 사이드바는 가로 스크롤 칩 행이라, 현재 보드가 화면 밖(오른쪽)에 있으면
  // 어느 보드를 보고 있는지 알 길이 없다(390px 실측: 13번째 칩이 x=931). 선택이 바뀔 때마다
  // 활성 칩을 보이는 곳으로 당긴다. 세로 목록(데스크톱)에서도 무해하다.
  const activeRef = useRef<HTMLButtonElement | null>(null);
  // biome-ignore lint/correctness/useExhaustiveDependencies: selected 가 바뀌거나 boards 가 도착해야 ref 가 실제 칩에 붙는다 — 둘 다 재실행 조건이다
  useEffect(() => {
    const el = activeRef.current;
    if (el && typeof el.scrollIntoView === 'function') {
      el.scrollIntoView({ block: 'nearest', inline: 'center' });
    }
  }, [selected, boards]);

  return (
    <nav className="sidebar flex flex-col gap-0.5 overflow-y-auto border-r border-line px-2.5 py-4">
      <div className="sidebar-label">BOARDS</div>
      <button
        type="button"
        ref={selected === 'all' ? activeRef : undefined}
        className={`board-item flex items-center gap-2 rounded-md px-2.5 py-1.5 text-left ${selected === 'all' ? 'bg-surface-2 font-semibold text-text' : 'text-muted hover:bg-surface hover:text-text'}`}
        onClick={() => setSelected('all')}
      >
        전체
      </button>
      {boards.map((board) => (
        <button
          key={board.id}
          type="button"
          ref={selected === board.key ? activeRef : undefined}
          className={`board-item flex items-center gap-2 rounded-md px-2.5 py-1.5 text-left ${selected === board.key ? 'bg-surface-2 font-semibold text-text' : 'text-muted hover:bg-surface hover:text-text'}`}
          onClick={() => setSelected(board.key)}
        >
          {board.title}
          {doingBoards.has(board.id) && (
            <span className="doing-dot size-1.5 rounded-full bg-warm" title="처리중인 항목 있음" />
          )}
        </button>
      ))}
      {adding ? (
        <div className="py-0.5">
          <input
            className="w-full rounded-md border border-warm-dim bg-surface px-2.5 py-1.5 text-sm text-text"
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
        </div>
      ) : (
        <button
          type="button"
          className="board-item flex items-center gap-2 rounded-md px-2.5 py-1.5 text-left text-muted hover:bg-surface hover:text-text text-faint"
          onClick={() => setAdding(true)}
        >
          + 새 보드
        </button>
      )}
    </nav>
  );
}
