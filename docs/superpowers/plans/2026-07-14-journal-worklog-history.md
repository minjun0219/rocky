# journal worklog 자동 기록 + `/history` 다이제스트 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** journal 을 자동 축적되는 블랙박스 워크로그로 만들고(`Stop` hook), 축적된 워크로그를 `/history` 커맨드로 앵커 히스토리 다이제스트(`kind:"digest"`)로 증분 정리한다. 위키 증류(`wikiDir`)는 제거한다.

**Architecture:** 두 레이어 모두 기존 `AgentJournal` / `journal.jsonl` 를 재사용한다. Layer 1 = `Stop` hook 이 매 턴 트랜스크립트에서 요약을 뽑아 `kind:"turn"` 을 직접 append(LLM 0). Layer 2 = `/history` 슬래시커맨드가 배치 크기 적응적 Haiku/Sonnet 서브에이전트로 워크로그를 요약해 `kind:"digest"` 앵커 엔트리(다음 실행의 watermark 겸용)를 append.

**Tech Stack:** TypeScript (Bun runtime, no build), `@modelcontextprotocol/sdk`, Bun test. 새 런타임 prod-dep 없음.

## Global Constraints

- **언어/런타임**: TypeScript `type: module`, Bun 이 `.ts` 직접 실행. `dist/` 없음.
- **Import**: 확장자 없는 상대경로. `__dirname` 금지 — `import.meta` 사용.
- **의존성**: 새 prod-dep 추가 금지. Bun 내장 + 기존 코드만.
- **검증 게이트**: 매 커밋 전 `bun run check` / `bun run typecheck` / `bun test` 통과.
- **lockstep**: `rocky.json` shape 변경 시 `rocky.schema.json` ↔ `src/core/rocky-config.ts` 동시 갱신.
- **surface 동기화**: hooks / 커맨드 / config 변경 시 FEATURES.md(한국어) + AGENTS.md(영어) + README.md + `.claude-plugin/plugin.json` 갱신.
- **주석**: 설명 산문은 한국어, 식별자/경로/명령/URL 은 영어.
- **기본값 (spec 확정)**: `autoCapture` 기본 `true`, `captureMaxChars` 기본 `800`, `digestThreshold` 기본 `40`, 커맨드명 `/history`.

---

### Task 1: journal 코어 — wikiDir 제거 + watermark `curate`→`digest`

위키 증류를 폐기하고, 증분 watermark 기준을 `kind:"digest"` 로 바꾼다. `status()` 가 노출하던 `wikiDir` / `wikiDirSource` / `lastCurateAt` 을 정리한다.

**Files:**
- Modify: `src/core/journal.ts`
- Modify: `src/core/journal-handlers.ts:40-43` (status 핸들러 doc)
- Test: `src/core/journal.test.ts`

**Interfaces:**
- Produces: `AgentJournal` (wikiDir 제거), `JournalStatus { path, exists, totalEntries, sizeBytes, lastEntryAt?, dirSource, lastDigestAt?, projectKey }`, `createJournalFromEnv(config?: JournalEnvOptions)` where `JournalEnvOptions = { dir?: string }`, `AgentJournalOptions = { baseDir?, projectKey?, dirSource? }`, `JournalKind` 유니온에 `'turn' | 'digest'` 포함(`'curate'` 제거).

- [ ] **Step 1: journal.test.ts 에서 wikiDir 테스트 제거 + digest watermark 테스트로 교체**

`src/core/journal.test.ts` 에서 아래 테스트/단언을 **삭제**한다 (wikiDir 전면 제거):
- `surfaces wikiDir / projectKey / lastCurateAt for the curate workflow`
- `surfaces explicit dirSource / wikiDirSource unchanged before and after writes`
- `infers dirSource=default / wikiDirSource=unset when neither is provided`
- `clamps wikiDirSource=unset to config when wikiDir is present (invariant)`
- `clamps wikiDirSource=env to unset when wikiDir is absent (invariant)`
- `is applied to baseDir / wikiDir so ~ paths resolve under home` → `wikiDir` 부분만 제거(baseDir tilde 확장 단언은 유지)
- `lets ROCKY_JOURNAL_DIR / ROCKY_JOURNAL_WIKI_DIR win over config` → `ROCKY_JOURNAL_WIKI_DIR` / `getWikiDir` / `wikiDirSource` 단언 삭제, `ROCKY_JOURNAL_DIR` 부분만 유지
- 나머지 테스트의 `s.wikiDirSource` / `s.wikiDir` 단언 라인 삭제

그리고 아래 테스트를 **추가**한다 (projectKey 를 고정해 격리):

```ts
it('surfaces lastDigestAt from the newest kind:"digest" entry', async () => {
  const j = new AgentJournal({ baseDir: dir, projectKey: 'p-fixed' });
  const before = await j.status();
  expect(before.lastDigestAt).toBeUndefined();
  await j.append({ content: 'a turn happened', kind: 'turn', tags: ['turn'] });
  const mark = await j.append({ content: 'digest of 1 turn', kind: 'digest' });
  const after = await j.status();
  expect(after.lastDigestAt).toBe(mark.timestamp);
  expect(after.projectKey).toBe('p-fixed');
  // wikiDir 관련 필드는 더 이상 존재하지 않는다.
  expect((after as Record<string, unknown>).wikiDir).toBeUndefined();
  expect((after as Record<string, unknown>).wikiDirSource).toBeUndefined();
});
```

- [ ] **Step 2: 테스트 실행 → 실패 확인**

