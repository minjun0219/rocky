# rocky-todo MCP 브릿지 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** rocky-todo 도구가 데몬 생사와 무관하게 항상 세션에 존재하도록 stdio MCP 브릿지를 도입하고, 비활성 시 동의 기반 활성화 경로와 웹UI 에셋 경로 픽스를 추가한다.

**Architecture:** 데몬에서 `/mcp` HTTP MCP 표면을 제거하고, plugin.json 에 선언되는 stdio 브릿지(`mcp-stdio.ts`)가 유일한 MCP 서버가 된다. 브릿지는 도구 호출을 데몬의 기존 `/api/*` REST 로 포워딩하고(공유 `client.ts`), 데몬을 온디맨드로 health→spawn 한다. 비활성 상태에서는 `todo_enable` 6번째 도구가 노출되고 나머지 5개는 구조화된 안내 에러를 반환한다.

**Tech Stack:** TypeScript, Bun (build 없음, `.ts` 직접 실행), `@modelcontextprotocol/sdk` (McpServer + StdioServerTransport), `zod`, `bun test`.

## Global Constraints

- **의존성 0 추가**: 새 prod-dep 금지. `@modelcontextprotocol/sdk` + `zod` 는 기존 예외.
- **build 없음**: `import.meta.dir` / `import.meta.url` 사용, `__dirname` 금지. import 에 `.ts`/`.js` 확장자 붙이지 않음 (SDK 의 `...js` subpath 는 예외로 유지).
- **상대경로 import**: `src/todo/` 내부는 상대경로, core 는 `../core/<file>`.
- **테스트 격리**: fs 의존 테스트는 `mkdtempSync` 로 격리.
- **삭제 없음**: 도구/REST 표면에 delete 없음 — archive 만.
- **게이트**: `bun run check` / `bun run typecheck` / `bun test` 통과가 완료 조건.
- **커밋 메시지**: Conventional Commits (`type(scope): 한국어 요약`, 요약 ~50자).
- **주석**: 설명 산문은 한국어, 식별자/경로/명령은 영어 원형.

---

### Task 1: 웹UI 에셋 경로 chdir 픽스

타 cwd 에서 데몬이 spawn 되면 Bun HTML 번들이 asset public path 를 `process.cwd()` 기준으로 계산해 `/../../<cwd>/chunk-*.css` 로 깨진다. `Bun.serve` 전에 ui 디렉터리로 chdir 해서 고정한다.

**Files:**
- Modify: `src/todo/daemon.ts` (startDaemon 내부, `Bun.serve` 호출 직전)

**Interfaces:**
- Consumes: 없음 (기존 `import.meta.dir`, `join` 은 이미 import 됨 — `join` 은 line 2 에서 import 확인)
- Produces: 없음 (동작만 변경)

- [ ] **Step 1: chdir 1줄 추가**

`src/todo/daemon.ts` 의 `const server = Bun.serve({` (현재 line 64) 바로 앞에 삽입:

```ts
  // Bun 의 HTML 번들은 asset public path 를 process.cwd() 기준으로 계산한다.
  // CLI/브릿지가 호출자 cwd 를 상속시켜 spawn 하면 /../../<cwd> 로 깨지므로 ui 디렉터리로 고정한다.
  process.chdir(join(import.meta.dir, 'ui'));

  const server = Bun.serve({
```

`runtime.dir`(db 경로)은 `resolveTodoRuntimeConfig` 가 절대경로로 반환하고 `mkdirSync(runtime.dir)` 도 그 앞줄(line 59)에서 이미 실행되므로 chdir 영향 없음.

- [ ] **Step 2: 타 cwd 에서 수동 검증**

이 픽스는 Bun 번들 런타임 동작이라 유닛 테스트로 잡기 애매하다. 수동 검증:

```bash
# 스크래치 디렉터리에서 격리 포트/디렉터리로 데몬 기동
cd /tmp && ROCKY_TODO_ENABLED=1 ROCKY_TODO_EXPOSE=off ROCKY_TODO_PORT=8891 \
  ROCKY_TODO_DIR=/tmp/rocky-todo-probe \
  bun run <repo>/src/todo/daemon.ts &
sleep 4
curl -s http://127.0.0.1:8891/ | grep -o 'href="[^"]*chunk[^"]*"'
# 기대: href="/chunk-....css"  (앞에 /../ 없음)
curl -s -o /dev/null -w "%{http_code}\n" \
  "http://127.0.0.1:8891$(curl -s http://127.0.0.1:8891/ | grep -o '/chunk-[^\"]*\.css' | head -1)"
# 기대: 200
lsof -ti:8891 | xargs kill
```

⚠️ 주의: 검증용 데몬은 반드시 `ROCKY_TODO_EXPOSE=off` + 전용 포트로 띄운다 — user rocky.json 의 `expose:["tailscale-serve"]` 를 상속하면 `tailscale serve` 를 프로브 포트로 덮어쓴다. 실제 데몬(8636)이 떠 있다면 검증 후 `tailscale serve --bg 8636` 로 원복 확인.

- [ ] **Step 3: 게이트 + 커밋**

```bash
bun run check && bun run typecheck && bun test
git add src/todo/daemon.ts
git commit -m "fix(todo): 웹UI 에셋 경로를 ui 디렉터리로 고정"
```

---

### Task 2: REST 클라이언트 공유 모듈 추출 (`client.ts`)

`cli.ts` 에 인라인된 `CliContext`/`health`/`ensureDaemon`/`request` 를 `client.ts` 로 옮겨 CLI 와 브릿지가 공유한다. 순수 리팩터 — 동작 변경 없음, 기존 CLI 테스트 통과가 게이트.

**Files:**
- Create: `src/todo/client.ts`
- Create: `src/todo/client.test.ts`
- Modify: `src/todo/cli.ts` (인라인 정의 제거 + import)

**Interfaces:**
- Consumes: `resolveTodoRuntimeConfig` 반환의 `port`/`dir` (from `./config`), `detectActor` (from `./actor`)
- Produces:
  - `interface CliContext { baseUrl: string; port: number; dir: string; actor: string }`
  - `function buildContext(opts: { port: number; dir: string; actor: string }): CliContext`
  - `function health(baseUrl: string): Promise<boolean>`
  - `function ensureDaemon(ctx: CliContext): Promise<void>`
  - `function request<T>(ctx: CliContext, method: string, path: string, body?: unknown): Promise<T>`

