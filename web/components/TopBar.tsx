import { MoreHorizontal } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import type { ThemePref } from '../lib';
import { useUiStore } from '../store';
import { logUsage } from '../usage';
import { BoardSwitcher } from './BoardSwitcher';

const THEME_LABEL: Record<ThemePref, string> = {
  auto: '자동',
  light: '라이트',
  dark: '다크',
};

/**
 * 머리줄 — 한 줄(40px 이하). 보드 스위처 · (끊겼을 때만) 연결 표시 · `⋯` 메뉴.
 * 예전엔 워드마크·`LINK ♪`·활동 띠·테마 아이콘·보관됨 체크박스·이름 버튼이 설명 없이 늘어서
 * 360px 에서 두 줄로 접혔다(`web/DESIGN.md` "Layout", Known Gaps). 드물게 쓰는 것은 메뉴로.
 */
export function TopBar() {
  const connected = useUiStore((s) => s.connected);
  return (
    <header className="topbar relative flex min-h-10 items-center gap-2 border-b border-line bg-surface px-3 py-1">
      <BoardSwitcher />
      <div className="flex-1" />
      {connected ? null : (
        <span
          className="link-status inline-flex items-center gap-1.5 font-mono text-chip text-dead"
          title="데몬과의 실시간 연결이 끊겼다 — 다시 붙는 중. 보이는 내용은 마지막으로 받은 것이다."
          role="status"
        >
          <span className="size-1.5 rounded-full bg-current" aria-hidden />
          연결 끊김
        </span>
      )}
      <HeaderMenu />
    </header>
  );
}

/**
 * `⋯` 메뉴 — 자주 안 쓰는 것들: 테마, 보관된 항목 보기, 편집자 이름, 새로고침·전체 보기
 * (cmux Dock 에 주소창 없이 띄우면 브라우저의 새로고침·홈이 없다 — DESIGN.md "Environment").
 */
function HeaderMenu() {
  const actor = useUiStore((s) => s.actor);
  const setActor = useUiStore((s) => s.setActor);
  const showArchived = useUiStore((s) => s.showArchived);
  const setShowArchived = useUiStore((s) => s.setShowArchived);
  const themePref = useUiStore((s) => s.themePref);
  const setThemePref = useUiStore((s) => s.setThemePref);
  const setSelected = useUiStore((s) => s.setSelected);
  const [open, setOpen] = useState(false);
  const [draft, setDraft] = useState(actor);
  const rootRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) {
      return;
    }
    const onDown = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) {
        setOpen(false);
      }
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        setOpen(false);
      }
    };
    document.addEventListener('mousedown', onDown);
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('mousedown', onDown);
      document.removeEventListener('keydown', onKey);
    };
  }, [open]);

  const saveActor = () => {
    const next = draft.trim();
    if (next !== '' && next !== actor) {
      setActor(next);
    }
  };

  return (
    <div className="relative" ref={rootRef}>
      <button
        type="button"
        className="header-menu-button inline-flex size-8 items-center justify-center rounded-md text-muted hover:bg-surface-2 hover:text-text"
        aria-label="메뉴"
        aria-expanded={open}
        onClick={() => {
          setDraft(actor);
          setOpen((v) => !v);
        }}
      >
        <MoreHorizontal size={18} aria-hidden />
      </button>
      {open ? (
        <div
          className="header-menu absolute right-0 top-full z-30 mt-1 flex w-64 flex-col gap-3 rounded-[10px] border border-line bg-surface p-3 text-sm"
          role="menu"
        >
          <fieldset className="m-0 flex items-center gap-1 border-0 p-0">
            <legend className="mb-1 font-mono text-chip text-faint">테마</legend>
            {(['auto', 'light', 'dark'] as const).map((pref) => (
              <button
                key={pref}
                type="button"
                role="menuitemradio"
                aria-checked={themePref === pref}
                className={`min-h-8 flex-1 rounded-md px-2 ${themePref === pref ? 'bg-surface-2 font-semibold text-text' : 'text-muted hover:text-text'}`}
                onClick={() => {
                  logUsage('web:theme');
                  setThemePref(pref);
                }}
              >
                {THEME_LABEL[pref]}
              </button>
            ))}
          </fieldset>
          <label className="flex min-h-8 cursor-pointer items-center gap-2 text-text">
            <input
              type="checkbox"
              checked={showArchived}
              onChange={(e) => setShowArchived(e.target.checked)}
            />
            보관된 항목도 보기
          </label>
          <label className="flex flex-col gap-1">
            <span className="font-mono text-chip text-faint">
              편집자 이름 — 웹에서 고친 것은 이 이름으로 남는다
            </span>
            <input
              aria-label="편집자 이름"
              className="actor-input min-h-8 rounded-md border border-line bg-bg px-2 font-mono text-meta text-text"
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
              onBlur={saveActor}
              onKeyDown={(e) => {
                if (e.key === 'Enter') {
                  saveActor();
                }
              }}
            />
          </label>
          <div className="flex gap-2 border-t border-line pt-2">
            <button
              type="button"
              role="menuitem"
              className="min-h-8 flex-1 rounded-md px-2 text-muted hover:bg-surface-2 hover:text-text"
              onClick={() => {
                setOpen(false);
                setSelected('all');
              }}
            >
              전체 보기
            </button>
            <button
              type="button"
              role="menuitem"
              className="min-h-8 flex-1 rounded-md px-2 text-muted hover:bg-surface-2 hover:text-text"
              onClick={() => window.location.reload()}
            >
              새로고침
            </button>
          </div>
        </div>
      ) : null}
    </div>
  );
}
