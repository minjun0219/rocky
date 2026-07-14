# worklog 자동 기록 + `/recall` 다이제스트 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 기존 `journal` 도메인을 `worklog` 로 전면 개명하고, 자동 축적되는 블랙박스 워크로그(`Stop` hook, `kind:"turn"`) + 축적분을 앵커 히스토리 다이제스트(`/recall`, `kind:"digest"`)로 증분 정리하는 반자동 레이어를 붙인다. 위키 증류(`wikiDir`)는 제거한다.

**Architecture:** 두 레이어 모두 `Worklog` / `worklog.jsonl` 를 재사용한다. Layer 1 = `Stop` hook 이 매 턴 트랜스크립트에서 요약을 뽑아 `kind:"turn"` 을 직접 append(LLM 0). Layer 2 = `/recall` 슬래시커맨드가 배치 크기 적응적 Haiku/Sonnet 서브에이전트로 워크로그를 요약해 `kind:"digest"` 앵커 엔트리(다음 실행의 watermark 겸용)를 append.

**Tech Stack:** TypeScript (Bun runtime, no build), `@modelcontextprotocol/sdk`, Bun test. 새 런타임 prod-dep 없음.

## Global Constraints

- **언어/런타임**: TypeScript `type: module`, Bun 이 `.ts` 직접 실행. `dist/` 없음.
- **Import**: 확장자 없는 상대경로. `__dirname` 금지 — `import.meta` 사용.
- **의존성**: 새 prod-dep 추가 금지. Bun 내장 + 기존 코드만.
- **검증 게이트**: 매 커밋 전 `bun run check` / `bun run typecheck` / `bun test` 통과.
- **lockstep**: `rocky.json` shape 변경 시 `rocky.schema.json` ↔ `src/core/rocky-config.ts` 동시 갱신.
- **surface 동기화**: 개명 / hooks / 커맨드 / config 변경 시 FEATURES.md(한국어) + AGENTS.md(영어) + README.md + `.claude-plugin/plugin.json` 갱신.
- **주석**: 설명 산문은 한국어, 식별자/경로/명령/URL 은 영어.
- **기본값 (spec 확정)**: `autoCapture` 기본 `true`, `captureMaxChars` 기본 `800`, `digestThreshold` 기본 `40`, 커맨드명 `/recall`.
- **네이밍 규칙 (개명 후, 전 태스크 공통)**:
  - 도구 `worklog_append/read/search/status` · 클래스 `Worklog` · 파일 `src/core/worklog.ts` / `worklog-handlers.ts`
  - 팩토리 `createWorklogFromEnv` · 핸들러 `handleWorklog{Append,Read,Search,Status}`
  - 타입 `WorklogEntry/WorklogKind/WorklogStatus/WorklogAppendInput/WorklogReadOptions/WorklogSearchOptions/WorklogDirSource/WorklogEnvOptions/WorklogOptions/WorklogConfig`
  - config 키 `worklog` · env `ROCKY_WORKLOG_DIR` / `ROCKY_WORKLOG_AUTO_CAPTURE`
  - 저장 `~/.config/rocky/worklog/<key>/worklog.jsonl` (상수 `WORKLOG_FILE = 'worklog.jsonl'`, `DEFAULT_WORKLOG_ROOT`)
  - `kind` 값(`turn`/`digest`/`decision`/`blocker`/`answer`/`note`)은 그대로.

---

### Task 0: `journal` → `worklog` 전면 개명 (동작 변화 없음)

기존 journal 도메인을 worklog 로 순수 리네임한다. **동작·시그니처 불변**, 이름만 바꾸는 커밋. wikiDir 제거·watermark 변경·자동 기록은 이후 태스크에서.

**Files:**
- Rename: `src/core/journal.ts` → `src/core/worklog.ts`, `src/core/journal-handlers.ts` → `src/core/worklog-handlers.ts`, `src/core/journal.test.ts` → `src/core/worklog.test.ts`
- Modify: `src/core/index.ts` (barrel export), `src/index.ts` (도구 등록 + import), `src/index.test.ts` (`JOURNAL_TOOLS`→`WORKLOG_TOOLS`)
- Grep-sweep: `journal` / `Journal` / `JOURNAL` 잔존 참조

**Interfaces:**
- Produces (개명): `Worklog` 클래스, `createWorklogFromEnv`, `handleWorklog{Append,Read,Search,Status}`, `Worklog*` 타입들, 도구 `worklog_*`.

- [ ] **Step 1: 파일 rename (git mv)**

```bash
git mv src/core/journal.ts src/core/worklog.ts
git mv src/core/journal-handlers.ts src/core/worklog-handlers.ts
git mv src/core/journal.test.ts src/core/worklog.test.ts
```

- [ ] **Step 2: 심볼 일괄 치환**