- [ ] **Step 1: client.test.ts 작성 (실패 상태)**

```ts
import { describe, expect, test } from 'bun:test';
import { buildContext, request } from './client';

describe('client', () => {
  test('buildContext 는 baseUrl 을 포트로 조립한다', () => {
    const ctx = buildContext({ port: 8636, dir: '/tmp/x', actor: 'claude-code' });
    expect(ctx.baseUrl).toBe('http://127.0.0.1:8636');
    expect(ctx.actor).toBe('claude-code');
  });

  test('request 는 x-rocky-actor 헤더를 붙이고 JSON 을 파싱한다', async () => {
    const seen: { headers?: Headers; body?: string } = {};
    const originalFetch = globalThis.fetch;
    // 데몬이 이미 떠 있다고 보이도록 health + 실제 요청을 fake
    globalThis.fetch = (async (url: string | URL | Request, init?: RequestInit) => {
      const u = String(url);
      if (u.endsWith('/api/health')) {
        return new Response(JSON.stringify({ ok: true, name: 'rocky-todo' }), { status: 200 });
      }
      seen.headers = new Headers(init?.headers);
      seen.body = init?.body as string | undefined;
      return new Response(JSON.stringify({ id: 'abc123' }), { status: 200 });
    }) as typeof fetch;
    try {
      const ctx = buildContext({ port: 8636, dir: '/tmp/x', actor: 'claude-code' });
      const result = await request<{ id: string }>(ctx, 'POST', '/api/todos', { title: 't' });
      expect(result.id).toBe('abc123');
      expect(seen.headers?.get('x-rocky-actor')).toBe('claude-code');
      expect(seen.body).toBe(JSON.stringify({ title: 't' }));
    } finally {
      globalThis.fetch = originalFetch;
    }
  });

  test('request 는 non-ok 응답의 error 필드를 throw 한다', async () => {
    const originalFetch = globalThis.fetch;
    globalThis.fetch = (async (url: string | URL | Request) => {
      if (String(url).endsWith('/api/health')) {
        return new Response(JSON.stringify({ ok: true, name: 'rocky-todo' }), { status: 200 });
      }
      return new Response(JSON.stringify({ error: 'boom' }), { status: 400 });
    }) as typeof fetch;
    try {
      const ctx = buildContext({ port: 8636, dir: '/tmp/x', actor: 'x' });
      await expect(request(ctx, 'GET', '/api/todos')).rejects.toThrow('boom');
    } finally {
      globalThis.fetch = originalFetch;
    }
  });
});
```

- [ ] **Step 2: 테스트 실패 확인**

Run: `bun test src/todo/client.test.ts`
Expected: FAIL — `Cannot find module './client'`

- [ ] **Step 3: client.ts 작성**

`cli.ts` 의 현재 line 172~230 (`CliContext`/`health`/`ensureDaemon`/`request`) 를 그대로 옮기고 `buildContext` 를 추가한다. 반환 타입은 store 타입을 쓰는 호출부에서 제네릭으로 지정하므로 client 자체는 제네릭 `request<T>` 만.

```ts
import { join } from 'node:path';

/**
 * rocky-todo 데몬의 얇은 REST 클라이언트 — CLI 와 stdio MCP 브릿지가 공유한다.
 *
 * 데몬이 죽어 있으면 `ensureDaemon` 이 detached spawn 후 health 가 응답할 때까지
 * (최대 ~5s) 기다린다. 모든 요청에 `x-rocky-actor` 헤더를 붙여 히스토리에 남긴다.
 */

export interface CliContext {
  baseUrl: string;
  port: number;
  dir: string;
  actor: string;
}

/** port/dir/actor 로 CliContext 를 조립한다 (baseUrl 은 127.0.0.1 루프백). */
export function buildContext(opts: { port: number; dir: string; actor: string }): CliContext {
  return {
    baseUrl: `http://127.0.0.1:${opts.port}`,
    port: opts.port,
    dir: opts.dir,
    actor: opts.actor,
  };
}

export async function health(baseUrl: string): Promise<boolean> {
  try {
    const res = await fetch(`${baseUrl}/api/health`, { signal: AbortSignal.timeout(700) });
    return res.ok;
  } catch {
    return false;
  }
}

/** 데몬이 안 떠 있으면 detached spawn 하고 health 가 응답할 때까지 (최대 ~5s) 기다린다. */
export async function ensureDaemon(ctx: CliContext): Promise<void> {
  if (await health(ctx.baseUrl)) {
    return;
  }
  const daemonPath = join(import.meta.dir, 'daemon.ts');
  Bun.spawn({
    cmd: [process.execPath, 'run', daemonPath],
    stdio: ['ignore', 'ignore', 'ignore'],
    env: process.env,
  }).unref();
  for (let i = 0; i < 25; i++) {
    await Bun.sleep(200);
    if (await health(ctx.baseUrl)) {
      return;
    }
  }
  throw new Error(
    `rocky-todo daemon did not start on port ${ctx.port} — check \`rocky-todo daemon status\``,
  );
}

