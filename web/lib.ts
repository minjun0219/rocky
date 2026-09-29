/**
 * UI 순수 헬퍼 — actor 톤(두 대기 컨셉), 시간 표기, 초경량 markdown 렌더 토큰화.
 */
import { isAgentActor } from './actors';
import type { HandoffView, PrSnapshot, TodoView } from './types';
import type { Comment, HistoryEntry } from './types';

/**
 * actor → 시각 톤. 에이전트는 warm(앰버), 사람은 cool(아이스 블루).
 * "누가 했나"를 온도로 인코딩하는 것이 이 UI 의 시그니처다.
 */
export function actorTone(actor: string): 'warm' | 'cool' {
  return isAgentActor(actor) ? 'warm' : 'cool';
}

/** doing 경과가 이 시간(ms)을 넘으면 stale 로 표시한다. */
export const STALE_MS = 30 * 60 * 1000;

export function isStale(doingSince: string | undefined, now = Date.now()): boolean {
  if (!doingSince) {
    return false;
  }
  return now - Date.parse(doingSince) > STALE_MS;
}

/** doing 뱃지에 붙일 수식어 — 없으면 null (평범한 "처리중"). */
export interface DoingWarning {
  /** 뱃지에 붙는 짧은 꼬리표. */
  label: string;
  /** 툴팁 — 왜 이렇게 보이는지. */
  title: string;
  /** 심각도. `dead` 는 아무도 안 들고 있다는 뜻이라 더 강하게 표시한다. */
  tone: 'dead' | 'idle' | 'slow';
}

/**
 * doing 하나를 어떻게 경고할지 정한다.
 *
 * 서버가 세션을 실제로 대조한 판정(`doingState`)이 있으면 그걸 우선한다 — 30분 경과
 * 규칙보다 언제나 정확하기 때문이다. 판정이 없거나(`unknown`, 구버전 데몬) 세션은
 * 멀쩡한데(`live`) 오래 걸리는 경우에만 기존 시간 규칙으로 물러난다.
 *
 * `idle` 을 따로 두는 이유: 세션은 살아 있는데 턴이 끝났고 `done` 이 안 온 상태다.
 * 죽은 것(`gone`)과 사람이 취할 행동이 다르다 — 이건 그 세션에 말을 걸면 이어진다.
 */
export function doingWarning(todo: TodoView, now = Date.now()): DoingWarning | null {
  if (todo.doingState === 'gone') {
    return { label: '세션 없음', title: '이 항목을 들고 있던 세션이 사라졌다', tone: 'dead' };
  }
  if (todo.doingState === 'idle') {
    return {
      label: '멈춤',
      title: '세션은 살아 있지만 턴이 끝났고 완료 처리가 없다',
      tone: 'idle',
    };
  }
  if (todo.doingState === 'live') {
    return null;
  }
  return isStale(todo.doingSince, now)
    ? { label: '오래됨', title: '30분 이상 갱신 없음', tone: 'slow' }
    : null;
}

/** "방금" / "N분" / "N시간" / "N일" — doing 뱃지와 히스토리 타임스탬프용. */
export function formatElapsed(iso: string, now = Date.now()): string {
  const ms = Math.max(0, now - Date.parse(iso));
  const min = Math.floor(ms / 60_000);
  if (min < 1) {
    return '방금';
  }
  if (min < 60) {
    return `${min}분`;
  }
  const hours = Math.floor(min / 60);
  if (hours < 24) {
    return `${hours}시간`;
  }
  return `${Math.floor(hours / 24)}일`;
}

/** 마감일 표기 — "8/1" 형태. 지난 날짜 여부는 isOverdue 로 별도 판단. */
export function formatDue(due: string): string {
  const [, month, day] = due.split('-');
  if (!month || !day) {
    return due;
  }
  return `${Number(month)}/${Number(day)}`;
}

