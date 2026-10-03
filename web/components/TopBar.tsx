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

import { useNowRows } from './NowTable';
import { ViewSwitch } from './ViewSwitch';

/**
 * 머리줄 — 42px 초슬림 단일 바.
 * 보드 스위처 · 세그먼트 탭(ViewSwitch) · 실시간 에이전트 인디케이터 · (끊겼을 때만) 연결 표시 · `⋯` 메뉴 통합.
 */
export function TopBar() {
  const connected = useUiStore((s) => s.connected);
  const nowRows = useNowRows();
  const runCount = nowRows.filter((r) => r.group === 'run').length;

  return (
    <header className="topbar relative w-full max-w-full min-w-0 overflow-hidden sm:overflow-visible border-b border-line bg-surface/95 select-none backdrop-blur-xs">
      {/* 1행: 데스크톱에서는 3개 슬롯(좌-중-우), 모바일에서는 좌(보드)-우(메뉴) 상단 바 */}
      <div className="flex h-10 w-full min-w-0 items-center justify-between px-3">
        <div className="flex shrink-0 items-center">
          <BoardSwitcher />
        </div>

        {/* 데스크톱 세그먼트 탭 (>= 640px) */}
        <div className="hidden sm:flex sm:items-center sm:justify-center">
          <ViewSwitch />
        </div>

        {/* 우측 슬롯 (모바일/데스크톱 공통: 에이전트 인디케이터 + 연결 상태 + 메뉴) */}
        <div className="flex shrink-0 items-center gap-1.5">
          {runCount > 0 ? (
            <span
              className="status-dot-badge inline-flex items-center gap-1.5 rounded-[5px] bg-run-soft px-2 py-0.5 font-mono text-[11px] font-semibold tabular-nums text-run"
              title={`실행 중인 에이전트 ${runCount}개`}
            >
              <span className="size-1.5 animate-pulse rounded-full bg-run" aria-hidden />
              {runCount}
            </span>
          ) : null}
          {connected ? null : (
            <span
              className="link-status inline-flex items-center gap-1 font-mono text-chip text-dead"
              title="데몬과의 실시간 연결이 끊겼다 — 다시 붙는 중. 보이는 내용은 마지막으로 받은 것이다."
              role="status"
            >
              <span className="size-1.5 rounded-full bg-current" aria-hidden />
              <span className="hidden sm:inline">연결 끊김</span>
            </span>
          )}
          <HeaderMenu />
        </div>
      </div>

      {/* 2행: 모바일 전용 세그먼트 서브탭 (< 640px) */}
      <div className="flex sm:hidden w-full min-w-0 items-center justify-center border-t border-line/50 px-2 py-1 bg-surface-2/20">
        <ViewSwitch />
      </div>
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
  const showGithub = useUiStore((s) => s.showGithub);
  const setShowGithub = useUiStore((s) => s.setShowGithub);
  const themePref = useUiStore((s) => s.themePref);
  const setThemePref = useUiStore((s) => s.setThemePref);
  const setSelected = useUiStore((s) => s.setSelected);
  const [open, setOpen] = useState(false);
  const [draft, setDraft] = useState(actor);
  const rootRef = useRef<HTMLDivElement>(null);
  // 바깥 클릭으로 닫힐 때 입력칸이 blur 보다 먼저 사라진다 — 그 순간의 draft 를 저장하려고
  // 최신 값을 ref 로 들고 있는다(effect 는 열릴 때 한 번만 건다).
  const saveRef = useRef<() => void>(() => {});

  useEffect(() => {
    if (!open) {
      return;
    }
    const onDown = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) {
        saveRef.current();
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
  saveRef.current = saveActor;

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
          <label className="flex min-h-8 cursor-pointer items-center gap-2 text-text">
            <input
              type="checkbox"
              checked={showGithub}
              onChange={(e) => setShowGithub(e.target.checked)}
            />
            GitHub 탭 보기
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
