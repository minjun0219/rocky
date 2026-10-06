// rocky lab 의 순수 판정 — register.tsx 의 배선과 테스트가 같이 쓴다. 데몬을 읽어 그리기만 하므로
// 판정·저장 규칙은 여기 두지 않는다(그건 rocky_core 몫).

/** `rocky.json` 의 `lab` 블록 — `rocky_core::config::LabConfig` 와 같은 규칙. */
export type LabConfig = { toast: boolean; band: boolean; limits: boolean };

/**
 * 사용자 `rocky.json` 원문 → `lab` 설정. 파일 없음·파싱 실패·블록 없음·`enabled: false` 는 꺼짐(undefined).
 * 칸은 `false` 일 때만 끈다 — `rc` 블록과 같은 모양.
 */
export function parseLabConfig(raw: string | undefined): LabConfig | undefined {
  if (raw === undefined) {
    return undefined;
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return undefined;
  }
  const block = (parsed as { lab?: unknown } | null)?.lab;
  if (block === null || typeof block !== 'object' || Array.isArray(block)) {
    return undefined;
  }
  const on = (key: string) => (block as Record<string, unknown>)[key] !== false;
  if (!on('enabled')) {
    return undefined;
  }
  return { toast: on('toast'), band: on('band'), limits: on('limits') };
}

/** `GET /api/deliveries` 의 `sessions[]` 한 줄 — 받은편지함을 등록한 세션. */
export type InboxSession = {
  sessionId: string;
  board?: string | null;
  muted?: boolean;
  receivesPrFor: string[];
};

/** `GET /api/summary` 에서 band 가 쓰는 칸. `board` 는 cwd 로 고른 보드(없으면 전체 합계). */
export type Summary = {
  board?: string | null;
  doing: number;
  /** 캐시 모드(`cached=true`)에서 수집함 캐시가 없으면 빠진다. */
  collect?: number;
  handoffsOpen: number;
  overdue: number;
};

/** 엔진 `session.measure` 의 한도 창 하나. */
export type RateLimit = { kind: string; percentUsed: number };

const ROCKY_HEAD = /^(?:# )?rocky: /;

/**
 * 데몬이 받은편지함 소켓으로 보낸 메시지(`rocky_core::peer_inbox` · `handoff::build_handoff_poke`)면 toast 한 줄,
 * 아니면 undefined. 첫 줄만 쓰고 `owner/repo ` 는 뗀다 — 세션이 이미 아는 레포다.
 */
export function rockyToast(text: string): string | undefined {
  if (!ROCKY_HEAD.test(text)) {
    return undefined;
  }
  const head = (text.split('\n', 1)[0] ?? '')
    .replace(ROCKY_HEAD, '')
    .replace(/^[^\s/]+\/\S+ (#\d+)/, '$1');
  return `rocky · ${head}`;
}

const WINDOW: Record<string, string> = { five_hour: '5h', seven_day: '7d', spend_limit: 'spend' };

/** `rocky lab 5h 42% · 7d 18% · ctx 31%` — 아무것도 없으면 undefined(status 를 지운다). */
export function limitsLine(
  limits: readonly RateLimit[],
  contextPercent: number | undefined,
): string | undefined {
  const parts = limits.map((l) => `${WINDOW[l.kind] ?? l.kind} ${l.percentUsed}%`);
  if (contextPercent !== undefined) {
    parts.push(`ctx ${contextPercent}%`);
  }
  return parts.length === 0 ? undefined : `rocky lab ${parts.join(' · ')}`;
}

/** 보드 요약 칸 — 0 인 칸은 뺀다. */
export function summaryParts(s: Summary): string[] {
  const parts: string[] = [];
  if (s.doing > 0) {
    parts.push(`진행중 ${s.doing}`);
  }
  if (s.overdue > 0) {
    parts.push(`기한 지남 ${s.overdue}`);
  }
  if (s.handoffsOpen > 0) {
    parts.push(`핸드오프 ${s.handoffsOpen}`);
  }
  if ((s.collect ?? 0) > 0) {
    parts.push(`수집함 ${s.collect}`);
  }
  return parts;
}
