import { ChevronDown, ChevronRight } from 'lucide-react';
import { useEffect, useState } from 'react';
import { failingSurfaces, slowSurfaces } from '../lib';
import { api, useUiStore } from '../store';
import type { LogStats } from '../types';

const DAYS = 30;
const WEEKDAY = ['일', '월', '화', '수', '목', '금', '토'];

/**
 * 통계 — 작업로그 탭 맨 위, 접어 둔다(열 때만 묻는다). 반복해서 보는 질문만 둔다(설계: 처음엔 다섯 개):
 * 회고 — 레포별·요일별 턴, 턴이 몰린 할 일. 개선 — 느린 표면·실패하는 표면·쓰지 않은 표면(`rocky usage` 와 같은 집계).
 */
export function StatsPanel() {
  const actor = useUiStore((s) => s.actor);
  const openTodoDetail = useUiStore((s) => s.openTodoDetail);
  const [open, setOpen] = useState(false);
  const [stats, setStats] = useState<LogStats | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    if (!open || stats) {
      return;
    }
    let live = true;
    api<LogStats>(`/api/logs/stats?days=${DAYS}`, actor)
      .then((body) => live && setStats(body))
      .catch(() => live && setFailed(true));
    return () => {
      live = false;
    };
  }, [open, stats, actor]);

  const Chevron = open ? ChevronDown : ChevronRight;
  const head = 'm-0 mb-1 font-mono text-chip font-medium text-faint';
  const row = 'flex items-baseline gap-2 text-meta';
  return (
    <section className="mb-3 rounded-[10px] border border-line bg-surface" aria-label="통계">
      <button
        type="button"
        className="flex w-full items-center gap-1.5 px-3 py-2 text-left font-mono text-chip text-muted hover:text-text"
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
      >
        <Chevron size={13} aria-hidden />
        통계 · 최근 {DAYS}일
      </button>
      {open ? (
        <div className="grid gap-4 border-t border-line px-3 py-3 sm:grid-cols-2">
          {failed ? (
            <p className="m-0 text-meta text-faint">
              통계를 읽지 못했다. 데몬의 로그 색인을 확인하자.
            </p>
          ) : !stats ? (
            <p className="m-0 text-meta text-faint">읽는 중…</p>
          ) : (
            <>
              <div>
                <h3 className={head}>회고 · 턴 {stats.worklog.turns}</h3>
                {stats.worklog.byProject.slice(0, 5).map(([key, n]) => (
                  <div key={key} className={row}>
                    <span className="min-w-0 flex-1 truncate">
                      {key.replace(/-[0-9a-f]{8}$/, '')}
                    </span>
                    <span className="font-mono tabular-nums text-faint">{n}</span>
                  </div>
                ))}
                <div className="mt-2 flex gap-2 font-mono text-chip text-muted">
                  {stats.worklog.byWeekday.map((n, i) => (
                    <span key={WEEKDAY[i]}>
                      {WEEKDAY[i]} <span className="tabular-nums text-faint">{n}</span>
                    </span>
                  ))}
                </div>
                {stats.worklog.byTodo.length > 0 ? (
                  <div className="mt-2 flex flex-wrap gap-1">
                    {stats.worklog.byTodo.slice(0, 5).map(([ref, n]) => (
                      <button
                        key={ref}
                        type="button"
                        className="chip"
                        onClick={() => void openTodoDetail(ref)}
                      >
                        {ref} · {n}
                      </button>
                    ))}
                  </div>
                ) : null}
              </div>
              <div>
                <h3 className={head}>rocky 개선</h3>
                {slowSurfaces(stats.usage.surfaces).map((s) => (
                  <div key={`slow-${s.name}`} className={row}>
                    <span className="min-w-0 flex-1 truncate font-mono text-chip">{s.name}</span>
                    <span className="font-mono text-chip tabular-nums text-faint">
                      p50 {s.p50Ms}ms · p95 {s.p95Ms}ms
                    </span>
                  </div>
                ))}
                {failingSurfaces(stats.usage.surfaces).map((s) => (
                  <div key={`fail-${s.name}`} className={row}>
                    <span className="min-w-0 flex-1 truncate font-mono text-chip">{s.name}</span>
                    <span className="font-mono text-chip tabular-nums text-dead">
                      실패 {s.errors}/{s.count}
                    </span>
                  </div>
                ))}
                <details className="mt-1 text-meta text-muted">
                  <summary>쓰지 않은 표면 {stats.usage.unused.length}개</summary>
                  <p className="m-0 mt-1 font-mono text-chip text-faint">
                    {stats.usage.unused.map(([, name]) => name).join(' · ')}
                  </p>
                </details>
              </div>
            </>
          )}
        </div>
      ) : null}
    </section>
  );
}
