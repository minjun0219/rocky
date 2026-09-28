import { useEffect, useState } from 'react';
import { formatClock, nowRows } from '../lib';
import { useUiStore } from '../store';

/** 1초마다 갱신되는 현재 시각 — 경과 열이 초 단위로 흐르게. 행이 없으면 돌지 않는다. */
function useNow(active: boolean): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!active) {
      return;
    }
    const id = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(id);
  }, [active]);
  return now;
}

const STAMP: Record<'run' | 'mine' | 'dead', string> = {
  run: 'border-run text-run',
  mine: 'border-mine bg-mine-soft text-mine',
  dead: 'border-dashed border-dead text-dead',
};

/**
 * "지금" 표 — 첫 화면이 답할 한 가지: 무엇이 돌고 있고, 무엇이 내 차례인가.
 * 보고 있는 보드와 무관하게 전 보드를 본다. 행의 순서·합치기는 `nowRows`(순수)가 정한다.
 */
export function NowTable() {
  const nowTodos = useUiStore((s) => s.nowTodos);
  const handoffs = useUiStore((s) => s.nowHandoffs);
  const seenComments = useUiStore((s) => s.seenComments);
  const collect = useUiStore((s) => s.collect);
  const openTodoDetail = useUiStore((s) => s.openTodoDetail);
  const rows = nowRows({ todos: nowTodos, handoffs, seen: seenComments, collect });
  const now = useNow(rows.some((r) => r.since !== undefined));
  const mine = rows.filter((r) => r.stamp.tone !== 'run').length;

  return (
    <section className="now border-b border-line px-[26px] pb-3 pt-4" aria-label="지금">
      <h2 className="mb-2 flex items-baseline gap-2.5 font-mono text-micro font-medium uppercase tracking-[0.14em] text-muted">
        지금
        {mine > 0 ? (
          <span className="normal-case tracking-normal text-mine">내 차례 {mine}</span>
        ) : null}
      </h2>
      {rows.length === 0 ? (
        <p className="m-0 text-sm text-muted">도는 일도, 내 차례도 없다.</p>
      ) : (
        // 640~900px 사이에서 네 열이 폭을 넘치면 표만 가로로 흐르고 문서는 흐르지 않게.
        <div className="overflow-x-auto">
          <table className="now-table w-full border-collapse overflow-hidden rounded-[10px] border border-line bg-surface text-sm">
            <thead>
              <tr className="bg-surface-2 font-mono text-micro uppercase tracking-[0.12em] text-muted">
                <th className="px-3 py-2 text-left font-medium">항목</th>
                <th className="px-3 py-2 text-left font-medium">누가</th>
                <th className="px-3 py-2 text-left font-medium">경과</th>
                <th className="px-3 py-2 text-left font-medium">상태</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((row) => (
                <tr key={row.key} className="now-row border-t border-line">
                  <td className="now-item px-3 py-2">
                    {row.todoId ? (
                      <button
                        type="button"
                        className="text-left hover:text-mine"
                        onClick={() => void openTodoDetail(row.todoId as string)}
                      >
                        <span className="mr-2 font-mono text-chip text-muted">{row.ref}</span>
                        {row.title}
                      </button>
                    ) : (
                      <span>
                        <span className="mr-2 font-mono text-chip text-muted">{row.ref}</span>
                        {row.title}
                      </span>
                    )}
                    {row.unread > 0 ? (
                      <span className="ml-2 font-mono text-chip text-mine">💬 {row.unread}</span>
                    ) : null}
                  </td>
                  <td className="now-who whitespace-nowrap px-3 py-2 font-mono text-chip text-muted">
                    {row.who}
                  </td>
                  <td className="now-since whitespace-nowrap px-3 py-2 font-mono text-meta tabular-nums text-text">
                    {row.since ? formatClock(row.since, now) : '—'}
                  </td>
                  <td className="now-stamp whitespace-nowrap px-3 py-2">
                    <span
                      className={`inline-block rounded border px-1.5 py-0.5 font-mono text-micro tracking-[0.06em] ${STAMP[row.stamp.tone]}`}
                    >
                      {row.stamp.label}
                    </span>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </section>
  );
}
