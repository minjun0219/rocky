# 로키 소울 (Rocky Souls) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** rocky 플러그인 사용 시 "소울"(말투/성격 + 작업 방식 페르소나)을 프리셋/커스텀으로 고르고, `rocky.json` 에 고정 + `SessionStart` 훅으로 매 세션 자동 주입한다.

**Architecture:** 소울 = markdown 파일(frontmatter + 본문). 활성 소울 이름은 `rocky.json` 의 신규 스칼라 필드 `soul`. 순수 로직은 `src/core/soul.ts`(DI 가능), IO 는 `src/hooks/inject-soul.ts`(얇은 SessionStart 엔트리) — 기존 log-turn/transcript 분리 답습. MCP tool surface 는 불변(소울은 commands/hooks/skills 처럼 Claude Code 전용 표면).

**Tech Stack:** TypeScript (`type: module`), Bun 런타임(빌드 없음), 의존성 0 추가. 테스트는 `bun test` + `mkdtempSync` 격리.

**Spec:** `docs/superpowers/specs/2026-07-15-rocky-souls-design.md`

## Global Constraints

- **의존성 0 추가** — 표준 라이브러리 + Bun 빌트인만. frontmatter 파서도 직접 작성(js-yaml 미사용).
- **ESM 안전** — `__dirname` 금지. `import.meta.dir` / `import.meta.url` 사용.
- **Import 규칙** — 상대경로, 확장자(`.ts`/`.js`) 미부착.
- **Fail-open** — 훅은 어떤 오류에도 세션 시작을 막지 않고 항상 `exit 0`.
- **소울은 AGENTS.md 게이트/안전 규칙 위에 얹히는 레이어** — 주입 텍스트에 우선순위 preamble 필수.
- **MCP tool surface 불변** — `src/index.ts` / `src/index.test.ts` 손대지 않음.
- **rocky.json 변경은 lockstep** — `rocky-config.ts` + `rocky.schema.json` 동시 갱신.
- 게이트: `bun run check` / `bun run typecheck` / `bun test` 모두 통과.

---

### Task 1: `rocky.json` 에 `soul` 필드 추가 (config + schema lockstep)

**Files:**
- Modify: `src/core/rocky-config.ts` (`RockyConfig`, `ALLOWED_TOP_KEYS`, `validateConfig`, `mergeConfigs`, 신규 `validateSoul`)
- Modify: `src/core/rocky-config.test.ts` (soul 검증/머지 테스트 추가)
- Modify: `rocky.schema.json` (`soul` 프로퍼티 추가)

**Interfaces:**
- Consumes: 없음 (기존 로더).
- Produces: `RockyConfig.soul?: string` — 활성 소울 이름. 이후 Task 2/4 가 소비.

- [ ] **Step 1: soul 검증 실패 테스트 작성**

`src/core/rocky-config.test.ts` 의 `describe('validateConfig', ...)` 블록 안에 추가:

```ts
  it('accepts a valid soul name', () => {
    expect(validateConfig({ soul: 'rocky' }, 'test')).toEqual({ soul: 'rocky' });
  });

  it('rejects a soul that is not a string', () => {
    expect(() => validateConfig({ soul: 123 }, 'test')).toThrow(/soul must be a string/);
  });

  it('rejects a soul name with illegal characters', () => {
    expect(() => validateConfig({ soul: 'has space' }, 'test')).toThrow(/soul must match/);
  });
```

그리고 `describe('mergeConfigs', ...)` 블록(없으면 파일 하단에 신규 `describe`)에 추가:

```ts
describe('mergeConfigs soul', () => {
  it('project soul overrides user soul', () => {
    const merged = mergeConfigs({ soul: 'rocky' }, { soul: 'senior' });
    expect(merged.soul).toBe('senior');
  });

  it('keeps user soul when project omits it', () => {
    const merged = mergeConfigs({ soul: 'rocky' }, {});
    expect(merged.soul).toBe('rocky');
  });
});
```

- [ ] **Step 2: 테스트 실패 확인**

Run: `bun test src/core/rocky-config.test.ts`
Expected: FAIL — `validateConfig({ soul: 'rocky' })` 가 `unknown top-level key "soul"` 로 throw (아직 허용 키 아님).

- [ ] **Step 3: `rocky-config.ts` 에 soul 추가**

`RockyConfig` 인터페이스에 필드 추가:

```ts
export interface RockyConfig {
  $schema?: string;
  /** 활성 소울(페르소나) 이름. SessionStart 훅이 이 이름으로 소울 파일을 찾아 주입한다. */
  soul?: string;
  openapi?: {
    registry?: OpenapiRegistry;
  };
  seo?: SeoConfig;
  worklog?: WorklogConfig;
}
```

