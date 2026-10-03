import { useCallback, useEffect, useState } from 'react';
import { formatAge } from '../lib';
import { api, useUiStore } from '../store';
import type { DeliveryStatus } from '../types';

/** 알림 종류 → 사람이 읽는 말. */
const KIND: Record<string, string> = {
  'pr-ready': '머지 후보',
  'pr-conflict': '충돌',
  'pr-merged': '머지됨',
  'pr-ci-failed': 'CI 실패',
  'pr-review': '리뷰 도착',
  inbox: '수집함',
};

/** 세션 이름 — 작업 폴더 이름 + id 앞 8자. 세션 id 만으로는 어느 세션인지 모른다. */
function sessionLabel(sessionId: string, cwd?: string): string {
  const dir = cwd?.split('/').filter(Boolean).pop();
  return dir ? `${dir} · ${sessionId.slice(0, 8)}` : sessionId.slice(0, 8);
}

/**
 * 세션 전달 — 데몬이 PR 알림·수집함 알림을 어느 세션에 보내는지, 최근에 무엇을 보냈는지, 그리고 세션별
 * "보내지 않기"(그 보드의 다음 세션이 받는다)와 수집함 구독 해지. 세션 id 가 드러나는 화면이라 로컬에서 연
 * 화면에서만 보인다(서버가 403). 이벤트로 따라가지 않고 열 때와 "새로고침" 때 읽는다.
 */
export function SessionDelivery() {
  const actor = useUiStore((s) => s.actor);
  const [status, setStatus] = useState<DeliveryStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [now, setNow] = useState(() => Date.now());

  const load = useCallback(async () => {
    try {
      setStatus(await api<DeliveryStatus>('/api/deliveries', actor));
      setError(null);
      setNow(Date.now());
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [actor]);

  useEffect(() => {
    void load();
  }, [load]);

  const act = async (run: () => Promise<unknown>) => {
    try {
      await run();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
    await load();
  };
  const mute = (sessionId: string, muted: boolean) =>
    act(() =>
      api('/api/deliveries/mute', actor, {
        method: 'POST',
        body: JSON.stringify({ sessionId, muted }),
      }),
    );
  const unsubscribe = (sessionId: string, source: string) =>
    act(() =>
      api(
        `/api/inbox/subscriptions?sessionId=${encodeURIComponent(sessionId)}&source=${encodeURIComponent(source)}`,
        actor,
        { method: 'DELETE' },
      ),
    );

  const cwdOf = (id: string) => status?.sessions.find((s) => s.sessionId === id)?.cwd;

  return (
    <section className="mb-[26px]" aria-label="세션 전달">
      <div className="mb-1.5 flex items-baseline gap-2 border-b border-line pb-[5px]">
        <h2 className="m-0 font-mono text-chip font-medium text-faint">세션 전달</h2>
        <div className="flex-1" />
        <button
          type="button"
          className="tap text-chip text-faint hover:text-text"
          onClick={() => void load()}
        >
          새로고침
        </button>
      </div>
      {error ? (
        <p className="m-0 text-meta text-dead" role="alert">
          {error}
        </p>
      ) : null}
      {status ? (
        <>
          {status.sessions.length === 0 ? (
            <p className="m-0 mb-2 text-meta text-faint">받은편지함을 등록한 세션이 없다.</p>
          ) : (
            <ul className="m-0 mb-3 list-none p-0">
              {status.sessions.map((s) => (
                <li key={s.sessionId} className="flex items-center gap-2 py-1 text-meta">
                  <span className="min-w-0 flex-1 truncate">
                    <span className="font-mono">{sessionLabel(s.sessionId, s.cwd)}</span>
                    <span className="text-muted">
                      {s.muted
                        ? ' · 보내지 않음'
                        : s.receivesPrFor.length > 0
                          ? ` · PR 알림 받는 중(${s.receivesPrFor.join(', ')})`
                          : ''}
                    </span>
                  </span>
                  <button
                    type="button"
                    className="drawer-btn"
                    onClick={() => void mute(s.sessionId, !s.muted)}
                  >
                    {s.muted ? '다시 보내기' : '보내지 않기'}
                  </button>
                </li>
              ))}
            </ul>
          )}
          {status.subscriptions.length > 0 ? (
            <ul className="m-0 mb-3 list-none p-0" aria-label="수집함 구독">
              {status.subscriptions.map((sub) => (
                <li
                  key={`${sub.source}:${sub.sessionId}`}
                  className="flex items-center gap-2 py-1 text-meta"
                >
                  <span className="min-w-0 flex-1 truncate">
                    수집함 <span className="font-mono">{sub.source}</span> →{' '}
                    <span className="font-mono">
                      {sessionLabel(sub.sessionId, cwdOf(sub.sessionId))}
                    </span>
                  </span>
                  <button
                    type="button"
                    className="drawer-btn"
                    onClick={() => void unsubscribe(sub.sessionId, sub.source)}
                  >
                    구독 해지
                  </button>
                </li>
              ))}
            </ul>
          ) : null}
          {status.recent.length > 0 ? (
            <ul className="m-0 list-none p-0" aria-label="최근 보낸 알림">
              {status.recent.slice(0, 10).map((d) => (
                <li
                  key={`${d.at}:${d.sessionId}:${d.subject}`}
                  className="truncate py-0.5 text-meta text-muted"
                >
                  <span className={d.ok ? 'text-run' : 'text-dead'}>{d.ok ? '✓' : '✗'}</span>{' '}
                  {formatAge(d.at, now)} · {KIND[d.kind] ?? d.kind} ·{' '}
                  {d.url ? (
                    <a className="text-link" href={d.url} target="_blank" rel="noreferrer">
                      {d.subject}
                    </a>
                  ) : (
                    d.subject
                  )}{' '}
                  →{' '}
                  <span className="font-mono">{sessionLabel(d.sessionId, cwdOf(d.sessionId))}</span>
                </li>
              ))}
            </ul>
          ) : (
            <p className="m-0 text-meta text-faint">
              아직 보낸 알림이 없다(데몬을 다시 띄우면 비워진다).
            </p>
          )}
        </>
      ) : null}
    </section>
  );
}