export async function request<T>(
  ctx: CliContext,
  method: string,
  path: string,
  body?: unknown,
): Promise<T> {
  await ensureDaemon(ctx);
  const res = await fetch(`${ctx.baseUrl}${path}`, {
    method,
    headers: {
      ...(body !== undefined ? { 'content-type': 'application/json' } : {}),
      'x-rocky-actor': ctx.actor,
    },
    ...(body !== undefined ? { body: JSON.stringify(body) } : {}),
  });
  const payload = (await res.json()) as T & { error?: string };
  if (!res.ok) {
    throw new Error(payload.error ?? `${res.status} ${res.statusText}`);
  }
  return payload;
}
```

- [ ] **Step 4: cli.ts 에서 인라인 제거 + import**

`src/todo/cli.ts`:
1. 상단 import 에 추가: `import { buildContext, type CliContext, request } from './client';`
2. 로컬 `interface CliContext {...}` (line 172~177), `health` (line 179~186), `ensureDaemon` (line 188~208), `request` (line 210~230) 정의를 삭제.
3. `runCli` 안의 ctx 조립 (현재 `const ctx: CliContext = { baseUrl: ..., port: ..., dir: ..., actor: ... }`) 을 `const ctx = buildContext({ port: runtime.port, dir: runtime.dir, actor: str(flags.actor) ?? detectActor() });` 로 교체.
4. `printAddresses` 등에서 쓰는 `ctx.baseUrl`/`ctx.port` 는 그대로 동작 (buildContext 가 동일 필드 제공).

- [ ] **Step 5: 게이트 확인**

Run: `bun test src/todo/client.test.ts src/todo/cli.test.ts && bun run typecheck`
Expected: PASS (기존 cli.test.ts 도 통과 — 리팩터라 동작 불변)

- [ ] **Step 6: 커밋**

```bash
bun run check
git add src/todo/client.ts src/todo/client.test.ts src/todo/cli.ts
git commit -m "refactor(todo): REST 클라이언트를 client.ts 로 공유 추출"
```

---

### Task 3: 도구 스펙 단일화 (`mcp-tools.ts`)

5개 도구의 name/description/zod inputSchema 를 단일 출처로 뽑는다. 현재 `mcp.ts` 안에 인라인된 스펙을 옮긴다 (Task 6 에서 `mcp.ts` 삭제 시 이게 유일 출처).

**Files:**
- Create: `src/todo/mcp-tools.ts`
- Create: `src/todo/mcp-tools.test.ts`

**Interfaces:**
- Consumes: `zod`
- Produces:
  - `const TODO_TOOL_SPECS: ToolSpec[]` — 5개 (todo_list, todo_write, todo_status, note_list, note_write)
  - `interface ToolSpec { name: string; description: string; inputSchema: z.ZodRawShape; method: 'GET' | 'POST' | 'PATCH'; }` — 단, REST 매핑은 도구마다 분기가 필요하므로 method 는 스펙에 넣지 않고 브릿지 핸들러가 결정한다. **스펙에는 name/description/inputSchema 만 둔다.**
  - `const linkSchema` (재사용용 export)

- [ ] **Step 1: mcp-tools.test.ts 작성 (실패 상태)**

```ts
import { describe, expect, test } from 'bun:test';
import { TODO_TOOL_SPECS } from './mcp-tools';

describe('mcp-tools', () => {
  test('정확히 5개 도구를 정의한다', () => {
    expect(TODO_TOOL_SPECS.map((s) => s.name).sort()).toEqual([
      'note_list',
      'note_write',
      'todo_list',
      'todo_status',
      'todo_write',
    ]);
  });

  test('각 스펙은 description 과 inputSchema 를 갖는다', () => {
    for (const spec of TODO_TOOL_SPECS) {
      expect(spec.description.length).toBeGreaterThan(0);
      expect(typeof spec.inputSchema).toBe('object');
    }
  });

  test('todo_status 의 action enum 이 6개 전이를 담는다', () => {
    const spec = TODO_TOOL_SPECS.find((s) => s.name === 'todo_status');
    expect(spec).toBeDefined();
    // action 필드가 존재하는지만 확인 (zod 내부 구조 대신 파싱으로 검증)
    const shape = spec!.inputSchema as Record<string, { safeParse: (v: unknown) => { success: boolean } }>;
    expect(shape.action.safeParse('start').success).toBe(true);
    expect(shape.action.safeParse('nope').success).toBe(false);
  });
});
```

- [ ] **Step 2: 테스트 실패 확인**

Run: `bun test src/todo/mcp-tools.test.ts`
Expected: FAIL — `Cannot find module './mcp-tools'`

- [ ] **Step 3: mcp-tools.ts 작성**

현재 `mcp.ts` 의 각 `registerTool` 호출에서 description + inputSchema 를 그대로 옮긴다 (line 39~48, 68~82, 99~105, 114~121, 138~146).

```ts
import { z } from 'zod';

/**
 * rocky-todo 의 MCP 도구 스펙 단일 출처 — name / description / zod inputSchema.
 *
 * stdio 브릿지(mcp-stdio.ts)가 이 스펙에 REST 포워딩 핸들러를 바인딩한다.
 * 스펙과 핸들러를 분리해 도구 표면이 한 곳에서만 정의되게 한다.
 */

const actorSchema = z
  .string()
  .optional()
  .describe('who is acting (e.g. claude-code / codex / opencode); recorded in history');

export const linkSchema = z.object({ url: z.string(), title: z.string().optional() });

export interface ToolSpec {
  name: string;
  description: string;
  inputSchema: z.ZodRawShape;
}

