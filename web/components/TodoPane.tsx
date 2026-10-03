import { ChevronDown, ChevronRight } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import type { TodoView } from '../types';
import { resolveDropBefore } from '../lib';
import { useUiStore } from '../store';
import { BoardHeader } from './BoardHeader';
import { TodoItem } from './TodoItem';

/** 전체 보기에서 접어 둔 보드 — 보는 사람마다 다르니 브라우저에만 둔다. */
export const BOARD_COLLAPSED_KEY = 'rocky:todo-boards-collapsed';

function readCollapsedBoards(): Set<string> {
  try {
    const raw = JSON.parse(localStorage.getItem(BOARD_COLLAPSED_KEY) ?? '[]');
    return new Set(Array.isArray(raw) ? raw.filter((v) => typeof v === 'string') : []);
  } catch {
    return new Set();
  }
}

function writeCollapsedBoards(ids: Set<string>): void {
  try {
    localStorage.setItem(BOARD_COLLAPSED_KEY, JSON.stringify([...ids]));
  } catch {
    // 저장 못 해도 이번 화면에서는 접힌다.
  }
}

/**
 * 가운데 메인 — 선택된 보드의 섹션별 todo 트리 (전체 뷰에서는 보드별 그룹).
 * parentId 계층은 그룹 안에서 들여쓰기로 렌더된다.
 */
