/**
 * UI 순수 헬퍼 — 시간 표기, 클립보드, 테마, 피드의 내 차례 · PR 알림, 작업로그 표기.
 */
import { agentPhase } from './agents';
import type { PrSnapshot, RcStatus, SessionRow, SurfaceStat } from './types';

/**
 * 원격 제어 표면(탭·메뉴 스위치)을 보이나 — rc 블록이 켜졌거나 이 기기에 agy 가 있으면. agy 줄은 rc 블록과
 * 상관없이 설치 여부를 따른다(claude rc 를 못 쓰는 기기에서도 agy 는 켜고 끌 수 있다).
 */
export function rcVisible(rc: RcStatus | null | undefined): boolean {
  return rc?.configured === true || rc?.antigravity != null;
}

/** copyRef 가 실제로 건드리는 clipboard 표면 — 테스트에서 fake 로 대체 가능. */
export interface CopyRefClipboard {
  writeText(text: string): Promise<void>;
}

/** copyRef 가 실제로 건드리는 element(textarea)의 최소 표면. */
export interface CopyRefTextArea {
  value: string;
  setAttribute(name: string, value: string): void;
  style: { position: string; opacity: string };
  select(): void;
}

/** copyRef 가 실제로 건드리는 document 표면 — 테스트에서 fake 로 대체 가능. */
export interface CopyRefDocument {
  createElement(tagName: 'textarea'): CopyRefTextArea;
  body: {
    appendChild(node: CopyRefTextArea): void;
    removeChild(node: CopyRefTextArea): void;
  };
  execCommand(command: string): boolean;
}

/** copyRef 가 의존하는 전역 — 기본값은 실제 브라우저 전역, 테스트는 fake 를 주입한다. */
export interface CopyRefEnv {
  clipboard?: CopyRefClipboard;
  document?: CopyRefDocument;
}

/**
 * 실제 브라우저 전역을 가리키는 기본 env — 프로덕션 호출부는 이 값을 그대로 쓴다.
 *
 * 실제 `Document`/`Clipboard` 는 `CopyRefDocument`/`CopyRefClipboard` 보다 훨씬 넓은
 * 표면(제네릭 `appendChild<T extends Node>` 등)을 가져 구조적으로 딱 들어맞지 않는다 —
 * copyRef 가 실제로 쓰는 최소 표면만 뽑아낸 형태이므로 여기서만 단언(assert)한다.
 */
function defaultCopyRefEnv(): CopyRefEnv {
  return {
    clipboard: typeof navigator !== 'undefined' ? navigator.clipboard : undefined,
    document:
      typeof document !== 'undefined' ? (document as unknown as CopyRefDocument) : undefined,
  };
}

/**
 * 참조 문자열을 클립보드에 복사한다.
 *
 * `navigator.clipboard` 는 보안 컨텍스트(HTTPS·루프백)에서만 동작한다 — LAN 평문
 * HTTP(`192.168.x.x:8636`)로 접속하면 없다. 그 경우 execCommand 로 폴백하고,
 * 그마저 실패하면 false 를 돌려줘 호출자가 수동 복사 안내를 띄우게 한다.
 *
 * `env` 는 clipboard/document 접근을 주입하기 위한 선택 인자다 — 생략하면 실제
 * 전역을 쓰므로 프로덕션 호출부(`copyRef(text)`)는 그대로 동작한다. 테스트는
 * fake env 를 넘겨 보안 컨텍스트가 아닌 상황(LAN HTTP)의 execCommand 폴백을
 * DOM 없이 검증한다.
 */