export function isOverdue(due: string, now = new Date()): boolean {
  const today = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, '0')}-${String(now.getDate()).padStart(2, '0')}`;
  return due < today;
}

export type MdToken =
  | { type: 'text'; value: string }
  | { type: 'bold'; value: string }
  | { type: 'code'; value: string }
  | { type: 'link'; value: string };

/**
 * 초경량 markdown 토큰화 — **bold** / `code` / http(s) URL 만 지원.
 * React 노드로 조립하므로 HTML escape 는 불필요하다 (innerHTML 미사용).
 */
export function mdTokens(text: string): MdToken[] {
  const tokens: MdToken[] = [];
  const pattern = /(\*\*[^*]+\*\*|`[^`]+`|https?:\/\/\S+)/g;
  let last = 0;
  for (const match of text.matchAll(pattern)) {
    const index = match.index ?? 0;
    if (index > last) {
      tokens.push({ type: 'text', value: text.slice(last, index) });
    }
    const raw = match[0];
    if (raw.startsWith('**')) {
      tokens.push({ type: 'bold', value: raw.slice(2, -2) });
    } else if (raw.startsWith('`')) {
      tokens.push({ type: 'code', value: raw.slice(1, -1) });
    } else {
      tokens.push({ type: 'link', value: raw });
    }
    last = index + raw.length;
  }
  if (last < text.length) {
    tokens.push({ type: 'text', value: text.slice(last) });
  }
  return tokens;
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

/** 보드 스킬의 슬래시 커맨드 이름 — 플러그인 `rocky` 의 `skills/board`. */
const BOARD_SKILL_COMMAND = '/rocky:board';

/**
 * 참조를 클립보드에 넣을 슬래시 커맨드로 감싼다 — `rocky-12` → `/rocky:board rocky-12`.
 *
 * 참조만 복사하면 세션에 붙여넣었을 때 에이전트가 "이 문자열로 뭘 하라는 건지" 를 모른다.
 * 커맨드까지 함께 복사하면 붙여넣기 한 번이 곧 "이 항목을 맡아라" 가 된다. 화면에 보이는
 * 글자는 참조 그대로 두고 클립보드 값만 넓히는 것이 요점이다 — 버튼에 커맨드 전문을
 * 그리면 행이 읽히지 않는다.
 */
export function boardCommand(ref: string): string {
  return `${BOARD_SKILL_COMMAND} ${ref}`;
}

/** copyRefWithFeedback 이 복사 성공 후 몇 ms 뒤에 copied 플래그를 지우는지 — 기존
 * TodoItem/NotesRail/DetailDrawer(todo·note) 네 호출부가 각각 하드코딩했던 1200ms 를
 * 여기 하나로 고정한다. */
export const COPY_FEEDBACK_MS = 1200;

/** clipboard 접근 실패 시 안내하는 prompt 문구 — 기존 네 호출부가 복붙하던 문자열. */
const CLIPBOARD_UNAVAILABLE_MESSAGE = '클립보드에 접근할 수 없다 — 아래 텍스트를 직접 복사해라:';

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
 * `TodoItem`/`NotesRail`/`DetailDrawer`(todo·note 경로 둘 다) 네 곳이 복붙해 쓰던
 * 시퀀스 — `copyRef` 호출 → 성공하면 `copied` 플래그를 세우고 {@link COPY_FEEDBACK_MS}
 * 뒤 타이머로 해제 → 실패하면 `window.prompt` 로 수동 복사 안내 — 를 한 곳에 모은다.
 * 표면마다 타이머 길이·안내 문구가 따로 놀 여지를 없앤다.
 *
 * 훅이 아니라 순수 함수다 — `onCopied`(대개 컴포넌트의 `setCopied`)를 콜백으로 받아
 * 상태 갱신을 호출부에 위임하므로 React 렌더 사이클 없이 `bun:test` 로 단위
 * 테스트할 수 있다(신규 React 테스트 의존성 불필요). `env` 는 `copyRef` 와 같은 패턴으로
 * clipboard/document/prompt/setTimeout 접근을 주입한다 — 생략하면 실제 전역을 쓴다.
 *
 * title/aria-label 렌더링은 손대지 않는다 — 버튼의 보이는 텍스트(목록 행에서는 맨숫자,
 * 드로어에서는 전체 ref)만으로는 스크린리더가 제대로 안내하지 못한다는 과거 리뷰
 * 지적으로 각 호출부가 이미 명시적
 * `aria-label` 을 달아 두었고, 그건 이 헬퍼가 반환하는 `copied` 상태를 그대로 읽는
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

