# AGENTS.md

Guide for AI coding agents (Claude Code, opencode, codex) working in **this repository**.

> **Where things live.** Humans read [`README.md`](./README.md) (Korean — surface, config, env vars,
> quick start). Agents read this file (English — layout, scope, rules, checklist, review bar). Design
> rationale that isn't derivable from the code lives in [`docs/architecture.md`](./docs/architecture.md)
> — read it on demand, not by default. **Per-tool input/output is not documented in prose** — the tool
> definitions (`#[tool]` in `crates/rockyd/src/mcp.rs` and `crates/rocky-cli/src/worklog_mcp.rs`)
> are the single source; read them directly.
> Cross-project conventions (language, commit style, comment policy) live in the user-scope `AGENTS.md`.

## What rocky is

**rocky** (named after Project Hail Mary's Rocky) — the owner's **personal agent tool**: a Rust
daemon + CLI (`crates/`) and this thin Claude Code plugin on top. Absorbed the former `rocky-todo`
repo (2026-09-22); the merge kept both histories.

- **Daemon `rockyd`** (`crates/rockyd`) — system-wide single instance on `127.0.0.1:8636`,
  SQLite at `~/.config/rocky/todo/`. Serves the board REST + SSE and a streamable HTTP MCP with
  `todo_list` / `todo_write` / `todo_status` / `note_list` / `note_write`, and the **web UI**
  (`web/`, built to `dist/` at release and served at `/` from the sibling `dist/` of the binary).
- **CLI `rocky`** (`crates/rocky-cli`) — thin HTTP client + the four hook entries
  (`hook ensure-daemon` / `notify-todo` / `handoff-stop` / `log-turn`). `bin/rocky` is a sh bootstrap that
  downloads the release tarball for the plugin's version and execs the binary.
- **TUI `rocky-tui`** (`crates/rocky-tui`) — the board in a terminal split: REST/SSE client only,
  no DB, no adapters. Separate binary so `ratatui`/`crossterm` never link into the hook/CLI binary;
  `rocky tui` execs the sibling. Pure state and key mapping in `app.rs` (tested without a terminal),
  rendering in `ui.rs` (tested with `TestBackend`).
- **worklog stdio MCP server** (`rocky mcp worklog`, `crates/rocky-cli/src/worklog_mcp.rs`) —
  4 `worklog_*` tools, per project. It lives in the CLI, not the daemon, because the worklog is keyed
  by the caller's repo root and the daemon cannot see the caller's cwd; a plugin stdio server is spawned
  in the session's project directory. `hook log-turn` (Stop) appends the turn from the same crate.
  The same server is also the **rocky channel** (`crates/rocky-cli/src/channel.rs`): it declares the
  `claude/channel` experimental capability and forwards the daemon's `pr-ready` / `pr-conflict`
  transitions (SSE `/api/events` → `/api/changes` diff, watermark from process start) as
  `notifications/claude/channel`, which wakes the session. Claude Code delivers only in sessions
  launched with `--dangerously-load-development-channels plugin:rocky@rocky-marketplace` (research
  preview; our marketplace is not on the allowlist) — everywhere else the notification is dropped
  silently, so the capability is declared unconditionally. Pure event shaping is
  `rocky_core::notify::pr_channel_events` (same ready·conflict rule as the hook injection).
- **No TypeScript server code.** `package.json` carries dev tooling (biome, changesets, the
  release / bootstrap / permalink scripts under `scripts/` and `plugin/scripts/`) and the **browser
  bundle build** for `web/` (React + zustand + Tailwind v4, `bun run build:ui` → `dist/`). Everything
  that runs on the machine is Rust; the UI is a static bundle the daemon serves.

> **v0.23 removed** `openapi_*` (7), `seo_validate`, `notion_*` (4) and the `openapi-mcp` standalone
> CLI. A count over 39 repos / 5,216 logged turns found zero calls. They live in git history only —
> see *Scope → Out*.

**Claude Code-only surfaces** (not MCP tools, invisible to Codex/opencode): slash commands in
`plugin/commands/`, the hooks in `plugin/hooks/hooks.json` (SessionStart · UserPromptSubmit · Stop),
bundled skills in `plugin/skills/`, and subagents in `plugin/agents/`. This is a wiring
choice, not a host limitation — see `docs/architecture.md`.

> **Scope framing — read before calling a request out-of-scope.** rocky is a personal plugin, not a
> product scoped to whatever tools it ships today. The current surface is today's baseline, **not a ceiling**. When the owner
> asks for a domain or feature, **build it**. The "hold the line" discipline below guards only against
> *unrequested* scope creep; it never overrides an explicit owner request.

## Layout

```
rocky/                          single package — @minjun0219/rocky
├── .claude-plugin/marketplace.json  ★ this repo is its own marketplace — plugin source "./plugin"
├── plugin/                     ★ the Claude Code plugin — the only thing copied into the plugin cache
│   ├── .claude-plugin/plugin.json  plugin metadata + two MCP servers (rocky = daemon http, worklog = stdio)
│   ├── bin/rocky          sh bootstrap → release tarball → native binary (hooks + CLI + MCP entry)
│   ├── hooks/hooks.json        SessionStart (ensure-daemon), UserPromptSubmit (notify-todo), Stop (handoff-stop → log-turn)
│   ├── commands/ skills/ agents/   slash commands, bundled skills, reviewer subagent
│   └── scripts/permalink.ts    /rocky:finish uses it — must live inside the plugin to exist after install
├── Cargo.toml · Cargo.lock     Rust workspace — crates/rocky-core · rockyd · rocky-cli · rocky-tui
├── web/                        ★ board web UI (React 19 · zustand · Tailwind v4) — `bun run build:ui` → dist/ (gitignored).
│                                 **Read `web/DESIGN.md` before changing the UI** (tokens, information priority, narrow-panel rules).
│                                 The daemon serves the dist/ next to its binary at `/`. types.ts mirrors the Rust response types.
├── bridges/                    inbox adapters and notification bridges — commands registered in `todo.inbox[]` / `pr.notifiers[]`
│                                 (stdout JSON contract, docs/board.md). External-service code lives only here; file/ is the reference.
├── crates/                     ★ the daemon, CLI (incl. worklog MCP + hooks) and core (see docs/rewrite/)
├── rocky.schema.json           `rocky.json` JSON Schema — lockstep with crates/rocky-core/src/config.rs
├── biome.json                  lint / format (excludes .sisyphus, .claude)
├── docs/                       architecture, daemon (the daemon model's reasons, Korean), codex, opencode, hosts, backlog, board, rewrite/ (port record)
│   └── design/{specs,plans}/   design specs and plans (formerly docs/superpowers/) — past ones are kept as they are
└── scripts/                    Bun dev scripts — release-github, sync-plugin-version, check-changesets, bootstrap.test
```

## Scope (hold the line)

**In** — everything under *What rocky is* above, plus the config surface (`rocky.json`, project > user)
and the Claude Code-only surfaces. Surface details are in `README.md`; rationale in
`docs/architecture.md`.

**Out** — do not re-add without an explicit request:

- mysql / spec-pact / the old `pr-watch` **plugin** / the old agents & skills — archived on
  `archive/pre-openapi-only-slim`. (The daemon-side PR watch of 2026-09-28 — `rockyd::prwatch`,
  `rocky.json` `pr` block — is a different thing, built on an explicit owner decision; see
  *Daemon & install model* below. Do not confuse the two.)
  Those agents (`rocky` / `grace` / `mindy`) were opencode-format **persona & routing** agents; the
  current `agents/reviewer.md` is a role extracted from `/rocky:review`, not a revival of them.
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
- The rocky-todo **Tauri app** (`app/`, its root `DESIGN.md`) — left in rocky-todo's history when the repo
  was absorbed. (`web/DESIGN.md` is a different, current document for the web UI.) (The **web UI** was
  revived on 2026-09-28 by owner request as `web/` — it is a client
  of the daemon like the TUI, not a runtime; the reason to have it is reaching the board without the
  tailnet, via Cloudflare Tunnel + Access, which is the next piece.)
- The TypeScript reference implementation of the daemon (rocky-todo's `src/*.ts`) — the Rust
  crates are the implementation; the contract is `docs/rewrite/contract.md`.
- **External task-service integration inside the daemon, CLI, hooks, skills, or MCP tools.** The
  owner's task list is the rocky board and the record is `worklog_*`. External apps (Todoist,
  Google Tasks, …) are **separate inboxes, never synced**: rocky only reads them through the inbox
  adapter contract (`todo.inbox[]` → command → stdout JSON → `GET /api/inbox`) and references items by
  link. Adapter code lives only under `bridges/<name>/` (owner decision 2026-09-27,
  `docs/design/specs/2026-09-27-bridges-and-tui-design.md`); a service name appearing in `crates/`,
  `plugin/`, manifest keywords, or an MCP tool is the violation. The old `todoist` bundled skill
  stays in the owner's private plugin repo.
- Exposing worklog digests as MCP tools (`wiki_*`), worklog in the standalone CLI, auto-promotion into
  native memory, polling-based auto-digest. Record = `worklog_*` + the `Stop` hook; organize =
  `/rocky:recall` only.
- `openapi_*` / `seo_validate` / `notion_*` and the `openapi-mcp` standalone CLI — removed in v0.23
  after a usage count (39 repos, 5,216 turns) found zero calls. 4,145 LOC and six runtime deps
  (`swagger-parser`, `swagger2openapi`, `js-yaml`, `openapi-types`, `pino`, `ogpeek`) went with them.
  Recover from git history; the `ntn` CLI-delegation shape is documented in `docs/architecture.md`.
- npm publish automation (GitHub Release ≠ npm publish).

## Common commands

```bash
bun install         # install dependencies
bun run check       # Biome verify (no write)
bun run fix         # Biome safe fix + format
bun run typecheck   # tsc --noEmit
bun run test        # test:unit (scripts·plugin/scripts·bridges·web *.test.ts) + test:dom (web *.test.tsx, happy-dom preload).
                    # bare `bun test` skips the preload, so DOM tests fail — always `bun run test`
bun run build:ui    # web/ → dist/ (served by the daemon)
bunx changeset      # declare the version intent of a user-facing change (patch/minor/major)

cargo fmt --all --check                                   # Rust format
cargo clippy --workspace --all-targets -- -D warnings     # Rust lint (warnings fail)
cargo test --workspace                                    # Rust tests
cargo build --workspace                                   # target/debug/{rocky,rockyd}
```

**Version lockstep.** `package.json` = `.claude-plugin/plugin.json` = `Cargo.toml` (workspace) =
`Cargo.lock` members. `ensure-daemon` compares the daemon's reported `CARGO_PKG_VERSION` with its
own by exact string, and `bin/rocky` picks the release tarball by `plugin.json`'s version.
`bun run changeset:version` runs `scripts/sync-plugin-version.ts` to keep the four in step.

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

- **Language**: Rust (edition 2021, stable toolchain via `rust-toolchain.toml`) for everything that runs
  — daemon, CLI, hooks, MCP servers. TypeScript survives only as Bun dev scripts (`scripts/`,
  `plugin/scripts/`) and the plugin's markdown surfaces.
- **Rust rules**: `cargo fmt` + `cargo clippy --workspace --all-targets -- -D warnings` are gates
  (clippy sees tests too). Pure decision logic goes in `rocky-core` with integration tests in
  `crates/*/tests/`; the daemon and CLI only wire it. Fail-open in hooks — no `Result` out of a hook
  entry. Errors include context (input value, path, status code).
- **Dependencies**: avoid adding any. Workspace deps are declared once in the root `Cargo.toml`; prefer
  a crate already in `Cargo.lock` (e.g. `ring` for SHA-1 rather than a new `sha1`). A new runtime dep
  is a separate scope discussion. Dev-only Bun tooling is fine.
- **TS scripts**: no `.js` / `.ts` extensions on local imports, never `__dirname` (use
  `import.meta.dir`), tests as `*.test.ts` next to the script, fs isolation via `mkdtempSync`.
- **Contract fidelity**: the worklog on disk (JSONL shape, key order, project key
  `<basename>-<sha1[:8]>`) and the board REST/MCP surface (`docs/rewrite/contract.md`) are
  compatibility contracts with the old TypeScript implementations — golden tests pin them
  (`crates/rocky-core/tests/worklog_test.rs::project_key_matches_ts_golden`).

## Change checklist

1. `bun run check`, `bun run typecheck`, `bun run test` and `cargo fmt --all --check`,
   `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` all pass.
2. If the user-facing surface (tools / env vars) changed, sync `README.md` (humans) and this file
   (agents), and `.claude-plugin/plugin.json` when the Claude Code surface changed.
3. New env var → update its reading site (`crates/rocky-core/src/config.rs` /
   `crates/rocky-cli/src/hooks.rs` / `worklog_mcp.rs`) and the `README.md` env-var table.
4. Tool contract change → update the `#[tool]` definition (`crates/rockyd/src/mcp.rs` for the
   board, `crates/rocky-cli/src/worklog_mcp.rs` for worklog) and the matching test
   (`mcp_test.rs` / `worklog_mcp_test.rs`).
5. `rocky.json` shape change → update `rocky.schema.json` **and** `crates/rocky-core/src/config.rs`
   in lockstep.
6. Tool name surfacing again → the surface tests pin the exact tool lists (`TOOLS` in
   `crates/rockyd/tests/mcp_test.rs`, `crates/rocky-cli/tests/worklog_mcp_test.rs`); removed
   names (openapi / seo / notion / mysql / spec-pact / pr-watch) must not reappear.
7. User-facing change → `bunx changeset`. Tooling-only chores need none.
8. Removing or reshaping a surface (tool / route / command / hook / web action) → cite
   `rocky usage --since 90d` in the PR body (counts, last use). The usage log
   (`rocky_core::usage`, `~/.config/rocky/usage/*.jsonl`) exists so that "nobody uses it" is a
   number, not a memory. New surfaces go into `KNOWN_SURFACES` so they show up as unused until used.

## Daemon & install model

Rules only — the reasons, incident history and finer mechanics live in
[`docs/daemon.md`](./docs/daemon.md) (Korean). Pure decisions are in `rocky_core::*`, HTTP/process wiring
in `rockyd::*`, hooks and CLI in `rocky_cli::*`. Change a rule here and there together.

- **Install = enable.** There is no `todo.enabled`; `claude plugin disable rocky` turns it off.
- **Terminal `rocky`.** The bootstrap links `~/.local/bin/rocky` → `~/.local/share/rocky/current/rocky`
  (`link_cli`, never over someone else's real file); `rocky config show` reports a missing link or PATH,
  `rocky config link` fixes it. Sibling binaries are found next to the canonicalized real file.
- **Daemon start.** SessionStart `hook ensure-daemon` spawns it detached when health fails; the CLI spawns
  on demand; `rocky daemon install` makes it resident (launchd KeepAlive).
- **Version-aware restart.** The hook compares `/api/health` `version` with its own `CARGO_PKG_VERSION`
  (exact string) and replaces a stale daemon — SIGTERM by pid, or reinstall the launchd job when resident.
  If it cannot stop the old one it does not restart (an old board beats no board). `name` must be `"rocky"`.
  Same version on a different path is left alone by design. `UserPromptSubmit` (`notify-todo`) repeats the
  check with `RestartPolicy::OnlyIfOlder` so `/reload-plugins` upgrades without sessions flip-flopping.
  Each replacement step reports its failure instead of swallowing it; if the daemon ends up gone it is
  started outside launchd and a `⚠ rocky 데몬: …` warning is injected.
- **First session ordering** between SessionStart and the http MCP init is not guaranteed — a `failed`
  MCP on the first session clears with `/mcp` retry, the next session, or launchd.
- **Single global instance.** The port is the lock; only the user `rocky.json` `todo` block applies.
- **Demo / dev daemons** run with `ROCKY_CONFIG=<dedicated file> cargo run -p rockyd` (own port, `dir`,
  `expose: "off"`) so they never inherit the global `expose`.
- **Tailscale serve auto-claim never steals.** `decide_serve_action`: `claim` (free), `keep` (mine),
  `yield` (another live rocky daemon), `reclaim` (dead port). The manual `rocky tailscale on` is unguarded.
- **Local-only actions.** Issue creation, spawning sessions, and changing a board's `path` / `repo` /
  `autoResolve` require `is_local_request`: loopback peer **and** no proxy headers (`x-forwarded-*`,
  `forwarded`, `tailscale-user-*`, `cf-*`); a missing peer address is rejected (fail-closed).
- **Cross-site mutations are cut before routing** (`is_cross_site_request`): mutating methods with
  `Sec-Fetch-Site: cross-site` (falling back to `Origin`) get 403 on REST and `/mcp`; no header means a
  non-browser client and passes. Reads are never blocked.
- **Board meta** (`update_board`) changes key/title/description/repo/path/autoResolve in one transaction;
  `null` clears, empty string is 400. A key change keeps the old key in `board_aliases` (input only —
  output always uses the new key; a used key is retired). `match_board` sees only current keys.
- **Notes are CRDT documents** (`rocky_core::note_doc`, `yrs`): the daemon is the CRDT peer, so agents and
  the CLI never see Yjs — `set` is a minimal edit, `append` inserts at the end. `notes.content` is the read
  truth, `note_docs.state` the merge truth; save only when the state advanced, update content/history only
  when the text changed; state, content and history commit together. The store emits `NoteDocEvent` and the
  server broadcasts per note (routes never broadcast). Per-note SSE closes on lag. Title is not CRDT.
- **Refs.** Todos/notes carry a per-board number: resolve `rocky-12` → `12` (board context) → exact id → id
  prefix (`resolve_ref_id`), split on the rightmost `-`. Old `#12` forms are input-only. `note-N` is always a
  global note. Numbers are never reused. Comments have no numbers.
- **Handoff (board → session).** The daemon queues `handoffs`; the `Stop` hook claims one at a time and
  `UserPromptSubmit` checks at turn start. Archived todos are skipped. Idle sessions need a poke, which the
  handoff route returns (`poke`) — do not grow its text. Target = the single session whose cwd matches the
  board; otherwise the user picks. No TTL. The MCP tool count stays 5.
- **Handoff lifecycle / doing attribution.** `start` accepts the oldest delivered handoff and attributes
  `doing_session_id`; `done` completes and clears; a human `start` is not attributed. `resolve_doing_state`
  → `live` / `idle` / `gone` / `unknown`. `rockyd::sweep` auto-stops only agent-held `gone` doings older than
  24 h (`should_auto_release`) and comments why; human-held, `idle`, `unknown` are never touched.
- **PR watch** (`rockyd::prwatch`) polls repos of boards with a `repo` every `pr.intervalMinutes` and records
  `pr-*` transitions (actor `rocky`) on the board history. It only reads GitHub. Delivery: macOS banner
  (`pr.notify`), the session inbox (`pr.sessionNotify` — hooks register `CLAUDE_CODE_MESSAGING_SOCKET` via
  `POST /api/sessions/inbox`, the daemon writes one JSON line to the newest session of that board), and
  bridges (`pr.notifiers[]`, code only under `bridges/<name>/`). `pr-review` goes to the session only when
  the board's `autoResolve` is on. `ready` means **merge candidate** — the session judges before telling the
  user (`/rocky:resolve-reviews` step 8); reviews opened after merge go to the next PR (`after-merge`).
  **Budget:** GraphQL cost is the nodes requested by `first:`, not returned — per repo one `PR_LIST_QUERY`
  (state fragments) plus `detail_query` only for PRs actually open; pause below `RATE_LIMIT_FLOOR` (1,000)
  or on a rate-limit error until reset (`pause_for`). Measure `rateLimit { cost }` before changing cadence.
- **Statusline segment** (`GET /api/statusline`) renders the whole line in the daemon; session cache TTL is
  15 s on this route only; failures return an empty string. Board is resolved by `board_key_for_cwd`.
- **Spawning a session** (`claude --bg --worktree todo-<n>` in `boards.path`, `rockyd::spawnctl`) reuses a
  live session in that worktree instead of spawning; an uncached session list plus a 60 s `RecentSpawns`
  reservation (409 inside the window) guard against double spawns. `boards.path` must be absolute and is
  canonicalized. Spawning is async with a 30 s timeout and `kill_on_drop`; `--permission-mode` is not passed.

## Code review bar

Applies to PR review on this repo (Claude Code Code Review included). **Write every comment in
Korean**; keep identifiers, paths, and commands in English.

**Format** — open the summary with a tally: `🔴 N important / 🟡 M nit / 🟣 K pre-existing`, or
"중요 이슈 없음" when there are no Important findings. Each inline comment is three bullets:
`문제 / 영향 / 제안`, with an applicable fix snippet where possible.

**🔴 Important** — reserved for: a change pulling in anything under *Scope → Out* without an explicit
request; an input/output break in the `worklog_*` tools; `rocky.json` shape changed without `rocky.schema.json` **and**
`crates/rocky-core/src/config.rs` moving in lockstep; a user-facing surface changed without the
*Change checklist* doc sync; `__dirname` or `.js`/`.ts` extensions on local imports or any Node-only
API that breaks the Bun-only assumption; secrets in logs, error messages missing identifying context,
fs paths built from unsanitized external input; a new runtime dep where the stdlib or a Bun built-in
would do. Everything else is Nit at most.

**🟡 Nit** — cap 5 inline; collapse the rest into the summary as `유사 항목 N 개 더`. Style, naming,
JSDoc gaps, test-file placement (`*.test.ts` next to its source), missing `mkdtempSync` isolation.

**Do not report** — `bun.lock` and anything `.gitignore`d; type errors and test failures that
`cargo clippy` / `cargo test` / `bun test` already catch (exception: a new `crates/*/src/` module with no
test in `crates/*/tests/` is a Nit); a PR that was *explicitly asked* to pull in a `docs/backlog.md` item is not a
scope violation by that fact alone.

**Citation bar** — behavior claims ("this code does X") need a `path:line` citation, not an inference
from naming. Never raise an Important finding from naming alone.

**Re-review convergence** — from the second review of the same PR onward, post no new Nits: only
Important and newly introduced Pre-existing findings.

## Plugin source & dev loop

**This repo IS the plugin source AND its own marketplace** — no separate façade directory.
`.claude-plugin/marketplace.json` is the single marketplace and the plugin `source` is the relative
`"./plugin"`. Known limitation: the claude.ai web UI's server-side marketplace sync doesn't clone the repo, so
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