export async function copyRef(
  text: string,
  env: CopyRefEnv = defaultCopyRefEnv(),
): Promise<boolean> {
  if (env.clipboard?.writeText) {
    try {
      await env.clipboard.writeText(text);
      return true;
    } catch {
      // 권한 거부 — 아래 폴백으로 내려간다.
    }
  }
  const doc = env.document;
  if (!doc?.execCommand) {
    return false;
  }
  const area = doc.createElement('textarea');
  area.value = text;
  area.setAttribute('readonly', '');
  area.style.position = 'fixed';
  area.style.opacity = '0';
  doc.body.appendChild(area);
  area.select();
  try {
    return doc.execCommand('copy');
  } catch {
    return false;
  } finally {
    doc.body.removeChild(area);
  }
}

/** copyRefWithFeedback 이 복사 성공 후 몇 ms 뒤에 copied 플래그를 지우는지. */
export const COPY_FEEDBACK_MS = 1200;

/** clipboard 접근 실패 시 안내하는 prompt 문구 — 기존 네 호출부가 복붙하던 문자열. */
const CLIPBOARD_UNAVAILABLE_MESSAGE =
  '클립보드에 접근하지 못했어요. 아래 텍스트를 직접 복사하세요:';

/**
 * `copyRefWithFeedback` 이 의존하는 전역 — {@link CopyRefEnv}(clipboard/document) 에
 * 더해 실패 시 prompt 폴백과 "copied 플래그를 지우는 타이머"까지 주입 가능하게 확장한다.
 * 기본값은 실제 브라우저 전역(`window.prompt`/`window.setTimeout`) — 프로덕션 호출부는
 * `env` 를 생략해도 그대로 동작한다. 테스트는 fake 를 넘겨 1200ms 를 실제로 기다리지
 * 않고 성공/실패 경로를 검증한다 (`copyRef` 와 동일한 주입 패턴 — `src/ui/lib.test.ts`).
 */
export interface CopyRefWithFeedbackEnv extends CopyRefEnv {
  prompt?: (message: string, defaultValue?: string) => string | null;
  setTimeout?: (handler: () => void, ms: number) => unknown;
}

function defaultCopyRefWithFeedbackEnv(): CopyRefWithFeedbackEnv {
  return {
    ...defaultCopyRefEnv(),
    prompt: typeof window !== 'undefined' ? window.prompt.bind(window) : undefined,
    setTimeout: typeof window !== 'undefined' ? window.setTimeout.bind(window) : undefined,
  };
}

/**
 * 참조 문자열을 복사하고 성공/실패에 따라 UI 피드백을 건다.
 *
 * 시퀀스 — `copyRef` 호출 → 성공하면 `copied` 플래그를 세우고 {@link COPY_FEEDBACK_MS}
 * 뒤 타이머로 해제 → 실패하면 `window.prompt` 로 수동 복사 안내 — 를 한 곳에 모은다.
 * 표면마다 타이머 길이·안내 문구가 따로 놀 여지를 없앤다.
 *
 * 훅이 아니라 순수 함수다 — `onCopied`(대개 컴포넌트의 `setCopied`)를 콜백으로 받아
 * 상태 갱신을 호출부에 위임하므로 React 렌더 사이클 없이 `bun:test` 로 단위
 * 테스트할 수 있다(신규 React 테스트 의존성 불필요). `env` 는 `copyRef` 와 같은 패턴으로
 * clipboard/document/prompt/setTimeout 접근을 주입한다 — 생략하면 실제 전역을 쓴다.
 *
 * title/aria-label 렌더링은 손대지 않는다 — 이 헬퍼가 반환하는 `copied` 상태를 그대로 읽는
 * 호출부(JSX)의 책임으로 남긴다.
 */
export async function copyRefWithFeedback(
  ref: string,
  onCopied: (copied: boolean) => void,
  env: CopyRefWithFeedbackEnv = defaultCopyRefWithFeedbackEnv(),
): Promise<void> {
  const ok = await copyRef(ref, env);
  if (ok) {
    onCopied(true);
    env.setTimeout?.(() => onCopied(false), COPY_FEEDBACK_MS);
    return;
  }
  env.prompt?.(CLIPBOARD_UNAVAILABLE_MESSAGE, ref);
}

