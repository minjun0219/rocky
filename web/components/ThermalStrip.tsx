import { useEffect, useState } from 'react';
import { actorTone, formatElapsed } from '../lib';
import { useUiStore } from '../store';

interface HistoryEvent {
  id: number;
  actor: string;
  at: string;
}

const LIMIT = 48;

/** 캡션 한 줄 — 마지막 활동이 언제·누구였는지. 눈금만으로는 읽히지 않던 것을 글자로 준다. */
export function thermalCaption(events: HistoryEvent[], now = Date.now()): string {
  const latest = events[events.length - 1];
  if (!latest) {
    return '';
  }
  const who = actorTone(latest.actor) === 'warm' ? '에이전트' : '사람';
  const elapsed = formatElapsed(latest.at, now);
  return elapsed === '방금' ? `${who} · 방금` : `${who} · ${elapsed} 전`;
}

/**
 * 온도 띠 — 보드의 최근 활동 48건을 시간순(왼쪽=과거)으로 늘어놓은 눈금. 눈금 하나 =
 * 히스토리 이벤트 하나, 색은 두 대기 그대로(warm=에이전트, cool=사람), 과거로 갈수록
 * 식는다(투명해진다). "지금 이 보드가 얼마나 뜨겁고 누가 데우고 있나" 를 한 눈에 주는
 * 장식이었는데, 무슨 뜻인지가 안 읽혔다 — 그래서 눈금 옆에 **마지막 활동을 글자로**
 * 붙였다(`thermalCaption`). 눈금 위에 올리면 그 한 건의 actor·시각이 뜬다.
 */
export function ThermalStrip() {
  const [events, setEvents] = useState<HistoryEvent[]>([]);
  // todos 참조가 바뀔 때마다(= SSE 가 뭔가를 갱신할 때마다) 다시 읽는다.
  const todos = useUiStore((s) => s.todos);

  // biome-ignore lint/correctness/useExhaustiveDependencies(todos): todos 는 값이 아니라 재조회 트리거다 — SSE 가 뭔가를 갱신하면 참조가 바뀐다.
  useEffect(() => {
    let alive = true;
    fetch(`/api/history?limit=${LIMIT}`)
      .then((res) => (res.ok ? (res.json() as Promise<HistoryEvent[]>) : null))
      .then((rows) => {
        // 실패(HTTP 에러 포함)는 상태를 건드리지 않는다 — 장식이 순간적으로
        // 사라졌다 나타나는 것보다 마지막 성공 스냅샷이 낫다.
        if (alive && rows) {
          setEvents([...rows].reverse()); // 최신순으로 오므로 뒤집어 왼쪽=과거
        }
      })
      .catch(() => {}); // 네트워크 에러도 같은 원칙 — 침묵
    return () => {
      alive = false;
    };
  }, [todos]);

  if (events.length === 0) {
    return null;
  }
  const caption = thermalCaption(events);
  return (
    <div
      className="thermal flex items-center gap-3 max-[560px]:hidden"
      role="img"
      aria-label={`최근 활동 ${events.length}건 — 앰버는 에이전트, 블루는 사람. 마지막: ${caption}`}
      title="최근 활동 — 왼쪽이 과거, 오른쪽이 방금. 앰버는 에이전트, 블루는 사람"
    >
      <div className="flex h-3.5 items-stretch gap-[2px] overflow-hidden">
        {events.map((e, i) => (
          <span
            key={e.id}
            title={`${e.actor} · ${formatElapsed(e.at)} 전`}
            className="w-1 rounded-[1px]"
            style={{
              background: `var(--${actorTone(e.actor)})`,
              // 식는 곡선 — 최신(오른쪽)이 1, 과거로 갈수록 0.2 까지.
              opacity: 0.2 + 0.8 * (i / Math.max(1, events.length - 1)),
            }}
          />
        ))}
      </div>
      <span className="thermal-caption whitespace-nowrap font-mono text-micro tracking-[0.12em] text-muted">
        {caption}
      </span>
    </div>
  );
}