Run: `bun test src/core/journal.test.ts`
Expected: FAIL — `lastDigestAt` 은 아직 `JournalStatus` 에 없고, 삭제한 wikiDir 심볼 참조가 남아 있으면 컴파일/런타임 에러.

- [ ] **Step 3: journal.ts 에서 wikiDir 제거 + watermark 를 digest 로 변경**

`src/core/journal.ts` 수정:

1. `JournalKind` 유니온: `'curate'` → `'digest'` 로 바꾸고 `'turn'` 추가:
```ts
export type JournalKind =
  | 'decision'
  | 'blocker'
  | 'answer'
  | 'note'
  | 'turn'
  | 'digest'
  | (string & {});
```

2. `JournalWikiDirSource` 타입 정의(및 그 JSDoc) **삭제**.

3. `JournalStatus`: `wikiDir?` / `wikiDirSource` 필드 삭제, `lastCurateAt?` → `lastDigestAt?` 로 rename(JSDoc 도 "마지막 `kind:"digest"` watermark" 로):
```ts
export interface JournalStatus {
  path: string;
  exists: boolean;
  totalEntries: number;
  sizeBytes: number;
  lastEntryAt?: string;
  dirSource: JournalDirSource;
  /** 마지막 `kind:"digest"` watermark 의 timestamp (있으면). `/history` 증분 정리의 기준점. */
  lastDigestAt?: string;
  projectKey: string;
}
```

4. `AgentJournalOptions`: `wikiDir?` / `wikiDirSource?` 필드(및 JSDoc) 삭제. `baseDir` / `projectKey` / `dirSource` 만 남긴다.

5. 클래스 필드/생성자: `private readonly wikiDir?` 와 `private readonly wikiDirSource` 삭제, 생성자에서 `this.wikiDir = ...` / `this.wikiDirSource = ...` 블록 삭제. `getWikiDir()` 메서드 삭제.

6. `status()`: watermark 계산과 반환에서 wikiDir 제거, curate→digest:
```ts
const last = all[all.length - 1];
const lastDigest = [...all].reverse().find((e) => e.kind === 'digest');
return {
  path: this.file,
  exists: true,
  totalEntries: all.length,
  sizeBytes,
  projectKey: this.projectKey,
  dirSource: this.dirSource,
  lastEntryAt: last?.timestamp,
  ...(lastDigest ? { lastDigestAt: lastDigest.timestamp } : {}),
};
```
그리고 `status()` 의 `exists:false` 조기 반환에서도 `wikiDir` / `wikiDirSource` 스프레드를 제거:
```ts
if (!existsSync(this.file)) {
  return {
    path: this.file,
    exists: false,
    totalEntries: 0,
    sizeBytes: 0,
    projectKey: this.projectKey,
    dirSource: this.dirSource,
  };
}
```

7. `JournalEnvOptions`: `wikiDir?` 필드 삭제 → `{ dir?: string }` 만.

8. `createJournalFromEnv`: `envWiki` / `configWiki` / `wikiDir` / `wikiDirSource` 관련 로직 전부 삭제:
```ts
export function createJournalFromEnv(config: JournalEnvOptions = {}): AgentJournal {
  const envDir = firstNonEmpty(process.env.ROCKY_JOURNAL_DIR);
  const configDir = firstNonEmpty(config.dir);
  const baseDir = envDir ?? configDir;
  const dirSource: JournalDirSource = envDir ? 'env' : configDir ? 'config' : 'default';
  return new AgentJournal({ baseDir, dirSource });
}
```

`expandTilde` 는 `baseDir` 확장에 계속 쓰이므로 유지한다.

- [ ] **Step 4: journal-handlers.ts status doc 갱신**

`src/core/journal-handlers.ts:41` 의 JSDoc 을 wikiDir/curate 언급 없이:
```ts
/** 도구 핸들러: 저널 메타 + 마지막 digest watermark(lastDigestAt). */
export function handleJournalStatus(journal: AgentJournal): Promise<JournalStatus> {
  return journal.status();
}
```

- [ ] **Step 5: 테스트 실행 → 통과 확인**

Run: `bun test src/core/journal.test.ts`
Expected: PASS. 이어서 `bun run typecheck` 로 wikiDir 심볼 잔존 참조가 없는지 확인 (index.ts / journal-handlers 는 다음 스텝/태스크에서 정리되므로 이 시점 typecheck 실패가 남으면 그 파일도 함께 손본다).

- [ ] **Step 6: 커밋**

```bash
git add src/core/journal.ts src/core/journal-handlers.ts src/core/journal.test.ts
git commit -m "refactor(journal): wikiDir 제거 + watermark를 kind:\"digest\"로 전환

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: rocky-config — journal 필드 교체 (wikiDir 제거, autoCapture/captureMaxChars/digestThreshold 추가)

**Files:**
- Modify: `src/core/rocky-config.ts:85-97` (JournalConfig), `:216-238` (validateJournal)
- Modify: `rocky.schema.json` (lockstep)
- Test: `src/core/rocky-config.test.ts`

**Interfaces:**
- Produces: `JournalConfig = { dir?: string; autoCapture?: boolean; captureMaxChars?: number; digestThreshold?: number }` (no `wikiDir`).

- [ ] **Step 1: rocky-config.test.ts 에 실패 테스트 추가**

`src/core/rocky-config.test.ts` 에 추가 (기존 파일 패턴 따름 — `validateConfig(input, source)` 직접 호출):

```ts
it('accepts journal.autoCapture / captureMaxChars / digestThreshold', () => {
  const cfg = validateConfig(
    { journal: { dir: '/tmp/j', autoCapture: false, captureMaxChars: 500, digestThreshold: 10 } },
    'test',
  );
  expect(cfg.journal?.autoCapture).toBe(false);
  expect(cfg.journal?.captureMaxChars).toBe(500);
  expect(cfg.journal?.digestThreshold).toBe(10);
});