/**
 * 절대 작성 시각 — 오늘이면 `HH:MM`, 다른 날이면 `MM-DD HH:MM` (브라우저 로컬 타임존).
 * 상대 시각(`formatElapsed`)은 "언제 썼는지"를 정확히 못 알려줘 댓글에는 쓰지 않는다.
 */
export function formatStamp(iso: string, now = new Date()): string {
  const at = new Date(iso);
  const pad = (n: number) => String(n).padStart(2, '0');
  const hm = `${pad(at.getHours())}:${pad(at.getMinutes())}`;
  const sameDay =
    at.getFullYear() === now.getFullYear() &&
    at.getMonth() === now.getMonth() &&
    at.getDate() === now.getDate();
  return sameDay ? hm : `${pad(at.getMonth() + 1)}-${pad(at.getDate())} ${hm}`;
}

/** 사용자가 고른 테마 의도. `auto` 는 OS 설정을 따른다는 뜻이다. */
export type ThemePref = 'auto' | 'dark' | 'light';

/** 실제로 화면에 적용되는 테마 — `auto` 가 해석된 결과. */
export type ResolvedTheme = 'dark' | 'light';

/** 테마 선호를 담는 localStorage 키. */
export const THEME_KEY = 'rocky:theme';

/**
 * localStorage 에서 읽은 원문을 테마 선호로 해석한다.
 * 알 수 없는 값은 전부 `auto` 다 — 손으로 고쳤거나 옛 버전이 남긴 값이 화면을 깨뜨리면
 * 안 된다.
 */
export function readThemePref(stored: string | null): ThemePref {
  return stored === 'dark' || stored === 'light' || stored === 'auto' ? stored : 'auto';
}

/**
 * 테마 선호와 OS 설정으로부터 실제 적용할 테마를 해석한다.
 *
 * **`src/ui/index.html` 의 인라인 스크립트가 같은 규칙을 손으로 복제하고 있다** — 그쪽은
 * 번들 전에 첫 페인트를 막고 실행돼야 해서 이 모듈을 import 할 수 없다. 한쪽을 고치면
 * 반드시 다른 쪽도 고쳐야 한다. `src/ui/inline-theme.test.ts` 가 그 스크립트를 실제로
 * 실행해 두 경로의 결론이 갈라지는지 감시한다.
 */
export function resolveTheme(pref: ThemePref, prefersLight: boolean): ResolvedTheme {
  if (pref === 'auto') {
    return prefersLight ? 'light' : 'dark';
  }
  return pref;
}

// ── 피드의 "내 차례" — 사람 답을 기다리는 에이전트 ──

export type NowKind = 'agent';

/** 행이 어느 묶음에 가나 — `mine` = 내 차례(사람이 손대야 끝난다), `more` = 접힌 나머지의 요약 한 줄. */
export type NowGroup = 'mine' | 'more';

export interface NowRow {
  key: string;
  kind: NowKind;
  group: NowGroup;
  /** 세션 이름. */
  ref: string;
  /** 세션이 기다리는 것(Claude 가 남긴 요약). */
  title: string;
  /** 경과의 기준 시각(ISO). 없으면 시각이 빈다. */
  since?: string;
  /** 상태 글자 — 행의 둘째 줄에 작게. */
  state: string;
  /** 요약 한 줄(`group: 'more'`)이 접어 둔 행 수 — 묶음 머리의 개수가 접힘과 무관하게 남도록. */
  hidden?: number;
}

/** 내 차례에 펼쳐 싣는 행의 상한 — 넘치면 "N 더" 한 줄로 접는다. */
export const MINE_ROW_MAX = 5;

/**
 * 피드의 "내 차례" 행 — 사람 답을 기다리는 background 세션(`agentPhase` 가 `blocked`). 오래 기다린 것이 위다.
 * `expanded` 면 자르지 않는다. 보드(넘김 · 멈춘 진행 · 읽지 않은 댓글 · 수집함)는 2026-11-13 걷어냈다.
 */