/** 링크 URL → 짧은 출처 라벨 (github.com/owner/repo#12, todoist, …). */
export function linkLabel(url: string): string {
  try {
    const u = new URL(url);
    if (u.hostname === 'github.com') {
      const [owner, repo, kind, num] = u.pathname.slice(1).split('/');
      if (owner && repo && (kind === 'issues' || kind === 'pull') && num) {
        return `${repo}#${num}`;
      }
      return `${owner}/${repo ?? ''}`.replace(/\/$/, '');
    }
    if (u.hostname.includes('todoist')) {
      return 'todoist';
    }
    return u.hostname.replace(/^www\./, '');
  } catch {
    return url;
  }
}

/**
 * 이벤트가 편집 중인 요소에서 왔는지 판정한다.
 *
 * 드로어의 전역 Esc 리스너가 입력 중인 Esc 까지 가로채면, 사용자가 기대한 "입력 취소"
 * 대신 드로어가 통째로 닫히며 편집분이 날아간다. 전역 단축키는 이 판정으로 걸러 낸다.
 */
export function isEditableTarget(target: EventTarget | null): boolean {
  if (!target) {
    return false;
  }
  const el = target as { tagName?: string; isContentEditable?: boolean };
  const tag = el.tagName?.toUpperCase();
  return tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' || el.isContentEditable === true;
}

/** 히스토리 줄과 댓글 카드를 한 줄기로 묶은 타임라인 항목. */
export type TimelineItem =
  | { kind: 'history'; at: string; entry: HistoryEntry }
  | { kind: 'comment'; at: string; comment: Comment };

/**
 * 댓글 계열 히스토리 액션 중 타임라인/상세 화면에서 버리는 것 — 댓글 카드가 여전히
 * 그 사건을 대표하는 두 가지(작성/본문 수정)만 뺀다.
 *
 * 댓글 mutation 은 부모 todo 의 히스토리로도 기록된다(SSE·훅 주입 경로를 타기 위해서다).
 * `comment`/`comment-edit` 을 그대로 두면 같은 사건이 댓글 카드와 히스토리 한 줄로 두 번
 * 보인다. `comment-archive`/`comment-unarchive` 는 빼지 않는다 — 보관되면 카드 자체가
 * 사라지므로(대표하는 화면 요소가 없어짐) 타임라인에 흔적이 남아야 한다.
 *
 * `src/store.ts` 의 `DETAIL_HISTORY_EXCLUDED` 와 같은 값 쌍이다 — 여기서 별도로 export
 * 하는 이유는 이 파일이 브라우저에 번들되는 UI 코드라서다: `store.ts` 를 런타임으로
 * import 하면 `bun:sqlite` 가 클라이언트 번들 그래프에 끌려온다(기존 `import type`
 * 은 타입만 지워지니 안전하지만, 값 import 는 안 된다). 값이 둘로 나뉘어 있는 만큼
 * `src/ui/lib.test.ts` 가 두 목록의 내용이 같은지 회귀 테스트로 고정한다 — 셋째 액션이
 * 생기면 여기와 `src/store.ts` 양쪽을 함께 고쳐야 한다.
 */
export const DETAIL_HISTORY_EXCLUDED: readonly string[] = ['comment', 'comment-edit'];

const COMMENT_HISTORY_ACTIONS: ReadonlySet<string> = new Set(DETAIL_HISTORY_EXCLUDED);

/**
 * 히스토리와 댓글을 시간순(**최신 우선**)으로 병합한다. 드로어의 기존 히스토리 렌더가
 * 최신 우선이라 그 방향을 유지한다.
 */