아래 파일에서 치환 (긴 심볼부터 적용해 부분매칭 사고 방지):
- `AgentJournalOptions` → `WorklogOptions`
- `AgentJournal` → `Worklog`
- `createJournalFromEnv` → `createWorklogFromEnv`
- `handleJournal` → `handleWorklog`
- `JournalEntry`→`WorklogEntry`, `JournalKind`→`WorklogKind`, `JournalStatus`→`WorklogStatus`, `JournalAppendInput`→`WorklogAppendInput`, `JournalReadOptions`→`WorklogReadOptions`, `JournalSearchOptions`→`WorklogSearchOptions`, `JournalDirSource`→`WorklogDirSource`, `JournalEnvOptions`→`WorklogEnvOptions`, `JournalConfig`→`WorklogConfig`
- `DEFAULT_JOURNAL_ROOT`→`DEFAULT_WORKLOG_ROOT`, `JOURNAL_FILE`→`WORKLOG_FILE`, 상수값 `'journal.jsonl'`→`'worklog.jsonl'`
- 저장 루트 조각 `join(homedir(), '.config','rocky','journal')` 의 `'journal'`→`'worklog'`
- env `ROCKY_JOURNAL_DIR`→`ROCKY_WORKLOG_DIR`; `ROCKY_JOURNAL_WIKI_DIR`→`ROCKY_WORKLOG_WIKI_DIR` (이 wiki env 는 Task 1 에서 완전 삭제)
- 도구 이름 문자열 `'journal_append'`/`'journal_read'`/`'journal_search'`/`'journal_status'`→`'worklog_*'` (src/index.ts)
- `defaultProjectKey` 등 journal 단어 없는 헬퍼는 유지.

대상: `src/core/worklog.ts`, `worklog-handlers.ts`, `worklog.test.ts`, `src/core/index.ts`, `src/index.ts`, `src/index.test.ts`.
`src/core/index.ts` barrel: `export * from './journal(-handlers)'` → `'./worklog(-handlers)'`.

- [ ] **Step 3: index.test.ts surface 가드 개명**

```ts
/** worklog_* 는 기록 레이어 — CLI-gate 없이 항상 등록 (openapi + seo 와 함께 base surface). */
const WORKLOG_TOOLS = ['worklog_append', 'worklog_read', 'worklog_search', 'worklog_status'] as const;
```
`JOURNAL_TOOLS` 참조 전부(`expect(names).toEqual([...OPENAPI_TOOLS, ...WORKLOG_TOOLS].sort())`, gate-무관 등록 테스트, round-trip 의 도구 이름 `journal_append`/`journal_read`)를 `worklog_*` 로 치환. `process.env.ROCKY_JOURNAL_DIR`→`ROCKY_WORKLOG_DIR`. `REMOVED_TOOLS` 는 그대로.

- [ ] **Step 4: grep-sweep 잔존 참조 제거**

```bash
grep -rn "journal\|Journal\|JOURNAL" src/ | grep -vi "worklog"
```
Expected: 빈 결과(또는 Task 1 에서 지울 `ROCKY_WORKLOG_WIKI_DIR` 처럼 wiki 관련만). 남으면 치환.

- [ ] **Step 5: 게이트 실행 → 통과 확인**

Run: `bun run typecheck && bun test`
Expected: PASS — 이름만 바뀌고 동작 동일.

- [ ] **Step 6: 커밋**

```bash
git add -A
git commit -m "refactor: journal 도메인을 worklog 로 전면 개명 (동작 불변)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 1: worklog 코어 — wikiDir 제거 + watermark `curate`→`digest`

**Files:**
- Modify: `src/core/worklog.ts`, `src/core/worklog-handlers.ts` (status doc)
- Test: `src/core/worklog.test.ts`

**Interfaces:**
- Produces: `Worklog`(wikiDir 제거), `WorklogStatus { path, exists, totalEntries, sizeBytes, lastEntryAt?, dirSource, lastDigestAt?, projectKey }`, `createWorklogFromEnv(config?: WorklogEnvOptions={dir?})`, `WorklogOptions={baseDir?,projectKey?,dirSource?}`, `WorklogKind` 에 `'turn'|'digest'` 포함(`'curate'` 제거).

- [ ] **Step 1: worklog.test.ts 에서 wikiDir 테스트 제거 + digest watermark 테스트 추가**

**삭제**할 테스트/단언 (wikiDir 전면 제거):
- `surfaces wikiDir / projectKey / lastCurateAt for the curate workflow`
- `surfaces explicit dirSource / wikiDirSource unchanged before and after writes`
- `infers dirSource=default / wikiDirSource=unset when neither is provided`
- `clamps wikiDirSource=unset to config when wikiDir is present (invariant)`
- `clamps wikiDirSource=env to unset when wikiDir is absent (invariant)`
- `is applied to baseDir / wikiDir so ~ paths resolve under home` → wikiDir 부분만 제거(baseDir tilde 단언 유지)
- `lets ROCKY_WORKLOG_DIR / ROCKY_WORKLOG_WIKI_DIR win over config` → WIKI 단언 삭제, DIR 부분만 유지
- 나머지 테스트의 `s.wikiDirSource`/`s.wikiDir` 단언 삭제

**추가**:
```ts
it('surfaces lastDigestAt from the newest kind:"digest" entry', async () => {
  const w = new Worklog({ baseDir: dir, projectKey: 'p-fixed' });
  expect((await w.status()).lastDigestAt).toBeUndefined();
  await w.append({ content: 'a turn happened', kind: 'turn', tags: ['turn'] });
  const mark = await w.append({ content: 'digest of 1 turn', kind: 'digest' });
  const after = await w.status();
  expect(after.lastDigestAt).toBe(mark.timestamp);
  expect(after.projectKey).toBe('p-fixed');
  expect((after as Record<string, unknown>).wikiDir).toBeUndefined();
  expect((after as Record<string, unknown>).wikiDirSource).toBeUndefined();
});
```

- [ ] **Step 2: 테스트 실행 → 실패 확인**

Run: `bun test src/core/worklog.test.ts`
Expected: FAIL — `lastDigestAt` 미존재 / 삭제한 wikiDir 심볼 참조 에러.

- [ ] **Step 3: worklog.ts wikiDir 제거 + watermark digest 화**

1. `WorklogKind`: `'curate'`→`'digest'`, `'turn'` 추가:
```ts
export type WorklogKind =
  | 'decision' | 'blocker' | 'answer' | 'note' | 'turn' | 'digest' | (string & {});