export function nowRows(input: { agents?: SessionRow[]; expanded?: boolean }): NowRow[] {
  const mine: NowRow[] = [];
  for (const s of input.agents ?? []) {
    if (agentPhase(s) !== 'blocked') {
      continue;
    }
    mine.push({
      key: `agent:${s.sessionId}`,
      kind: 'agent',
      group: 'mine',
      ref: s.name,
      title: s.job?.needs ?? s.job?.detail ?? '답을 기다려요',
      since: s.job?.updatedAt ?? new Date(s.startedAt).toISOString(),
      state: '답 기다림',
    });
  }
  mine.sort((a, b) => (a.since ?? '').localeCompare(b.since ?? ''));
  const shown = input.expanded ? mine : mine.slice(0, MINE_ROW_MAX);
  const hidden = mine.length - shown.length;
  return hidden > 0
    ? [
        ...shown,
        {
          key: 'mine:more',
          kind: 'agent',
          group: 'more',
          ref: '',
          title: `내 차례 ${hidden}개 더`,
          hidden,
          state: '',
        },
      ]
    : shown;
}

/**
 * "지금" 의 시각 — `web/DESIGN.md` "Time Display". 초가 흐르는 표기는 **1시간 미만의 진행중**
 * (`live`)에만 쓴다. 30일이 넘으면 날짜로(`since` 면 "…부터").
 */
export function formatAge(
  iso: string,
  now = Date.now(),
  options: { live?: boolean; since?: boolean } = {},
): string {
  const at = Date.parse(iso);
  if (Number.isNaN(at)) {
    return '';
  }
  const sec = Math.max(0, Math.floor((now - at) / 1000));
  if (options.live && sec < 3600) {
    const mm = String(Math.floor(sec / 60)).padStart(2, '0');
    const ss = String(sec % 60).padStart(2, '0');
    return `${mm}:${ss}`;
  }
  if (sec < 60) {
    return '방금';
  }
  if (sec < 3600) {
    return `${Math.floor(sec / 60)}분`;
  }
  if (sec < 86_400) {
    return `${Math.floor(sec / 3600)}시간`;
  }
  const days = Math.floor(sec / 86_400);
  if (days < 30) {
    return `${days}일`;
  }
  const d = new Date(at);
  const date = `${d.getMonth() + 1}월 ${d.getDate()}일`;
  return options.since ? `${date}부터` : date;
}

/**
 * 묶음 머리 "내 차례 N" 의 N — 펼쳤든 접었든 같은 수여야 한다(접힘은 보여 주는 방식일 뿐 일의 수가
 * 아니다). 펼친 행 + "N개 더" 가 접어 둔 행.
 */
export function mineCount(rows: NowRow[]): number {
  return rows.reduce((n, r) => (r.group === 'mine' ? n + 1 : n + (r.hidden ?? 0)), 0);
}

// ── PR 현황 — 이 보드(또는 전 보드)의 열린 PR 전부. 데몬 PR 감시의 스냅숏(`/api/prs?open=true`) ──

/**
 * PR 한 건의 상태 — 손댈 필요가 큰 순서. 충돌 → 머지 가능 → CI 실패 → 결정 필요(🚀) →
 * 대기(CI 도는 중·처리 안 된 스레드) → 초안.
 */
export type PrStatus = 'conflict' | 'ready' | 'failing' | 'decide' | 'waiting' | 'draft';

const PR_STATUS_ORDER: Record<PrStatus, number> = {
  conflict: 0,
  ready: 1,
  failing: 2,
  decide: 3,
  waiting: 4,
  draft: 5,
};

export function prStatus(p: PrSnapshot): PrStatus {
  if (p.mergeState === 'DIRTY') {
    return 'conflict';
  }
  if (p.ready) {
    return 'ready';
  }
  if (p.isDraft) {
    return 'draft';
  }
  if (p.ci === 'fail') {
    return 'failing';
  }
  if (p.decision > 0) {
    return 'decide';
  }
  return 'waiting';
}