it('rejects journal.wikiDir (removed key)', () => {
  expect(() => validateConfig({ journal: { wikiDir: '/tmp/vault' } }, 'test')).toThrow(
    /unknown key "wikiDir"/,
  );
});

it('rejects non-boolean journal.autoCapture', () => {
  expect(() => validateConfig({ journal: { autoCapture: 'yes' } }, 'test')).toThrow(
    /autoCapture must be a boolean/,
  );
});

it('rejects non-positive-integer journal.captureMaxChars', () => {
  expect(() => validateConfig({ journal: { captureMaxChars: 0 } }, 'test')).toThrow(
    /captureMaxChars must be a positive integer/,
  );
});
```

기존에 `wikiDir` 를 유효값으로 검증하던 테스트가 있으면 삭제/수정한다.

- [ ] **Step 2: 테스트 실행 → 실패 확인**

Run: `bun test src/core/rocky-config.test.ts`
Expected: FAIL — 새 키를 아직 검증하지 않고 `wikiDir` 이 여전히 허용됨.

- [ ] **Step 3: rocky-config.ts 수정**

1. `JournalConfig` 인터페이스 교체:
```ts
/**
 * `journal_*` 도구 + `Stop` hook 자동 기록 + `/history` 다이제스트 설정.
 * `dir` 은 env `ROCKY_JOURNAL_DIR` 로, `autoCapture` 는 env `ROCKY_JOURNAL_AUTO_CAPTURE` 로
 * override 된다.
 */
export interface JournalConfig {
  /** 저널 JSONL 저장 디렉터리. 미지정 시 프로젝트별 기본 경로. */
  dir?: string;
  /** Stop hook 자동 워크로그 기록 on/off. 기본 true. env `ROCKY_JOURNAL_AUTO_CAPTURE` 우선. */
  autoCapture?: boolean;
  /** turn 엔트리 req/did 각 필드의 최대 글자 수. 기본 800. */
  captureMaxChars?: number;
  /** `/history` 가 Haiku↔Sonnet 을 가르는 신규 엔트리 수 임계. 기본 40. */
  digestThreshold?: number;
}
```

2. `ALLOWED_JOURNAL_KEYS` 교체:
```ts
const ALLOWED_JOURNAL_KEYS = new Set(['dir', 'autoCapture', 'captureMaxChars', 'digestThreshold']);
```

3. `validateJournal` 교체:
```ts
function validateJournal(journal: unknown, source: string): void {
  if (journal === null || typeof journal !== 'object' || Array.isArray(journal)) {
    throw new Error(`${source}: journal must be an object`);
  }
  const obj = journal as Record<string, unknown>;
  for (const key of Object.keys(obj)) {
    if (!ALLOWED_JOURNAL_KEYS.has(key)) {
      throw new Error(`${source}: journal: unknown key "${key}"`);
    }
  }
  if (obj.dir !== undefined && (typeof obj.dir !== 'string' || obj.dir.trim().length === 0)) {
    throw new Error(`${source}: journal.dir must be a non-empty string`);
  }
  if (obj.autoCapture !== undefined && typeof obj.autoCapture !== 'boolean') {
    throw new Error(`${source}: journal.autoCapture must be a boolean`);
  }
  for (const key of ['captureMaxChars', 'digestThreshold'] as const) {
    if (obj[key] !== undefined) {
      const v = obj[key];
      if (typeof v !== 'number' || !Number.isInteger(v) || v < 1) {
        throw new Error(`${source}: journal.${key} must be a positive integer`);
      }
    }
  }
}
```

- [ ] **Step 4: rocky.schema.json lockstep 갱신**

`rocky.schema.json` 의 `journal` 프로퍼티에서 `wikiDir` 제거, 아래 추가 (기존 `dir` 옆):
```json
"autoCapture": { "type": "boolean", "description": "Stop hook 자동 워크로그 기록 on/off (기본 true)" },
"captureMaxChars": { "type": "integer", "minimum": 1, "description": "turn 엔트리 req/did 최대 글자 수 (기본 800)" },
"digestThreshold": { "type": "integer", "minimum": 1, "description": "/history Haiku↔Sonnet 임계 (기본 40)" }
```
`journal` 의 `additionalProperties: false` 와 `wikiDir` 정의 블록이 있으면 wikiDir 블록만 삭제.

- [ ] **Step 5: 테스트 실행 → 통과 확인**

Run: `bun test src/core/rocky-config.test.ts && bun run typecheck`
Expected: PASS.

- [ ] **Step 6: 커밋**

```bash
git add src/core/rocky-config.ts src/core/rocky-config.test.ts rocky.schema.json
git commit -m "feat(config): journal.autoCapture/captureMaxChars/digestThreshold 추가, wikiDir 제거

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: 트랜스크립트 추출 순수 함수 (`extractTurn` / `buildTurnContent`)

Stop hook 이 쓸 순수 추출 로직. IO 없이 테스트 가능하게 분리.

**Files:**
- Create: `src/hooks/transcript.ts`
- Test: `src/hooks/transcript.test.ts`

**Interfaces:**
- Produces:
  - `interface TurnParts { req: string; tools: string[]; did: string }`
  - `extractTurn(transcriptText: string): TurnParts | null`
  - `buildTurnContent(parts: TurnParts, maxChars: number): string`

