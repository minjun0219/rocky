# AGENTS.md

Guide for AI coding agents (Claude Code, opencode, codex) working in **this repository**.

> **Where things live.** Humans read [`README.md`](./README.md) (Korean — surface, config, env vars,
> quick start). Agents read this file (English — layout, scope, rules, checklist, review bar). Design
> rationale that isn't derivable from the code lives in [`docs/architecture.md`](./docs/architecture.md)
> — read it on demand, not by default. **Per-tool input/output is not documented in prose** — the tool
> definitions in `src/index.ts` / `src/standalone.ts` are the single source; read them directly.
> Cross-project conventions (language, commit style, comment policy) live in the user-scope `AGENTS.md`.

## What rocky is

**rocky** (named after Project Hail Mary's Rocky) — the owner's personal Claude Code plugin, shipped as
a **single Bun package** (no workspaces, no `packages/`, no build step).

Two entry points:

- **Full-surface stdio MCP server** (`src/index.ts`) — launched by the Claude Code plugin
  (`.claude-plugin/plugin.json`'s `mcpServers`), and registerable directly by Codex CLI and opencode.
  Tools: 7 `openapi_*` + `seo_validate` + 4 `worklog_*`, plus 4 `notion_*` **only when the `ntn` CLI is
  detected at startup**.
- **`openapi-mcp` standalone CLI** (`bin/openapi-mcp` → `src/standalone.ts`, npm) — the same 7
  `openapi_*` tools, nothing else.

All openapi handlers live once in `src/core/handlers.ts` so the two entry points can't drift.

**Claude Code-only surfaces** (not MCP tools, invisible to Codex/opencode): slash commands in
`commands/`, the single `Stop` hook in `hooks/`, and bundled skills in `skills/`. This is a wiring
choice, not a host limitation — see `docs/architecture.md`.

> **Scope framing — read before calling a request out-of-scope.** rocky is a personal plugin, not an
> OpenAPI-scoped product. The current surface is today's baseline, **not a ceiling**. When the owner
> asks for a domain or feature, **build it**. The "hold the line" discipline below guards only against
> *unrequested* scope creep; it never overrides an explicit owner request.

## Layout

```
rocky/                          single package — @minjun0219/rocky
├── .claude-plugin/
│   ├── marketplace.json        ★ this repo is its own marketplace (rocky-marketplace, source "./")
│   └── plugin.json             ★ plugin metadata + the only MCP server rocky ships
├── rocky.schema.json           `rocky.json` JSON Schema — lockstep with src/core/rocky-config.ts
├── biome.json                  lint / format (excludes .sisyphus, .claude)
├── commands/                   ★ slash commands — brainstorm, review, finish, resolve-reviews, recall
├── hooks/hooks.json            ★ Stop (log-turn) — the only hook. Nothing runs at SessionStart,
│                                 so the plugin adds nothing to session context.
├── skills/                     ★ bundled skills — writing-cc-plugin, todoist
├── bin/openapi-mcp             bun shebang, arg parsing → src/standalone
├── docs/                       architecture, openapi-mcp, codex, opencode, hosts, backlog
│   └── design/{specs,plans}/   설계·계획 산출물 (구 docs/superpowers/) — 과거분은 그대로 보존
└── src/
    ├── index.ts                ★ plugin entry — MCP registration only; logic lives in ./core
    ├── index.test.ts           surface guard: base 12 tools (+4 notion_* when ntn), REMOVED_TOOLS leak check
    ├── standalone.ts           standalone stdio MCP — 7 openapi tools + SpecRegistry
    ├── hooks/                  hook entries — log-turn + transcript (pure parser). Fail-open.
    └── core/                   shared implementation (barrel: index.ts)
        ├── handlers.ts         ★ the 7 openapi handlers — single source for both entry points
        ├── openapi: adapter, cache, config-loader, fetcher, filter, indexer, logger,
        │            openapi-registry, parser, registry, schema, url
        ├── notion:  notion-cli (ntn delegation), notion-cache, notion-chunking, notion-diff,
        │            notion-handlers
        ├── worklog: worklog (append-only JSONL), worklog-handlers
        ├── rocky-config.ts     `rocky.json` loader (project > user)
        ├── seo-validate.ts     seo_validate core + handler (ogpeek, SSRF guard)
        └── __fixtures__/ + *.test.ts
```

**Imports**: all relative. Plugin entry uses the barrel `./core`; standalone and `bin/` use
`./core/<file>` subpaths. The `@minjun0219/openapi-core` workspace name is gone.

## Scope (hold the line)

**In** — everything under *What rocky is* above, plus the config surface (`rocky.json`, project > user)
and the Claude Code-only surfaces. Surface details are in `README.md`; rationale in
`docs/architecture.md`.

**Out** — do not re-add without an explicit request:

- mysql / spec-pact / pr-watch / the old agents & skills — archived on `archive/pre-openapi-only-slim`.
- The old native `@opencode-ai/plugin` surface — once kept in-tree under `.archive/`, now removed
  (recover from git history if ever needed). Current opencode support is stdio MCP registration and
  is **not** a revival of it.
- The `/rocky:opencode` delegation runtime (`opencode-companion.ts`, `opencode-{jobs,cli,runner,render}.ts`,
  the `session-jobs` hook, the `opencode` config block). Removed in v0.19 — 1,737 LOC that had run a
  single job. Recover from git history if it earns its place.
- Souls (`souls/`, `soul.ts`, the `inject-soul` hook, `rocky.json`'s `soul` / `callsign`) and the
  statusline (`statusline/`, `statusline.ts`, `sync-statusline`) — removed in v0.19. They were fun,
  not load-bearing; the soul was also the only thing rocky put in session context (605 chars after a
  compression pass, then dropped entirely).
- `/rocky:codex` and `/rocky:issue` — removed in v0.19. Codex delegation is covered by the official
  `openai/codex-plugin-cc` plugin.
- Anything rocky-todo (daemon / web UI / CLI / hooks / tools) — separate repo `minjun0219/rocky-todo`.
  `rocky.json` still **tolerates** a `todo` block because the file is shared; rocky just ignores it.
- Exposing worklog digests as MCP tools (`wiki_*`), worklog in the standalone CLI, auto-promotion into
  native memory, polling-based auto-digest. Record = `worklog_*` + the `Stop` hook; organize =
  `/rocky:recall` only.
- Notion DB / child-page recursion, YAML frontmatter parsing, any Notion path other than `ntn`.
- OpenAPI YAML stream parsing, SDK codegen, multi-spec merge, mock servers / UI.
- npm publish automation (GitHub Release ≠ npm publish).

## Common commands

```bash
bun install         # 의존성 설치
bun run check       # Biome verify (no write)
bun run fix         # Biome safe fix + format
bun run typecheck   # tsc --noEmit
bun test            # 모든 src/**/*.test.ts
bunx changeset      # user-facing 변경의 버전 의도 선언 (patch/minor/major)
```

`lint` / `lint:fix` / `format` exist too for narrower runs.

**Release (changesets).** A PR with user-facing changes declares intent via `bunx changeset` (commit
`.changeset/*.md`). On merge to main, `changesets/action` opens a "Version Packages" PR carrying the
`package.json` + `.claude-plugin/plugin.json` bump and the `CHANGELOG.md` update — the plugin.json sync
is done by `scripts/sync-plugin-version.ts` inside `bun run changeset:version`, because changesets only
bumps `package.json`. Merging that PR triggers `scripts/release-github.ts`, which idempotently creates
the `v<version>` tag and GitHub Release. npm publish is **not** automated.

**Git hooks (husky).** `bun install` runs `prepare: "husky"`, wiring `core.hooksPath` to `.husky/_`.
`.husky/pre-commit` runs `lint-staged` (biome) + a secret scan (`gitleaks protect --staged`, falling
back to a built-in grep). `.husky/pre-push` runs `typecheck` + `test`. Bypass with `--no-verify`. CI
re-runs the same gates plus a `gitleaks` job. Only `.husky/pre-commit` and `.husky/pre-push` are
tracked — `.husky/_` is gitignored by husky itself.

**Do not add a Stop/PostToolUse hook that runs typecheck or tests** — pre-push and CI already cover it
deterministically, and a per-turn gate would just make every turn slow.

## Coding rules

- **Language**: TypeScript (`type: module`). Bun runs `.ts` directly — no build, no `dist/`.
- **Imports**: no `.js` / `.ts` extensions (`moduleResolution: Bundler` + `allowImportingTsExtensions`).
  External packages that require a `.js` subpath (`@modelcontextprotocol/sdk/...js`) stay as-is.
- **ESM safety**: never `__dirname`. Use `import.meta.url` + `fileURLToPath`, or Bun's `import.meta.dir`.
- **Errors**: include context — input value, timeout, status code, handle mismatch.
- **Dependencies**: avoid adding any. Prefer the standard library and Bun built-ins; HTTP goes through
  Bun's native `fetch` (with the `tls` option). Existing prod-dep exceptions:
  `@modelcontextprotocol/sdk` + `zod`, `@apidevtools/swagger-parser` + `swagger2openapi` + `js-yaml` +
  `openapi-types` + `pino`, and `ogpeek`. Dev-only tooling is fine (`husky`, `lint-staged`). Any new
  runtime dep is a separate scope discussion.
- **Tests**: `*.test.ts` next to the source, run with `bun test`. Isolate fs-dependent tests with
  `mkdtempSync`. Handler behavior is tested in `src/core/handlers.test.ts`; `src/index.test.ts` only
  guards the surface (tool count / leak regression).
- **JSDoc**: write it on exported functions and classes when touching this repo, but it is not a hard
  lint gate.

## Change checklist

1. `bun run check`, `bun run typecheck`, `bun test` all pass.
2. If the user-facing surface (tools / env vars / handles) changed, sync `README.md` (humans) and this
   file (agents), plus `docs/openapi-mcp.md` when the standalone CLI changed, and
   `.claude-plugin/plugin.json` when the Claude Code surface changed.
3. New env var → update its reading site (`src/core/cache.ts` / `config-loader.ts` / `rocky-config.ts`)
   and the `README.md` env-var table.
4. Tool contract change → update registration in `src/index.ts` and/or `src/standalone.ts`, and the
   shared handler in `src/core/handlers.ts`.
5. `rocky.json` shape change → update `rocky.schema.json` **and** `src/core/rocky-config.ts` in lockstep.
6. Tool name surfacing again → update `REMOVED_TOOLS` in `src/index.test.ts` (currently guards
   mysql / spec-pact / pr-watch; `worklog_*` presence is asserted via `WORKLOG_TOOLS`).
7. User-facing change → `bunx changeset`. Tooling-only chores need none.

## Code review bar

Applies to PR review on this repo (Claude Code Code Review included). **Write every comment in
Korean**; keep identifiers, paths, and commands in English.

**Format** — open the summary with a tally: `🔴 N important / 🟡 M nit / 🟣 K pre-existing`, or
"중요 이슈 없음" when there are no Important findings. Each inline comment is three bullets:
`문제 / 영향 / 제안`, with an applicable fix snippet where possible.

**🔴 Important** — reserved for: a change pulling in anything under *Scope → Out* without an explicit
request; an input/output break in the 7 `openapi_*` tools (shared by `src/index.ts` and
`src/standalone.ts`); `rocky.json` shape changed without `rocky.schema.json` **and**
`src/core/rocky-config.ts` moving in lockstep; a user-facing surface changed without the
*Change checklist* doc sync; `__dirname` or `.js`/`.ts` extensions on local imports or any Node-only
API that breaks the Bun-only assumption; secrets in logs, error messages missing identifying context,
fs paths built from unsanitized external input; a new runtime dep where the stdlib or a Bun built-in
would do. Everything else is Nit at most.

**🟡 Nit** — cap 5 inline; collapse the rest into the summary as `유사 항목 N 개 더`. Style, naming,
JSDoc gaps, test-file placement (`*.test.ts` next to its source), missing `mkdtempSync` isolation.

**Do not report** — `bun.lock` and anything `.gitignore`d; type errors and test failures that
`bun run typecheck` / `bun test` already catch (exception: a new `src/core/` module with no adjacent
`*.test.ts` is a Nit); a PR that was *explicitly asked* to pull in a `docs/backlog.md` item is not a
scope violation by that fact alone.

**Citation bar** — behavior claims ("this code does X") need a `path:line` citation, not an inference
from naming. Never raise an Important finding from naming alone.

**Re-review convergence** — from the second review of the same PR onward, post no new Nits: only
Important and newly introduced Pre-existing findings.

## Plugin source & dev loop

**This repo IS the plugin source AND its own marketplace** — no separate façade directory.
`.claude-plugin/marketplace.json` is the single marketplace and the plugin `source` is the relative
`"./"`. Known limitation: the claude.ai web UI's server-side marketplace sync doesn't clone the repo, so
a relative source fails there — accepted trade-off, install via CLI.

```bash
claude plugin marketplace add minjun0219/rocky
claude plugin install rocky@rocky-marketplace
```

Installs clone GitHub `main` into the plugin cache — the plugin is **not** read from a working tree. The
dev loop is push-based: edit → push to `main` → `claude plugin update rocky@rocky-marketplace`.
`/reload-plugins` does not see uncommitted edits. For a working-tree session, use
`claude --plugin-dir <repo>`.

**Why there is no `.mcp.json` here:** the installed plugin root is a clone of this repo root, so a
repo-root `.mcp.json` would leak into the *installed* plugin's MCP config on top of `plugin.json`'s
`mcpServers`. Keep such servers at user scope instead.

**Single sources**: humans = `README.md`, agents = this file, rationale = `docs/architecture.md`,
per-tool contracts = the tool definitions themselves. Do not add a new root-level sibling doc — that is
exactly what `FEATURES.md` and `REVIEW.md` were before they were folded into these three.
