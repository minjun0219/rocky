# AGENTS.md

Shared guide for AI coding agents (Claude Code, opencode, codex, etc.) working in this repository.

> **Single sources of truth.** Humans read [`FEATURES.md`](./FEATURES.md) (Korean — tools / config / Quick start). Agents read this file (English — Layout / MVP scope / coding rules / change checklist). [`README.md`](./README.md) is a one-page entry that links to both.

## Project in one line

**rocky** (named after Project Hail Mary's Rocky) — a **single Bun package** whose `src/core/` OpenAPI core backs two distribution targets — a **Claude Code plugin** (marketplace; MCP server declared in `.claude-plugin/plugin.json`'s `mcpServers`, entry `src/index.ts`) and a host-agnostic **`openapi-mcp` standalone stdio CLI** (`bin/openapi-mcp` → `src/standalone.ts`, npm). Both expose the **same 7 openapi tools** (`openapi_get` / `openapi_refresh` / `openapi_status` / `openapi_search` / `openapi_envs` / `openapi_endpoint` / `openapi_tags`); the Claude Code plugin adds one plugin-only tool, `seo_validate` (OG / Twitter Card / JSON-LD / favicon meta validation via `ogpeek`, `src/core/seo-validate.ts`). From v0.5 the Claude Code plugin also adds **four CLI-gated `notion_*` tools** (`notion_get` / `notion_refresh` / `notion_status` / `notion_extract`, `src/core/notion-handlers.ts`) — registered only when the official Notion CLI (`ntn`) is detected at startup; rocky never touches Notion tokens / OAuth itself, all page access is delegated to `ntn pages get` (`src/core/notion-cli.ts`), same policy as the `gh`-based slash commands. Separately, the Claude Code plugin ships **slash commands** in `commands/` (`/finish`, `/pr-watch` — `gh` CLI based, not MCP tools). No workspaces, no `packages/` — one `package.json`, one `tsconfig.json`.