- [ ] **Step 1: 실패 테스트 작성**

`src/hooks/transcript.test.ts`:
```ts
import { describe, expect, it } from 'bun:test';
import { buildTurnContent, extractTurn } from './transcript';

// 한 턴: user 프롬프트 → assistant(tool_use ×2) → user(tool_result) → assistant(최종 텍스트)
const TRANSCRIPT = [
  { type: 'user', message: { role: 'user', content: [{ type: 'text', text: '엔드포인트 검색해줘' }] } },
  {
    type: 'assistant',
    message: {
      role: 'assistant',
      content: [
        { type: 'text', text: '검색합니다' },
        { type: 'tool_use', name: 'openapi_search', input: {} },
        { type: 'tool_use', name: 'openapi_search', input: {} },
      ],
    },
  },
  { type: 'user', message: { role: 'user', content: [{ type: 'tool_result', content: 'ok' }] } },
  { type: 'assistant', message: { role: 'assistant', content: [{ type: 'text', text: '3개 찾았습니다' }] } },
]
  .map((e) => JSON.stringify(e))
  .join('\n');

describe('extractTurn', () => {
  it('pulls the last real user prompt, tool names (deduped w/ count), and final assistant text', () => {
    const parts = extractTurn(TRANSCRIPT);
    expect(parts).not.toBeNull();
    expect(parts?.req).toBe('엔드포인트 검색해줘');
    expect(parts?.tools).toEqual(['openapi_search(×2)']);
    expect(parts?.did).toBe('3개 찾았습니다');
  });

  it('ignores tool_result-only user messages as the prompt boundary', () => {
    // 마지막 real user prompt 는 tool_result 가 아니라 텍스트 프롬프트여야 한다
    const parts = extractTurn(TRANSCRIPT);
    expect(parts?.req).toBe('엔드포인트 검색해줘');
  });

  it('returns null when there is no user prompt', () => {
    const onlyAssistant = JSON.stringify({
      type: 'assistant',
      message: { role: 'assistant', content: [{ type: 'text', text: 'hi' }] },
    });
    expect(extractTurn(onlyAssistant)).toBeNull();
  });

  it('skips malformed lines gracefully', () => {
    const withGarbage = `not json\n${TRANSCRIPT}\n{"partial":`;
    expect(extractTurn(withGarbage)?.req).toBe('엔드포인트 검색해줘');
  });
});

describe('buildTurnContent', () => {
  it('formats req/tools/did and collapses whitespace + truncates', () => {
    const s = buildTurnContent({ req: 'a  b\n\nc', tools: ['x', 'y'], did: 'done' }, 4);
    expect(s).toBe('req: a b… | tools: x, y | did: done');
  });

  it('shows (none) for empty parts and caps tools at 20', () => {
    const many = Array.from({ length: 25 }, (_, i) => `t${i}`);
    const s = buildTurnContent({ req: '', tools: many, did: '' }, 800);
    expect(s.startsWith('req: (none) | tools: t0, t1')).toBe(true);
    expect(s).toContain('did: (none)');
    expect(s.split('tools: ')[1]?.split(' | ')[0]?.split(', ').length).toBe(20);
  });
});
```

- [ ] **Step 2: 테스트 실행 → 실패 확인**

Run: `bun test src/hooks/transcript.test.ts`
Expected: FAIL — `./transcript` 모듈 없음.

- [ ] **Step 3: transcript.ts 구현**

`src/hooks/transcript.ts`:
```ts
/**
 * Claude Code 트랜스크립트(JSONL) 에서 "마지막 한 턴" 을 기계적으로 추출한다.
 * LLM 없이 동작 — Stop hook 이 워크로그 한 줄을 만들 재료(req/tools/did)만 뽑는다.
 */

/** 한 턴에서 뽑아낸 요약 재료. */
export interface TurnParts {
  /** 이 턴을 연 사용자 프롬프트 텍스트. */
  req: string;
  /** 이 턴에서 호출된 도구 이름 (중복은 `이름(×N)`). */
  tools: string[];
  /** 이 턴 마지막 assistant 텍스트 블록. */
  did: string;
}

interface RawBlock {
  type?: string;
  text?: string;
  name?: string;
}
interface RawMessage {
  role?: string;
  content?: string | RawBlock[];
}
interface RawEntry {
  message?: RawMessage;
}

/** content(문자열 또는 블록 배열) 에서 text 블록만 이어붙인다. */
function textOf(content: string | RawBlock[] | undefined): string {
  if (typeof content === 'string') {
    return content.trim();
  }
  if (!Array.isArray(content)) {
    return '';
  }
  return content
    .filter((b): b is RawBlock => !!b && b.type === 'text' && typeof b.text === 'string')
    .map((b) => b.text as string)
    .join('\n')
    .trim();
}

/** tool_result 만 담긴 user 메시지(=도구 응답)는 프롬프트가 아니다 — text 블록이 있어야 한다. */
function isRealUserPrompt(msg: RawMessage): boolean {
  if (msg.role !== 'user') {
    return false;
  }
  if (typeof msg.content === 'string') {
    return msg.content.trim().length > 0;
  }
  if (!Array.isArray(msg.content)) {
    return false;
  }
  return msg.content.some(
    (b) => !!b && b.type === 'text' && typeof b.text === 'string' && b.text.trim().length > 0,
  );
}

/**
 * 트랜스크립트 텍스트에서 마지막 real user prompt ~ 끝까지를 한 턴으로 보고 추출.
 * 프롬프트가 하나도 없으면 null. 손상 라인은 skip.
 */
export function extractTurn(transcriptText: string): TurnParts | null {
  const entries: RawEntry[] = [];
  for (const line of transcriptText.split('\n')) {
    const t = line.trim();
    if (!t) {
      continue;
    }
    try {
      const parsed = JSON.parse(t);
      if (parsed && typeof parsed === 'object') {
        entries.push(parsed as RawEntry);
      }
    } catch {
      // 손상/부분 라인 skip
    }
  }
  let startIdx = -1;
  for (let i = entries.length - 1; i >= 0; i--) {
    const msg = entries[i]?.message;
    if (msg && isRealUserPrompt(msg)) {
      startIdx = i;
      break;
    }
  }
  if (startIdx < 0) {
    return null;
  }
  const req = textOf(entries[startIdx]?.message?.content);
  const toolCounts = new Map<string, number>();
  let did = '';
  for (let i = startIdx + 1; i < entries.length; i++) {
    const msg = entries[i]?.message;
    if (!msg || msg.role !== 'assistant') {
      continue;
    }
    if (Array.isArray(msg.content)) {
      for (const b of msg.content) {
        if (b && b.type === 'tool_use' && typeof b.name === 'string') {
          toolCounts.set(b.name, (toolCounts.get(b.name) ?? 0) + 1);
        }
      }
    }
    const txt = textOf(msg.content);
    if (txt) {
      did = txt;
    }
  }
  const tools = [...toolCounts.entries()].map(([name, n]) => (n > 1 ? `${name}(×${n})` : name));
  if (!req && !did && tools.length === 0) {
    return null;
  }
  return { req, tools, did };
}

/** req/tools/did 를 한 줄로 조립 — 공백 축약 + maxChars truncate + tools 20개 cap. */
export function buildTurnContent(parts: TurnParts, maxChars: number): string {
  const clip = (s: string): string => {
    const one = s.replace(/\s+/g, ' ').trim();
    return one.length > maxChars ? `${one.slice(0, maxChars)}…` : one;
  };
  const req = clip(parts.req) || '(none)';
  const tools = parts.tools.slice(0, 20).join(', ') || '(none)';
  const did = clip(parts.did) || '(none)';
  return `req: ${req} | tools: ${tools} | did: ${did}`;
}
```

- [ ] **Step 4: 테스트 실행 → 통과 확인**

Run: `bun test src/hooks/transcript.test.ts`
Expected: PASS (6 tests).

- [ ] **Step 5: 커밋**

```bash
git add src/hooks/transcript.ts src/hooks/transcript.test.ts
git commit -m "feat(hooks): 트랜스크립트 턴 추출 순수 함수 extractTurn/buildTurnContent

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: Stop hook — gating + `log-turn.ts` 엔트리 + `hooks/hooks.json`

트랜스크립트 추출을 실제 hook 으로 배선한다. **절대 턴을 막지 않는다** (항상 exit 0).

**Files:**
- Create: `src/hooks/log-turn.ts`
- Create: `hooks/hooks.json`
- Test: `src/hooks/log-turn.test.ts` (gating 순수 함수만 단위 테스트)

**Interfaces:**
- Consumes: `extractTurn` / `buildTurnContent` (Task 3), `createJournalFromEnv` / `JournalEnvOptions` (Task 1), `loadConfig` + `JournalConfig` (Task 2).
- Produces: `shouldCapture(env: NodeJS.ProcessEnv, config: JournalConfig | undefined): boolean`.

- [ ] **Step 1: gating 실패 테스트 작성**

`src/hooks/log-turn.test.ts`:
```ts
import { describe, expect, it } from 'bun:test';
import { shouldCapture } from './log-turn';

describe('shouldCapture', () => {
  it('defaults to true when nothing is set', () => {
    expect(shouldCapture({}, undefined)).toBe(true);
    expect(shouldCapture({}, {})).toBe(true);
  });

  it('config.autoCapture:false disables', () => {
    expect(shouldCapture({}, { autoCapture: false })).toBe(false);
  });

  it('env ROCKY_JOURNAL_AUTO_CAPTURE=0/false/off disables (wins over config true)', () => {
    for (const v of ['0', 'false', 'off', 'no']) {
      expect(shouldCapture({ ROCKY_JOURNAL_AUTO_CAPTURE: v }, { autoCapture: true })).toBe(false);
    }
  });

  it('env with any other value enables (wins over config false)', () => {
    expect(shouldCapture({ ROCKY_JOURNAL_AUTO_CAPTURE: '1' }, { autoCapture: false })).toBe(true);
  });
});
```

- [ ] **Step 2: 테스트 실행 → 실패 확인**

Run: `bun test src/hooks/log-turn.test.ts`
Expected: FAIL — `./log-turn` / `shouldCapture` 없음.

- [ ] **Step 3: log-turn.ts 구현**

`src/hooks/log-turn.ts`:
```ts
import { existsSync } from 'node:fs';
import { readFile } from 'node:fs/promises';
import { createJournalFromEnv } from '../core/journal';
import type { JournalConfig } from '../core/rocky-config';
import { loadConfig } from '../core/rocky-config';
import { buildTurnContent, extractTurn } from './transcript';

/**
 * Stop hook: 매 응답 종료 시 트랜스크립트에서 이번 턴을 뽑아 `kind:"turn"` 한 줄을
 * append 한다. 결정론적(LLM 0). 어떤 실패도 턴을 막지 않도록 항상 exit 0.
 */

/** env(우선) → config(기본 true) 로 자동 캡처 여부 결정. `0/false/off/no` 만 비활성. */
export function shouldCapture(
  env: NodeJS.ProcessEnv,
  config: JournalConfig | undefined,
): boolean {
  const raw = env.ROCKY_JOURNAL_AUTO_CAPTURE;
  if (typeof raw === 'string' && raw.trim().length > 0) {
    const v = raw.trim().toLowerCase();
    return !(v === '0' || v === 'false' || v === 'off' || v === 'no');
  }
  return config?.autoCapture !== false;
}

interface StopHookInput {
  transcript_path?: string;
  cwd?: string;
}

async function readStdin(): Promise<string> {
  let raw = '';
  for await (const chunk of process.stdin) {
    raw += chunk;
  }
  return raw;
}

async function run(): Promise<void> {
  const raw = await readStdin();
  let input: StopHookInput;
  try {
    input = JSON.parse(raw) as StopHookInput;
  } catch {
    return; // stdin 이 JSON 이 아니면 조용히 종료
  }
  const projectRoot = input.cwd ?? process.cwd();
  const { config } = await loadConfig({ projectRoot });
  if (!shouldCapture(process.env, config.journal)) {
    return;
  }
  const path = input.transcript_path;
  if (!path || !existsSync(path)) {
    return;
  }
  const parts = extractTurn(await readFile(path, 'utf8'));
  if (!parts) {
    return;
  }
  const maxChars = config.journal?.captureMaxChars ?? 800;
  const content = buildTurnContent(parts, maxChars);
  const journal = createJournalFromEnv(config.journal);
  await journal.append({ content, kind: 'turn', tags: ['turn'] });
}

// 엔트리로 직접 실행될 때만 run(). 테스트에서 import 하면 실행되지 않는다.
if (import.meta.main) {
  run()
    .catch(() => {
      // 절대 턴을 막지 않는다 — 모든 오류 삼킴
    })
    .finally(() => process.exit(0));
}
```

> 참고: `import.meta.main` 은 Bun 이 파일을 엔트리로 실행할 때만 true — 테스트 import 시 `run()` 이 안 돈다.

- [ ] **Step 4: 테스트 실행 → 통과 확인**

Run: `bun test src/hooks/log-turn.test.ts`
Expected: PASS (4 tests).

- [ ] **Step 5: hooks/hooks.json 생성**

`hooks/hooks.json`:
```json
{
  "hooks": {
    "Stop": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "bun run \"${CLAUDE_PLUGIN_ROOT}/src/hooks/log-turn.ts\""
          }
        ]
      }
    ]
  }
}
```

- [ ] **Step 6: 엔드투엔드 수동 검증**

fixture stdin 으로 hook 을 직접 실행해 turn 엔트리가 쌓이는지 확인:
```bash
ROCKY_JOURNAL_DIR=/tmp/rocky-hooktest \
  bun run src/hooks/log-turn.ts <<'EOF'
{"transcript_path":"/tmp/does-not-exist.jsonl","cwd":"/tmp"}
EOF
echo "exit: $?"   # transcript 부재 → no-op, exit 0 이어야 함
```
Expected: `exit: 0`, 파일 없이 조용히 종료. (실제 트랜스크립트 왕복은 Task 6 의 `/reload-plugins` 검증에서 확인.)

- [ ] **Step 7: 커밋**

```bash
git add src/hooks/log-turn.ts src/hooks/log-turn.test.ts hooks/hooks.json
git commit -m "feat(hooks): Stop hook log-turn — 매 턴 kind:\"turn\" 자동 기록 (autoCapture 기본 ON)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: `/history` 커맨드 (curate.md 삭제 → history.md 생성)

