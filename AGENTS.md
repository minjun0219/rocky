# AGENTS.md

Shared guide for AI coding agents (Claude Code, opencode, codex, etc.) working in this repository.

> **Single sources of truth.** Humans read [`FEATURES.md`](./FEATURES.md) (Korean — tools / config / Quick start). Agents read this file (English — Layout / MVP scope / coding rules / change checklist). [`README.md`](./README.md) is a one-page entry that links to both.

## Project in one line

**rocky** (named after Project Hail Mary's Rocky) — a **single Bun package** with one full-surface stdio MCP server (`src/index.ts`) consumed by multiple hosts, plus a smaller host-agnostic **`openapi-mcp` standalone stdio CLI** (`bin/openapi-mcp` → `src/standalone.ts`, npm). The full-surface server is launched by the **Claude Code plugin** (marketplace; MCP server declared in `.claude-plugin/plugin.json`'s `mcpServers`) and can also be launched directly by **OpenAI Codex CLI** via `~/.codex/config.toml` (`bun run <repo>/src/index.ts`) and **opencode** via the `mcp` section of `opencode.json` (`type:"local"`, `command:["bun","run","<repo>/src/index.ts"]`). `src/core/` holds the shared OpenAPI core. The standalone CLI exposes the **same 7 openapi tools** (`openapi_get` / `openapi_refresh` / `openapi_status` / `openapi_search` / `openapi_envs` / `openapi_endpoint` / `openapi_tags`); the full-surface server additionally exposes `seo_validate` (OG / Twitter Card / JSON-LD / favicon meta validation via `ogpeek`, `src/core/seo-validate.ts`). From v0.5 the full-surface server also adds **four CLI-gated `notion_*` tools** (`notion_get` / `notion_refresh` / `notion_status` / `notion_extract`, `src/core/notion-handlers.ts`) — registered only when the official Notion CLI (`ntn`) is detected at startup; rocky never touches Notion tokens / OAuth itself, all page access is delegated to `ntn pages get` (`src/core/notion-cli.ts`), same policy as the `gh`-based slash commands. From v0.6 the full-surface server also adds **four `worklog_*` tools** (`worklog_append` / `worklog_read` / `worklog_search` / `worklog_status`, `src/core/worklog.ts` + `src/core/worklog-handlers.ts`; renamed from `journal_*` in v0.9) — the **record (기록) layer**: an append-only local JSONL agent worklog. Unlike notion these are **not CLI-gated** (pure filesystem, no external dep) so they are always registered. From v0.9 the **Claude Code plugin's** `Stop` hook (`hooks/hooks.json` → `src/hooks/log-turn.ts`, transcript parsing in `src/hooks/transcript.ts`) deterministically (no LLM) appends a `kind:"turn"` worklog entry after every turn — toggle via `worklog.autoCapture` / env `ROCKY_WORKLOG_AUTO_CAPTURE` (default on), truncated to `worklog.captureMaxChars` (default 800). This turn auto-capture (and its `ROCKY_WORKLOG_AUTO_CAPTURE` toggle) is **Claude Code-only** — Codex and opencode run no hooks — even though the `worklog_*` tools themselves work on all three full-surface hosts. rocky only records / stores; the paired **organize (정리) layer** is the `/recall` slash command (host LLM, replacing the earlier `/curate`), which reads the worklog and incrementally distills an anchor history digest — each digest is appended back into the worklog itself as a `kind:"digest"` entry (linking back to source entry ids) rather than written to an external wiki, so there is no more `wikiDir` and it stays distinct from Claude Code's native memory. Separately, the Claude Code plugin ships **slash commands** in `commands/` (`/finish` — `gh` CLI based; `/recall` — reads `worklog_*` and appends a `kind:"digest"` entry; `/codex` — delegates one task to Codex (`codex exec`) in an isolated worktree and supervises gates / MCP-surface / diff-scope before merging (no auto-merge); `/opencode` — delegates one task to opencode (`opencode run`) in an isolated worktree and supervises gates / MCP-surface / diff-scope before merging (no auto-merge); `/issue` — `gh` CLI based, captures a rocky feature idea / bug raised while working in *another* repo as a GitHub Issue on `minjun0219/rocky` (gathers session context, checks for duplicate open issues, confirms a draft before creating; never auto-creates); `/soul` — lists / sets / previews / scaffolds souls (personas), writing the `soul` key in `rocky.json`; none are MCP tools) and two **skills** in `skills/` — `writing-cc-plugin` (a Claude Code plugin authoring guide + full manifest/component reference, `SKILL.md` + `reference.md`, distilled from the official `/ko/plugins` + `/ko/plugins-reference` docs) and `delegating-to-codex` (the canonical reusable pattern + guardrails for handing a self-contained coding / review / advisory task to a headless OpenAI model via the `codex` CLI — self-contained-prompt rule, `codex exec`/`codex review`/advisory recipes, sandbox+model selection, post-run verification; the `/codex` command is one worktree+supervise+merge application layered on this skill). These slash commands and the skills are Claude Code-only and are not exposed to Codex or opencode; Codex and opencode consume only the MCP tools. PR watching / review handling is delegated to Claude Code's built-in `/autofix-pr` (cloud session + GitHub App webhooks) — the former `/pr-watch` command was removed in v0.8. Also Claude Code-only: **souls** (personas) — markdown files (frontmatter `name`/`description` + persona body) at bundled presets `souls/<name>.md` (`rocky` / `senior` / `terse`) or custom `~/.config/rocky/souls/<name>.md` (same name → custom wins, identity = filename stem); `rocky.json`'s optional `soul` field (`src/core/soul.ts`, project overrides user) names the active one, and a new `SessionStart` hook (`hooks/hooks.json` → `src/hooks/inject-soul.ts`, `matcher: "startup|clear|compact"` so it (re)injects on a fresh/cleared/compacted context but skips `resume`) resolves and injects it as `additionalContext` — fail-open, and a layer over AGENTS.md/CLAUDE.md's gates/safety, never an override; default (no `soul` set) is vanilla, no injection. The optional `callsign` field (single line, non-blank, at most 40 chars, Korean/spaces OK, project overrides user) names what the soul calls the user — the hook appends it to the injected soul context as one directive line that beats the soul body's default form of address; ignored when no `soul` is set. `/soul` switches the active soul (asking for a callsign during setup) and `/soul call` sets/clears the callsign alone. This adds no new MCP tools; `src/index.ts`'s surface and Codex and opencode are unaffected. No workspaces, no `packages/` — one `package.json`, one `tsconfig.json`.
Previous toolkit surfaces (journal / mysql / spec-pact / pr-watch + rocky / grace / mindy agents + 5 skills) live on [`archive/pre-openapi-only-slim`](https://github.com/minjun0219/rocky/tree/archive/pre-openapi-only-slim); the former native opencode plugin is archived in-tree at [`.archive/agent-toolkit-opencode/`](./.archive/agent-toolkit-opencode) (excluded from all gates). That archive was an in-process `@opencode-ai/plugin` surface exposing only the old openapi tools; current opencode support is not a revival of that plugin. opencode now consumes rocky exactly like other hosts: by spawning the host-agnostic stdio MCP server (`src/index.ts`) from `opencode.json`, which exposes the full surface. Domains re-enter in follow-up PRs via one of two shapes (plugin-bound handlers, or a separate CLI entry alongside `openapi-mcp`). The shape is decided per domain at re-introduction time. **notion was the first re-added domain (v0.5, plugin-bound + `ntn` CLI-gated)** — its shape (external CLI delegation, no tokens in rocky) is the template for future auth-bearing domains. **journal was the second (v0.6, plugin-bound, always-on)** — split into a record layer (`journal_*` MCP tools, deterministic storage) and an organize layer (`/curate` slash command, host-LLM distillation into a configured wiki); this record↔organize split is the template for memory-shaped domains that must stay distinct from Claude Code's native memory. **journal was renamed `worklog` in v0.9**, which also added a third piece — a deterministic `Stop` hook that auto-captures every turn (`kind:"turn"`) — while the organize layer moved from an external wiki (`/curate`) to an in-worklog anchor digest (`/recall`, `kind:"digest"`); the record↔organize split itself is unchanged, only the organize target moved from an external file tree into the worklog's own append log.

> **Scope framing (read this before treating a feature request as out-of-scope).** rocky is the owner's **personal Claude Code plugin**, not an OpenAPI-scoped product. The current openapi-only surface is today's **slim baseline, not a ceiling** — the owner grows rocky by adding whatever domains / features they need. So when the owner asks for a domain or feature (notion, journal, a brand-new capability, …), the answer is to **build it** — follow *Reintroduction strategy* for an archived domain, or just add a new one — not to shelve it as "out of scope". The "hold the line" discipline in *MVP scope* only guards against **unrequested** scope creep; it never overrides an explicit owner request.

## Layout

```
rocky/                                      single package — @minjun0219/rocky
├── package.json                            { bin: { "openapi-mcp": "./bin/openapi-mcp" }, exports: { ".", "./standalone" } }
├── tsconfig.json                           단일 컴파일러 옵션 + include ["src/**/*.ts"]
├── biome.json                              lint / format (!.sisyphus, !.claude 제외)
├── rocky.schema.json                       `rocky.json` JSON Schema (IDE autocomplete)
├── .claude-plugin/marketplace.json         ★ 이 레포를 그대로 설치 가능한 마켓플레이스로 (name rocky-marketplace, plugin rocky @ source "./")
├── .claude-plugin/plugin.json              ★ plugin metadata + mcpServers (via ${CLAUDE_PLUGIN_ROOT}/src/index.ts)
├── README.md / FEATURES.md / AGENTS.md / REVIEW.md / LICENSE
├── docs/openapi-mcp.md                     standalone CLI 보조 문서
├── docs/codex.md                           Codex CLI 에서 full-surface `src/index.ts` 를 MCP 서버로 등록하는 보조 문서
├── docs/opencode.md                        opencode CLI 에서 full-surface `src/index.ts` 를 MCP 서버로 등록하는 보조 문서
├── docs/backlog.md                         백로그 — 보류 항목 + 도메인 재추가 후보 + 비전 메모 (구 ROADMAP.md 의 live 항목 이관)
├── commands/                               ★ Claude Code plugin 슬래시 커맨드 (자동 발견, gh CLI 기반, MCP tool surface 와 별개)
│   ├── finish.md                           `/finish` — 게이트 → 커밋 → 푸시 → PR 생성 (PR 감시는 빌트인 /autofix-pr 위임)
│   ├── recall.md                           `/recall` — worklog_* 를 읽어 앵커 히스토리 다이제스트(kind:"digest")로 증분 요약 (정리 레이어, gh 불필요, v0.9 에서 curate.md 대체)
│   ├── codex.md                            `/codex` — task 를 Codex(codex exec)에 위임(격리 worktree) + Claude 감시(게이트/표면/스코프), 자동 병합 X
│   ├── opencode.md                         `/opencode` — task 를 opencode(opencode run)에 위임(격리 worktree) + Claude 감시(게이트/표면/스코프), 자동 병합 X
│   ├── issue.md                            `/issue` — 다른 레포에서 떠오른 rocky 개선 아이디어/버그를 minjun0219/rocky GitHub Issue 로 캡처 (gh CLI 기반, 맥락 수집 + 유사 이슈 조회 + 초안 확인 후 생성)
│   └── soul.md                             `/rocky:soul` — 소울(페르소나) 선택 + 호칭 설정 (list/set/call/show/new), MCP tool 아님
├── hooks/                                  ★ Claude Code plugin 훅 (자동 발견, MCP tool surface 와 별개)
│   └── hooks.json                          `SessionStart` (matcher `startup|clear|compact`) → `bun run ${CLAUDE_PLUGIN_ROOT}/src/hooks/inject-soul.ts` (소울 자동 주입) + `Stop` → `bun run ${CLAUDE_PLUGIN_ROOT}/src/hooks/log-turn.ts` (v0.9, 턴 자동 기록)
├── skills/                                 ★ Claude Code plugin 번들 스킬 (기본 위치 자동 스캔, 플러그인 전용, MCP tool 아님)
│   ├── writing-cc-plugin/                  `/rocky:writing-cc-plugin` — CC 플러그인 작성 가이드 + 레퍼런스
│   │   ├── SKILL.md                        작성 워크플로우 + gotcha 표 + quick reference
│   │   └── reference.md                    전체 스펙 §1–9 + 작성·배포 워크플로우 §10 (/ko/plugins[-reference] 증류)
│   └── delegating-to-codex/                `/rocky:delegating-to-codex` — 자기완결 task 를 headless OpenAI 모델(codex CLI)에 위임하는 패턴 + 가드레일
│       └── SKILL.md                        자기완결 프롬프트 원칙 + codex exec/review/자문 레시피 + 샌드박스/모델 선택 + 가드레일 + 실행 후 검증 (/codex 커맨드가 얹히는 메커니즘 레이어)
├── souls/                                  ★ 번들 프리셋 소울 (rocky / senior / terse), 커스텀은 ~/.config/rocky/souls/
│   ├── rocky.md                            frontmatter(name/description) + 페르소나 본문
│   ├── senior.md                           〃
│   └── terse.md                            〃
├── bin/
│   └── openapi-mcp                         `#!/usr/bin/env bun` shebang, arg 파싱 → src/standalone
└── src/
    ├── index.ts                            ★ Claude Code plugin 진입점 — MCP 등록만 (7 openapi + seo_validate + 4 worklog_*, + CLI-gated notion_*), handler 호출은 ./core
    ├── index.test.ts                       in-memory MCP smoke (base 12 tool: 7 openapi + seo_validate + 4 worklog_*, +4 notion_* when ntn detected — 누수 가드)
    ├── standalone.ts                       standalone stdio MCP 로직 — 7 tool 등록 + `SpecRegistry` (seo_validate / worklog_* / notion_* 미포함)
    ├── hooks/                              ★ Claude Code plugin hook 구현 (hooks/hooks.json 이 참조, MCP tool 아님)
    │   ├── inject-soul.ts                  SessionStart hook entry — resolveSoulName → readSoul → buildSoulContext → additionalContext (fail-open)
    │   ├── log-turn.ts                     Stop hook entry — shouldCapture(env/config) → extractTurn → worklog.append({ kind: "turn" }) (v0.9)
    │   └── transcript.ts                   트랜스크립트 순수 파서 — extractTurn / buildTurnContent (req/did 추출 + captureMaxChars truncate)
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
        ├── rocky-config.ts                 `rocky.json` 로더 (project > user, openapi + seo + worklog)
        ├── seo-validate.ts                 `seo_validate` 코어 + handler (ogpeek, SSRF 가드) — plugin 전용
        ├── soul.ts                         소울(페르소나) 코어 — list/read/resolve souls + 자체 frontmatter 파서 + buildSoulContext (순수/DI, hook + /rocky:soul 이 소비)
        ├── url.ts                          URL join / synthetic operationId
        ├── worklog.ts                      append-only JSONL 워크로그(기록 레이어) — Worklog + 프로젝트별 dir + lastDigestAt(status 노출) + createWorklogFromEnv (plugin, 항상 등록; v0.9 에서 journal.ts 개명 + wikiDir 제거)
        ├── worklog-handlers.ts             worklog_* 4 handler (append / read / search / status) — 진입점은 등록만 (v0.9 에서 journal-handlers.ts 개명)
        ├── __fixtures__/                   petstore 2.0 / 3.0 (JSON + YAML)
        └── *.test.ts                       unit tests + handlers.test.ts
```

**Import 규칙**: 플러그인 진입점(`src/index.ts`)은 barrel `./core` 를, standalone(`src/standalone.ts`)·bin 은 `./core/<file>` subpath 를 상대경로로 import 한다. 더 이상 `@minjun0219/openapi-core` workspace 이름을 쓰지 않는다.

## MVP scope (hold the line)

**In**: OpenAPI / Swagger spec 캐시 + endpoint search + tag list + cross-spec scoped search + `host:env:spec` registry (`rocky.json`, project > user precedence), 공유 7 openapi tool 을 full-surface server (`src/index.ts`, consumed by Claude Code plugin + Codex CLI + opencode) 와 standalone CLI (`src/standalone.ts`) 에 공유, 단일 Bun 패키지. 모든 openapi handler 는 `src/core/handlers.ts` 한 곳에 정의 — 두 진입점 간 drift 방지. 추가로 **`seo_validate`** (단일 URL 의 OG / Twitter Card / JSON-LD / favicon 메타를 `ogpeek` 으로 검증, 기본 SSRF 가드 — IP literal 기준 private/loopback 차단, `rocky.json` 의 `seo.allowPrivateHosts` / `seo.timeoutMs` 기본값, 도구 인자 우선) 는 full-surface server 에만 등록 (handler `src/core/seo-validate.ts`, standalone CLI 미포함). 그리고 **`notion_*` 4 도구** (`notion_get` / `notion_refresh` / `notion_status` / `notion_extract`) 는 full-surface server 에만, 그리고 **공식 Notion CLI (`ntn`) 가 기동 시 탐지될 때만** 등록 — 페이지 접근은 전부 `ntn pages get <id> --json` 위임 (rocky 는 토큰 / OAuth 를 직접 다루지 않음), 캐시는 `ROCKY_NOTION_CACHE_DIR` 아래 page 당 `.json` + `.md` 두 파일, `notion_refresh` 는 기존 캐시 대비 heading-section diff (자체 LCS, 외부 dep 0) 를 함께 반환. `buildServer({ notionCli })` 로 executor 를 주입해 테스트에서 fake 로 대체. 그리고 **`worklog_*` 4 도구** (`worklog_append` / `worklog_read` / `worklog_search` / `worklog_status`, v0.9 에서 `journal_*` 개명) 는 full-surface server 에만, **CLI-gate 없이 항상** 등록 — append-only 로컬 JSONL 기록 레이어 (`src/core/worklog.ts`, 외부 dep 0, `resolveCacheKey` 재사용). 저장은 프로젝트별 (`~/.config/rocky/worklog/<project-key>/worklog.jsonl`, `ROCKY_WORKLOG_DIR` / `rocky.json` 의 `worklog.dir` 로 변경). v0.9 부터 `Stop` hook (`hooks/hooks.json` → `src/hooks/log-turn.ts`) 이 매 턴 종료 시 결정론적으로(LLM 미사용) `kind:"turn"` 항목을 자동 append 한다 — 토글은 `worklog.autoCapture` / env `ROCKY_WORKLOG_AUTO_CAPTURE` (기본 on), truncate 길이는 `worklog.captureMaxChars` (기본 800). 짝이 되는 정리(整理) 레이어는 rocky 도구가 아니라 **`/recall` 슬래시 커맨드** (v0.9 에서 구 `/curate` 대체) — 워크로그를 읽어 마지막 digest watermark(`lastDigestAt`) 이후 항목만 증분 요약하고, 새 항목 수(`worklog.digestThreshold`, 기본 40) 에 따라 Haiku/Sonnet 서브에이전트를 골라 앵커 히스토리 다이제스트를 만들어 `kind:"digest"` 한 줄로 append 한다 — 더 이상 외부 wiki 는 없다, 다이제스트는 워크로그 자체 안에 산다. rocky 는 기록·저장만, 증류(LLM)는 호스트 몫이라 네이티브 메모리와 역할이 겹치지 않는다. `buildServer({ worklog })` 로 tmpdir 워크로그 주입 가능.

**Out**: mysql / spec-pact / pr-watch / agents / skills (전부 archive 브랜치 박제), the old native opencode plugin (`.archive/agent-toolkit-opencode/` 박제; distinct from current opencode stdio MCP host support), 나머지 도메인 재추가 (별도 PR), npm publish 자동화 (별도 PR). notion 은 재추가 완료 (v0.5), journal 은 재추가 완료 (v0.6, v0.9 에서 `worklog` 로 개명). **worklog 정리 결과를 rocky MCP 도구(`wiki_*`)로 노출 · worklog 를 standalone CLI 에 추가 · 네이티브 메모리로의 자동 승격/동기화 · 폴링 기반 자동 digest** 는 out — 기록은 `worklog_*` 도구 (+ `Stop` hook 자동 turn 캡처), 정리는 `/recall` 슬래시 커맨드(워크로그 내부 `kind:"digest"` entry)만. **Notion DB / child-page 재귀 · YAML frontmatter 파싱 · `ntn` 외 다른 Notion 접근 경로 (직접 API / MCP)** 는 out — 단일 페이지 캐시 + `ntn` 위임만. OpenAPI YAML stream parsing, full SDK code generation, multi-spec merge, mock servers, UI 도 모두 out.

## Reintroduction strategy (archive → main)

Re-adding a domain (mysql / spec-pact / pr-watch — notion already re-added in v0.5, journal already re-added in v0.6 and renamed `worklog` in v0.9, both reference shapes) is **always a separate PR** that follows this template:

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
bunx changeset      # user-facing 변경의 버전 의도 선언 (patch/minor/major)
bun run changeset:version  # (로컬 수동 시) 버전 범프 + CHANGELOG + plugin.json sync
```

**Release (changesets).** user-facing 변경이 있는 PR 은 `bunx changeset` 으로 의도를 선언한다(`.changeset/*.md` 커밋). main 병합 시 `.github/workflows/release.yml` 의 `changesets/action` 이 pending changeset 을 모아 "Version Packages" PR 을 자동으로 열고, 그 PR 이 `package.json` + `.claude-plugin/plugin.json` 범프와 `CHANGELOG.md` 갱신을 담는다 (버전 sync 는 `bun run changeset:version` 안의 `scripts/sync-plugin-version.ts` 가 처리 — changesets 는 `package.json` 만 범프하므로). 그 Version PR 을 병합해 main 버전이 오르면, 같은 워크플로의 독립 스텝(`scripts/release-github.ts`)이 `v<version>` 태그와 GitHub Release(노트=`CHANGELOG.md` 해당 섹션)를 멱등하게 생성한다. npm publish 는 자동화 대상이 아니다 (GitHub Release ≠ npm publish).

**Git hooks (husky).** `bun install` 시 `prepare: "husky"` 가 `core.hooksPath` 를 `.husky/_` 로 배선한다 (clone 받은 기여자도 자동 활성화). `.husky/pre-commit` 은 staged 파일에 `lint-staged`(biome check/format) + 시크릿 스캔(`gitleaks` 있으면 `gitleaks protect --staged`, 없으면 내장 grep fallback)을, `.husky/pre-push` 는 `typecheck` + `test` 를 돌린다. 긴급 우회는 `git commit --no-verify` / `git push --no-verify`. CI(`.github/workflows/ci.yml`)는 같은 게이트 + `gitleaks` 잡을 PR·main push 마다 재실행한다. `.husky/_` 는 husky 자체 `.gitignore` 로 커밋 제외 — 추적 대상은 `.husky/pre-commit` / `.husky/pre-push` 두 파일뿐.

## Coding rules

- **Language**: TypeScript (`type: module`). Bun runs `.ts` directly — no build, no `dist/`.
- **Imports**: do not append `.js` / `.ts` extensions (`moduleResolution: Bundler` + `allowImportingTsExtensions`). 모두 상대경로 — 플러그인 진입점은 barrel `./core`, standalone·bin 은 `./core/<file>` subpath. (`@modelcontextprotocol/sdk/...js` 처럼 외부 패키지가 요구하는 `.js` subpath 는 그대로 둔다.)
- **ESM safety**: never use `__dirname`. Use `import.meta.url` + `fileURLToPath`, or Bun's `import.meta.dir`.
- **Repo-local JSDoc**: write JSDoc on exported functions / classes when touching this repository, but do not treat it as a custom hard-lint gate. Korean comments are fine for tricky logic.
- **Errors**: include context in messages (input value, timeout, status code, handle mismatch, …).
- **Dependencies**: avoid adding any if possible. Prefer the standard library and Bun built-ins. **Explicit prod-dep exceptions:** `@modelcontextprotocol/sdk` + `zod` (MCP wire protocol + blessed schema dialect), `@apidevtools/swagger-parser` + `swagger2openapi` + `js-yaml` + `openapi-types` + `pino` (OpenAPI parsing / conversion / structured stderr logging), `ogpeek` (`seo_validate` 의 HTML fetch + OG / Twitter Card / JSON-LD / favicon 파싱 — 손으로 유지할 표면이 아님). HTTP transport는 Bun의 native `fetch` (with `tls` option) 직접 사용. Dev-only tooling (linters / formatters / git-hook 러너) 는 OK — 현재 devDep 예외로 `husky` + `lint-staged` (pre-commit/pre-push 게이트, 런타임 미포함). New runtime deps 는 별도 scope 논의. (`@opencode-ai/plugin` 은 아카이브된 네이티브 opencode 플러그인과 함께 제거됨 — 현재 opencode 지원은 stdio MCP 등록 방식이라 이 의존성을 다시 추가하지 않는다.)
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
8. If a removed-domain tool name needs to surface again, update the `REMOVED_TOOLS` array in `src/index.test.ts` — it currently guards against mysql / spec-pact / pr-watch leakage (journal was un-removed in v0.6, renamed `worklog` in v0.9; its always-on presence is asserted via `WORKLOG_TOOLS`).
9. If the change is user-facing (tools / commands / hooks / config surface), declare the version intent with `bunx changeset` (patch / minor / major) so the release workflow can bump on merge. Tooling-only chores need no changeset.

## Plugin source & dev loop

**This repo IS the plugin source AND its own marketplace — there is no separate façade directory.** `.claude-plugin/marketplace.json` (name `rocky-marketplace`) is the single marketplace, and the plugin `source` is the relative `"./"` (the repo itself). Known limitation: the claude.ai web UI's server-side marketplace sync does not clone the repo, so a relative source fails there ("marketplace sync failed") — accepted trade-off; install via CLI (`claude plugin marketplace add minjun0219/rocky`, or the `/plugin` slash command in-session). `.claude-plugin/plugin.json`'s `mcpServers` (`${CLAUDE_PLUGIN_ROOT}/src/index.ts`) is the **only** MCP server the plugin ships.

Install for personal use (once):

```bash
claude plugin marketplace add minjun0219/rocky
claude plugin install rocky@rocky-marketplace
```

Installs clone the repo from GitHub `main` into the plugin cache — the plugin is **not** read in place from a working tree. The dev loop is therefore push-based: edit → push to `main` → `claude plugin update rocky` (a fresh session picks up updates too). `/reload-plugins` does not see uncommitted working-tree edits.

**Why there is no `.mcp.json` in this repo:** the installed plugin root is a clone of this repo root, so any repo-root `.mcp.json` would leak into the *installed* plugin's MCP config (on top of `plugin.json`'s `mcpServers`). `context7` (external-library docs, handy while developing here) therefore lives at **user scope** instead of a repo `.mcp.json`:

```bash
claude mcp add --scope user --transport http context7 https://mcp.context7.com/mcp
```

## Output / communication

- Default conversation language with the user is Korean. Keep code identifiers / paths / commands in English.
- Keep change summaries short (one-line summary, bullets only when needed). Do not produce long-form reports.
- Write code review outputs (summary / inline / suggestions) in Korean by default.
- When requesting a PR review, explicitly ask for Korean review comments (`모든 리뷰 코멘트는 한국어로 작성해 주세요.`).
- PR and commit titles must follow Conventional Commits style (`type(scope): Korean summary` or `type: Korean summary`). Do not pad the title — no enumerations, qualifiers, or parenthetical asides beyond the one main change (summary part must not exceed roughly 50 chars); displaced detail goes in the body. Do not thin the body to compensate for a shorter title.
- PR title / body and user-facing change descriptions should also be written in Korean.
- **Single sources**: humans = `FEATURES.md` (Korean), agents = this `AGENTS.md` (English). Do not introduce a new sibling doc — fold new content into one of the two.