export const TODO_TOOL_SPECS: ToolSpec[] = [
  {
    name: 'todo_list',
    description:
      '공유 todo 보드 조회. board 로 보드 하나, 생략 시 전체. id 를 주면 해당 todo 상세 + 히스토리, boards:true 면 보드 목록. 필터: status / label / includeArchived.',
    inputSchema: {
      board: z.string().optional().describe('board key (usually the repo name)'),
      id: z.string().optional().describe('todo id (or unique prefix) for detail + history'),
      boards: z.boolean().optional().describe('true → list boards instead of todos'),
      status: z.enum(['todo', 'doing', 'done']).optional(),
      label: z.string().optional(),
      includeArchived: z.boolean().optional(),
    },
  },
  {
    name: 'todo_write',
    description:
      'todo 생성/수정. id 없으면 생성(board + title 필수), 있으면 부분 수정. section 은 이름으로 자동 upsert. links 에 GitHub 이슈 / Todoist URL 을 첨부해 맥락을 연결한다. 삭제는 없다 — todo_status 의 archive 를 쓴다.',
    inputSchema: {
      id: z.string().optional().describe('omit to create, set to patch an existing todo'),
      board: z.string().optional().describe('board key — required when creating'),
      title: z.string().optional().describe('required when creating'),
      description: z.string().optional().describe('markdown detail'),
      section: z.string().optional().describe('section name (upserted within the board)'),
      parentId: z.string().optional().describe('parent todo id for hierarchy'),
      priority: z.enum(['p1', 'p2', 'p3', 'p4']).optional(),
      due: z.string().optional().describe('ISO date, e.g. 2026-08-01'),
      labels: z.array(z.string()).optional(),
      links: z.array(linkSchema).optional(),
      actor: actorSchema,
    },
  },
  {
    name: 'todo_status',
    description:
      'todo 상태 전이. start=처리 시작(누가 작업중인지 웹 UI 에 표시됨 — 작업 착수 시 반드시 호출), stop=중단, done=완료, reopen=재오픈, archive/unarchive=보관/복원.',
    inputSchema: {
      id: z.string().describe('todo id (or unique prefix)'),
      action: z.enum(['start', 'stop', 'done', 'reopen', 'archive', 'unarchive']),
      actor: actorSchema,
    },
  },
  {
    name: 'note_list',
    description:
      '스크래치패드/메모 조회. board 로 보드 소속, global:true 로 보드 미소속 메모. id 를 주면 상세 + 히스토리.',
    inputSchema: {
      board: z.string().optional(),
      global: z.boolean().optional(),
      id: z.string().optional(),
      includeArchived: z.boolean().optional(),
    },
  },
  {
    name: 'note_write',
    description:
      '스크래치패드/메모 작성. id 없으면 생성(title 필수), 있으면 수정. mode: set=content 교체(기본) / append=뒤에 이어붙임 / archive=보관 / unarchive=복원. 삭제는 없다.',
    inputSchema: {
      id: z.string().optional(),
      board: z.string().optional().describe('omit for a global note'),
      title: z.string().optional(),
      content: z.string().optional(),
      mode: z.enum(['set', 'append', 'archive', 'unarchive']).optional(),
      actor: actorSchema,
    },
  },
];
```

- [ ] **Step 4: 테스트 통과 확인**

Run: `bun test src/todo/mcp-tools.test.ts`
Expected: PASS (3 tests)

- [ ] **Step 5: 커밋**

```bash
bun run check && bun run typecheck
git add src/todo/mcp-tools.ts src/todo/mcp-tools.test.ts
git commit -m "refactor(todo): MCP 도구 스펙을 mcp-tools.ts 로 단일화"
```

---

### Task 4: 활성화 코어 (`enable.ts`)

user rocky.json 에 `todo.enabled=true` 를 병합 기록(기존 키 보존)하고 데몬을 기동한다. `todo_enable` 도구와 CLI `enable` 이 공유한다.

**Files:**
- Create: `src/todo/enable.ts`
- Create: `src/todo/enable.test.ts`

**Interfaces:**
- Consumes: `USER_CONFIG_PATH` (from `../core/rocky-config`), `ensureDaemon`/`buildContext` (from `./client`)
- Produces:
  - `function writeEnabledFlag(configPath: string): void` — rocky.json 로드→`todo.enabled=true` 병합→기록 (순수 fs, 테스트 대상)
  - `interface EnableResult { ok: boolean; url: string; hint: string }`
  - `function enableTodo(opts: { port: number; dir: string; configPath?: string }): Promise<EnableResult>`

- [ ] **Step 1: enable.test.ts 작성 (실패 상태)**

```ts
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { describe, expect, test } from 'bun:test';
import { writeEnabledFlag } from './enable';

describe('writeEnabledFlag', () => {
  test('기존 파일의 다른 키를 보존하며 todo.enabled=true 를 병합한다', () => {
    const dir = mkdtempSync(join(tmpdir(), 'rocky-enable-'));
    const path = join(dir, 'rocky.json');
    writeFileSync(path, JSON.stringify({ soul: 'rocky', callsign: 'Logan', todo: { port: 9000 } }));
    writeEnabledFlag(path);
    const parsed = JSON.parse(readFileSync(path, 'utf8'));
    expect(parsed.soul).toBe('rocky');
    expect(parsed.callsign).toBe('Logan');
    expect(parsed.todo.port).toBe(9000);
    expect(parsed.todo.enabled).toBe(true);
  });

  test('파일이 없으면 새로 만든다', () => {
    const dir = mkdtempSync(join(tmpdir(), 'rocky-enable-'));
    const path = join(dir, 'rocky.json');
    writeEnabledFlag(path);
    const parsed = JSON.parse(readFileSync(path, 'utf8'));
    expect(parsed.todo.enabled).toBe(true);
  });

  test('파싱 불가한 파일은 덮지 않고 throw 한다', () => {
    const dir = mkdtempSync(join(tmpdir(), 'rocky-enable-'));
    const path = join(dir, 'rocky.json');
    writeFileSync(path, '{ not valid json');
    expect(() => writeEnabledFlag(path)).toThrow();
    // 원본 보존 확인
    expect(readFileSync(path, 'utf8')).toBe('{ not valid json');
  });
});
```

- [ ] **Step 2: 테스트 실패 확인**

Run: `bun test src/todo/enable.test.ts`
Expected: FAIL — `Cannot find module './enable'`

- [ ] **Step 3: enable.ts 작성**

```ts
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname } from 'node:path';
import { USER_CONFIG_PATH } from '../core/rocky-config';
import { buildContext, ensureDaemon } from './client';

/**
 * rocky-todo 활성화 코어 — `todo_enable` 도구와 CLI `enable` 이 공유한다.
 *
 * user rocky.json 에 `todo.enabled=true` 를 병합 기록(기존 키 보존)하고 데몬을 기동한다.
 * launchd 상주 등록은 하지 않는다 (`rocky-todo daemon install` 로 분리 유지).
 */

/** user rocky.json 을 로드해 todo.enabled=true 를 병합 기록한다. 파싱 실패 시 덮지 않고 throw. */
export function writeEnabledFlag(configPath: string = USER_CONFIG_PATH): void {
  let root: Record<string, unknown> = {};
  if (existsSync(configPath)) {
    const raw = readFileSync(configPath, 'utf8');
    try {
      const parsed = JSON.parse(raw) as unknown;
      if (typeof parsed !== 'object' || parsed === null) {
        throw new Error('rocky.json 최상위가 객체가 아니다');
      }
      root = parsed as Record<string, unknown>;
    } catch (error) {
      throw new Error(
        `rocky.json 파싱 실패 (${configPath}) — 활성화를 중단한다: ${error instanceof Error ? error.message : String(error)}`,
      );
    }
  }
  const todo = (typeof root.todo === 'object' && root.todo !== null ? root.todo : {}) as Record<
    string,
    unknown
  >;
  todo.enabled = true;
  root.todo = todo;
  mkdirSync(dirname(configPath), { recursive: true });
  writeFileSync(configPath, `${JSON.stringify(root, null, 2)}\n`);
}

export interface EnableResult {
  ok: boolean;
  url: string;
  hint: string;
}