`ALLOWED_TOP_KEYS` 에 `'soul'` 추가:

```ts
const ALLOWED_TOP_KEYS = new Set(['$schema', 'soul', 'openapi', 'seo', 'worklog']);
```

`validateConfig` 안, `if (config.worklog !== undefined) { ... }` 바로 다음에 추가:

```ts
  if (config.soul !== undefined) {
    validateSoul(config.soul, source);
  }
```

파일에 신규 함수 추가 (기존 `ID_PATTERN` 재사용):

```ts
/**
 * `soul` 필드 검증. 활성 소울 이름 — 파일명으로 쓰이므로 `ID_PATTERN`
 * (`[a-zA-Z0-9_-]+`) 만 허용한다 (경로 이스케이프 / 콜론 방지).
 */
function validateSoul(soul: unknown, source: string): void {
  if (typeof soul !== 'string') {
    throw new Error(`${source}: soul must be a string`);
  }
  if (!ID_PATTERN.test(soul)) {
    throw new Error(`${source}: soul must match ${ID_PATTERN} (alphanumeric, "_" or "-" only) — got "${soul}"`);
  }
}
```

`mergeConfigs` 안, `out` 을 반환하기 전에 스칼라 override 추가 (project 우선):

```ts
  // soul 은 스칼라 — project 가 있으면 user 를 덮어쓴다.
  if (project.soul !== undefined) {
    out.soul = project.soul;
  }
```

- [ ] **Step 4: 테스트 통과 확인**

Run: `bun test src/core/rocky-config.test.ts`
Expected: PASS (신규 3 + 2 테스트 포함 전부 통과).

- [ ] **Step 5: `rocky.schema.json` 에 `soul` 추가 (lockstep)**

`properties` 안 `$schema` 다음에 추가:

```json
    "soul": {
      "type": "string",
      "pattern": "^[a-zA-Z0-9_-]+$",
      "description": "활성 소울(페르소나) 이름. SessionStart 훅이 souls/<name>.md (번들) 또는 ~/.config/rocky/souls/<name>.md (커스텀) 를 찾아 세션 컨텍스트에 주입한다. 미설정 시 주입 없음(vanilla)."
    },
```

- [ ] **Step 6: 커밋**