```
2. `WorklogWikiDirSource` 타입/JSDoc **삭제**.
3. `WorklogStatus`: `wikiDir?`/`wikiDirSource` 삭제, `lastCurateAt?`→`lastDigestAt?`:
```ts
export interface WorklogStatus {
  path: string; exists: boolean; totalEntries: number; sizeBytes: number;
  lastEntryAt?: string; dirSource: WorklogDirSource;
  /** 마지막 `kind:"digest"` watermark 의 timestamp. `/recall` 증분 정리 기준점. */
  lastDigestAt?: string;
  projectKey: string;
}
```
4. `WorklogOptions`: `wikiDir?`/`wikiDirSource?` 삭제.
5. 클래스: `wikiDir`/`wikiDirSource` 필드 + 생성자 할당 삭제, `getWikiDir()` 삭제.
6. `status()` — wiki 제거, curate→digest:
```ts
const last = all[all.length - 1];
const lastDigest = [...all].reverse().find((e) => e.kind === 'digest');
return {
  path: this.file, exists: true, totalEntries: all.length, sizeBytes,
  projectKey: this.projectKey, dirSource: this.dirSource,
  lastEntryAt: last?.timestamp,
  ...(lastDigest ? { lastDigestAt: lastDigest.timestamp } : {}),
};
```
`exists:false` 조기 반환도 wiki 스프레드 제거:
```ts
if (!existsSync(this.file)) {
  return { path: this.file, exists: false, totalEntries: 0, sizeBytes: 0,
    projectKey: this.projectKey, dirSource: this.dirSource };
}
```
7. `WorklogEnvOptions`: `wikiDir?` 삭제 → `{ dir?: string }`.
8. `createWorklogFromEnv`: wiki 로직 + `ROCKY_WORKLOG_WIKI_DIR` 삭제:
```ts
export function createWorklogFromEnv(config: WorklogEnvOptions = {}): Worklog {
  const envDir = firstNonEmpty(process.env.ROCKY_WORKLOG_DIR);
  const configDir = firstNonEmpty(config.dir);
  const baseDir = envDir ?? configDir;
  const dirSource: WorklogDirSource = envDir ? 'env' : configDir ? 'config' : 'default';
  return new Worklog({ baseDir, dirSource });
}
```
`expandTilde` 는 유지.

- [ ] **Step 4: worklog-handlers.ts status doc 갱신**

```ts
/** 도구 핸들러: worklog 메타 + 마지막 digest watermark(lastDigestAt). */
export function handleWorklogStatus(worklog: Worklog): Promise<WorklogStatus> {
  return worklog.status();
}
```

- [ ] **Step 5: 테스트 + typecheck**

Run: `bun test src/core/worklog.test.ts && bun run typecheck`
Expected: PASS.

- [ ] **Step 6: 커밋**

```bash
git add src/core/worklog.ts src/core/worklog-handlers.ts src/core/worklog.test.ts
git commit -m "refactor(worklog): wikiDir 제거 + watermark를 kind:\"digest\"로 전환

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: rocky-config — worklog 키 + 새 필드 (wikiDir 제거)

**Files:**
- Modify: `src/core/rocky-config.ts` (WorklogConfig, RockyConfig.worklog, validateWorklog, top-level 가드, mergeConfigs)
- Modify: `rocky.schema.json` (lockstep)
- Test: `src/core/rocky-config.test.ts`

**Interfaces:**
- Produces: `WorklogConfig = { dir?, autoCapture?, captureMaxChars?, digestThreshold? }`; `RockyConfig.worklog?: WorklogConfig` (구 `journal` 키 대체).

> Task 0 에서 `JournalConfig`→`WorklogConfig` 타입명은 개명 완료. 이 태스크는 **config 키 `journal`→`worklog`** + **필드 교체** + **top-level 키 가드**.

- [ ] **Step 1: 실패 테스트 추가**

```ts
it('accepts worklog.autoCapture / captureMaxChars / digestThreshold', () => {
  const cfg = validateConfig(
    { worklog: { dir: '/tmp/w', autoCapture: false, captureMaxChars: 500, digestThreshold: 10 } }, 'test');
  expect(cfg.worklog?.autoCapture).toBe(false);
  expect(cfg.worklog?.captureMaxChars).toBe(500);
  expect(cfg.worklog?.digestThreshold).toBe(10);
});
it('rejects worklog.wikiDir (removed key)', () => {
  expect(() => validateConfig({ worklog: { wikiDir: '/x' } }, 'test')).toThrow(/unknown key "wikiDir"/);
});
it('rejects legacy top-level journal key', () => {
  expect(() => validateConfig({ journal: { dir: '/j' } }, 'test')).toThrow(/unknown top-level key "journal"/);
});
it('rejects non-boolean worklog.autoCapture', () => {
  expect(() => validateConfig({ worklog: { autoCapture: 'yes' } }, 'test')).toThrow(/autoCapture must be a boolean/);
});
```
기존 `journal`/`wikiDir` 유효값 테스트가 있으면 삭제/수정.