/** rocky.json 에 enabled 기록 후 데몬을 기동한다 (health 대기 포함). */
export async function enableTodo(opts: {
  port: number;
  dir: string;
  configPath?: string;
}): Promise<EnableResult> {
  writeEnabledFlag(opts.configPath ?? USER_CONFIG_PATH);
  const ctx = buildContext({ port: opts.port, dir: opts.dir, actor: 'rocky-todo' });
  await ensureDaemon(ctx);
  return {
    ok: true,
    url: ctx.baseUrl,
    hint: '재부팅 후에도 상시 기동하려면 rocky-todo daemon install',
  };
}
```

- [ ] **Step 4: 테스트 통과 확인**

Run: `bun test src/todo/enable.test.ts`
Expected: PASS (3 tests)

- [ ] **Step 5: 커밋**

```bash
bun run check && bun run typecheck
git add src/todo/enable.ts src/todo/enable.test.ts
git commit -m "feat(todo): 활성화 코어 enable.ts 추가"
```

---

### Task 5: stdio MCP 브릿지 (`mcp-stdio.ts`)

plugin.json 에 선언될 유일한 MCP 서버. 6개 도구(5 + `todo_enable`)를 등록하고, 활성 시 REST 포워딩, 비활성 시 구조화된 안내 에러를 반환한다.

**Files:**
- Create: `src/todo/mcp-stdio.ts`
- Create: `src/todo/mcp-stdio.test.ts`

**Interfaces:**
- Consumes: `TODO_TOOL_SPECS` (from `./mcp-tools`), `buildContext`/`request` (from `./client`), `enableTodo` (from `./enable`), `resolveTodoRuntimeConfig` (from `./config`), `detectActor` (from `./actor`), `loadConfig`/`DEFAULT_TODO_DIR`, `McpServer`/`StdioServerTransport` (SDK)
- Produces:
  - `function buildBridgeServer(deps: BridgeDeps): McpServer` — DI 로 테스트 가능
  - `interface BridgeDeps { enabled: boolean; forward: (name: string, args: Record<string, unknown>) => Promise<unknown>; enable: () => Promise<unknown> }`
  - `function toolToRest(name: string, args: Record<string, unknown>): { method: string; path: string; body?: unknown }` — 도구→REST 매핑 (순수, 테스트 대상)

- [ ] **Step 1: mcp-stdio.test.ts 작성 (실패 상태)**

브릿지 로직을 DI 로 검증한다. 실제 데몬 spawn/stdio 없이 `buildBridgeServer` 의 도구 핸들러를 직접 호출한다. McpServer 내부 핸들러 접근 대신, 순수 매핑 함수와 disabled-guard 를 테스트한다.

```ts
import { describe, expect, test } from 'bun:test';
import { disabledGuidance, toolToRest } from './mcp-stdio';

describe('toolToRest', () => {
  test('todo_list (필터) → GET /api/todos', () => {
    expect(toolToRest('todo_list', { board: 'rocky' })).toEqual({
      method: 'GET',
      path: '/api/todos?board=rocky',
    });
  });

  test('todo_list boards → GET /api/boards', () => {
    expect(toolToRest('todo_list', { boards: true })).toEqual({
      method: 'GET',
      path: '/api/boards',
    });
  });

  test('todo_list id → GET /api/todos/:id', () => {
    expect(toolToRest('todo_list', { id: 'abc' })).toEqual({
      method: 'GET',
      path: '/api/todos/abc',
    });
  });

  test('todo_write 생성 → POST /api/todos', () => {
    expect(toolToRest('todo_write', { board: 'r', title: 't' })).toEqual({
      method: 'POST',
      path: '/api/todos',
      body: { board: 'r', title: 't' },
    });
  });

  test('todo_write 수정 → PATCH /api/todos/:id', () => {
    expect(toolToRest('todo_write', { id: 'abc', title: 't2' })).toEqual({
      method: 'PATCH',
      path: '/api/todos/abc',
      body: { title: 't2' },
    });
  });

  test('todo_status → POST /api/todos/:id/status', () => {
    expect(toolToRest('todo_status', { id: 'abc', action: 'start' })).toEqual({
      method: 'POST',
      path: '/api/todos/abc/status',
      body: { action: 'start' },
    });
  });

  test('note_write archive → POST /api/notes/:id/archive', () => {
    expect(toolToRest('note_write', { id: 'n1', mode: 'archive' })).toEqual({
      method: 'POST',
      path: '/api/notes/n1/archive',
      body: undefined,
    });
  });

  test('note_write 생성 → POST /api/notes', () => {
    expect(toolToRest('note_write', { title: '메모' })).toEqual({
      method: 'POST',
      path: '/api/notes',
      body: { title: '메모' },
    });
  });
});

describe('disabledGuidance', () => {
  test('노출 범위 3가지와 todo_enable 지시를 담는다', () => {
    const g = disabledGuidance();
    expect(g.error).toBe('rocky-todo disabled');
    expect(g.guidance).toContain('127.0.0.1');
    expect(g.guidance).toContain('todo.db');
    expect(g.guidance).toContain('todo_enable');
  });
});
```

- [ ] **Step 2: 테스트 실패 확인**

Run: `bun test src/todo/mcp-stdio.test.ts`
Expected: FAIL — `Cannot find module './mcp-stdio'`

- [ ] **Step 3: mcp-stdio.ts 작성**

`toolToRest` 는 도구별 REST 매핑을 순수 함수로 만든다. `x-rocky-actor` 헤더는 `request` 가 ctx.actor 로 붙이므로 body 의 actor 는 그대로 전달(REST 서버는 헤더 actor 를 우선 사용).

```ts
import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import { z } from 'zod';
import pkg from '../../package.json' with { type: 'json' };
import { detectActor } from './actor';
import { buildContext, request } from './client';
import { DEFAULT_TODO_DIR, resolveTodoRuntimeConfig } from './config';
import { enableTodo } from './enable';
import { loadConfig } from '../core/rocky-config';
import { TODO_TOOL_SPECS } from './mcp-tools';

/**
 * rocky-todo 의 stdio MCP 브릿지 — plugin.json 에 선언되는 유일한 MCP 서버.
 *
 * 도구 호출을 데몬의 기존 /api/* REST 로 포워딩하고(client.ts), 데몬을 온디맨드로
 * health→spawn 한다. 비활성(todo.enabled=false) 상태에서는 todo_enable 만 실질 동작하고
 * 나머지 5개는 구조화된 안내 에러를 반환한다 — 도구가 세션에서 사라지지 않게 하는 게 핵심.
 */

function q(params: Record<string, string | undefined>): string {
  const usp = new URLSearchParams();
  for (const [k, v] of Object.entries(params)) {
    if (v !== undefined && v !== '') {
      usp.set(k, v);
    }
  }
  const s = usp.toString();
  return s ? `?${s}` : '';
}