Previous toolkit surfaces (journal / mysql / spec-pact / pr-watch + rocky / grace / mindy agents + 5 skills) live on [`archive/pre-openapi-only-slim`](https://github.com/minjun0219/rocky/tree/archive/pre-openapi-only-slim); the former opencode plugin is archived in-tree at [`.archive/agent-toolkit-opencode/`](./.archive/agent-toolkit-opencode) (excluded from all gates). Domains re-enter in follow-up PRs via one of two shapes (plugin-bound handlers, or a separate CLI entry alongside `openapi-mcp`). The shape is decided per domain at re-introduction time. **notion was the first re-added domain (v0.5, plugin-bound + `ntn` CLI-gated)** — its shape (external CLI delegation, no tokens in rocky) is the template for future auth-bearing domains.

> **Scope framing (read this before treating a feature request as out-of-scope).** rocky is the owner's **personal Claude Code plugin**, not an OpenAPI-scoped product. The current openapi-only surface is today's **slim baseline, not a ceiling** — the owner grows rocky by adding whatever domains / features they need. So when the owner asks for a domain or feature (notion, journal, a brand-new capability, …), the answer is to **build it** — follow *Reintroduction strategy* for an archived domain, or just add a new one — not to shelve it as "out of scope". The "hold the line" discipline in *MVP scope* only guards against **unrequested** scope creep; it never overrides an explicit owner request.

## Layout

```
rocky/                                      single package — @minjun0219/rocky
├── package.json                            { bin: { "openapi-mcp": "./bin/openapi-mcp" }, exports: { ".", "./standalone" } }
├── tsconfig.json                           단일 컴파일러 옵션 + include ["src/**/*.ts"]
├── biome.json                              lint / format (!.sisyphus, !.claude 제외)
├── rocky.schema.json                       `rocky.json` JSON Schema (IDE autocomplete)
├── .mcp.json                               ★ dev-only Claude Code trust (context7 + rocky via ${CLAUDE_PROJECT_DIR}). 배포 X.
├── .claude-plugin/plugin.json              ★ marketplace metadata + mcpServers (via ${CLAUDE_PLUGIN_ROOT}/src/index.ts)
├── README.md / FEATURES.md / AGENTS.md / ROADMAP.md / REVIEW.md / LICENSE
├── docs/openapi-mcp.md                     standalone CLI 보조 문서
├── commands/                               ★ Claude Code plugin 슬래시 커맨드 (자동 발견, gh CLI 기반, MCP tool surface 와 별개)
│   ├── finish.md                           `/finish` — 게이트 → 커밋 → 푸시 → PR 생성
│   └── pr-watch.md                         `/pr-watch` — 열린 PR 을 머지 가능 상태까지 감시·알림 (자동 머지 X)
├── bin/
│   └── openapi-mcp                         `#!/usr/bin/env bun` shebang, arg 파싱 → src/standalone
└── src/
    ├── index.ts                            ★ Claude Code plugin 진입점 — MCP 등록만 (7 openapi + seo_validate), handler 호출은 ./core
    ├── index.test.ts                       in-memory MCP smoke (8 tool, 누수 가드)
    ├── standalone.ts                       standalone stdio MCP 로직 — 7 tool 등록 + `SpecRegistry` (seo_validate 미포함)
    └── core/                               ← 구 @minjun0219/openapi-core
        ├── index.ts                        barrel (플러그인이 `./core` 로 소비)
        ├── adapter.ts                      `rocky.json` registry → SpecRegistry + 핸들 평탄화
        ├── cache.ts                        sha1-keyed disk cache (`schemaVersion: 1`, TTL, conditional GET)
        ├── config-loader.ts                standalone `openapi-mcp.json` 로더 (XDG, YAML/JSON)
        ├── fetcher.ts                      Bun `fetch` 기반 HTTP + conditional GET + TLS opt
        ├── filter.ts                       점수화 검색 (operationId>path>summary>description)
        ├── handlers.ts                     ★ 7 plugin handler 공유 (`handleSwagger*`)
        ├── indexer.ts                      IndexedSpec / EndpointDetail / TagSummary
        ├── logger.ts                       pino → stderr only
        ├── notion-cache.ts                 Notion 페이지 TTL 파일 캐시 + resolveCacheKey + notionToMarkdown (plugin, CLI-gated)
        ├── notion-cli.ts                   `ntn` CLI 위임 (fakeable NotionCliExecutor + detect + `pages get --json` 파서)
        ├── notion-chunking.ts              긴 페이지 heading chunk + 규칙 기반 액션 추출
        ├── notion-diff.ts                  refresh 시 heading-section 단위 markdown diff (자체 LCS, dep 0)
        ├── notion-handlers.ts              notion_* 4 handler (get / refresh / status / extract) — 진입점은 등록만
        ├── openapi-registry.ts             host:env:spec 핸들 / 스코프 / 평탄화
        ├── parser.ts                       yaml + swagger2→3 + `$ref` deref (swagger-parser)
        ├── registry.ts                     메모리 + 디스크 registry + TTL + 백그라운드 revalidate
        ├── schema.ts                       `openapi-mcp.json` zod schema
        ├── rocky-config.ts                 `rocky.json` 로더 (project > user, openapi + seo)
        ├── seo-validate.ts                 `seo_validate` 코어 + handler (ogpeek, SSRF 가드) — plugin 전용
        ├── url.ts                          URL join / synthetic operationId
        ├── __fixtures__/                   petstore 2.0 / 3.0 (JSON + YAML)
        └── *.test.ts                       unit tests + handlers.test.ts
```

**Import 규칙**: 플러그인 진입점(`src/index.ts`)은 barrel `./core` 를, standalone(`src/standalone.ts`)·bin 은 `./core/<file>` subpath 를 상대경로로 import 한다. 더 이상 `@minjun0219/openapi-core` workspace 이름을 쓰지 않는다.

## MVP scope (hold the line)

**In**: OpenAPI / Swagger spec 캐시 + endpoint search + tag list + cross-spec scoped search + `host:env:spec` registry (`rocky.json`, project > user precedence), 공유 7 openapi tool 을 2 배포 타깃 (Claude Code plugin + standalone CLI) 에 공유, 단일 Bun 패키지. 모든 openapi handler 는 `src/core/handlers.ts` 한 곳에 정의 — 두 진입점 간 drift 방지. 추가로 **`seo_validate`** (단일 URL 의 OG / Twitter Card / JSON-LD / favicon 메타를 `ogpeek` 으로 검증, 기본 SSRF 가드 — IP literal 기준 private/loopback 차단, `rocky.json` 의 `seo.allowPrivateHosts` / `seo.timeoutMs` 기본값, 도구 인자 우선) 는 Claude Code plugin 에만 등록 (handler `src/core/seo-validate.ts`, standalone CLI 미포함). 그리고 **`notion_*` 4 도구** (`notion_get` / `notion_refresh` / `notion_status` / `notion_extract`) 는 Claude Code plugin 에만, 그리고 **공식 Notion CLI (`ntn`) 가 기동 시 탐지될 때만** 등록 — 페이지 접근은 전부 `ntn pages get <id> --json` 위임 (rocky 는 토큰 / OAuth 를 직접 다루지 않음), 캐시는 `ROCKY_NOTION_CACHE_DIR` 아래 page 당 `.json` + `.md` 두 파일, `notion_refresh` 는 기존 캐시 대비 heading-section diff (자체 LCS, 외부 dep 0) 를 함께 반환. `buildServer({ notionCli })` 로 executor 를 주입해 테스트에서 fake 로 대체.

**Out**: journal / mysql / spec-pact / pr-watch / agents / skills (전부 archive 브랜치 박제), opencode plugin (`.archive/agent-toolkit-opencode/` 박제), 나머지 도메인 재추가 (별도 PR), npm publish 자동화 (별도 PR). notion 은 재추가 완료 (v0.5). **Notion DB / child-page 재귀 · YAML frontmatter 파싱 · `ntn` 외 다른 Notion 접근 경로 (직접 API / MCP)** 는 out — 단일 페이지 캐시 + `ntn` 위임만. OpenAPI YAML stream parsing, full SDK code generation, multi-spec merge, mock servers, UI 도 모두 out.

## Reintroduction strategy (archive → main)

Re-adding a domain (journal / mysql / spec-pact / pr-watch — notion already re-added in v0.5 as the reference shape) is **always a separate PR** that follows this template:

1. **Decision**: 도메인을 (a) Claude Code plugin 에 직접 합류 (`src/core/` 에 코드 + `src/index.ts` 에 tool 등록) (b) `openapi-mcp` 옆에 별도 CLI 진입점 (`bin/<domain>-mcp` + `src/<domain>.ts`, host 독립성이 높을 때) 둘 중 하나로 정한다. 결정 기록은 PR description 에 한 줄.
2. **Port from archive**: `git checkout archive/pre-openapi-only-slim -- <files>` 로 lib / skill / agent 가져온다. 이전 `lib/<domain>.ts` 는 `src/core/<domain>.ts` 로 옮긴다.
3. **Shared handler 자리**: `src/core/handlers.ts` 옆에 도메인 handler 를 둔다 (openapi 와 동일 패턴) — 진입점은 등록만.
4. **Config shape**: `rocky.json` 에 도메인 키를 다시 넣는다면 `src/core/rocky-config.ts` 의 `RockyConfig` 와 `rocky.schema.json` 을 lockstep 으로 갱신.
5. **Surface**: 도메인이 plugin 에 합류하면 `src/index.ts` 에서 도구를 등록한다 — 누수 회귀 가드 (`src/index.test.ts` 의 `REMOVED_TOOLS` 배열) 도 함께 갱신.
6. **Docs**: `FEATURES.md` 의 tool 표 / config 표 갱신. `README.md` 의 surface 카운트 갱신. `AGENTS.md` (이 파일) 의 Layout / *Project in one line* 갱신.

## Common commands

```bash
bun install         # 의존성 설치
bun run check       # Biome verify (no write)
bun run fix         # Biome safe fix + format
bun run lint        # Biome lint only
bun run lint:fix    # Biome lint write
bun run format      # Biome format only
bun run typecheck   # tsc --noEmit
bun test            # 모든 src/**/*.test.ts
```

## Coding rules

- **Language**: TypeScript (`type: module`). Bun runs `.ts` directly — no build, no `dist/`.
- **Imports**: do not append `.js` / `.ts` extensions (`moduleResolution: Bundler` + `allowImportingTsExtensions`). 모두 상대경로 — 플러그인 진입점은 barrel `./core`, standalone·bin 은 `./core/<file>` subpath. (`@modelcontextprotocol/sdk/...js` 처럼 외부 패키지가 요구하는 `.js` subpath 는 그대로 둔다.)
- **ESM safety**: never use `__dirname`. Use `import.meta.url` + `fileURLToPath`, or Bun's `import.meta.dir`.
- **Repo-local JSDoc**: write JSDoc on exported functions / classes when touching this repository, but do not treat it as a custom hard-lint gate. Korean comments are fine for tricky logic.
- **Errors**: include context in messages (input value, timeout, status code, handle mismatch, …).
- **Dependencies**: avoid adding any if possible. Prefer the standard library and Bun built-ins. **Explicit prod-dep exceptions:** `@modelcontextprotocol/sdk` + `zod` (MCP wire protocol + blessed schema dialect), `@apidevtools/swagger-parser` + `swagger2openapi` + `js-yaml` + `openapi-types` + `pino` (OpenAPI parsing / conversion / structured stderr logging), `ogpeek` (`seo_validate` 의 HTML fetch + OG / Twitter Card / JSON-LD / favicon 파싱 — 손으로 유지할 표면이 아님). HTTP transport는 Bun의 native `fetch` (with `tls` option) 직접 사용. Dev-only tooling (linters / formatters) 는 OK. New runtime deps 는 별도 scope 논의. (`@opencode-ai/plugin` 은 opencode plugin 아카이브와 함께 제거됨.)
- **Tests**: keep `*.test.ts` next to the source and run with `bun test`. Isolate fs-dependent tests with `mkdtempSync`. 핸들러 동작 자체는 `src/core/handlers.test.ts` 에서 검증, 플러그인 진입점의 `src/index.test.ts` 는 surface (tool 개수 / 누수 회귀) 만 검증.

## Runtime project comment guidance

When this toolkit is used against a runtime / downstream project, JSDoc and Korean comments are **agent guidance**, not a lint contract.

- Add JSDoc for important public / shared methods, code with domain rules or edge cases, contracts that another agent / caller must understand, or when the user / reviewer explicitly asks for explanation.
- Skip JSDoc for private helpers, obvious one-file glue code, local callbacks, and test fixtures when names and types already explain the behavior.
- Prefer Korean for explanatory prose comments. Keep code identifiers, file paths, commands, URLs, API paths, and library / framework names in their original English form.
- Never generate a runtime project lint config solely to enforce JSDoc or Korean-comment policy unless the user explicitly asks for that project's lint setup.

## Change checklist

1. `bun run check` passes
2. `bun run typecheck` passes
3. `bun test` passes
4. If the user-facing surface (tools / env vars / handles) changes, sync the **two single sources** — `FEATURES.md` (Korean, humans) and `AGENTS.md` (English, agents — this file's *Project in one line* + *Layout*) — and the entry pages: `README.md` always, `docs/openapi-mcp.md` when the standalone CLI surface changes, `.claude-plugin/plugin.json` when the Claude Code surface (tools or MCP server declaration) changes.
5. If a new env var is added, update the env-reading site that consumes it (`src/core/cache.ts` / `config-loader.ts` / `rocky-config.ts`). Update the `FEATURES.md` env-var table on every addition.
6. If a tool contract changes, update the registration in `src/index.ts` (plugin) and/or `src/standalone.ts` (CLI) as appropriate, and the shared handler in `src/core/handlers.ts` (implementation).
7. If `rocky.json` shape changes, update **both** `rocky.schema.json` (IDE autocomplete) **and** `src/core/rocky-config.ts` (runtime validation) — they must stay in lockstep.
8. If a removed-domain tool name needs to surface again, update the `REMOVED_TOOLS` array in `src/index.test.ts` — it currently guards against journal / mysql / notion / spec-pact / pr-watch leakage.

## MCP servers

The **dev-only** repo-root `.mcp.json` (project scope; `${CLAUDE_PROJECT_DIR}` expands, `${CLAUDE_PLUGIN_ROOT}` does **not**) registers two MCP servers for working against this repo. Approve both on first trust prompt:

- [`context7`](https://github.com/upstash/context7) — up-to-date documentation for external libraries.
- `rocky` — `bun run ${CLAUDE_PROJECT_DIR}/src/index.ts`. Exposes the 7-tool plugin surface.

End users install the plugin via marketplace; the server they get is declared in `.claude-plugin/plugin.json`'s `mcpServers` (`${CLAUDE_PLUGIN_ROOT}/src/index.ts`), **not** the repo-root `.mcp.json` — so the dev-only `context7` entry never leaks to installs. The `.mcp.json` file is not part of the published `files`.

## Output / communication

- Default conversation language with the user is Korean. Keep code identifiers / paths / commands in English.
- Keep change summaries short (one-line summary, bullets only when needed). Do not produce long-form reports.
- Write code review outputs (summary / inline / suggestions) in Korean by default.
- When requesting a PR review, explicitly ask for Korean review comments (`모든 리뷰 코멘트는 한국어로 작성해 주세요.`).
- PR titles must follow Conventional Commits style (`type(scope): Korean summary` or `type: Korean summary`).
- PR title / body and user-facing change descriptions should also be written in Korean.
- **Single sources**: humans = `FEATURES.md` (Korean), agents = this `AGENTS.md` (English). Do not introduce a new sibling doc — fold new content into one of the two.