export function TodoPane() {
  const todos = useUiStore((s) => s.todos);
  const boards = useUiStore((s) => s.boards);
  const sections = useUiStore((s) => s.sections);
  const selected = useUiStore((s) => s.selected);
  const addTodo = useUiStore((s) => s.addTodo);
  const moveTodo = useUiStore((s) => s.moveTodo);
  const [draft, setDraft] = useState('');
  /** 전체 보기에서 접은 보드(id) — 보드가 많으면 한 화면에 다 펼쳐 둘 이유가 없다. */
  const [collapsed, setCollapsed] = useState<Set<string>>(readCollapsedBoards);
  const toggleBoard = (id: string) => {
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (!next.delete(id)) {
        next.add(id);
      }
      writeCollapsedBoards(next);
      return next;
    });
  };
  /** 드래그 중인 todo 와 시작 포인터 — 핸들 pointerdown 에서 세팅, 같은 포인터의
   * pointerup 에서만 해제한다 (멀티터치의 다른 손가락이 드래그를 끊지 않게). */
  const [drag, setDrag] = useState<{ todoId: string; pointerId: number } | null>(null);
  /** 포인터가 올라간 행과 절반 위치 — 삽입선 표시용. */
  const [over, setOver] = useState<{ id: string; after: boolean } | null>(null);
  const overRef = useRef(over);
  overRef.current = over;

  const handleDown = (e: React.PointerEvent, todo: TodoView) => {
    e.preventDefault(); // 핸들에서의 텍스트 선택·스크롤 개시를 막는다 (touch-none 과 한 쌍)
    setDrag({ todoId: todo.id, pointerId: e.pointerId });
  };

  useEffect(() => {
    if (!drag) {
      return;
    }
    const onMove = (e: PointerEvent) => {
      if (e.pointerId !== drag.pointerId) {
        return;
      }
      const row = document
        .elementFromPoint(e.clientX, e.clientY)
        ?.closest<HTMLElement>('[data-todo-id]');
      if (!row?.dataset.todoId) {
        setOver(null);
        return;
      }
      const rect = row.getBoundingClientRect();
      setOver({ id: row.dataset.todoId, after: e.clientY > rect.top + rect.height / 2 });
    };
    const finish = (commit: boolean) => {
      const dropped = overRef.current;
      setDrag(null);
      setOver(null);
      if (!commit || !dropped) {
        return;
      }
      const siblings = todos.map((t) => ({
        id: t.id,
        boardId: t.boardId,
        sectionId: t.sectionId,
        parentId: t.parentId,
      }));
      const target = resolveDropBefore(siblings, drag.todoId, dropped.id, dropped.after);
      if (target) {
        void moveTodo(drag.todoId, target.before);
      }
    };
    const onUp = (e: PointerEvent) => {
      if (e.pointerId === drag.pointerId) {
        finish(true);
      }
    };
    const onCancel = (e: PointerEvent) => {
      if (e.pointerId === drag.pointerId) {
        finish(false);
      }
    };
    window.addEventListener('pointermove', onMove);
    window.addEventListener('pointerup', onUp);
    window.addEventListener('pointercancel', onCancel);
    return () => {
      window.removeEventListener('pointermove', onMove);
      window.removeEventListener('pointerup', onUp);
      window.removeEventListener('pointercancel', onCancel);
    };
  }, [drag, moveTodo, todos]);

  const byId = new Map(todos.map((t) => [t.id, t]));
  const childrenOf = new Map<string, TodoView[]>();
  const roots: TodoView[] = [];
  for (const todo of todos) {
    if (todo.parentId && byId.has(todo.parentId)) {
      const siblings = childrenOf.get(todo.parentId) ?? [];
      siblings.push(todo);
      childrenOf.set(todo.parentId, siblings);
    } else {
      roots.push(todo);
    }
  }

  const renderTree = (items: TodoView[], depth: number, showDone: boolean): React.ReactNode =>
    items
      .filter((todo) => {
        // showDone이 false이면, 자신도 done이고 자손에도 active가 없는 완전 완료 노드는 숨긴다.
        // 단, 자신이 done이라도 하위에 열린 작업(active descendant)이 있으면 부모는 보여야 한다!
        if (!showDone && !hasActiveDescendant(todo, childrenOf)) {
          return false;
        }
        return true;
      })
      .map((todo) => (
        <div
          key={todo.id}
          // 삽입선 — 드래그 중 포인터가 올라간 행의 위(before) 또는 아래(after)에 표시
          className={
            over?.id === todo.id && drag
              ? over.after
                ? 'border-b-2 border-warm'
                : 'border-t-2 border-warm'
              : ''
          }
        >
          <TodoItem todo={todo} depth={depth} onHandleDown={handleDown} />
          {renderTree(childrenOf.get(todo.id) ?? [], depth + 1, showDone)}
        </div>
      ));

  // 그룹핑 — 보드 뷰: 섹션별 / 전체 뷰: 보드별
  const groups: { key: string; title: string; items: TodoView[] }[] = [];
  if (selected === 'all') {
    const boardTitle = new Map(boards.map((b) => [b.id, b.title]));
    for (const board of boards) {
      const items = roots.filter((t) => t.boardId === board.id);
      if (items.length > 0) {
        groups.push({ key: board.id, title: boardTitle.get(board.id) ?? board.key, items });
      }
    }
  } else {
    const noSection = roots.filter((t) => !t.sectionId);
    if (noSection.length > 0) {
      groups.push({ key: '__none', title: '일반', items: noSection });
    }
    // 빈 섹션은 그리지 않는다 — 섹션은 항목을 담을 때만 의미가 있고, 빈 헤더가 쌓이면
    // 노이즈다. 드로어에서 항목을 옮기면 그때 나타난다.
    for (const section of sections) {
      const items = roots.filter((t) => t.sectionId === section.id);
      if (items.length > 0) {
        groups.push({ key: section.id, title: section.title, items });
      }
    }
  }

  // 보드 정체(이름·slug·설명·GitHub)는 그 보드를 보고 있을 때만 의미가 있다. 전체 뷰는
  // 여러 보드를 한 화면에 모으므로 헤더를 그리지 않는다.
  const currentBoard = selected === 'all' ? undefined : boards.find((b) => b.key === selected);

  return (
    <main className="todo-pane min-w-0 flex-1 overflow-y-auto px-[26px] py-4">
      {/*
        `key` 로 보드가 바뀌면 헤더를 새로 만든다. 없으면 React 가 같은 인스턴스를 재사용해
        열려 있던 편집 폼과 그 입력값(직전 보드의 것)이 그대로 남고, 그 상태로 저장하면
        **지금 보고 있는 보드**가 직전 보드의 값으로 덮어써진다(rename 포함).
      */}
      {currentBoard && <BoardHeader key={currentBoard.key} board={currentBoard} />}
      {selected !== 'all' && (
        <form
          className="below-head sticky top-0 z-[1] mb-3.5 border-b border-line/80 bg-bg/95 pb-3 backdrop-blur-xs"
          onSubmit={(e) => {
            e.preventDefault();
            const title = draft.trim();
            if (title === '') {
              return;
            }
            setDraft('');
            void addTodo({ board: selected, title });
          }}
        >
          <input
            className="quick-add-input mb-0 w-full rounded-lg border border-line/80 bg-surface px-3.5 py-2 text-sm text-text shadow-2xs transition-all placeholder:text-faint focus:border-run focus:ring-1 focus:ring-run/30 focus-visible:outline-none"
            placeholder="+ 새 작업 (Enter 로 추가)"
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
          />
        </form>
      )}

      {groups.length === 0 && (
        <div className="empty-state px-1 py-[18px] text-sm text-faint">
          아직 항목이 없다.{' '}
          {selected === 'all'
            ? '보드를 골라 작업을 추가해 보자.'
            : '위 입력창으로 첫 작업을 추가하자.'}
        </div>
      )}

      {groups.map((group) => {
        const label =
          'mb-2 border-b border-line/70 pb-1 font-mono text-chip font-medium uppercase tracking-[0.2em] text-muted';
        // 보드 화면: 섹션별로 분리하고 각 섹션이 독립적인 완료 접기 상태를 가진다.
        if (selected !== 'all') {
          return (
            <TodoSection
              key={group.key}
              title={group.title}
              roots={group.items}
              childrenOf={childrenOf}
              renderTree={renderTree}
              labelClass={label}
            />
          );
        }
        // 전체 보기: 보드별 그룹 접기 및 보드 내 완료된 작업 접기
        const folded = collapsed.has(group.key);
        const boardTodos = todos.filter((t) => t.boardId === group.key);
        return (
          <BoardGroup
            key={group.key}
            title={group.title}
            roots={group.items}
            allBoardTodos={boardTodos}
            childrenOf={childrenOf}
            renderTree={renderTree}
            folded={folded}
            onToggleFold={() => toggleBoard(group.key)}
            labelClass={label}
          />
        );
      })}
    </main>
  );
}