- [ ] **Step 2: 테스트 실행 → 실패 확인**

Run: `bun test src/core/rocky-config.test.ts`
Expected: FAIL.

- [ ] **Step 3: rocky-config.ts 수정**

1. `WorklogConfig`:
```ts
/** `worklog_*` 도구 + Stop hook 자동 기록 + `/recall` 다이제스트 설정. */
export interface WorklogConfig {
  dir?: string;
  /** Stop hook 자동 워크로그 기록 on/off. 기본 true. env `ROCKY_WORKLOG_AUTO_CAPTURE` 우선. */
  autoCapture?: boolean;
  /** turn 엔트리 req/did 최대 글자 수. 기본 800. */
  captureMaxChars?: number;
  /** `/recall` Haiku↔Sonnet 임계(신규 엔트리 수). 기본 40. */
  digestThreshold?: number;
}
```
2. `RockyConfig`: `journal?` → `worklog?: WorklogConfig`.
3. top-level 가드 (validateConfig 초입):
```ts
const ALLOWED_TOP_KEYS = new Set(['$schema', 'openapi', 'seo', 'worklog']);
for (const key of Object.keys(config)) {
  if (!ALLOWED_TOP_KEYS.has(key)) throw new Error(`${source}: unknown top-level key "${key}"`);
}
```
`if (config.journal !== undefined) validateJournal(...)` → `if (config.worklog !== undefined) validateWorklog(config.worklog, source)`.
4. `validateJournal` → `validateWorklog`:
```ts
const ALLOWED_WORKLOG_KEYS = new Set(['dir', 'autoCapture', 'captureMaxChars', 'digestThreshold']);
function validateWorklog(worklog: unknown, source: string): void {
  if (worklog === null || typeof worklog !== 'object' || Array.isArray(worklog)) {
    throw new Error(`${source}: worklog must be an object`);
  }
  const obj = worklog as Record<string, unknown>;
  for (const key of Object.keys(obj)) {
    if (!ALLOWED_WORKLOG_KEYS.has(key)) throw new Error(`${source}: worklog: unknown key "${key}"`);
  }
  if (obj.dir !== undefined && (typeof obj.dir !== 'string' || obj.dir.trim().length === 0)) {
    throw new Error(`${source}: worklog.dir must be a non-empty string`);
  }
  if (obj.autoCapture !== undefined && typeof obj.autoCapture !== 'boolean') {
    throw new Error(`${source}: worklog.autoCapture must be a boolean`);
  }
  for (const key of ['captureMaxChars', 'digestThreshold'] as const) {
    const v = obj[key];
    if (v !== undefined && (typeof v !== 'number' || !Number.isInteger(v) || v < 1)) {
      throw new Error(`${source}: worklog.${key} must be a positive integer`);
    }
  }
}
```
5. `mergeConfigs`: `project.journal` → `project.worklog`:
```ts
if (project.worklog) out.worklog = { ...out.worklog, ...project.worklog };
```

- [ ] **Step 4: rocky.schema.json lockstep**

top-level `properties` 의 `journal` → `worklog`, `wikiDir` 제거:
```json
"worklog": {
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "dir": { "type": "string", "description": "worklog JSONL 저장 디렉터리" },
    "autoCapture": { "type": "boolean", "description": "Stop hook 자동 기록 on/off (기본 true)" },
    "captureMaxChars": { "type": "integer", "minimum": 1, "description": "turn req/did 최대 글자 (기본 800)" },
    "digestThreshold": { "type": "integer", "minimum": 1, "description": "/recall Haiku↔Sonnet 임계 (기본 40)" }
  }
}
```

- [ ] **Step 5: 테스트 + typecheck**

Run: `bun test src/core/rocky-config.test.ts && bun run typecheck`
Expected: PASS. (index.ts 가 `toolkitConfig.journal` 참조 시 → `toolkitConfig.worklog` 로. Task 0 에서 바꿨으면 no-op.)

- [ ] **Step 6: 커밋**

```bash
git add src/core/rocky-config.ts src/core/rocky-config.test.ts rocky.schema.json
git commit -m "feat(config): worklog 키 + autoCapture/captureMaxChars/digestThreshold + top-level 가드, wikiDir 제거

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: 트랜스크립트 추출 순수 함수 (`extractTurn` / `buildTurnContent`)

**Files:**
- Create: `src/hooks/transcript.ts`
- Test: `src/hooks/transcript.test.ts`

**Interfaces:**
- Produces: `interface TurnParts { req: string; tools: string[]; did: string }`, `extractTurn(transcriptText: string): TurnParts | null`, `buildTurnContent(parts: TurnParts, maxChars: number): string`.

- [ ] **Step 1: 실패 테스트 작성**

`src/hooks/transcript.test.ts`:
```ts
import { describe, expect, it } from 'bun:test';
import { buildTurnContent, extractTurn } from './transcript';

const TRANSCRIPT = [
  { type: 'user', message: { role: 'user', content: [{ type: 'text', text: '엔드포인트 검색해줘' }] } },
  { type: 'assistant', message: { role: 'assistant', content: [
    { type: 'text', text: '검색합니다' },
    { type: 'tool_use', name: 'openapi_search', input: {} },
    { type: 'tool_use', name: 'openapi_search', input: {} },
  ] } },
  { type: 'user', message: { role: 'user', content: [{ type: 'tool_result', content: 'ok' }] } },
  { type: 'assistant', message: { role: 'assistant', content: [{ type: 'text', text: '3개 찾았습니다' }] } },
].map((e) => JSON.stringify(e)).join('\n');