export function mergeTimeline(history: HistoryEntry[], comments: Comment[]): TimelineItem[] {
  const items: TimelineItem[] = [
    ...history
      .filter((entry) => !COMMENT_HISTORY_ACTIONS.has(entry.action))
      .map((entry) => ({ kind: 'history' as const, at: entry.at, entry })),
    ...comments.map((comment) => ({ kind: 'comment' as const, at: comment.createdAt, comment })),
  ];
  // 동률은 0 을 돌려 안정 정렬을 유지한다 (같은 밀리초의 두 항목이 뒤바뀌지 않게).
  return items.sort((a, b) => (a.at < b.at ? 1 : a.at > b.at ? -1 : 0));
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

/** localStorage 의 최소 계약 — 테스트에서 인메모리 대역을 넣기 위해 좁혀 둔다. */
export interface SeenStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

const SEEN_KEY = 'rocky-seen-comments';

/**
 * todo id → 마지막으로 확인한 댓글 시각(ISO). 깨진 값은 빈 커서로 취급한다.
 *
 * 값까지 문자열인지 검사해 걸러낸다 — localStorage 는 다른 탭·구버전·수동 편집이
 * 무엇이든 써 넣을 수 있고, 숫자/객체가 섞이면 `hasUnreadComments` 의 문자열 비교가
 * 커서를 엉뚱하게 판정한다. 걸러진 항목은 "본 적 없음"(= 미확인)으로 떨어진다.
 */
export function readSeen(storage: SeenStorage): Record<string, string> {
  try {
    const parsed = JSON.parse(storage.getItem(SEEN_KEY) ?? '{}') as unknown;
    if (parsed === null || typeof parsed !== 'object' || Array.isArray(parsed)) {
      return {};
    }
    const seen: Record<string, string> = {};
    for (const [id, at] of Object.entries(parsed as Record<string, unknown>)) {
      if (typeof at === 'string') {
        seen[id] = at;
      }
    }
    return seen;
  } catch {
    return {};
  }
}

/** 이 todo 의 댓글을 `at` 까지 확인했다고 기록한다. */
export function markSeen(storage: SeenStorage, todoId: string, at: string): void {
  const seen = readSeen(storage);
  seen[todoId] = at;
  storage.setItem(SEEN_KEY, JSON.stringify(seen));
}

/** 읽음 커서보다 새로운 댓글이 있는지 — 배지 강조 조건. */
export function hasUnreadComments(
  todo: { id: string; lastCommentAt?: string },
  seen: Record<string, string>,
): boolean {
  if (!todo.lastCommentAt) {
    return false;
  }
  const at = seen[todo.id];
  return at === undefined || at < todo.lastCommentAt;
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

/** 드래그 정렬에서 형제로 인정되는 조건 — 같은 보드·섹션·부모 안에서만 순서를 바꾼다. */
export interface ReorderSibling {
  id: string;
  boardId: string;
  sectionId?: string;
  parentId?: string;
}

/**
 * 드롭 결과를 move API 의 `before` 값으로 바꾼다 (순수 — 단위 테스트 대상).
 *
 * @param siblings 화면 표시 순서의 형제 목록 (드래그 중인 항목 포함)
 * @param dragId   끌고 있는 항목
 * @param overId   포인터가 올라간 항목
 * @param after    포인터가 그 항목의 아래쪽 절반에 있었는가
 * @returns `{ before }` — null 은 맨 끝. **이동이 무의미하면 undefined** (제자리 드롭,
 *   형제가 아닌 대상, 자기 자신).
 */
export function resolveDropBefore(
  siblings: ReorderSibling[],
  dragId: string,
  overId: string,
  after: boolean,
): { before: string | null } | undefined {
  const drag = siblings.find((s) => s.id === dragId);
  const over = siblings.find((s) => s.id === overId);
  if (!drag || !over || dragId === overId) {
    return undefined;
  }
  if (
    drag.boardId !== over.boardId ||
    (drag.sectionId ?? null) !== (over.sectionId ?? null) ||
    (drag.parentId ?? null) !== (over.parentId ?? null)
  ) {
    return undefined; // 섹션·부모를 넘는 이동은 정렬이 아니라 소속 변경이다 — 드로어의 몫
  }
  const order = siblings.map((s) => s.id);
  const overIndex = order.indexOf(overId);
  const beforeId = after ? (order[overIndex + 1] ?? null) : overId;
  if (beforeId === dragId) {
    return undefined; // 결과가 제자리
  }
  // 바로 앞 형제의 "아래"로 놓는 것도 제자리다
  const dragIndex = order.indexOf(dragId);
  if (beforeId === null && dragIndex === order.length - 1) {
    return undefined;
  }
  if (beforeId !== null && order.indexOf(beforeId) === dragIndex + 1) {
    return undefined;
  }
  return { before: beforeId };
}

// ── "지금" 표 — 첫 화면이 답할 한 가지: 무슨 일이 돌고 있고, 무엇이 내 차례인가 ──

export type NowKind = 'dead' | 'handoff' | 'doing' | 'unread' | 'collect' | 'pr';

/**
 * 행이 어느 묶음에 가나 — `mine` = 내 차례(사람이 손대야 끝난다), `run` = 돌고 있음(보기만),
 * `more` = 접힌 나머지의 요약 한 줄. `web/DESIGN.md` "Information Priority".
 */
export type NowGroup = 'mine' | 'run' | 'more';

/** 행 앞 글리프 — 색만으로 말하지 않도록 상태마다 모양이 다르다(DESIGN.md "State Vocabulary"). */
export type NowGlyph = 'run' | 'mine' | 'dead' | 'unknown';

export interface NowRow {
  key: string;
  kind: NowKind;
  group: NowGroup;
  glyph: NowGlyph;
  /** 표시 참조 — todo 는 `acorn-server-28`, 수집함은 `수집함`. */
  ref: string;
  title: string;
  /** 눌러서 열 todo. 수집함·PR 행에는 없다. */
  todoId?: string;
  /** 바깥 링크(PR). 있으면 항목이 새 탭으로 열린다. */
  url?: string;
  /** 누가 들고 있나 — 색이 아니라 글자로. */
  who: 'AGENT' | 'YOU' | '—';
  /** 경과의 기준 시각(ISO). 없으면 시각이 빈다. */
  since?: string;
  /** 진행중 — 1시간 미만이면 초가 흐른다(`formatAge` 의 `live`). */
  live: boolean;
  /** 읽지 않은 댓글 수 — 0 이면 표시하지 않는다. */
  unread: number;
  /** 상태 글자 — 행의 둘째 줄에 작게. 색은 글리프가 말한다. */
  state: string;
  /** 요약 한 줄(`group: 'more'`)이 접어 둔 행 수 — 묶음 머리의 개수가 접힘과 무관하게 남도록. */
  hidden?: number;
}

/**
 * 내 차례의 순서 = 우선순위. PR 충돌 → 머지 가능 → 세션 없음·멈춤 → 안 집힌 넘김 →
 * 읽지 않은 댓글 → 수집함. 같은 순위 안에서는 오래 방치된 것이 위다.
 */
const MINE_RANK = {
  prConflict: 0,
  prReady: 1,
  stuck: 2,
  handoff: 3,
  unread: 4,
  collect: 5,
} as const;
/** 내 차례에 펼쳐 싣는 행의 상한 — 넘치면 "N 더" 한 줄로 접는다. */
export const MINE_ROW_MAX = 5;
/** 읽지 않은 댓글은 이 기간 안에 달린 것만, 이 수까지만 행이 된다. 나머지는 요약 한 줄. */
export const UNREAD_ROW_MAX = 3;
export const UNREAD_WINDOW_MS = 3 * 86_400_000;

/**
 * "지금" 의 행을 만든다 — 내 차례(우선순위순, 최대 `MINE_ROW_MAX`) + 돌고 있음. 같은 todo 가
 * 진행중이면서 댓글이 있으면 행 하나에 합친다. `expanded` 면 내 차례를 자르지 않는다.
 */
export function nowRows(
  input: {
    todos: TodoView[];
    handoffs: HandoffView[];
    seen: Record<string, string>;
    collect?: number | null;
    /** 데몬 PR 감시의 열린 PR — 확인·머지 가능한 것과 충돌난 것만 행이 된다. */
    prs?: PrSnapshot[];
    expanded?: boolean;
  },
  now = Date.now(),
): NowRow[] {
  const mine: { row: NowRow; rank: number; order: number }[] = [];
  const run: NowRow[] = [];
  const handled = new Set<string>();
  const todoById = new Map(input.todos.map((t) => [t.id, t]));
  let order = 0;
  const pushMine = (rank: number, row: NowRow) => mine.push({ row, rank, order: order++ });

  for (const t of input.todos) {
    if (t.status !== 'doing' || t.archivedAt) {
      continue;
    }
    handled.add(t.id);
    const warning = doingWarning(t, now);
    const base = {
      key: `doing:${t.id}`,
      kind: (warning?.tone === 'dead' ? 'dead' : 'doing') as NowKind,
      ref: t.ref,
      title: t.title,
      todoId: t.id,
      who: (t.doingBy ? (isAgentActor(t.doingBy) ? 'AGENT' : 'YOU') : '—') as NowRow['who'],
      since: t.doingSince,
      unread: hasUnreadComments(t, input.seen) ? t.commentCount : 0,
    };
    if (warning?.tone === 'dead' || warning?.tone === 'idle') {
      pushMine(MINE_RANK.stuck, {
        ...base,
        group: 'mine',
        glyph: warning.tone === 'dead' ? 'dead' : 'mine',
        live: false,
        state: warning.label,
      });
      continue;
    }
    // 세션 판정이 없으면(unknown) 모른다 — 경고색이 아니라 무채색 글리프로.
    const known = t.doingState === 'live';
    run.push({
      ...base,
      group: 'run',
      glyph: known ? 'run' : 'unknown',
      live: known,
      state: known ? '진행중' : '세션 모름',
    });
  }

  for (const h of input.handoffs) {
    // 이미 착수한 배달은 위 진행중 행이 말한다.
    if ((h.status === 'delivered' && h.acceptedAt) || h.status === 'cancelled') {
      continue;
    }
    const todo = todoById.get(h.todoId);
    if (todo) {
      handled.add(todo.id);
    }
    pushMine(MINE_RANK.handoff, {
      key: `handoff:${h.id}`,
      kind: 'handoff',
      group: 'mine',
      glyph: h.status === 'pending' && h.stale ? 'dead' : 'mine',
      ref: todo?.ref ?? h.todoId,
      title: todo?.title ?? '(항목)',
      todoId: h.todoId,
      who: isAgentActor(h.actor) ? 'AGENT' : 'YOU',
      since: h.createdAt,
      live: false,
      unread: todo && hasUnreadComments(todo, input.seen) ? todo.commentCount : 0,
      state:
        h.status === 'pending'
          ? h.stale
            ? '넘김 · 세션 없음'
            : '넘김 · 아직 안 집음'
          : '넘김 · 집었는데 미착수',
    });
  }

  // PR — 데몬이 판정한 "내 차례": 충돌난 것, 확인·머지해도 되는 것. 대기 중인 것은 잡음이라 뺀다.
  for (const p of input.prs ?? []) {
    if (p.state !== 'OPEN' || (!p.ready && p.mergeState !== 'DIRTY')) {
      continue;
    }
    const conflict = p.mergeState === 'DIRTY';
    pushMine(conflict ? MINE_RANK.prConflict : MINE_RANK.prReady, {
      key: `pr:${p.repo}#${p.number}`,
      kind: 'pr',
      group: 'mine',
      glyph: conflict ? 'dead' : 'mine',
      ref: `${p.repo.split('/')[1] ?? p.repo} #${p.number}`,
      title: p.title,
      url: p.url,
      who: 'YOU',
      since: p.updatedAt,
      live: false,
      unread: 0,
      state: conflict ? 'PR 충돌' : 'PR 확인·머지',
    });
  }

  // 읽지 않은 댓글 — 끝난 일의 댓글은 내 차례가 아니다. 최근 것만 몇 행, 나머지(오래된 것 포함)는
  // 요약 한 줄로 — 새 브라우저는 전부 "안 읽음" 이라 수십 행이 쏟아질 수 있다.
  const unread = input.todos
    .filter((t) => !t.archivedAt && t.status !== 'done' && !handled.has(t.id))
    .filter((t) => hasUnreadComments(t, input.seen))
    .sort((a, b) => (b.lastCommentAt ?? '').localeCompare(a.lastCommentAt ?? ''));
  const recent = unread
    .filter((t) => t.lastCommentAt && now - Date.parse(t.lastCommentAt) <= UNREAD_WINDOW_MS)
    .slice(0, UNREAD_ROW_MAX);
  for (const t of recent) {
    pushMine(MINE_RANK.unread, {
      key: `unread:${t.id}`,
      kind: 'unread',
      group: 'mine',
      glyph: 'mine',
      ref: t.ref,
      title: t.title,
      todoId: t.id,
      who: '—',
      since: t.lastCommentAt,
      live: false,
      unread: t.commentCount,
      state: '읽지 않은 댓글',
    });
  }
  const restUnread = unread.length - recent.length;

  if (input.collect && input.collect > 0) {
    pushMine(MINE_RANK.collect, {
      key: 'collect',
      kind: 'collect',
      group: 'mine',
      glyph: 'mine',
      ref: '수집함',
      title: `아직 안 올린 항목 ${input.collect}건`,
      who: 'YOU',
      live: false,
      unread: 0,
      state: '보드로 올릴까',
    });
  }

  // 순위 → 같은 순위 안에서는 오래된 것부터(가장 오래 방치된 것이 위). 읽지 않은 댓글만은 위에서
  // 이미 최신순으로 잘라 넣었으므로 넣은 순서를 지킨다.
  mine.sort((a, b) => {
    if (a.rank !== b.rank) {
      return a.rank - b.rank;
    }
    if (a.rank === MINE_RANK.unread) {
      return a.order - b.order;
    }
    return (a.row.since ?? '').localeCompare(b.row.since ?? '');
  });
  const shown = input.expanded ? mine : mine.slice(0, MINE_ROW_MAX);
  const rows = shown.map((m) => m.row);
  const hidden = mine.length - shown.length;
  if (hidden > 0) {
    rows.push(moreRow('mine:more', `내 차례 ${hidden}개 더`, hidden));
  }
  if (restUnread > 0) {
    rows.push(moreRow('unread:more', `읽지 않은 댓글 ${restUnread}건 더 — 목록의 💬`, restUnread));
  }
  run.sort((a, b) => (a.since ?? '').localeCompare(b.since ?? ''));
  return [...rows, ...run];
}

function moreRow(key: string, title: string, hidden: number): NowRow {
  return {
    key,
    hidden,
    kind: 'unread',
    group: 'more',
    glyph: 'mine',
    ref: '',
    title,
    who: '—',
    live: false,
    unread: 0,
    state: '',
  };
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

/** 1초 틱이 필요한가 — 초가 흐르는 행(1시간 미만의 진행중)이 있을 때만. 나머지는 1분 틱이면 된다. */
export function needsSecondTick(rows: NowRow[], now = Date.now()): boolean {
  return rows.some((r) => r.live && r.since !== undefined && now - Date.parse(r.since) < 3_600_000);
}

/**
 * 묶음 머리 "내 차례 N" 의 N — 펼쳤든 접었든 같은 수여야 한다(접힘은 보여 주는 방식일 뿐 일의 수가
 * 아니다). 펼친 행 + "N개 더" 가 접어 둔 행.
 */
export function mineCount(rows: NowRow[]): number {
  return rows.reduce(
    (n, r) => (r.group === 'mine' ? n + 1 : r.key === 'mine:more' ? n + (r.hidden ?? 0) : n),
    0,
  );
}

/**
 * 가로 스크롤 컨테이너 안에서 한 요소를 가운데 오게 하는 `scrollLeft` — 양 끝에서는 넘치지 않게
 * 자른다. 문서 스크롤은 건드리지 않는 대안이 필요해서 둔다(`scrollIntoView` 는 조상을 전부 움직인다).
 * 좌표는 컨테이너 **내용** 기준(스크롤 0 일 때의 x)이다.
 */
export function centeredScrollLeft(box: {
  elStart: number;
  elWidth: number;
  viewWidth: number;
  contentWidth: number;
}): number {
  const max = Math.max(0, box.contentWidth - box.viewWidth);
  const target = box.elStart - (box.viewWidth - box.elWidth) / 2;
  return Math.round(Math.min(max, Math.max(0, target)));
}