**Files:**
- Delete: `commands/curate.md`
- Create: `commands/history.md`

**Interfaces:** 없음 (프로세스/산문). journal MCP 도구(`journal_status`/`journal_read`/`journal_search`/`journal_append`) + `Task`(서브에이전트) 를 쓴다.

- [ ] **Step 1: curate.md 삭제 + history.md 작성**

```bash
git rm commands/curate.md
```

`commands/history.md`:
```markdown
---
description: 워크로그(journal 의 kind:"turn" + 수동 decision/blocker)를 읽어 앵커 히스토리 다이제스트로 정리한다 — 마지막 digest 이후 항목만 증분 요약해 kind:"digest" 엔트리로 남기고, 각 앵커는 원본 엔트리 id 로 드릴다운 가능하게 한다. 배치 크기에 따라 Haiku/Sonnet 서브에이전트를 고른다.
argument-hint: "[집중할 주제/힌트] (생략 가능)"
allowed-tools: mcp__plugin_rocky_rocky__journal_status, mcp__plugin_rocky_rocky__journal_read, mcp__plugin_rocky_rocky__journal_search, mcp__plugin_rocky_rocky__journal_append, Task
---

# history — 워크로그 → 앵커 히스토리 다이제스트

rocky 의 journal 은 **기록(logbook)** 레이어다 — `Stop` hook 이 매 턴 `kind:"turn"` 워크로그를
자동으로 쌓는다(수동 `decision`/`blocker`/`answer`/`note` 도 공존). 이 커맨드는 그 워크로그를
읽어 **히스토리 다이제스트**로 정리한다 — 별도 위키 문서가 아니라, 워크로그로 **찾아 들어갈 수
있는 앵커**(각 항목이 원본 엔트리 id 를 가리킴)로. `$ARGUMENTS` 는 집중할 주제 힌트(있으면).

## 원칙

1. **rocky 는 기록만, 정리는 이 커맨드가.** 요약(어떤 순간을 앵커로 남길지)은 호스트 LLM /
   서브에이전트가 한다.
2. **증분.** 마지막 `kind:"digest"` watermark 이후 항목만 처리한다.
3. **저널은 불변.** 기존 줄을 지우거나 편집하지 않는다. 다이제스트도 `journal_append` 로 한 줄.
4. **앵커는 드릴다운용.** 각 앵커는 요약 + 원본 엔트리 `id`(+timestamp) 를 담아, 읽는 쪽이
   필요하면 `journal_read` 로 원문을 찾아갈 수 있게 한다.
5. **네이티브 메모리와 별개.** 이 다이제스트는 journal 안에 산다. Claude Code 글로벌 메모리를
   건드리지 않는다.

## 절차

### 1. 상태 확인 → watermark

```
journal_status
```
- `totalEntries` 가 0 이면 "정리할 워크로그 없음" 후 종료.
- 마지막 watermark 조회:
  ```
  journal_read { kind: "digest", limit: 1 }
  ```
  결과가 있으면 그 `timestamp` 가 watermark, 없으면 첫 실행.

### 2. 새 워크로그 수집 (증분)

- watermark 가 있으면 그 이후만:
  ```
  journal_read { since: <watermark>, limit: 500 }
  ```
  없으면 전체를 최근부터 넉넉한 limit 으로.
- `$ARGUMENTS` 힌트가 있으면 `journal_search` 로 관련 항목 보강.
- `kind:"digest"` 항목은 정리 대상에서 제외.
- 새 항목 수 `n` 을 센다. `n == 0` → "새 워크로그 없음" 후 종료 (watermark 안 남김).

### 3. 적응적 모델로 서브에이전트 dispatch

- `journal.digestThreshold` (기본 40) 기준: `n <= 40` → **Haiku**, `n > 40` → **Sonnet**.
- `Task` 로 서브에이전트를 띄운다 (model 을 위 규칙대로 지정). 서브에이전트에 수집한
  워크로그 항목(각 항목의 `id` / `timestamp` / `kind` / `content` 포함)을 넘기고, 아래
  **앵커 다이제스트**를 만들게 한다:
  - raw 나열 금지 — 의미 있는 순간(결정 / 전환 / blocker / 사용자 답변)만 골라 서술.
  - 각 앵커 끝에 원본 엔트리 `id:<id> (<ts>)` 를 붙여 드릴다운 가능하게.
  - 포맷 예:
    ```markdown
    ## digest — <n> entries, <first ts> … <last ts>
    - <결정/전환 요약> → id:<entry-id> (<ts>)
    - <blocker 해결> → id:<entry-id> (<ts>)
    ```

### 4. 다이제스트 append (watermark 겸용)

```
journal_append {
  kind: "digest",
  content: "<서브에이전트가 만든 앵커 다이제스트>",
  tags: ["digest"]
}
```
- 이 엔트리 timestamp 가 다음 `/history` 의 `since` 기준점.

### 5. 마무리

- 만든 다이제스트(앵커 목록)를 한국어로 보고한다 — 드릴다운 id 포함. 장문 리포트 금지.

## 예외 처리

- `totalEntries == 0` 또는 새 항목 0 → no-op 종료 (watermark 안 남김).
- 서브에이전트 실패 → 다이제스트를 append 하지 말고 실패만 알린다 (watermark 오염 방지).
```