export interface PrRow {
  key: string;
  repo: string;
  number: number;
  title: string;
  url: string;
  status: PrStatus;
  /** 상태 글자 — 행의 둘째 줄. CI 와 스레드를 짧게. */
  detail: string;
  updatedAt: string;
}

const CI_LABEL: Record<PrSnapshot['ci'], string> = {
  pass: 'CI 통과',
  fail: 'CI 실패',
  pending: 'CI 실행 중',
};

export function prDetail(p: PrSnapshot, status: PrStatus): string {
  if (status === 'conflict') {
    return '충돌';
  }
  if (status === 'ready') {
    return '머지 후보';
  }
  if (status === 'draft') {
    return '초안';
  }
  const parts = [CI_LABEL[p.ci]];
  if (p.decision > 0) {
    parts.push(`결정 필요 ${p.decision}`);
  }
  if (p.unhandled > 0) {
    parts.push(`스레드 ${p.unhandled}`);
  }
  return parts.join(' · ');
}

/**
 * PR 묶음의 행 — 열린 것만, 손댈 필요가 큰 순서, 같은 상태 안에서는 최근 갱신 먼저. `repo` 를 주면
 * 그 레포 것만(보고 있는 보드의 레포), `null` 이면 전 보드.
 */
export function prRows(prs: PrSnapshot[], repo: string | null): PrRow[] {
  return prs
    .filter((p) => p.state === 'OPEN' && (repo === null || p.repo === repo))
    .map((p) => {
      const status = prStatus(p);
      return {
        key: `${p.repo}#${p.number}`,
        repo: p.repo,
        number: p.number,
        title: p.title,
        url: p.url,
        status,
        detail: prDetail(p, status),
        updatedAt: p.updatedAt,
      };
    })
    .sort(
      (a, b) =>
        PR_STATUS_ORDER[a.status] - PR_STATUS_ORDER[b.status] ||
        b.updatedAt.localeCompare(a.updatedAt),
    );
}

/**
 * GitHub 항목을 숨길 때의 키. PR 은 **상태까지** 키에 넣는다 — "대기" 일 때 숨긴 PR 이 충돌·머지 가능으로
 * 바뀌면 다시 보여야 한다(사람이 움직일 일이 생겼다). 수집함 항목은 소스와 id.
 */
export function githubHideKey(
  item:
    | { kind: 'pr'; key: string; status: PrStatus }
    | { kind: 'inbox'; source: string; id: string },
): string {
  return item.kind === 'pr' ? `pr:${item.key}:${item.status}` : `inbox:${item.source}:${item.id}`;
}

/**
 * 피드의 PR 알림 종류 — 오너가 손댈 PR 만. 순서가 곧 화면 순서(위가 급하다). 할 일 쪽(넘김·멈춘 진행·
 * 읽지 않은 댓글·수집함)은 `nowRows` 의 내 차례가 맡는다.
 */
export type AlertKind = 'decide' | 'merge' | 'conflict' | 'ci';

export const ALERT_LABEL: Record<AlertKind, string> = {
  decide: '결정 필요',
  merge: '머지 후보',
  conflict: '충돌',
  ci: 'CI 실패',
};

const ALERT_ORDER: AlertKind[] = ['decide', 'merge', 'conflict', 'ci'];

/** 충돌·CI 실패는 세션이 먼저 푼다 — 이만큼 그대로면 오너 몫으로 올린다. */
export const STUCK_AFTER_MS = 30 * 60 * 1000;