/**
 * 이 항목 또는 그 자손 중에 하나라도 status !== 'done'인 것이 있는지 재귀적으로 확인한다.
 * 자손 중 하나라도 열려 있으면 부모도 완료 접힘 영역으로 숨지 않고 활성 목록에 남아야 한다.
 */
function hasActiveDescendant(todo: TodoView, childrenOf: Map<string, TodoView[]>): boolean {
  if (todo.status !== 'done') {
    return true;
  }
  const children = childrenOf.get(todo.id);
  if (!children || children.length === 0) {
    return false;
  }
  return children.some((child) => hasActiveDescendant(child, childrenOf));
}

interface TodoSectionProps {
  title: string;
  roots: TodoView[];
  childrenOf: Map<string, TodoView[]>;
  renderTree: (items: TodoView[], depth: number, showDone: boolean) => React.ReactNode;
  labelClass: string;
}

/** 한 보드 안의 섹션 — 독립적인 완료 항목(루트 및 하위 작업) 펼침/접힘 상태를 가진다. */
function TodoSection({ title, roots, childrenOf, renderTree, labelClass }: TodoSectionProps) {
  const [showDone, setShowDone] = useState(false);

  // 활성 루트: 자신 또는 자손에 미완료 작업이 남아있는 항목들
  const activeRoots = roots.filter((t) => hasActiveDescendant(t, childrenOf));
  // 완전 완료 루트: 자신과 모든 자손이 전부 완료된 항목들
  const doneRoots = roots.filter((t) => !hasActiveDescendant(t, childrenOf));

  // 이 섹션에 속한 모든 항목 중 완료(done) 상태인 항목의 총 개수 (하위 작업 포함)
  let totalDoneCount = 0;
  const countDone = (list: TodoView[]) => {
    for (const item of list) {
      if (item.status === 'done') {
        totalDoneCount++;
      }
      const children = childrenOf.get(item.id);
      if (children && children.length > 0) {
        countDone(children);
      }
    }
  };
  countDone(roots);

  return (
    <section className="mb-6">
      <div className={labelClass}>{title}</div>
      {renderTree(activeRoots, 0, showDone)}

      {totalDoneCount > 0 && (
        <div className="mt-2 border-t border-line/40 pt-1.5">
          <button
            type="button"
            className="inline-flex items-center gap-1.5 rounded px-1.5 py-1 font-mono text-chip text-muted transition-colors hover:text-text"
            onClick={() => setShowDone((prev) => !prev)}
            aria-expanded={showDone}
          >
            <ChevronRight
              size={12}
              aria-hidden
              className={`transition-transform duration-150 ${showDone ? 'rotate-90' : ''}`}
            />
            <span>완료된 작업 {totalDoneCount}개</span>
          </button>
          {showDone && doneRoots.length > 0 && (
            <div className="mt-1">{renderTree(doneRoots, 0, true)}</div>
          )}
        </div>
      )}
    </section>
  );
}

