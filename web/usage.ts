/**
 * 웹 UI 사용 이벤트 — 서버를 안 거치는 조작을 이름으로 남긴다(`POST /api/usage`).
 * 이름은 `web:<event>` 고정이고 서버가 `rocky_core::usage::KNOWN_SURFACES` 와 대조한다 —
 * 새 이벤트를 만들면 그쪽에도 넣어야 "안 쓰임" 으로 잡힌다. 실패는 침묵(로그가 화면을 막지 않는다).
 */
export type WebUsageEvent =
  | 'web:now-row'
  | 'web:board-tab'
  | 'web:todo-open'
  | 'web:notes-toggle'
  | 'web:theme'
  | 'web:archived-toggle'
  | 'web:quick-add';

let actorHeader = 'unknown';

/** 스토어가 actor 를 바꿀 때 알려 준다 — 이 모듈은 스토어를 import 하지 않는다(순환 방지). */
export function setUsageActor(actor: string): void {
  actorHeader = actor;
}

/** 한 건 보낸다. `keepalive` 라 탭이 닫혀도 나간다. 테스트·비브라우저에서는 fetch 가 없을 수 있다. */
export function logUsage(name: WebUsageEvent, meta?: Record<string, string | number>): void {
  if (typeof fetch !== 'function') {
    return;
  }
  try {
    void fetch('/api/usage', {
      method: 'POST',
      keepalive: true,
      headers: {
        'content-type': 'application/json',
        'x-rocky-actor': actorHeader,
        'x-rocky-client': 'web',
      },
      body: JSON.stringify(meta ? { name, meta } : { name }),
    }).catch(() => {});
  } catch {
    // 침묵 — 사용 로그는 본업이 아니다.
  }
}