export interface AlertRow {
  /** 화면 키(종류 + 대상). */
  key: string;
  /** 숨기기 키 — 종류까지 넣는다: 같은 PR 이라도 "충돌" 을 숨긴 뒤 "머지 후보" 가 되면 다시 보인다. */
  hideKey: string;
  kind: AlertKind;
  title: string;
  /** 둘째 줄 — 어느 레포의 몇 번(`rocky #12`). */
  detail: string;
  /** GitHub 의 PR 주소 — 누르면 새 탭. */
  url: string;
  at: string;
}

/**
 * 피드의 PR 알림 — 구독한 열린 PR 중 결정 필요(👀)·머지 후보는 바로, 충돌·CI 실패는 30분 넘게 그대로일
 * 때만(그 전엔 세션이 푼다). 숨긴 것은 뺀다.
 */
export function alertRows(prs: PrSnapshot[], hidden: readonly string[], now: number): AlertRow[] {
  const rows: AlertRow[] = [];
  for (const p of prs) {
    if (p.state !== 'OPEN') {
      continue;
    }
    const status = prStatus(p);
    const stale = now - Date.parse(p.updatedAt) > STUCK_AFTER_MS;
    const kind: AlertKind | null =
      status === 'decide'
        ? 'decide'
        : status === 'ready'
          ? 'merge'
          : status === 'conflict' && stale
            ? 'conflict'
            : status === 'failing' && stale
              ? 'ci'
              : null;
    if (kind === null) {
      continue;
    }
    const key = `${p.repo}#${p.number}`;
    rows.push({
      key: `${kind}:${key}`,
      hideKey: `alert:${kind}:${key}`,
      kind,
      title: p.title,
      detail: `${p.repo.split('/')[1] ?? p.repo} #${p.number}`,
      url: p.url,
      at: p.updatedAt,
    });
  }
  return rows
    .filter((r) => !hidden.includes(r.hideKey))
    .sort(
      (a, b) =>
        ALERT_ORDER.indexOf(a.kind) - ALERT_ORDER.indexOf(b.kind) || b.at.localeCompare(a.at),
    );
}

/**
 * SSE 이벤트(`ChangeEvent` — `{entity, entityId, action, boardId?}`)가 PR 스냅숏을 바꿨을 수 있나. 보드 · 노트 화면을
 * 걷어낸 뒤로 웹이 데몬 변경에서 다시 받는 것은 PR 뿐이다 — 할 일 · 섹션 · 노트 이벤트는 건너뛰고, PR 감시 전이(`pr-*`)나
 * 모르는 모양이면 다시 받는다(맥 앱 `touchesPrs` 와 같은 규칙).
 */
export function touchesPrs(events: readonly unknown[]): boolean {
  return events.some((raw) => {
    const event = raw as { entity?: unknown } | null;
    const entity = typeof event?.entity === 'string' ? event.entity : '';
    return entity !== 'todo' && entity !== 'note' && entity !== 'section';
  });
}

/**
 * 턴 기록(`log-turn`, `req: … | tools: … | did: …`)을 푼다 — 작업로그 화면이 요청을 제목처럼, 결과를 둘째 줄로
 * 보여 준다. 모양이 다르면(손으로 쓴 `decision` 등) 본문 전체를 `did` 로.
 */
export function parseTurn(content: string): { req: string; tools: string; did: string } {
  const m = /^req: (.*?) \| tools: (.*?) \| did: (.*)$/s.exec(content);
  if (!m) {
    return { req: '', tools: '', did: content };
  }
  const tools = m[2] === '(none)' ? '' : (m[2] ?? '');
  return { req: labelInjected(m[1] ?? ''), tools, did: m[3] ?? '' };
}

/**
 * 마크다운을 목록에 쓸 평문으로 — 작업로그의 결과(마지막 답)는 마크다운이라 그대로 두면 `**원인:**`·
 * `[#293](https://…)` 이 글자로 보인다. 목록 행은 눌러 펼치는 버튼이라 링크를 살릴 수 없어(버튼 안 링크) 글자만 남긴다.
 */