/** 도구 호출을 데몬 REST 요청으로 매핑한다 (순수). */
export function toolToRest(
  name: string,
  args: Record<string, unknown>,
): { method: string; path: string; body?: unknown } {
  const s = (v: unknown): string | undefined => (typeof v === 'string' ? v : undefined);
  switch (name) {
    case 'todo_list': {
      if (args.boards) {
        return { method: 'GET', path: '/api/boards' };
      }
      if (args.id) {
        return { method: 'GET', path: `/api/todos/${s(args.id)}` };
      }
      return {
        method: 'GET',
        path: `/api/todos${q({ board: s(args.board), status: s(args.status), label: s(args.label), includeArchived: args.includeArchived ? 'true' : undefined })}`,
      };
    }
    case 'todo_write': {
      if (args.id) {
        const { id, ...body } = args;
        return { method: 'PATCH', path: `/api/todos/${s(id)}`, body };
      }
      return { method: 'POST', path: '/api/todos', body: args };
    }
    case 'todo_status':
      return {
        method: 'POST',
        path: `/api/todos/${s(args.id)}/status`,
        body: { action: args.action },
      };
    case 'note_list': {
      if (args.id) {
        return { method: 'GET', path: `/api/notes/${s(args.id)}` };
      }
      return {
        method: 'GET',
        path: `/api/notes${q({ board: s(args.board), global: args.global ? 'true' : undefined, includeArchived: args.includeArchived ? 'true' : undefined })}`,
      };
    }
    case 'note_write': {
      if (args.id) {
        if (args.mode === 'archive' || args.mode === 'unarchive') {
          return { method: 'POST', path: `/api/notes/${s(args.id)}/${args.mode}`, body: undefined };
        }
        const { id, mode, ...rest } = args;
        return { method: 'PATCH', path: `/api/notes/${s(id)}`, body: { ...rest, mode } };
      }
      return { method: 'POST', path: '/api/notes', body: args };
    }
    default:
      throw new Error(`unknown tool: ${name}`);
  }
}

/** 비활성 상태에서 5개 도구가 반환하는 구조화된 안내. */
export function disabledGuidance(): { error: string; guidance: string } {
  return {
    error: 'rocky-todo disabled',
    guidance:
      '켜기 전에 사용자에게 알리고 동의를 받아라: (1) 127.0.0.1:8636 에 상주 데몬이 뜬다 (2) 보드 데이터는 ~/.config/rocky/todo/todo.db 에 저장된다 (3) user rocky.json 에 todo.enabled=true 가 기록된다. 동의를 받은 뒤에만 todo_enable 을 호출한다.',
  };
}

export interface BridgeDeps {
  enabled: boolean;
  forward: (name: string, args: Record<string, unknown>) => Promise<unknown>;
  enable: () => Promise<unknown>;
}

function jsonResult(value: unknown) {
  return { content: [{ type: 'text' as const, text: JSON.stringify(value) }] };
}

/** 6개 도구가 등록된 McpServer 를 만든다 (DI — transport 바인딩은 호출자 몫). */
export function buildBridgeServer(deps: BridgeDeps): McpServer {
  const server = new McpServer({ name: 'rocky-todo', version: pkg.version });

  for (const spec of TODO_TOOL_SPECS) {
    server.registerTool(
      spec.name,
      { description: spec.description, inputSchema: spec.inputSchema },
      async (args: Record<string, unknown>) => {
        if (!deps.enabled) {
          return jsonResult(disabledGuidance());
        }
        return jsonResult(await deps.forward(spec.name, args));
      },
    );
  }

  server.registerTool(
    'todo_enable',
    {
      description:
        'rocky-todo 를 활성화한다. 사용자에게 노출 범위(상주 데몬 127.0.0.1:8636 / SQLite 저장 / rocky.json 기록)를 설명하고 동의를 받은 뒤에만 호출한다. user rocky.json 에 todo.enabled=true 를 기록하고 데몬을 기동한다.',
      inputSchema: {},
    },
    async () => jsonResult(await deps.enable()),
  );

  return server;
}

if (import.meta.main) {
  const { config } = await loadConfig({ projectRoot: DEFAULT_TODO_DIR });
  const runtime = resolveTodoRuntimeConfig(process.env, config.todo);
  const ctx = buildContext({ port: runtime.port, dir: runtime.dir, actor: detectActor() });
  const server = buildBridgeServer({
    enabled: runtime.enabled,
    forward: async (name, args) => {
      const { method, path, body } = toolToRest(name, args);
      return request(ctx, method, path, body);
    },
    enable: () => enableTodo({ port: runtime.port, dir: runtime.dir }),
  });
  const transport = new StdioServerTransport();
  await server.connect(transport);
}
```

- [ ] **Step 4: 테스트 통과 확인**

Run: `bun test src/todo/mcp-stdio.test.ts`
Expected: PASS (9 tests)

- [ ] **Step 5: 브릿지 진입점 스모크 (수동)**

initialize + tools/list 가 데몬 없이도 6개를 반환하는지 확인:

```bash
printf '%s\n%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' \
  | ROCKY_TODO_ENABLED=0 bun run src/todo/mcp-stdio.ts 2>/dev/null \
  | grep -o '"name":"[a-z_]*"'
# 기대: todo_list, todo_write, todo_status, note_list, note_write, todo_enable (6개)
```

- [ ] **Step 6: 커밋**

```bash
bun run check && bun run typecheck && bun test
git add src/todo/mcp-stdio.ts src/todo/mcp-stdio.test.ts
git commit -m "feat(todo): stdio MCP 브릿지 추가"
```

---

### Task 6: 데몬 `/mcp` 제거

데몬을 순수 REST + SSE + 웹UI 로 만든다. 브릿지(Task 5)가 완성됐으므로 데몬 내부 MCP 표면을 삭제한다.

**Files:**
- Modify: `src/todo/daemon.ts` (import + `/mcp` 라우트 제거)
- Delete: `src/todo/mcp.ts`
- Delete: `src/todo/mcp.test.ts`

**Interfaces:**
- Consumes: 없음
- Produces: 없음 (표면 축소)

- [ ] **Step 1: daemon.ts 에서 /mcp 제거**

`src/todo/daemon.ts`:
1. `import { createMcpFetchHandler } from './mcp';` (line 6) 삭제.
2. `const mcp = createMcpFetchHandler({ store });` (line 62) 삭제.
3. routes 에서 `'/mcp': (req) => mcp(req),` (line 72) 삭제. 남는 routes:

```ts
    routes: {
      '/': ui,
      '/api/*': (req) => api.fetch(req),
    },