describe('extractTurn', () => {
  it('pulls last real user prompt, deduped tool names w/ count, final assistant text', () => {
    const parts = extractTurn(TRANSCRIPT);
    expect(parts?.req).toBe('엔드포인트 검색해줘');
    expect(parts?.tools).toEqual(['openapi_search(×2)']);
    expect(parts?.did).toBe('3개 찾았습니다');
  });
  it('ignores tool_result-only user messages as the prompt boundary', () => {
    expect(extractTurn(TRANSCRIPT)?.req).toBe('엔드포인트 검색해줘');
  });
  it('returns null when there is no user prompt', () => {
    const onlyAssistant = JSON.stringify({ type: 'assistant', message: { role: 'assistant', content: [{ type: 'text', text: 'hi' }] } });
    expect(extractTurn(onlyAssistant)).toBeNull();
  });
  it('skips malformed lines gracefully', () => {
    expect(extractTurn(`not json\n${TRANSCRIPT}\n{"partial":`)?.req).toBe('엔드포인트 검색해줘');
  });
});

describe('buildTurnContent', () => {
  it('collapses whitespace and joins parts', () => {
    expect(buildTurnContent({ req: 'a  b\n\nc', tools: ['x', 'y'], did: 'done' }, 800))
      .toBe('req: a b c | tools: x, y | did: done');
  });
  it('truncates each field to maxChars with an ellipsis', () => {
    expect(buildTurnContent({ req: 'abcdefgh', tools: [], did: '' }, 4))
      .toBe('req: abcd… | tools: (none) | did: (none)');
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
Expected: FAIL — 모듈 없음.

- [ ] **Step 3: transcript.ts 구현**

`src/hooks/transcript.ts`:
```ts
/**
 * Claude Code 트랜스크립트(JSONL) 에서 "마지막 한 턴" 을 기계적으로 추출한다.
 * LLM 없이 동작 — Stop hook 이 워크로그 한 줄을 만들 재료(req/tools/did)만 뽑는다.
 */

export interface TurnParts {
  req: string;
  tools: string[];
  did: string;
}

interface RawBlock { type?: string; text?: string; name?: string }
interface RawMessage { role?: string; content?: string | RawBlock[] }
interface RawEntry { message?: RawMessage }

function textOf(content: string | RawBlock[] | undefined): string {
  if (typeof content === 'string') return content.trim();
  if (!Array.isArray(content)) return '';
  return content
    .filter((b): b is RawBlock => !!b && b.type === 'text' && typeof b.text === 'string')
    .map((b) => b.text as string)
    .join('\n')
    .trim();
}

function isRealUserPrompt(msg: RawMessage): boolean {
  if (msg.role !== 'user') return false;
  if (typeof msg.content === 'string') return msg.content.trim().length > 0;
  if (!Array.isArray(msg.content)) return false;
  return msg.content.some(
    (b) => !!b && b.type === 'text' && typeof b.text === 'string' && b.text.trim().length > 0,
  );
}

export function extractTurn(transcriptText: string): TurnParts | null {
  const entries: RawEntry[] = [];
  for (const line of transcriptText.split('\n')) {
    const t = line.trim();
    if (!t) continue;
    try {
      const parsed = JSON.parse(t);
      if (parsed && typeof parsed === 'object') entries.push(parsed as RawEntry);
    } catch {
      // 손상/부분 라인 skip
    }
  }
  let startIdx = -1;
  for (let i = entries.length - 1; i >= 0; i--) {
    const msg = entries[i]?.message;
    if (msg && isRealUserPrompt(msg)) { startIdx = i; break; }
  }
  if (startIdx < 0) return null;
  const req = textOf(entries[startIdx]?.message?.content);
  const toolCounts = new Map<string, number>();
  let did = '';
  for (let i = startIdx + 1; i < entries.length; i++) {
    const msg = entries[i]?.message;
    if (!msg || msg.role !== 'assistant') continue;
    if (Array.isArray(msg.content)) {
      for (const b of msg.content) {
        if (b && b.type === 'tool_use' && typeof b.name === 'string') {
          toolCounts.set(b.name, (toolCounts.get(b.name) ?? 0) + 1);
        }
      }
    }
    const txt = textOf(msg.content);
    if (txt) did = txt;
  }
  const tools = [...toolCounts.entries()].map(([name, n]) => (n > 1 ? `${name}(×${n})` : name));
  if (!req && !did && tools.length === 0) return null;
  return { req, tools, did };
}

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

- [ ] **Step 4: 테스트 통과 확인**

Run: `bun test src/hooks/transcript.test.ts`
Expected: PASS (6 tests).

- [ ] **Step 5: 커밋**

```bash
git add src/hooks/transcript.ts src/hooks/transcript.test.ts
git commit -m "feat(hooks): 트랜스크립트 턴 추출 순수 함수 extractTurn/buildTurnContent

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: Stop hook — gating + `log-turn.ts` + `hooks/hooks.json`

**Files:**
- Create: `src/hooks/log-turn.ts`, `hooks/hooks.json`
- Test: `src/hooks/log-turn.test.ts`

**Interfaces:**
- Consumes: `extractTurn`/`buildTurnContent`(Task 3), `createWorklogFromEnv`/`WorklogEnvOptions`(Task 1), `loadConfig`+`WorklogConfig`(Task 2).
- Produces: `shouldCapture(env: NodeJS.ProcessEnv, config: WorklogConfig | undefined): boolean`.

- [ ] **Step 1: gating 실패 테스트**

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
  it('env=0/false/off/no disables (wins over config true)', () => {
    for (const v of ['0', 'false', 'off', 'no'])
      expect(shouldCapture({ ROCKY_WORKLOG_AUTO_CAPTURE: v }, { autoCapture: true })).toBe(false);
  });
  it('env other value enables (wins over config false)', () => {
    expect(shouldCapture({ ROCKY_WORKLOG_AUTO_CAPTURE: '1' }, { autoCapture: false })).toBe(true);
  });
});
```

- [ ] **Step 2: 테스트 실행 → 실패 확인**

Run: `bun test src/hooks/log-turn.test.ts`
Expected: FAIL — 모듈/함수 없음.

- [ ] **Step 3: log-turn.ts 구현**

`src/hooks/log-turn.ts`:
```ts
import { existsSync } from 'node:fs';
import { readFile } from 'node:fs/promises';
import { createWorklogFromEnv } from '../core/worklog';
import type { WorklogConfig } from '../core/rocky-config';
import { loadConfig } from '../core/rocky-config';
import { buildTurnContent, extractTurn } from './transcript';

/**
 * Stop hook: 매 응답 종료 시 트랜스크립트에서 이번 턴을 뽑아 `kind:"turn"` 한 줄을
 * append 한다. 결정론적(LLM 0). 어떤 실패도 턴을 막지 않도록 항상 exit 0.
 */

/** env(우선) → config(기본 true). `0/false/off/no` 만 비활성. */
export function shouldCapture(env: NodeJS.ProcessEnv, config: WorklogConfig | undefined): boolean {
  const raw = env.ROCKY_WORKLOG_AUTO_CAPTURE;
  if (typeof raw === 'string' && raw.trim().length > 0) {
    const v = raw.trim().toLowerCase();
    return !(v === '0' || v === 'false' || v === 'off' || v === 'no');
  }
  return config?.autoCapture !== false;
}

interface StopHookInput { transcript_path?: string; cwd?: string }

async function readStdin(): Promise<string> {
  let raw = '';
  for await (const chunk of process.stdin) raw += chunk;
  return raw;
}

async function run(): Promise<void> {
  const raw = await readStdin();
  let input: StopHookInput;
  try {
    input = JSON.parse(raw) as StopHookInput;
  } catch {
    return;
  }
  const projectRoot = input.cwd ?? process.cwd();
  const { config } = await loadConfig({ projectRoot });
  if (!shouldCapture(process.env, config.worklog)) return;
  const path = input.transcript_path;
  if (!path || !existsSync(path)) return;
  const parts = extractTurn(await readFile(path, 'utf8'));
  if (!parts) return;
  const maxChars = config.worklog?.captureMaxChars ?? 800;
  const content = buildTurnContent(parts, maxChars);
  await createWorklogFromEnv(config.worklog).append({ content, kind: 'turn', tags: ['turn'] });
}

if (import.meta.main) {
  run()
    .catch(() => {
      // 절대 턴을 막지 않는다 — 모든 오류 삼킴
    })
    .finally(() => process.exit(0));
}
```

- [ ] **Step 4: 테스트 통과 확인**

Run: `bun test src/hooks/log-turn.test.ts`
Expected: PASS (4 tests).

- [ ] **Step 5: hooks/hooks.json 생성**

```json
{
  "hooks": {
    "Stop": [
      {
        "hooks": [
          { "type": "command", "command": "bun run \"${CLAUDE_PLUGIN_ROOT}/src/hooks/log-turn.ts\"" }
        ]
      }
    ]
  }
}
```

- [ ] **Step 6: 엔드투엔드 수동 검증**

```bash
ROCKY_WORKLOG_DIR=/tmp/rocky-hooktest \
  bun run src/hooks/log-turn.ts <<'EOF'
{"transcript_path":"/tmp/does-not-exist.jsonl","cwd":"/tmp"}
EOF
echo "exit: $?"
```
Expected: `exit: 0`, transcript 부재라 no-op.

- [ ] **Step 7: 커밋**

```bash
git add src/hooks/log-turn.ts src/hooks/log-turn.test.ts hooks/hooks.json
git commit -m "feat(hooks): Stop hook log-turn — 매 턴 kind:\"turn\" 자동 기록 (autoCapture 기본 ON)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: `/recall` 커맨드 (curate.md 삭제 → recall.md 생성)

**Files:**
- Delete: `commands/curate.md`
- Create: `commands/recall.md`

**Interfaces:** 없음 (프로세스/산문). `worklog_*` 도구 + `Task`.

- [ ] **Step 1: curate.md 삭제 + recall.md 작성**

```bash
git rm commands/curate.md
```

`commands/recall.md`:
```markdown
---
description: 워크로그(worklog 의 kind:"turn" + 수동 decision/blocker)를 읽어 앵커 히스토리 다이제스트로 정리한다 — 마지막 digest 이후 항목만 증분 요약해 kind:"digest" 엔트리로 남기고, 각 앵커는 원본 엔트리 id 로 드릴다운 가능하게 한다. 배치 크기에 따라 Haiku/Sonnet 서브에이전트를 고른다.
argument-hint: "[집중할 주제/힌트] (생략 가능)"
allowed-tools: mcp__plugin_rocky_rocky__worklog_status, mcp__plugin_rocky_rocky__worklog_read, mcp__plugin_rocky_rocky__worklog_search, mcp__plugin_rocky_rocky__worklog_append, Task
---

# recall — 워크로그 → 앵커 히스토리 다이제스트

rocky 의 worklog 는 **기록(logbook)** 레이어다 — `Stop` hook 이 매 턴 `kind:"turn"` 워크로그를
자동으로 쌓는다(수동 `decision`/`blocker`/`answer`/`note` 공존). 이 커맨드는 그 워크로그를 읽어
**히스토리 다이제스트**로 정리한다 — 별도 위키 문서가 아니라, 워크로그로 **찾아 들어갈 수 있는
앵커**(각 항목이 원본 엔트리 id 를 가리킴)로. `$ARGUMENTS` 는 집중할 주제 힌트(있으면).

## 원칙

1. **rocky 는 기록만, 정리는 이 커맨드가.** 요약(어떤 순간을 앵커로 남길지)은 호스트 LLM /
   서브에이전트가 한다.
2. **증분.** 마지막 `kind:"digest"` watermark 이후 항목만 처리한다.
3. **worklog 는 불변.** 기존 줄을 지우거나 편집하지 않는다. 다이제스트도 `worklog_append` 한 줄.
4. **앵커는 드릴다운용.** 각 앵커는 요약 + 원본 엔트리 `id`(+timestamp) 를 담아, 읽는 쪽이
   필요하면 `worklog_read` 로 원문을 찾아가게 한다.
5. **네이티브 메모리와 별개.** 이 다이제스트는 worklog 안에 산다. Claude Code 글로벌 메모리를
   건드리지 않는다.

## 절차

### 1. 상태 확인 → watermark

```
worklog_status
```
- `totalEntries` 가 0 이면 "정리할 워크로그 없음" 후 종료.
- 마지막 watermark: `worklog_read { kind: "digest", limit: 1 }` → 있으면 그 `timestamp`, 없으면 첫 실행.

### 2. 새 워크로그 수집 (증분)

- watermark 이후만: `worklog_read { since: <watermark>, limit: 500 }`. 없으면 전체를 최근부터.
- `$ARGUMENTS` 힌트가 있으면 `worklog_search` 로 보강.
- `kind:"digest"` 항목은 제외. 새 항목 수 `n` 을 센다. `n == 0` → no-op 종료 (watermark 안 남김).

### 3. 적응적 모델로 서브에이전트 dispatch

- `worklog.digestThreshold`(기본 40) 기준: `n <= 40` → **Haiku**, `n > 40` → **Sonnet**.
- `Task` 로 서브에이전트를 띄운다 (model 을 위 규칙대로). 수집 항목(각 `id`/`timestamp`/`kind`/
  `content`)을 넘기고 아래 **앵커 다이제스트**를 만들게 한다:
  - raw 나열 금지 — 의미 있는 순간(결정/전환/blocker/사용자 답변)만.
  - 각 앵커 끝에 원본 `id:<id> (<ts>)`.
  - 포맷:
    ```markdown
    ## digest — <n> entries, <first ts> … <last ts>
    - <결정/전환 요약> → id:<entry-id> (<ts>)
    - <blocker 해결> → id:<entry-id> (<ts>)
    ```

### 4. 다이제스트 append (watermark 겸용)

```
worklog_append { kind: "digest", content: "<앵커 다이제스트>", tags: ["digest"] }
```
- 이 엔트리 timestamp 가 다음 `/recall` 의 `since` 기준점.

### 5. 마무리

- 만든 다이제스트(앵커 목록)를 한국어로 보고 — 드릴다운 id 포함. 장문 금지.

## 예외 처리

- `totalEntries == 0` 또는 새 항목 0 → no-op 종료 (watermark 안 남김).
- 서브에이전트 실패 → 다이제스트 append 하지 말고 실패만 알린다 (watermark 오염 방지).
```

- [ ] **Step 2: 게이트 + 커밋**

Run: `bun run check`
Expected: PASS.
```bash
git add commands/recall.md
git commit -m "feat(command): /recall — 워크로그를 앵커 히스토리 다이제스트로 (구 /curate 대체)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 6: surface 테스트 마무리 + 문서/매니페스트 동기화

**Files:**
- Modify: `src/index.ts` (`worklog_status` 설명 — wikiDir/curate 잔존 제거)
- Modify: `src/index.test.ts` (wikiDir 테스트 → dirSource 검증)
- Modify: `.claude-plugin/plugin.json`, `package.json`, `FEATURES.md`, `AGENTS.md`, `README.md`

**Interfaces:** 없음.

- [ ] **Step 1: index.test.ts wikiDir 테스트 교체**

Task 0 에서 `worklog_status` 로 rename 된 wikiDir 테스트를 교체:
```ts
test('worklog_status reports exists=false without wikiDir fields', async () => {
  const client = await connect({ notionCli: absentNotionCli });
  try {
    const result = await client.callTool({ name: 'worklog_status', arguments: {} });
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
`ROCKY_WORKLOG_WIKI_DIR` 세팅 잔재가 있으면 삭제.

- [ ] **Step 2: index.ts worklog_status 설명 갱신**

wikiDir/curate/`ROCKY_WORKLOG_WIKI_DIR` 언급 제거:
```ts
'worklog 메타(파일 경로, 존재 여부, 유효 항목 수 — 손상 라인 skip, 바이트 크기, 마지막 항목 시각) + 마지막 digest watermark(lastDigestAt) + 경로 출처(dirSource)를 조회한다. `/recall` 이 정리 시작 시 이걸로 증분 기준점을 확인한다. remote 호출 없음. 저장 위치는 `worklog.dir`(rocky.json) 또는 `ROCKY_WORKLOG_DIR`(env 우선)로 변경 가능하다.'
```
`worklog_append` 등 다른 설명에 journal/wikiDir 잔존이 있으면 함께 정리.

- [ ] **Step 3: surface 테스트 실행**

Run: `bun test src/index.test.ts`
Expected: PASS — 도구 4개(`worklog_*`), wikiDir 부재, 제거 도메인 누수 없음.

- [ ] **Step 4: plugin.json + package.json**

`.claude-plugin/plugin.json` `description` 을 worklog 개명 + Stop hook 자동 기록 + `/recall` 로 갱신(`/curate`·wiki·journal 문구 제거), `keywords` `"journal"`→`"worklog"`. 버전 `0.9.0` bump + `package.json` `version` lockstep.

- [ ] **Step 5: FEATURES.md / AGENTS.md / README.md**

- `FEATURES.md`(한국어): 도구 표 `journal_*`→`worklog_*`; `Stop` hook 자동 기록(`kind:"turn"`) + env `ROCKY_WORKLOG_DIR`/`ROCKY_WORKLOG_AUTO_CAPTURE` + config `worklog.{autoCapture,captureMaxChars,digestThreshold}` 추가; `/curate`·wikiDir·`ROCKY_JOURNAL_*` 제거; `/recall` 추가.
- `AGENTS.md`(영어): *Project in one line* — journal→worklog 개명 + hooks 자동 기록 + `/recall`, wiki/`/curate` 제거. *Layout* — `hooks/hooks.json`, `src/hooks/log-turn.ts`, `src/hooks/transcript.ts`, `commands/recall.md`, `src/core/worklog.ts`/`worklog-handlers.ts` 반영; `journal.ts`/`curate.md`/wikiDir 제거. *MVP scope* / *Reintroduction* / *Change checklist* 의 journal·wikiDir·curate 서술 갱신 (`JOURNAL_TOOLS`→`WORKLOG_TOOLS`).
- `README.md`: surface 설명 journal→worklog, `/curate`→`/recall`, hooks 자동 기록 추가.

- [ ] **Step 6: 전체 게이트 + 커밋**

Run: `bun run check && bun run typecheck && bun test`
Expected: 전부 PASS.
```bash
git add src/index.ts src/index.test.ts .claude-plugin/plugin.json package.json FEATURES.md AGENTS.md README.md
git commit -m "docs(worklog): worklog 개명 + 자동 기록 + /recall surface 반영, wikiDir/curate 제거

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

- [ ] **Step 7: 로컬 플러그인 왕복 검증 (수동)**

`/reload-plugins` 후:
1. 아무 턴이나 돌린 뒤 `worklog_status` → `exists:true`, `kind:"turn"` 누적 확인.
2. `/recall` 실행 → `kind:"digest"` 앵커 다이제스트 생성 + 보고 확인.
3. `ROCKY_WORKLOG_AUTO_CAPTURE=0` 로 끄고 턴 → 새 turn 안 쌓이는지 확인.

---

## Self-Review

**Spec coverage:**
- journal→worklog 전면 개명 → Task 0(+ 후속 태스크가 worklog 네이밍 유지). ✅
- Layer 1 (Stop hook 자동 turn) → Task 3 + Task 4. ✅
- Layer 2 (`/recall` 적응적 Haiku/Sonnet, kind:"digest", watermark) → Task 5. ✅
- 위키/wikiDir 제거 → Task 1 + Task 2 + Task 6. ✅
- autoCapture 기본 ON + kill switch → Task 2 + Task 4(shouldCapture). ✅
- truncation/tools cap → Task 3. ✅
- 적응적 임계(digestThreshold 40) → Task 2 + Task 5. ✅
- read-back 없음 → 자동 주입 미도입. ✅
- surface 카운트 불변(도구 4개, 이름만 worklog_*) → Task 0 + Task 6. ✅
- 문서/매니페스트 동기화 → Task 6. ✅

**Placeholder scan:** 모든 코드 스텝에 실제 구현/테스트 포함. 모호 표현 없음. ✅

**Type consistency:**
- Task 0 이후 전 태스크가 `Worklog`/`createWorklogFromEnv`/`WorklogConfig`/`worklog_*` 통일. ✅
- `WorklogStatus.lastDigestAt`(Task 1) ↔ `/recall` 은 `worklog_read {kind:"digest"}` 로 watermark 조회(Task 5) — kind 조회라 정합. index.ts 설명도 `lastDigestAt`(Task 6). ✅
- `shouldCapture(env, config?)`(Task 4) ↔ `WorklogConfig.autoCapture`(Task 2) 일치. ✅
- `createWorklogFromEnv(config?: WorklogEnvOptions={dir?})`(Task 1) ↔ log-turn 이 `config.worklog`(WorklogConfig ⊇ dir) 전달(Task 4) — 구조적 호환. ✅
- `extractTurn`/`buildTurnContent`/`TurnParts`(Task 3) ↔ log-turn import(Task 4) 이름 일치. ✅