export function mdToPlain(md: string): string {
  // 코드(펜스 본문·인라인 코드)는 글자 그대로 남긴다 — 먼저 자리표로 빼 두어야 `__init__` 같은 식별자가
  // 강조 치환에 깎이지 않는다. 펜스는 구분자(```lang)만 걷고 본문은 살린다.
  const code: string[] = [];
  const hold = (text: string) => `\uE000${code.push(text) - 1}\uE000`;
  return md
    .replace(/```[^\n]*\n?([\s\S]*?)```/g, (_m, body: string) => hold(body))
    .replace(/`([^`]+)`/g, (_m, body: string) => hold(body))
    .replace(/!\[([^\]]*)\]\([^)]*\)/g, '$1')
    .replace(/\[([^\]]+)\]\([^)]*\)/g, '$1')
    .replace(/(\*\*|__)(.+?)\1/g, '$2')
    .replace(/(^|[^*\w])\*(?!\s)([^*]+?)\*(?!\w)/g, '$1$2')
    .replace(/^\s{0,3}#{1,6}\s+/gm, '')
    .replace(/^\s{0,3}>\s?/gm, '')
    .replace(/^\s*(?:[-*+]|\d+\.)\s+(?:\[[ xX]\]\s+)?/gm, '')
    .replace(/^[ \t]*\|?(?:[ \t]*:?-{3,}:?[ \t]*\|)+[ \t]*:?-*:?[ \t]*$/gm, '')
    .replace(/^[ \t]*\|(.*?)\|?[ \t]*$/gm, (_m, row: string) =>
      row
        .split('|')
        .map((c) => c.trim())
        .join(' · '),
    )
    .replace(/\uE000(\d+)\uE000/g, (_m, i: string) => code[Number(i)] ?? '')
    .replace(/\s+/g, ' ')
    .trim();
}

/**
 * 도구 목록의 MCP 이름을 짧게 — `mcp__plugin_playwright_playwright__browser_click(×3)` →
 * `playwright:browser_click(×3)`. 서버 이름은 플러그인 접두사를 떼고 마지막 조각만.
 */
export function shortTools(tools: string): string {
  return tools.replace(
    /mcp__([A-Za-z0-9_-]+?)__([A-Za-z0-9_-]+)/g,
    (_m, server: string, tool: string) => {
      const name =
        server
          .replace(/^plugin_/, '')
          .split('_')
          .at(-1) ?? server;
      return `${name}:${tool}`;
    },
  );
}

/**
 * 하네스가 넣은 메시지를 짧은 이름으로 — Stop 훅(`rocky_core::transcript::label_injected`)이 새 기록부터 하는 일을
 * 그 전 기록에도 보여 줄 때 똑같이 한다.
 */
export function labelInjected(req: string): string {
  const head = req.trimStart();
  if (head.startsWith('<task-notification>')) {
    return '(백그라운드 작업 알림)';
  }
  if (head.startsWith('<bash-stdout>') || head.startsWith('<bash-stderr>')) {
    return '(셸 출력)';
  }
  return req;
}

/** 느린 표면 — 표본이 `min` 번 이상인 것 중 p95 가 큰 순. 몇 번 안 쓴 것의 한 번 느림에 휘둘리지 않게. */
export function slowSurfaces(surfaces: readonly SurfaceStat[], min = 5, top = 5): SurfaceStat[] {
  return surfaces
    .filter((s) => s.count >= min && s.p95Ms !== undefined)
    .sort((a, b) => (b.p95Ms ?? 0) - (a.p95Ms ?? 0))
    .slice(0, top);
}

/** 실패가 잦은 표면 — 실패율 순(같으면 실패 수). */
export function failingSurfaces(surfaces: readonly SurfaceStat[], top = 5): SurfaceStat[] {
  return surfaces
    .filter((s) => s.errors > 0)
    .sort((a, b) => b.errors / b.count - a.errors / a.count || b.errors - a.errors)
    .slice(0, top);
}