```

- [ ] **Step 2: mcp.ts / mcp.test.ts 삭제**

```bash
git rm src/todo/mcp.ts src/todo/mcp.test.ts
```

- [ ] **Step 3: 잔존 참조 확인**

Run: `grep -rn "createMcpFetchHandler\|buildTodoMcpServer\|from './mcp'\|from \"./mcp\"" src/`
Expected: 출력 없음 (mcp-stdio / mcp-tools 는 다른 파일이라 매치 안 됨 — 정확한 문자열 `'./mcp'` 만 확인)

- [ ] **Step 4: 게이트 확인**

Run: `bun run typecheck && bun test`
Expected: PASS (mcp.test.ts 사라져도 나머지 통과)

- [ ] **Step 5: 데몬 REST 스모크 (수동)**

```bash
ROCKY_TODO_ENABLED=1 ROCKY_TODO_EXPOSE=off ROCKY_TODO_PORT=8892 \
  ROCKY_TODO_DIR=/tmp/rocky-todo-probe6 bun run src/todo/daemon.ts &
sleep 3
curl -s -o /dev/null -w "health %{http_code}\n" http://127.0.0.1:8892/api/health   # 기대: 200
curl -s -o /dev/null -w "mcp %{http_code}\n" -X POST http://127.0.0.1:8892/mcp      # 기대: 404
lsof -ti:8892 | xargs kill
```

- [ ] **Step 6: 커밋**

```bash
bun run check
git add -A src/todo/daemon.ts
git commit -m "refactor(todo): 데몬에서 /mcp 제거 — 브릿지로 일원화"
```

---

### Task 7: plugin.json 등록 + CLI enable/mcp setup 갱신

브릿지를 플러그인 MCP 서버로 선언하고, CLI 에 `enable` 커맨드와 stdio 기반 `mcp setup` 안내를 반영한다.

**Files:**
- Modify: `.claude-plugin/plugin.json` (mcpServers)
- Modify: `src/todo/cli.ts` (enable 커맨드 + mcpSetupGuide 재작성 + INFO_COMMANDS)

**Interfaces:**
- Consumes: `enableTodo` (from `./enable`)
- Produces: CLI `enable` 서브커맨드

- [ ] **Step 1: plugin.json 에 브릿지 등록**

`.claude-plugin/plugin.json` 의 `mcpServers` 에 `rocky-todo` 추가:

```json
  "mcpServers": {
    "rocky": {
      "type": "stdio",
      "command": "bun",
      "args": ["run", "${CLAUDE_PLUGIN_ROOT}/src/index.ts"]
    },
    "rocky-todo": {
      "type": "stdio",
      "command": "bun",
      "args": ["run", "${CLAUDE_PLUGIN_ROOT}/src/todo/mcp-stdio.ts"]
    }
  }
```

- [ ] **Step 2: cli.ts 에 enable 커맨드 추가**

`src/todo/cli.ts`:
1. import 추가: `import { enableTodo } from './enable';`
2. `INFO_COMMANDS` (현재 line 328) 에 `enable` 은 넣지 않는다 — enable 은 실제 활성화 동작이므로 비활성 게이트를 우회할 필요 없이 자기가 켠다. 대신 게이트 조건을 `enable` 제외로 바꾼다:

```ts
  const INFO_COMMANDS = new Set([undefined, 'help', 'mcp', 'enable']);
```

3. switch 에 case 추가 (예: `mcp` case 앞):

```ts
    case 'enable': {
      const result = await enableTodo({ port: runtime.port, dir: runtime.dir });
      console.log(`✓ rocky-todo 활성화됨 — ${result.url}`);
      console.log(`  ${result.hint}`);
      return;
    }
```

4. HELP 문자열(현재 line ~296)에 한 줄 추가:

```
  rocky-todo enable                            todo.enabled=true 기록 + 데몬 기동 (동의 후)
