import { RefreshCw } from 'lucide-react';
import { useEffect, useState } from 'react';
import { formatAge, parseTurn } from '../lib';
import { api, useUiStore } from '../store';
import type { WorklogEntry } from '../types';
import { logUsage } from '../usage';

/** 한 번에 받는 줄 수 — "더 보기" 가 이만큼씩 더 받는다. */
const PAGE = 50;
const KINDS = ['turn', 'decision', 'blocker', 'answer'] as const;

/**
 * 작업로그 — 고른 보드(그 `path` 의 레포)의 작업 기록을 최신순으로, "전체" 면 전 레포. 데몬의 로그 색인
 * (`GET /api/logs/worklog`, 1분마다 JSONL 에서 옮긴다)을 읽는다. 턴 기록은 요청을 제목처럼, 결과를 둘째 줄로.
 * 실시간 이벤트가 없어(색인은 주기 작업) 열 때와 새로고침 때 다시 받는다.
 */
export function WorklogPane() {
  const actor = useUiStore((s) => s.actor);
  const selected = useUiStore((s) => s.selected);
  const openTodoDetail = useUiStore((s) => s.openTodoDetail);
  const [kind, setKind] = useState('');
  const [query, setQuery] = useState('');
  const [text, setText] = useState('');
  const [entries, setEntries] = useState<WorklogEntry[]>([]);
  const [state, setState] = useState<'loading' | 'ready' | 'unlinked' | 'error'>('loading');
  const [more, setMore] = useState(false);
  const [reload, setReload] = useState(0);
  const [expanded, setExpanded] = useState<Set<string>>(() => new Set());

  // 검색어는 치는 동안 묻지 않는다 — 멈춘 뒤 300ms.
  useEffect(() => {
    const timer = setTimeout(() => setQuery(text.trim()), 300);
    return () => clearTimeout(timer);
  }, [text]);

  const url = (before?: string) => {
    const params = new URLSearchParams({ limit: String(PAGE) });
    if (selected !== 'all') {
      params.set('board', selected);
    }
    if (kind) {
      params.set('kind', kind);
    }
    if (query) {
      params.set('q', query);
    }
    if (before) {
      params.set('before', before);
    }
    return `/api/logs/worklog?${params.toString()}`;
  };

  // biome-ignore lint/correctness/useExhaustiveDependencies: url 은 이 값들로만 만들어진다 — reload 는 새로고침 신호
  useEffect(() => {
    let live = true;
    setState('loading');
    api<{ entries: WorklogEntry[]; unlinked?: boolean }>(url(), actor)
      .then((body) => {
        if (!live) {
          return;
        }
        setEntries(body.entries ?? []);
        setMore((body.entries ?? []).length === PAGE);
        setState(body.unlinked ? 'unlinked' : 'ready');
      })
      .catch(() => live && setState('error'));
    return () => {
      live = false;
    };
  }, [selected, kind, query, actor, reload]);

  const loadMore = async () => {
    const last = entries.at(-1);
    if (!last) {
      return;
    }
    const body = await api<{ entries: WorklogEntry[] }>(url(last.timestamp), actor).catch(
      () => null,
    );
    if (body) {
      setEntries((prev) => [...prev, ...body.entries]);
      setMore(body.entries.length === PAGE);
    }
  };

  const now = Date.now();
  return (
    <main className="min-w-0 flex-1 overflow-y-auto px-4 py-3" aria-label="작업로그">
      <div className="mb-3 flex flex-wrap items-center gap-2">
        <input
          className="min-w-0 flex-1 rounded-lg border border-line bg-surface px-3 py-[7px] text-sm text-text placeholder:text-faint"
          placeholder="찾기 — 본문에 들어간 글자"
          value={text}
          onChange={(e) => setText(e.target.value)}
          aria-label="작업로그 찾기"
        />
        <select
          className="rounded-lg border border-line bg-surface px-2 py-[7px] text-sm text-text"
          value={kind}
          onChange={(e) => setKind(e.target.value)}
          aria-label="종류"
        >
          <option value="">모든 종류</option>
          {KINDS.map((k) => (
            <option key={k} value={k}>
              {k}
            </option>
          ))}
        </select>
        <button
          type="button"
          className="flex size-8 items-center justify-center rounded-md text-muted hover:bg-surface-2 hover:text-text"
          aria-label="새로고침"
          onClick={() => setReload((n) => n + 1)}
        >
          <RefreshCw size={15} aria-hidden />
        </button>
      </div>
      {state === 'unlinked' ? (
        <p className="m-0 text-meta text-faint">
          이 보드에 폴더(path)가 없어 어느 레포의 기록인지 모른다 — 보드 설정에서 path 를 걸면
          보인다.
        </p>
      ) : state === 'error' ? (
        <p className="m-0 text-meta text-faint">작업로그를 못 읽었다 — 데몬을 확인하고 새로고침.</p>
      ) : state === 'ready' && entries.length === 0 ? (
        <p className="m-0 text-meta text-faint">기록 없음</p>
      ) : (
        <ul className="m-0 list-none overflow-hidden rounded-[10px] border border-line bg-surface p-0">
          {entries.map((entry) => {
            const turn = parseTurn(entry.content);
            const title = turn.req || turn.did;
            const second = turn.req ? turn.did : '';
            const meta = [
              entry.kind,
              selected === 'all' ? entry.projectKey.replace(/-[0-9a-f]{8}$/, '') : '',
              turn.tools,
              formatAge(entry.timestamp, now),
            ].filter(Boolean);
            return (
              <li key={entry.id} className="border-t border-line px-3 py-2 first:border-t-0">
                <div className="now-title text-sm leading-[1.45] text-text">{title}</div>
                {second ? (
                  // 결과는 길다(마지막 답 전체) — 세 줄까지, 누르면 펼친다.
                  <button
                    type="button"
                    // line-clamp 는 display:-webkit-box 라 block 과 같이 두면 진다 — 펼쳤을 때만 block.
                    className={`mt-0.5 w-full border-0 bg-transparent p-0 text-left text-meta text-muted ${
                      expanded.has(entry.id) ? 'block' : 'line-clamp-3'
                    }`}
                    aria-expanded={expanded.has(entry.id)}
                    onClick={() =>
                      setExpanded((prev) => {
                        const next = new Set(prev);
                        if (!next.delete(entry.id)) {
                          next.add(entry.id);
                        }
                        return next;
                      })
                    }
                  >
                    {second}
                  </button>
                ) : null}
                <div className="mt-0.5 flex flex-wrap items-center gap-x-2 font-mono text-chip text-faint">
                  <span className="truncate">{meta.join(' · ')}</span>
                  {entry.todoRef ? (
                    <button
                      type="button"
                      className="chip"
                      onClick={() => {
                        logUsage('web:worklog-todo', {});
                        void openTodoDetail(entry.todoRef as string);
                      }}
                    >
                      {entry.todoRef}
                    </button>
                  ) : null}
                </div>
              </li>
            );
          })}
        </ul>
      )}
      {state === 'ready' && more ? (
        <button
          type="button"
          className="mt-3 w-full rounded-lg border border-line py-2 text-sm text-muted hover:text-text"
          onClick={() => void loadMore()}
        >
          더 보기
        </button>
      ) : null}
    </main>
  );
}