interface BoardGroupProps {
  title: string;
  roots: TodoView[];
  allBoardTodos: TodoView[];
  childrenOf: Map<string, TodoView[]>;
  renderTree: (items: TodoView[], depth: number, showDone: boolean) => React.ReactNode;
  folded: boolean;
  onToggleFold: () => void;
  labelClass: string;
}

/** 전체 보기 화면에서의 각 보드 그룹 — 보드 자체 접기 + 보드 내 완료된 작업 독립 접기 지원. */
function BoardGroup({
  title,
  roots,
  allBoardTodos,
  childrenOf,
  renderTree,
  folded,
  onToggleFold,
  labelClass,
}: BoardGroupProps) {
  const [showDone, setShowDone] = useState(false);

  // 활성 루트: 자신 또는 자손에 미완료 작업이 남아있는 항목들
  const activeRoots = roots.filter((t) => hasActiveDescendant(t, childrenOf));
  // 완전 완료 루트: 자신과 모든 자손이 전부 완료된 항목들
  const doneRoots = roots.filter((t) => !hasActiveDescendant(t, childrenOf));

  // 이 보드에 속한 모든 항목 중 완료(done) 상태인 항목의 총 개수
  const totalDoneCount = allBoardTodos.filter((t) => t.status === 'done').length;
  const count = allBoardTodos.length;
  const Chevron = folded ? ChevronRight : ChevronDown;

  return (
    <section className={folded ? 'mb-3' : 'mb-6'}>
      <button
        type="button"
        className={`${labelClass} flex w-full items-center gap-1.5 text-left transition-colors hover:text-text`}
        aria-expanded={!folded}
        onClick={onToggleFold}
      >
        <Chevron size={13} aria-hidden className="shrink-0" />
        <span className="min-w-0 truncate">{title}</span>
        <span className="ml-auto tracking-normal tabular-nums text-faint">{count}</span>
      </button>

      {!folded && (
        <>
          {renderTree(activeRoots, 0, showDone)}

          {totalDoneCount > 0 && (
            <div className="mt-2 border-t border-line/40 pt-1.5">
              <button
                type="button"
                className="inline-flex items-center gap-1.5 rounded px-1.5 py-1 font-mono text-chip text-muted transition-colors hover:text-text"
                onClick={() => setShowDone((prev) => !prev)}
                aria-expanded={showDone}
              >
                <ChevronRight
                  size={12}
                  aria-hidden
                  className={`transition-transform duration-150 ${showDone ? 'rotate-90' : ''}`}
                />
                <span>완료된 작업 {totalDoneCount}개</span>
              </button>
              {showDone && doneRoots.length > 0 && (
                <div className="mt-1">{renderTree(doneRoots, 0, true)}</div>
              )}
            </div>
          )}
        </>
      )}
    </section>
  );
}