```

- [ ] **Step 3: mcpSetupGuide 재작성**

`src/todo/cli.ts` 의 `mcpSetupGuide` (현재 line 721~735) 를 교체 — 데몬 `/mcp` 는 사라졌으므로 stdio 브릿지 기준으로:

```ts
function mcpSetupGuide(): string {
  return `rocky-todo MCP 는 stdio 브릿지(src/todo/mcp-stdio.ts)로 노출된다 — 데몬의 /mcp 는 없다.

Claude Code:
  rocky 플러그인이 자동 등록한다 (plugin.json 의 mcpServers.rocky-todo).
  과거 http 로 수동 등록했다면 제거: claude mcp remove rocky-todo

opencode (~/.config/opencode/opencode.json):
  { "mcp": { "rocky-todo": { "type": "local",
      "command": ["bun", "run", "<rocky-repo>/src/todo/mcp-stdio.ts"] } } }

Codex (~/.codex/config.toml):
  [mcp_servers.rocky-todo]
  command = "bun"
  args = ["run", "<rocky-repo>/src/todo/mcp-stdio.ts"]`;
}
```

그리고 호출부 (현재 line 546) `console.log(mcpSetupGuide(ctx.baseUrl));` → `console.log(mcpSetupGuide());`.

- [ ] **Step 4: 게이트 확인**

Run: `bun run typecheck && bun test src/todo/cli.test.ts`
Expected: PASS

`.claude-plugin/plugin.json` 이 유효 JSON 인지:
Run: `bun -e "JSON.parse(require('fs').readFileSync('.claude-plugin/plugin.json','utf8')); console.log('ok')"`
Expected: `ok`

- [ ] **Step 5: 커밋**

```bash
bun run check
git add .claude-plugin/plugin.json src/todo/cli.ts
git commit -m "feat(todo): 브릿지 plugin.json 등록 + CLI enable 커맨드"
```

---

### Task 8: 문서 + changeset

user-facing 표면(도구 + 등록 방식) 변경을 두 단일 출처와 진입 페이지에 반영하고 changeset 을 남긴다.

**Files:**
- Modify: `AGENTS.md` (Layout + Project in one line)
- Modify: `FEATURES.md` (도구 표 + 등록 방식)
- Modify: `README.md` (surface 카운트)
- Modify: `docs/rocky-todo.md` (호스트별 등록 재작성)
- Modify: `skills/todo/SKILL.md` (도구 게이트 + 가드레일)
- Create: `.changeset/rocky-todo-mcp-bridge.md`

**Interfaces:**
- Consumes: 없음
- Produces: 없음 (문서)

- [ ] **Step 1: AGENTS.md 갱신**

1. Layout 의 `src/todo/` 블록에 신규 파일 추가, `mcp.ts` 제거:
```
    │   ├── mcp-stdio.ts                     stdio MCP 브릿지 — plugin.json 에 선언되는 유일한 MCP 서버, /api/* 로 포워딩 + todo_enable
    │   ├── mcp-tools.ts                     5개 도구 스펙(name/description/zod inputSchema) 단일 출처 — 브릿지가 소비
    │   ├── client.ts                        데몬 REST 클라이언트(request/ensureDaemon/health) — CLI + 브릿지 공유
    │   ├── enable.ts                        활성화 코어 — rocky.json 에 todo.enabled=true 병합 + 데몬 기동 (todo_enable / CLI enable 공유)
```
그리고 기존 `mcp.ts` 라인 삭제, `daemon.ts` 설명에서 `/mcp` 제거.

2. *Project in one line* 의 rocky-todo 문단에서 "데몬의 `/mcp` (streamable HTTP)" 및 "The daemon's MCP endpoint is deliberately NOT declared in plugin.json" 문장을 수정: 데몬은 REST/SSE/웹UI 만(`/mcp` 없음), MCP 는 plugin.json 에 선언된 stdio 브릿지(`mcp-stdio.ts`)가 담당하며 데몬을 온디맨드 health→spawn 한다. 도구는 5→6개(+`todo_enable`, 비활성 시 안내 에러). 데몬 lifecycle 은 여전히 독립.

3. MVP scope 의 rocky-todo "표면은 넷" → "표면은 셋(/,/api/*,/api/events)" + "MCP 는 stdio 브릿지"로 수정. "5 도구: ..." → "6 도구: ... + todo_enable".

- [ ] **Step 2: FEATURES.md 갱신**

rocky-todo 도구 표에 `todo_enable` 행 추가. 호스트별 등록 설명을 "Claude Code 자동 / Codex·opencode stdio 커맨드"로 갱신. 데몬 `/mcp` 언급 제거.

- [ ] **Step 3: README.md 갱신**

surface 카운트에서 rocky-todo 도구 수 5→6 반영 (README 의 해당 문장을 grep 으로 찾아 수정):
Run: `grep -n "todo_list\|rocky-todo\|5 tools\|5개" README.md`

- [ ] **Step 4: docs/rocky-todo.md 갱신**

호스트별 MCP 등록 섹션을 Task 7 Step 3 의 `mcpSetupGuide` 와 동일 내용으로 재작성. 데몬 `/mcp` 를 쓰던 모든 예시 제거. `todo_enable` + 동의 절차 한 문단 추가. CLI 표에 `enable` 추가.

- [ ] **Step 5: skills/todo/SKILL.md 갱신**

"도구 게이트" 섹션의 다음 문장을 교체:
- 기존: `CLI 가 "기본 비활성" 에러를 내면 ... 안내하고 멈춘다 (임의로 켜지 않는다).`
- 신규: 비활성(`todo_enable` 만 보이거나 5개 도구가 `rocky-todo disabled` 안내 에러)일 때는 **사용자에게 노출 범위(상주 데몬 127.0.0.1:8636 / SQLite 저장 / rocky.json 기록)를 설명하고 동의를 받은 뒤** `todo_enable`(또는 CLI `rocky-todo enable`)을 호출한다. 동의 없이는 켜지 않는다.

"가드레일" 섹션에 한 줄 추가: `- 동의 없는 자동 활성화 금지 — todo_enable 은 사용자 동의 후에만.`

- [ ] **Step 6: changeset 작성**

`.changeset/rocky-todo-mcp-bridge.md`:

```markdown
---
"@minjun0219/rocky": minor
---

rocky-todo MCP 를 stdio 브릿지로 전환한다. 데몬의 `/mcp` 를 제거하고 plugin.json 에 선언된 브릿지(`src/todo/mcp-stdio.ts`)가 유일한 MCP 서버가 되어, 데몬 생사와 무관하게 도구가 항상 세션에 존재하고 온디맨드로 데몬을 기동한다. 비활성 상태를 위한 `todo_enable` 도구(동의 후 활성화)를 추가하고, 타 cwd 에서 spawn 될 때 웹UI 에셋 경로가 깨지던 문제를 고친다.
```

- [ ] **Step 7: 최종 게이트 + 커밋**

```bash
bun run check && bun run typecheck && bun test
git add AGENTS.md FEATURES.md README.md docs/rocky-todo.md skills/todo/SKILL.md .changeset/rocky-todo-mcp-bridge.md
git commit -m "docs(todo): MCP 브릿지 전환 문서 반영 + changeset"
```

---

## Self-Review

**1. Spec coverage:**
- ① MCP 항상 존재 → Task 5 (브릿지) + Task 7 (plugin.json 등록). ✓
- ② 에셋 경로 → Task 1. ✓
- ③ 동의 활성화 → Task 4 (enable.ts) + Task 5 (disabledGuidance) + Task 8 Step 5 (스킬). ✓
- 결정 2 (데몬 /mcp 제거) → Task 6. ✓
- 결정 3 (REST + 공유 타입) → Task 2 (client.ts). ✓
- drift 방지 축 ① (mcp-tools) → Task 3. ✓
- 도구→REST 매핑 커버리지 → Task 5 (toolToRest + 테스트). ✓
- actor 보너스 → Task 5 (client 의 x-rocky-actor). ✓
- 문서 체크리스트 → Task 8. ✓

**2. Placeholder scan:** 모든 code step 에 실제 코드/명령/기대출력 포함. TBD/TODO 없음. ✓

**3. Type consistency:**
- `CliContext` — Task 2 정의, Task 4/5 에서 `buildContext` 로 생성. ✓
- `request<T>(ctx, method, path, body?)` — Task 2 정의, Task 5 forward 에서 사용. ✓
- `toolToRest(name, args): { method, path, body? }` — Task 5 정의·테스트·진입점 사용 일치. ✓
- `enableTodo({ port, dir, configPath? })` — Task 4 정의, Task 5/7 호출 일치. ✓
- `TODO_TOOL_SPECS` / `ToolSpec {name, description, inputSchema}` — Task 3 정의, Task 5 소비. ✓
- `mcpSetupGuide()` — Task 7 에서 인자 제거(구 `mcpSetupGuide(baseUrl)`), 호출부도 함께 수정. ✓

이상 없음.