```bash
git add src/core/rocky-config.ts src/core/rocky-config.test.ts rocky.schema.json
git commit -m "feat(soul): rocky.json 에 활성 소울 필드(soul) 추가

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: 소울 코어 로직 `src/core/soul.ts`

**Files:**
- Create: `src/core/soul.ts`
- Create: `src/core/soul.test.ts`

**Interfaces:**
- Consumes: `RockyConfig` (Task 1) 의 `soul` 필드 — `resolveSoulName` 이 읽음.
- Produces:
  - `interface SoulSummary { name: string; description: string; source: 'preset' | 'custom'; path: string }`
  - `interface Soul { name: string; description: string; body: string; source: 'preset' | 'custom'; path: string }`
  - `parseFrontmatter(raw: string): { name?: string; description?: string; body: string }`
  - `resolveDefaultSoulDirs(): { presetDir: string; customDir: string }`
  - `resolveSoulName(config: { soul?: string }): string | undefined`
  - `listSouls(dirs?: { presetDir: string; customDir: string }): SoulSummary[]`
  - `readSoul(name: string, dirs?: { presetDir: string; customDir: string }): Soul | null`
  - `buildSoulContext(soul: Soul): string`

- [ ] **Step 1: 실패 테스트 작성**

`src/core/soul.test.ts`:

```ts
import { describe, it, expect, beforeEach } from 'bun:test';
import { mkdtempSync, mkdirSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import {
  parseFrontmatter,
  resolveSoulName,
  listSouls,
  readSoul,
  buildSoulContext,
} from './soul';

let presetDir: string;
let customDir: string;

const soulFile = (name: string, description: string, body: string) =>
  `---\nname: ${name}\ndescription: ${description}\n---\n\n${body}\n`;

beforeEach(() => {
  const root = mkdtempSync(join(tmpdir(), 'rocky-soul-'));
  presetDir = join(root, 'presets');
  customDir = join(root, 'custom');
  mkdirSync(presetDir, { recursive: true });
  mkdirSync(customDir, { recursive: true });
  writeFileSync(join(presetDir, 'rocky.md'), soulFile('rocky', '헤일메리 로키', '따뜻한 동료 본문'));
  writeFileSync(join(presetDir, 'terse.md'), soulFile('terse', '간결', '답부터 본문'));
});

const dirs = () => ({ presetDir, customDir });

describe('parseFrontmatter', () => {
  it('extracts name/description and body', () => {
    const parsed = parseFrontmatter('---\nname: x\ndescription: y\n---\n\nbody text\n');
    expect(parsed.name).toBe('x');
    expect(parsed.description).toBe('y');
    expect(parsed.body.trim()).toBe('body text');
  });

  it('returns whole input as body when no frontmatter', () => {
    const parsed = parseFrontmatter('just body\n');
    expect(parsed.name).toBeUndefined();
    expect(parsed.body.trim()).toBe('just body');
  });
});

describe('resolveSoulName', () => {
  it('returns the configured soul', () => {
    expect(resolveSoulName({ soul: 'rocky' })).toBe('rocky');
  });
  it('returns undefined when unset', () => {
    expect(resolveSoulName({})).toBeUndefined();
  });
});

describe('listSouls', () => {
  it('lists preset souls with source tag', () => {
    const souls = listSouls(dirs());
    const rocky = souls.find((s) => s.name === 'rocky');
    expect(rocky?.source).toBe('preset');
    expect(rocky?.description).toBe('헤일메리 로키');
  });

  it('custom overrides a preset of the same name', () => {
    writeFileSync(join(customDir, 'rocky.md'), soulFile('rocky', '내 로키', '커스텀 본문'));
    const souls = listSouls(dirs());
    const rocky = souls.filter((s) => s.name === 'rocky');
    expect(rocky).toHaveLength(1);
    expect(rocky[0]!.source).toBe('custom');
    expect(rocky[0]!.description).toBe('내 로키');
  });
});

describe('readSoul', () => {
  it('reads a preset soul body', () => {
    const soul = readSoul('rocky', dirs());
    expect(soul?.body.trim()).toBe('따뜻한 동료 본문');
    expect(soul?.source).toBe('preset');
  });

  it('prefers a custom soul over a preset', () => {
    writeFileSync(join(customDir, 'rocky.md'), soulFile('rocky', '내 로키', '커스텀 본문'));
    const soul = readSoul('rocky', dirs());
    expect(soul?.body.trim()).toBe('커스텀 본문');
    expect(soul?.source).toBe('custom');
  });

  it('returns null for an unknown soul', () => {
    expect(readSoul('nope', dirs())).toBeNull();
  });
});

describe('buildSoulContext', () => {
  it('wraps body with a precedence preamble', () => {
    const soul = readSoul('rocky', dirs())!;
    const ctx = buildSoulContext(soul);
    expect(ctx).toContain('따뜻한 동료 본문');
    expect(ctx).toContain('AGENTS.md');
    expect(ctx).toContain('rocky');
  });
});
```

- [ ] **Step 2: 테스트 실패 확인**

Run: `bun test src/core/soul.test.ts`
Expected: FAIL — `Cannot find module './soul'`.

- [ ] **Step 3: `src/core/soul.ts` 구현**

```ts
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';

/**
 * 소울(페르소나) 코어 로직 — 순수/DI. IO 엔트리(`src/hooks/inject-soul.ts`)와
 * 슬래시 커맨드가 이 모듈을 소비한다.
 *
 * 소울 = markdown 파일. frontmatter(`name` / `description`) + 본문(페르소나 지침).
 * 프리셋은 번들 `souls/`, 커스텀은 `~/.config/rocky/souls/`. 같은 이름이면 커스텀이 이긴다.
 *
 * 소울은 AGENTS.md 의 게이트/안전 규칙 위에 얹히는 "플레이버 + 작업 스타일 레이어" 다.
 * `buildSoulContext` 가 그 우선순위 preamble 을 앞에 붙인다.
 */

/** 소울 디렉터리 쌍. 테스트/커맨드가 임의 경로를 주입할 수 있게 파라미터화. */
export interface SoulDirs {
  /** 번들 프리셋 디렉터리 (`<pluginRoot>/souls`). */
  presetDir: string;
  /** 커스텀 소울 디렉터리 (`~/.config/rocky/souls`). */
  customDir: string;
}

/** 목록 항목 — 본문 없이 메타만. */
export interface SoulSummary {
  name: string;
  description: string;
  source: 'preset' | 'custom';
  path: string;
}

/** 본문 포함 소울. */
export interface Soul extends SoulSummary {
  body: string;
}

/**
 * 최소 frontmatter 파서 (의존성 0). 문서가 `---\n...\n---\n` 로 시작하면 그 안에서
 * `name:` / `description:` 라인만 읽고, 나머지를 body 로 돌려준다. frontmatter 가 없으면
 * 입력 전체가 body.
 */
export function parseFrontmatter(raw: string): {
  name?: string;
  description?: string;
  body: string;
} {
  const normalized = raw.replace(/^﻿/, '');
  if (!normalized.startsWith('---')) {
    return { body: normalized };
  }
  // 첫 줄(---) 이후의 닫는 --- 를 찾는다.
  const lines = normalized.split('\n');
  let end = -1;
  for (let i = 1; i < lines.length; i++) {
    if (lines[i]!.trim() === '---') {
      end = i;
      break;
    }
  }
  if (end === -1) {
    return { body: normalized };
  }
  const meta: { name?: string; description?: string } = {};
  for (let i = 1; i < end; i++) {
    const line = lines[i]!;
    const m = line.match(/^\s*(name|description)\s*:\s*(.*)$/);
    if (m) {
      const key = m[1] as 'name' | 'description';
      // 양끝 따옴표 제거 (선택).
      meta[key] = m[2]!.trim().replace(/^["']|["']$/g, '');
    }
  }
  const body = lines.slice(end + 1).join('\n');
  return { ...meta, body };
}

/**
 * 기본 소울 디렉터리. 프리셋은 이 파일 기준 `<pluginRoot>/souls` (src/core → ../../souls),
 * 커스텀은 `~/.config/rocky/souls`. `import.meta.dir` 사용 — `__dirname` 금지.
 */
export function resolveDefaultSoulDirs(): SoulDirs {
  return {
    presetDir: join(import.meta.dir, '..', '..', 'souls'),
    customDir: join(homedir(), '.config', 'rocky', 'souls'),
  };
}

/** config 의 활성 소울 이름. 미설정이면 undefined. */
export function resolveSoulName(config: { soul?: string }): string | undefined {
  const name = config.soul?.trim();
  return name && name.length > 0 ? name : undefined;
}

/** 한 디렉터리의 `*.md` 를 요약으로 읽는다. 없거나 못 읽으면 빈 배열. */
function listDir(dir: string, source: 'preset' | 'custom'): SoulSummary[] {
  if (!existsSync(dir)) {
    return [];
  }
  let names: string[];
  try {
    names = readdirSync(dir).filter((f) => f.endsWith('.md'));
  } catch {
    return [];
  }
  const out: SoulSummary[] = [];
  for (const file of names) {
    const path = join(dir, file);
    const fallback = file.replace(/\.md$/, '');
    try {
      const parsed = parseFrontmatter(readFileSync(path, 'utf8'));
      out.push({
        name: parsed.name?.trim() || fallback,
        description: parsed.description?.trim() ?? '',
        source,
        path,
      });
    } catch {
      // 못 읽는 파일은 조용히 건너뛴다.
    }
  }
  return out;
}

/**
 * 프리셋 + 커스텀 소울 머지. 이름이 겹치면 커스텀이 프리셋을 덮어쓴다.
 * name 오름차순 정렬.
 */
export function listSouls(dirs: SoulDirs = resolveDefaultSoulDirs()): SoulSummary[] {
  const byName = new Map<string, SoulSummary>();
  for (const s of listDir(dirs.presetDir, 'preset')) {
    byName.set(s.name, s);
  }
  for (const s of listDir(dirs.customDir, 'custom')) {
    byName.set(s.name, s); // 커스텀이 프리셋을 덮어씀
  }
  return [...byName.values()].sort((a, b) => a.name.localeCompare(b.name));
}

/** 이름으로 한 소울을 읽는다 (커스텀 우선). 없으면 null. */
export function readSoul(name: string, dirs: SoulDirs = resolveDefaultSoulDirs()): Soul | null {
  for (const [dir, source] of [
    [dirs.customDir, 'custom'],
    [dirs.presetDir, 'preset'],
  ] as const) {
    const path = join(dir, `${name}.md`);
    if (!existsSync(path)) {
      continue;
    }
    try {
      const parsed = parseFrontmatter(readFileSync(path, 'utf8'));
      return {
        name: parsed.name?.trim() || name,
        description: parsed.description?.trim() ?? '',
        body: parsed.body,
        source,
        path,
      };
    } catch {
      return null;
    }
  }
  return null;
}

/**
 * 소울 본문을 세션 주입용 컨텍스트로 감싼다. 앞에 우선순위 preamble 을 붙여, 페르소나가
 * AGENTS.md 의 게이트/안전 규칙과 충돌하면 항상 후자가 이기도록 명시한다.
 */
export function buildSoulContext(soul: Soul): string {
  return [
    `# rocky soul: ${soul.name}`,
    '',
    '아래는 이 세션에 선택된 rocky 소울(페르소나)이다. 말투/성격 + 작업 방식의 레이어일 뿐,',
    'AGENTS.md / CLAUDE.md 의 게이트·검증·안전 규칙을 절대 덮어쓰지 않는다 — 충돌 시 그 규칙이 이긴다.',
    '',
    soul.body.trim(),
  ].join('\n');
}
```

- [ ] **Step 4: 테스트 통과 확인**

Run: `bun test src/core/soul.test.ts`
Expected: PASS (전부).

- [ ] **Step 5: 커밋**

```bash
git add src/core/soul.ts src/core/soul.test.ts
git commit -m "feat(soul): 소울 코어 로직(list/read/resolve/buildContext + frontmatter)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: 프리셋 소울 3종 (`souls/*.md`)

**Files:**
- Create: `souls/rocky.md`
- Create: `souls/senior.md`
- Create: `souls/terse.md`
- Create: `src/core/soul-presets.test.ts` (번들 프리셋이 실제로 로드/파싱되는지 가드)

**Interfaces:**
- Consumes: `listSouls` / `readSoul` / `resolveDefaultSoulDirs` (Task 2).
- Produces: 번들 프리셋 파일 3개 — `resolveDefaultSoulDirs().presetDir` 에서 발견돼야 함.

- [ ] **Step 1: 프리셋 가드 테스트 작성**

`src/core/soul-presets.test.ts`:

```ts
import { describe, it, expect } from 'bun:test';
import { listSouls, readSoul, resolveDefaultSoulDirs } from './soul';

describe('bundled preset souls', () => {
  const dirs = resolveDefaultSoulDirs();

  it('ships rocky / senior / terse presets', () => {
    const names = listSouls(dirs).map((s) => s.name).sort();
    expect(names).toEqual(expect.arrayContaining(['rocky', 'senior', 'terse']));
  });

  for (const name of ['rocky', 'senior', 'terse']) {
    it(`preset ${name} parses with a description and non-empty body`, () => {
      const soul = readSoul(name, dirs);
      expect(soul).not.toBeNull();
      expect(soul!.description.length).toBeGreaterThan(0);
      expect(soul!.body.trim().length).toBeGreaterThan(0);
      expect(soul!.source).toBe('preset');
    });
  }
});
```

- [ ] **Step 2: 테스트 실패 확인**

Run: `bun test src/core/soul-presets.test.ts`
Expected: FAIL — 프리셋 파일이 아직 없어 `listSouls` 가 빈 배열 / `readSoul` null.

- [ ] **Step 3: 프리셋 파일 3개 작성**

`souls/rocky.md`:

```markdown
---
name: rocky
description: 헤일메리 로키 — 따뜻하고 충직한 엔지니어 동료. 게이트 먼저, 완료 주장 전 검증.
---

너는 rocky — 프로젝트 헤일메리의 로키에서 이름을 딴, 사용자의 충직한 엔지니어 동료다.

## 말투 / 성격
- 따뜻하고 협력적이다. 사용자를 "동료"로 대한다.
- 간결한 한국어로 답한다. 군더더기 인사말/사과를 반복하지 않는다.
- 좋은 결과가 나오면 담백하게 인정한다 ("good, good"). 과장하지 않는다.
- 코드 식별자 / 경로 / 명령 / API 경로 / 라이브러리 이름은 영어 원형으로 둔다.

## 작업 방식
- 게이트를 먼저 생각한다 — `bun run check` / `typecheck` / `bun test` 통과를 최종 완료의 조건으로 삼는다.
- "됐다 / 고쳤다 / 통과한다" 는 실제로 실행해 확인한 뒤에만 단언한다. 확인 못 했으면 그렇게 말한다.
- 되돌리기 어려운 작업(삭제/외부 전송/푸시)은 명시적 승인 없이는 하지 않는다.
- 장문 리포트 대신 한 줄 요약 + 필요한 만큼의 불릿.
```

`souls/senior.md`:

```markdown
---
name: senior
description: 진지한 시니어 엔지니어 — 군더더기 없이, 트레이드오프 우선, 근거 있는 반대.
---

너는 진지한 시니어 엔지니어다. 사용자의 결정을 더 좋게 만드는 것이 목표다.

## 말투 / 성격
- 직설적이고 군더더기 없다. 아부하지 않는다.
- 동의할 수 없으면 근거를 들어 반대한다(push back). 다만 결정권은 사용자에게 있음을 존중한다.
- 간결한 한국어. 코드 식별자 / 경로 / 명령은 영어 원형.

## 작업 방식
- 선택지가 있으면 트레이드오프를 먼저 제시하고 추천안을 하나 고른다 — 장황한 나열 금지.
- 이미 정해진 결정을 다시 논쟁하지 않는다. 확립된 사실을 재유도하지 않는다.
- 리스크가 큰 변경은 이유와 대안을 짧게 밝힌 뒤 진행 여부를 확인한다.
- 게이트(check/typecheck/test)를 완료의 기준으로 삼고, 통과를 실행으로 검증한다.
```

`souls/terse.md`:

```markdown
---
name: terse
description: 최소한의 말 — 답부터, 서론 없음.
---

너는 극도로 간결하다.

## 말투 / 성격
- 서론 / 맺음말 / 반복 인사 없이 답부터 낸다.
- 한 번에 이해되면 한 줄로 끝낸다. 불필요한 부연 금지.
- 한국어. 코드 식별자 / 경로 / 명령은 영어 원형.

## 작업 방식
- 요청받은 것만 한다. 묻지 않은 곳으로 범위를 넓히지 않는다.
- 검증이 필요한 주장은 실행으로 확인한 뒤에만 단언한다.
- 게이트(check/typecheck/test)는 여전히 완료의 기준이다 — 이건 생략하지 않는다.
```

- [ ] **Step 4: 테스트 통과 확인**

Run: `bun test src/core/soul-presets.test.ts`
Expected: PASS.

- [ ] **Step 5: 커밋**

```bash
git add souls/rocky.md souls/senior.md souls/terse.md src/core/soul-presets.test.ts
git commit -m "feat(soul): 프리셋 소울 3종(rocky / senior / terse)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: SessionStart 훅 — `src/hooks/inject-soul.ts` + `hooks/hooks.json`

**Files:**
- Create: `src/hooks/inject-soul.ts`
- Modify: `hooks/hooks.json` (SessionStart 엔트리 추가)

**Interfaces:**
- Consumes: `loadConfig` (`src/core/rocky-config.ts`), `resolveSoulName` / `readSoul` / `buildSoulContext` (Task 2).
- Produces: SessionStart stdout JSON `{ hookSpecificOutput: { hookEventName: "SessionStart", additionalContext } }`. 순수 빌더 `buildInjection(context: string | null): string` 를 export 해 테스트.

- [ ] **Step 1: 빌더 실패 테스트 작성**

`src/hooks/inject-soul.test.ts`:

```ts
import { describe, it, expect } from 'bun:test';
import { buildInjection } from './inject-soul';

describe('buildInjection', () => {
  it('emits SessionStart additionalContext when soul context present', () => {
    const out = buildInjection('PERSONA BODY');
    expect(out).not.toBeNull();
    const parsed = JSON.parse(out!);
    expect(parsed.hookSpecificOutput.hookEventName).toBe('SessionStart');
    expect(parsed.hookSpecificOutput.additionalContext).toBe('PERSONA BODY');
  });

  it('returns null when there is no soul context (vanilla)', () => {
    expect(buildInjection(null)).toBeNull();
  });
});
```

- [ ] **Step 2: 테스트 실패 확인**

Run: `bun test src/hooks/inject-soul.test.ts`
Expected: FAIL — `Cannot find module './inject-soul'`.

- [ ] **Step 3: `src/hooks/inject-soul.ts` 구현**

```ts
import { loadConfig } from '../core/rocky-config';
import { buildSoulContext, readSoul, resolveSoulName } from '../core/soul';

/**
 * SessionStart hook: 활성 소울(페르소나)을 세션 컨텍스트에 주입한다.
 *   config.soul → 소울 파일(커스텀 우선, 없으면 번들) → additionalContext.
 * 어떤 실패도 세션 시작을 막지 않도록 항상 exit 0, 문제 시 빈 출력(vanilla).
 */

interface SessionStartInput {
  cwd?: string;
}

/**
 * 주입할 컨텍스트 문자열을 SessionStart stdout JSON 으로 만든다. context 가 null 이면
 * (소울 미설정 / 파일 없음) null 을 돌려준다 — caller 는 아무것도 출력하지 않는다.
 */
export function buildInjection(context: string | null): string | null {
  if (!context) {
    return null;
  }
  return JSON.stringify({
    hookSpecificOutput: {
      hookEventName: 'SessionStart',
      additionalContext: context,
    },
  });
}

async function readStdin(): Promise<string> {
  let raw = '';
  for await (const chunk of process.stdin) {
    raw += chunk;
  }
  return raw;
}

/** cwd 로 config 를 읽어 활성 소울 컨텍스트를 만든다. 없으면 null. */
async function resolveContext(cwd: string): Promise<string | null> {
  const { config } = await loadConfig({ projectRoot: cwd });
  const name = resolveSoulName(config);
  if (!name) {
    return null;
  }
  const soul = readSoul(name);
  if (!soul) {
    process.stderr.write(`[rocky soul] configured soul "${name}" not found — skipping\n`);
    return null;
  }
  return buildSoulContext(soul);
}

async function run(): Promise<void> {
  const raw = await readStdin();
  let input: SessionStartInput = {};
  try {
    input = JSON.parse(raw) as SessionStartInput;
  } catch {
    // stdin 이 비었거나 JSON 이 아니면 cwd fallback.
  }
  const cwd = input.cwd ?? process.cwd();
  const context = await resolveContext(cwd);
  const out = buildInjection(context);
  if (out) {
    process.stdout.write(out);
  }
}

if (import.meta.main) {
  run()
    .catch(() => {
      // 절대 세션 시작을 막지 않는다 — 모든 오류 삼킴
    })
    .finally(() => process.exit(0));
}
```

- [ ] **Step 4: 빌더 테스트 통과 확인**

Run: `bun test src/hooks/inject-soul.test.ts`
Expected: PASS.

- [ ] **Step 5: `hooks/hooks.json` 에 SessionStart 추가**

파일 전체를 다음으로 교체:

```json
{
  "hooks": {
    "SessionStart": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "bun run \"${CLAUDE_PLUGIN_ROOT}/src/hooks/inject-soul.ts\""
          }
        ]
      }
    ],
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

- [ ] **Step 6: 훅 엔트리 수동 스모크 (선택, fail-open 확인)**

Run: `echo '{"cwd":"'"$PWD"'"}' | bun run src/hooks/inject-soul.ts; echo "exit=$?"`
Expected: `soul` 미설정이면 출력 없음 + `exit=0`. (레포 `rocky.json` 없음/soul 없음이면 vanilla.)

- [ ] **Step 7: 커밋**

```bash
git add src/hooks/inject-soul.ts src/hooks/inject-soul.test.ts hooks/hooks.json
git commit -m "feat(soul): SessionStart 훅으로 활성 소울 자동 주입 (fail-open)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: 슬래시 커맨드 `/rocky:soul` (`commands/soul.md`)

**Files:**
- Create: `commands/soul.md`

**Interfaces:**
- Consumes: 없음(순수 지시문). 호스트 LLM 이 파일 read/write + `src/core/soul.ts` 위치를 안내받아 동작.
- Produces: 없음(런타임 코드 아님).

- [ ] **Step 1: `commands/soul.md` 작성**

```markdown
---
description: 로키의 소울(페르소나 — 말투/성격 + 작업 방식)을 고른다. 목록 보기 / 활성 소울 변경(rocky.json 의 soul) / 미리보기 / 커스텀 소울 스캐폴딩. 프리셋은 souls/, 커스텀은 ~/.config/rocky/souls/.
argument-hint: "[list | <name> | show [name] | new <name>] [--project]"
allowed-tools: Read, Write, Edit, Bash
---

# soul — 로키 소울(페르소나) 선택

rocky 의 "소울" 은 말투/성격 + 작업 방식을 담은 페르소나다. 활성 소울은 `rocky.json` 의
`soul` 필드에 고정되고, `SessionStart` 훅이 매 세션 자동 주입한다(다음 세션부터 반영).
소울 파일: 프리셋 `souls/<name>.md` (번들), 커스텀 `~/.config/rocky/souls/<name>.md`.
`$ARGUMENTS` 로 서브커맨드를 받는다.

## 서브커맨드

- **(없음) 또는 `list`** — 사용 가능한 소울(프리셋+커스텀)을 나열하고 현재 활성 소울을 표시한다.
  - 프리셋 목록: `souls/*.md`. 커스텀 목록: `~/.config/rocky/souls/*.md` (있으면). 같은 이름이면 커스텀이 이긴다.
  - 각 파일의 frontmatter `name` / `description` 한 줄로 보여준다.
  - 현재 활성: `~/.config/rocky/rocky.json` 과 (있으면) `./rocky.json` 의 `soul` 필드(프로젝트 우선).

- **`<name>`** — 활성 소울을 `<name>` 으로 바꾼다.
  1. 해당 이름의 소울이 프리셋/커스텀에 실제 있는지 먼저 확인한다. 없으면 목록을 보여주고 멈춘다.
  2. 대상 파일: 기본 `~/.config/rocky/rocky.json`(user), `--project` 면 `./rocky.json`(project).
  3. **쓰기 전 사용자에게 확인**한다. 승인 후, 대상 JSON 을 읽어 `soul` 키만 갱신한다(다른 필드 보존, 파일 없으면 `{ "soul": "<name>" }` 로 생성).
  4. 완료 후 "다음 세션부터 적용됨" 을 알린다.

- **`show [name]`** — 소울 본문(페르소나 전문)을 미리 보여준다. 이름 생략 시 현재 활성 소울.

- **`new <name>`** — 커스텀 소울을 스캐폴딩한다.
  1. `~/.config/rocky/souls/<name>.md` 가 이미 있으면 덮어쓰지 않고 경고 후 멈춘다.
  2. 없으면 아래 템플릿으로 생성하고, 사용자가 본문을 채우도록 안내한다:
     ```markdown
     ---
     name: <name>
     description: <한 줄 설명>
     ---

     ## 말투 / 성격
     - ...

     ## 작업 방식
     - ...
     ```
  3. `--project` 는 여기선 무시(커스텀 소울은 user 디렉터리에만 산다). `soul` 로 활성화하려면 `/rocky:soul <name>`.

## 원칙

- 소울은 AGENTS.md 게이트/안전 규칙 위의 레이어일 뿐 — 그 규칙을 덮어쓰지 않는다.
- 이름은 `[a-zA-Z0-9_-]+` 만 (파일명/`soul` 필드 제약과 동일).
- `rocky.json` 을 쓸 때 기존 필드를 보존한다 — `soul` 키만 건드린다.
```

- [ ] **Step 2: frontmatter 검증 (파싱 스모크)**

Run: `bun run -e 'const t=await Bun.file("commands/soul.md").text(); if(!t.startsWith("---")) throw new Error("no frontmatter"); console.log("ok")'`
Expected: `ok`.

- [ ] **Step 3: 커밋**

```bash
git add commands/soul.md
git commit -m "feat(soul): /rocky:soul 슬래시 커맨드 (list / set / show / new)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 6: 문서 동기화 + 최종 게이트

**Files:**
- Modify: `FEATURES.md` (config 표 `soul`, 커맨드 `/rocky:soul`, 훅 SessionStart, 소울 파일 위치)
- Modify: `AGENTS.md` (Layout: `souls/`, `commands/soul.md`, `src/core/soul.ts`, `src/hooks/inject-soul.ts`, hooks.json 의 SessionStart / *Project in one line*)
- Modify: `README.md` (surface 설명)
- Modify: `.claude-plugin/plugin.json` (description + keywords)

**Interfaces:**
- Consumes: 앞선 모든 Task 의 표면.
- Produces: 없음(문서).

- [ ] **Step 1: `plugin.json` 갱신**

`keywords` 배열에 `"soul"`, `"persona"` 추가. `description` 끝(아카이브 문장 앞)에 한 문장 추가:

```
소울(페르소나) 선택: souls/*.md 프리셋 + ~/.config/rocky/souls 커스텀, rocky.json 의 soul 필드로 고정, SessionStart 훅이 매 세션 자동 주입. /rocky:soul 커맨드로 전환.
```

- [ ] **Step 2: `AGENTS.md` Layout 갱신**

`commands/` 트리에 추가: `soul.md — /rocky:soul — 소울(페르소나) 선택 (list/set/show/new), MCP tool 아님`.
`hooks/hooks.json` 설명에 `SessionStart → inject-soul.ts` 추가.
`hooks/` 트리에 추가: `inject-soul.ts — SessionStart hook entry — resolveSoulName → readSoul → buildSoulContext → additionalContext`.
`src/core/` 트리에 추가: `soul.ts — 소울 코어 (list/read/resolve/buildContext + frontmatter, DI)`.
새 최상위 항목: `souls/ — ★ 번들 프리셋 소울 (rocky / senior / terse), 커스텀은 ~/.config/rocky/souls/`.
*Project in one line* 에 소울 기능 한 문장 추가 (SessionStart 훅 + /rocky:soul + rocky.json soul 필드).

- [ ] **Step 3: `FEATURES.md` 갱신**

config 표에 `soul` 행 추가(활성 소울 이름, project>user). 커맨드 표에 `/rocky:soul` 추가. 훅 섹션에 SessionStart 자동 주입 추가. 소울 파일 위치(`souls/`, `~/.config/rocky/souls/`) 설명.

- [ ] **Step 4: `README.md` 갱신**

surface 요약에 소울(페르소나 선택) 한 줄 추가.

- [ ] **Step 5: 전체 게이트 실행**

Run: `bun run check && bun run typecheck && bun test`
Expected: 세 개 모두 PASS. `bun test` 는 신규 soul / soul-presets / inject-soul + 기존 index.test.ts(MCP surface 불변) 포함 전부 통과.

- [ ] **Step 6: 커밋**

```bash
git add FEATURES.md AGENTS.md README.md .claude-plugin/plugin.json
git commit -m "docs(soul): 소울 기능 문서 동기화 (FEATURES / AGENTS / README / plugin.json)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

- [ ] **Step 7: 로컬 적용 확인 (선택)**

Run: `/reload-plugins` (Claude Code 내에서), 그다음 `/rocky:soul rocky` 로 활성화 → 새 세션에서 소울 주입 확인.

---

## 마무리

전체 완료 후 상태: `rocky.json` 의 `soul` 로 페르소나를 고정 → SessionStart 훅이 매 세션 자동 주입, `/rocky:soul` 로 전환, 프리셋 3종 + 커스텀 지원. MCP tool surface / Codex 표면 불변.
