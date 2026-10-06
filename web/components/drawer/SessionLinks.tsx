import { useEffect, useState } from 'react';
import { formatStamp } from '../../lib';
import { api, useUiStore } from '../../store';
import type { HandoffPhase, HandoffView, SessionRow, TodoView } from '../../types';

const PHASE_LABEL: Record<HandoffPhase, string> = {
  pending: '대기 중',
  delivered: '받음 · 착수 전',
  accepted: '착수',
  completed: '완료',
  cancelled: '취소',
};

/** 보낸 기록은 최근 것부터 이만큼만 — 나머지는 "외 N건" 으로 접는다. */
const HISTORY_LIMIT = 5;

/**
 * 세션 목록의 한 행을 사람이 읽는 상태로. 목록을 못 얻었으면 undefined — "모름" 을 "없음" 으로
 * 그리지 않는다. background 세션의 수명 상태(`state`)가 턴 상태(`status`)보다 먼저다.
 */
function sessionStateLabel(row: SessionRow | undefined, available: boolean): string | undefined {
  if (!available) {
    return undefined;
  }
  if (!row) {
    return '세션 없음';
  }
  if (row.state === 'done') {
    return '끝남';
  }
  if (row.state === 'blocked') {
    return '답을 기다림';
  }
  return row.status === 'busy' ? '작업 중' : '쉬는 중';
}

/**
 * 이 할 일과 Claude Code 세션의 연결 — 지금 진행을 든 세션과, 이 할 일을 보낸 기록(핸드오프 전부).
 * 둘 다 없으면 아무것도 그리지 않는다. 진행 세션은 서버가 세션을 아는 경로(받은 핸드오프 ·
 * 스스로 착수한 것을 훅이 붙임)로만 채워지므로, 손으로 시작한 진행에는 이 줄이 없다.
 */
export function SessionLinks({ todo }: { todo: TodoView }) {
  const actor = useUiStore((s) => s.actor);
  // 열린 핸드오프는 SSE 로 다시 읽힌다 — 이 할 일 몫이 바뀌면(보냄·집어감·취소) 기록도 다시 읽는다.
  const openKey = useUiStore((s) =>
    s.handoffs
      .filter((h) => h.todoId === todo.id)
      .map((h) => `${h.id}:${h.status}`)
      .join(','),
  );
  const holder = todo.status === 'doing' ? todo.doingSessionId : undefined;
  const [handoffs, setHandoffs] = useState<HandoffView[]>([]);
  const [sessions, setSessions] = useState<{ available: boolean; list: SessionRow[] }>({
    available: false,
    list: [],
  });

  // biome-ignore lint/correctness/useExhaustiveDependencies: status·updatedAt·openKey 는 다시 읽을 신호로만 쓴다
  useEffect(() => {
    setHandoffs([]);
    let live = true;
    api<HandoffView[]>(`/api/handoffs?todo=${encodeURIComponent(todo.id)}`, actor)
      .then((list) => {
        if (live) {
          setHandoffs(list);
        }
      })
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [todo.id, todo.status, todo.updatedAt, openKey, actor]);

  useEffect(() => {
    setSessions({ available: false, list: [] });
    if (!holder) {
      return;
    }
    let live = true;
    api<{ available: boolean; sessions: SessionRow[] }>('/api/sessions', actor)
      .then((result) => {
        if (live) {
          setSessions({ available: result.available, list: result.sessions ?? [] });
        }
      })
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [holder, actor]);

  const history = [...handoffs].sort((a, b) => b.createdAt.localeCompare(a.createdAt));
  if (!holder && history.length === 0) {
    return null;
  }
  const holderRow = holder ? sessions.list.find((s) => s.sessionId === holder) : undefined;
  const holderName =
    holderRow?.name ??
    handoffs.find((h) => h.sessionId === holder)?.sessionName ??
    holder?.slice(0, 8);
  const holderState = holder ? sessionStateLabel(holderRow, sessions.available) : undefined;

  return (
    <section aria-label="연결된 세션">
      <div className="drawer-section-label">세션</div>
      <ul className="flex flex-col gap-1 text-meta">
        {holder ? (
          <li className="flex flex-wrap items-baseline gap-x-1.5" title={holderRow?.cwd}>
            <span className="text-muted">진행</span>
            <span className="min-w-0 truncate text-text">{holderName}</span>
            {holderState ? <span className="text-muted">· {holderState}</span> : null}
            <span className="text-faint">
              · {todo.doingSessionClaimed ? '스스로 착수' : '보내서 받음'}
            </span>
          </li>
        ) : null}
        {history.slice(0, HISTORY_LIMIT).map((h) => (
          <li key={h.id} className="flex flex-wrap items-baseline gap-x-1.5" title={h.sessionCwd}>
            <span className="font-mono text-chip tabular-nums text-faint">
              {formatStamp(h.createdAt)}
            </span>
            <span className="min-w-0 truncate text-text">
              {h.sessionName ?? h.sessionId.slice(0, 8)}
            </span>
            <span className="text-muted">· {PHASE_LABEL[h.phase]}</span>
          </li>
        ))}
        {history.length > HISTORY_LIMIT ? (
          <li className="text-faint">외 {history.length - HISTORY_LIMIT}건</li>
        ) : null}
      </ul>
    </section>
  );
}