- [ ] **Step 2: 게이트 확인**

Run: `bun run check`
Expected: PASS (commands/*.md 는 Biome 대상 아님 — 회귀 없음 확인용).

- [ ] **Step 3: 커밋**

```bash
git add commands/history.md
git commit -m "feat(command): /history — 워크로그를 앵커 히스토리 다이제스트로 (구 /curate 대체)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 6: surface 테스트 + 문서/매니페스트 동기화

새 MCP 도구는 없다 — surface 회귀 가드를 재확인하고, wikiDir 를 참조하던 index 테스트/설명과 사람용/에이전트용 문서를 갱신한다.

**Files:**
- Modify: `src/index.ts` (`journal_status` 설명 문자열)
- Modify: `src/index.test.ts:162-183` (wikiDir 테스트)
- Modify: `.claude-plugin/plugin.json` (description)
- Modify: `FEATURES.md`, `AGENTS.md`, `README.md`

**Interfaces:** 없음 (문서 + 설명 문자열).

- [ ] **Step 1: index.test.ts wikiDir 테스트 교체**

`src/index.test.ts` 의 `journal_status reports exists=false and surfaces wikiDir (no writes)` 테스트를 아래로 교체 (wikiDir 대신 dirSource/exists 검증):
```ts
test('journal_status reports exists=false without wikiDir fields', async () => {
  const client = await connect({ notionCli: absentNotionCli });
  try {
    const result = await client.callTool({ name: 'journal_status', arguments: {} });
    const content = (result.content as Array<{ type: string; text: string }>)[0];
    const parsed = JSON.parse(content!.text);
    expect(parsed.exists).toBe(false);
    expect(parsed.totalEntries).toBe(0);
    expect(parsed.wikiDir).toBeUndefined();
    expect(parsed.wikiDirSource).toBeUndefined();
    expect(typeof parsed.dirSource).toBe('string');
  } finally {
    await client.close().catch(() => undefined);
  }
});
```

- [ ] **Step 2: index.ts journal_status 설명 갱신**

`src/index.ts` 의 `journal_status` 등록 설명 문자열(라인 ~301)에서 wikiDir/curate 언급 제거:
```ts
'저널 메타(파일 경로, 존재 여부, 유효 항목 수 — 손상 라인 skip, 바이트 크기, 마지막 항목 시각) + 마지막 digest watermark(lastDigestAt) + 경로 출처(dirSource)를 조회한다. `/history` 가 정리 시작 시 이걸로 증분 기준점을 확인한다. remote 호출 없음. 저널 저장 위치는 `journal.dir`(rocky.json) 또는 `ROCKY_JOURNAL_DIR`(env 우선)로 변경 가능하다.'
```
(같은 파일에 `wikiDir` / `ROCKY_JOURNAL_WIKI_DIR` 을 언급하는 다른 설명 문자열이 있으면 함께 정리.)

- [ ] **Step 3: surface 테스트 실행 → 통과 확인**

Run: `bun test src/index.test.ts`
Expected: PASS — 도구 개수 불변(`OPENAPI_TOOLS + JOURNAL_TOOLS`), wikiDir 필드 부재, 제거 도메인 누수 없음.

- [ ] **Step 4: plugin.json description 갱신**

`.claude-plugin/plugin.json` 의 `description` 에서 `/curate ...wiki...` 문구를 제거하고, journal 이 `Stop` hook 으로 자동 워크로그를 기록하며 `/history` 로 다이제스트한다는 내용으로 교체. (버전 bump 는 선택 — 유지하거나 `0.9.0` 로 올린다; 올린다면 `package.json` 도 lockstep.)

- [ ] **Step 5: FEATURES.md / AGENTS.md / README.md 갱신**

- `FEATURES.md`(한국어): journal 표에 `Stop` hook 자동 기록(`kind:"turn"`), env `ROCKY_JOURNAL_AUTO_CAPTURE`, config `autoCapture`/`captureMaxChars`/`digestThreshold` 추가; `/curate`·wikiDir·`ROCKY_JOURNAL_WIKI_DIR` 제거; `/history` 커맨드 추가.
- `AGENTS.md`(영어): *Project in one line* 에 hooks 자동 기록 + `/history` 반영, wiki/`/curate` 제거. *Layout* 에 `hooks/hooks.json`, `src/hooks/log-turn.ts`, `src/hooks/transcript.ts`, `commands/history.md` 추가, `commands/curate.md` 제거, journal.ts 의 wikiDir 설명 갱신. *MVP scope* 의 wikiDir/curate 서술을 worklog/history 로 갱신.
- `README.md`: surface 설명에서 `/curate`→`/history`, hooks 자동 기록 추가.

- [ ] **Step 6: 전체 게이트 + 커밋**

Run: `bun run check && bun run typecheck && bun test`
Expected: 전부 PASS.

```bash
git add src/index.ts src/index.test.ts .claude-plugin/plugin.json package.json FEATURES.md AGENTS.md README.md
git commit -m "docs(journal): worklog 자동 기록 + /history surface 반영, wikiDir/curate 제거

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

- [ ] **Step 7: 로컬 플러그인 왕복 검증 (수동)**

`/reload-plugins` 후:
1. 아무 턴이나 돌린 뒤 `journal_status` → `exists:true`, `kind:"turn"` 엔트리 누적 확인.
2. `/history` 실행 → `kind:"digest"` 앵커 다이제스트 생성 + 보고 확인.
3. `ROCKY_JOURNAL_AUTO_CAPTURE=0` 로 끄고 턴 → 새 turn 엔트리 안 쌓이는지 확인.

---

## Self-Review

**Spec coverage:**
- Layer 1 (Stop hook 자동 turn 기록) → Task 3(추출) + Task 4(hook/gating/hooks.json). ✅
- Layer 2 (`/history` 적응적 Haiku/Sonnet, kind:"digest", watermark 겸용) → Task 5. ✅
- 위키 증류 + wikiDir 제거 → Task 1(journal 코어) + Task 2(config) + Task 6(index/docs). ✅
- autoCapture 기본 ON + kill switch → Task 2(config 기본값) + Task 4(shouldCapture env/config). ✅
- truncation/tools cap → Task 3(buildTurnContent). ✅
- 적응적 모델 임계(digestThreshold 40) → Task 2(config) + Task 5(커맨드 규칙). ✅
- read-back 없음 → 자동 주입 hook 미도입(설계대로 out). ✅
- surface 카운트 불변(새 MCP 도구 없음) → Task 6(index.test). ✅
- 문서/매니페스트 동기화 → Task 6. ✅

**Placeholder scan:** 모든 코드 스텝에 실제 구현/테스트 코드 포함. "적절한 에러 처리" 류 표현 없음. ✅

**Type consistency:**
- `JournalStatus` 의 `lastDigestAt`(Task 1) ↔ `/history` 가 `journal_read {kind:"digest"}` 로 watermark 조회(Task 5) — 필드명이 아니라 kind 로 조회하므로 정합. index.ts 설명도 `lastDigestAt` 로 통일(Task 6). ✅
- `shouldCapture(env, config?)`(Task 4) ↔ `JournalConfig.autoCapture`(Task 2) 시그니처 일치. ✅
- `createJournalFromEnv(config?: JournalEnvOptions={dir?})`(Task 1) ↔ log-turn 이 `config.journal` 전달(Task 4) — `JournalConfig` ⊇ `JournalEnvOptions`(dir) 이라 구조적 호환. ✅
- `extractTurn`/`buildTurnContent`/`TurnParts`(Task 3) ↔ log-turn import(Task 4) 이름 일치. ✅
