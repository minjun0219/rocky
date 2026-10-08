# Architecture notes

Design rationale that is **not** derivable from reading the code. Load this on demand — `AGENTS.md`
stays short and points here. If you are touching worklog, read the matching section first. The
daemon & install model's reasons and incident history are in [`daemon.md`](./daemon.md).

## MCP tools are nearly free in context — do not "slim the surface" to save tokens

Claude Code's [Tool Search](https://code.claude.com/docs/ko/mcp#scale-with-mcp-tool-search) is **on by
default**: at session start only tool *names* and server instructions load; a tool's full definition
enters context when the model calls `ToolSearch` for it. At the time this was measured rocky shipped
16 tools (~9,000 characters of definitions) and the standing cost was 16 names — roughly 200 tokens.
Today's count is whatever the `#[tool]` definitions say (board + token tools in `crates/rockyd/src/mcp.rs`,
`worklog_*` in `crates/rocky-cli/src/worklog_mcp.rs`) — about a dozen names, the same order of cost.

This was measured the hard way in 2026-07: a slimming pass got as far as deleting the entire MCP
surface on the premise that it cost ~2,900 tokens per session, then reverted. If you propose removing
tools, the argument must be **maintenance cost or absence of real use** — never context savings.
(Tool Search is off under a non-first-party `ANTHROPIC_BASE_URL`, on Bedrock / Vertex / Foundry, or
with `ENABLE_TOOL_SEARCH=false`. rocky's owner runs none of those.)

Since v0.19 nothing is injected **unconditionally**: souls were the only fixed `SessionStart`
injection (1,310 chars, compressed to 605, then removed with the feature). What rocky adds now is
event-driven and empty when nothing happened — `SessionStart` (`hook ensure-daemon`) a short board
summary, `UserPromptSubmit` (`hook notify-todo`) only what changed since the last turn (human board
edits, subscribed PR transitions, inbox deliveries), and `Stop` (`hook handoff-stop`) a handoff prompt
only when this session claims one. `hook log-turn` writes to disk and returns nothing. So the standing
cost of installing rocky is still the tool names; the rest is paid per event.

### The Sentry-style `search_*` / `execute_*` meta-tool pair is not worth adding either

Same premise, different shape: "16 tools is a lot, hide them behind a catalog search like Sentry
does." Measured against the real Sentry MCP in 2026-07, it does not pay off here.

- **Sentry's reason does not apply.** It keeps *dozens* of operations in a catalog and surfaces only
  the ~9 most-used as first-class tools; the rest live behind `search_sentry_tools` /
  `execute_sentry_tool`. It is also a remote server serving every MCP client, including hosts with no
  tool search. rocky has 16 tools — no long tail to hide — and one real host.
- **The meta tool is not free.** One `search_sentry_tools` call returned the full JSON schema of 20
  tools: ~30,000 characters, roughly 8,000 tokens — three times rocky's entire tool-definition
  surface. Trading a 200-token standing cost for that plus an extra round trip per call is a loss.
- **It hurts discoverability.** Tool Search matches on names and descriptions. `worklog_search` being
  visible is what makes "what did we decide about X" route to rocky at all. Behind a single
  `rocky_execute`, the model never learns the capability exists — which is exactly why Sentry kept its
  nine common tools first-class instead of hiding everything.

Revisit only if rocky's tool count reaches ~40-50 (a genuine long tail appears), the primary host
loses Tool Search, or rocky ships as a remote server for other people.

## Host support: what is a rocky choice vs a host limitation

rocky exposes the full MCP tool surface to every host (Claude Code / Codex CLI / opencode /
Antigravity). Slash commands, hooks, skills and subagents ship **for Claude Code**; Antigravity gets a
smaller bundle (`antigravity/`: board · worklog skills, `PreInvocation` and `Stop` hooks); Codex and
opencode get MCP registration only.

This is a rocky wiring choice, not a host limitation. As of 2026 both Codex and opencode natively
support commands / hooks / skills / subagents (Codex also has `.codex-plugin/plugin.json` bundles and
a marketplace). Those surfaces are portable — rocky just has not shipped them there yet. Do not
document them as "impossible on Codex/opencode".

See `docs/hosts.md` for the mechanism-by-host breakdown.

## worklog: the record ↔ organize split

The `worklog_*` tools are the **record (기록) layer** only — deterministic, append-only JSONL, no LLM.
The paired **organize (정리) layer** is the `/rocky:recall` slash command, which runs on the host LLM.

Why the split matters: rocky records and stores, the host distills. That keeps the worklog from
overlapping with Claude Code's native memory, which is LLM-curated. Do not add an LLM summarizer
inside a rocky tool.

Digests live **inside** the worklog as `kind:"digest"` entries linking back to source entry ids —
not in an external wiki. `wikiDir` was removed in v0.9 when `/curate` became `/rocky:recall`.

The `Stop` hook (`rocky hook log-turn`, `crates/rocky-cli/src/hooks.rs`) auto-appends a `kind:"turn"` entry per turn, deterministically.
Antigravity gets the same entry from its own `Stop` hook (`rocky hook log-turn agy`, parser `rocky_core::transcript::agy`).
Codex and opencode have no auto-capture because rocky ships no hooks for them — but the `worklog_*` tools themselves work on
every host.

## openapi / seo / notion (removed in v0.23)

Removed together after the worklog itself answered the question: across 39 repos and 5,216 logged
turns, `openapi_*`, `seo_validate` and `notion_*` were called zero times while `worklog_read` alone
was called 78 times. That is the "absence of real use" argument the section above demands — not a
token argument. They are in git history (`git log --all -- src/core/notion-cli.ts` etc.).

One shape from that era is worth keeping in mind: **external CLI delegation instead of in-process
auth**. notion never touched tokens or OAuth — every page read went through `ntn pages get`, the
tools registered only when `ntn` was detected at startup, and tests injected a fake executor via
`buildServer({ notionCli })`. Any future auth-bearing domain should copy that, the same way the
`gh`-based slash commands already do.

## souls & the statusline templates (removed in v0.19)

Both are gone — `souls/*.md` + `soul.ts` + the `inject-soul` hook + `rocky.json`'s `soul`/`callsign`,
and `statusline/*.sh` + `statusline.ts` + `sync-statusline`. They were personality and chrome, not
load-bearing, and the soul was the one thing rocky injected into every session.

Today's `rocky statusline` is **not** those templates coming back: it is a CLI subcommand that prints
the daemon's board segment, and with `--full` the path · git · model · limit lines moved in from the
owner's separate statusline tool on 2026-10-05 (`docs/features/statusline.md`). It is called as `rocky statusline` on
`PATH` (the `~/.local/bin/rocky` link), never through the per-version plugin cache path — which is how
the stable-path constraint below is met.

Two constraints worth keeping if either ever returns: a soul must be a *layer over* AGENTS.md's gates
and safety rules, never an override (fail-open, no injection by default); and the statusline must be
installed to a stable path (`~/.config/rocky/statusline.sh`) rather than pointed at the per-version
plugin cache path, which breaks on every update. Recover from git history.

## Reintroduction strategy (archive → main)

Previous toolkit surfaces (mysql / spec-pact / pr-watch + rocky / grace / mindy agents + 5 skills)
live on [`archive/pre-openapi-only-slim`](https://github.com/minjun0219/rocky/tree/archive/pre-openapi-only-slim).
The former native opencode plugin used to sit in-tree at `.archive/agent-toolkit-opencode/`; it has
been removed and lives only in git history now. It was an in-process `@opencode-ai/plugin` surface,
**not** the ancestor of current opencode support, which is plain stdio MCP registration.

Re-adding a domain is **always a separate PR** following this template. The archive is TypeScript and
the core is Rust now (AGENTS.md language boundary), so archived code is a **spec to re-implement**, not
files to check out.

1. **Decision**: where it lives — pure judgment in `crates/rocky-core`, wiring in `crates/rockyd` when
   it is machine-wide (daemon route / MCP tool), or `crates/rocky-cli` when it needs the caller's cwd
   (the reason `worklog_*` is a stdio server). Which row of the AGENTS.md area map it hangs on. Record
   both in one line in the PR description.
2. **Read the old shape**: `git show archive/pre-openapi-only-slim:<path>` for behavior and tests.
3. **Config shape**: if `rocky.json` gains a key, update `crates/rocky-core/src/config.rs` and
   `rocky.schema.json` in lockstep.
4. **Surface**: tools go in the `#[tool]` definitions, and the pinned tool lists in
   `crates/rockyd/tests/it/mcp_test.rs` (`TOOLS`) / `crates/rocky-cli/tests/it/worklog_mcp_test.rs` must
   change with them; new CLI commands and routes go in `KNOWN_SURFACES` (`crates/rocky-core/src/usage.rs`).
5. **Docs**: `README.md` surface / config / env tables, the `AGENTS.md` area map and Layout, and a
   `docs/features/<feature>.md`.

Reference shape still in tree: **worklog** (`crates/rocky-cli/src/worklog_mcp.rs` — per-project, always
on). The auth-bearing template (**notion**, v0.5 → removed v0.23, CLI-delegated) is in git history.

## Version history (why things look the way they do)

`CHANGELOG.md` covers 0.11+. Earlier structural moves, for context:

- **v0.5** — notion re-added, first domain back from the archive. CLI-gated on `ntn`.
- **v0.6** — journal re-added (record layer `journal_*` + organize layer `/curate` writing to a wiki dir).
- **v0.8** — `/pr-watch` removed. PR review handling later became `/rocky:resolve-reviews`; Claude Code's
  built-in `/autofix-pr` remains a separate option for CI-failure autofixes.
- **v0.9** — `journal_*` → `worklog_*`; `Stop` hook turn auto-capture added; organize layer moved from
  an external wiki (`/curate`) to in-worklog `kind:"digest"` entries (`/rocky:recall`). `wikiDir` dropped.
- **v0.13** — rocky-todo shared board daemon bundled here.
- **v0.16** — `/rocky:codex` became self-contained (the command itself was removed in v0.19); the `delegating-to-codex` skill was removed because
  the official `openai/codex-plugin-cc` plugin now covers general Codex delegation. rocky's remaining
  value there is worktree isolation + the plugin-surface integrity check.
- **v0.17** — opencode delegation runtime (companion CLI + job store + session hooks). **Removed in
  v0.19** — 1,737 LOC that ran a single job across its whole life.
- **v0.23** — `openapi_*` / `seo_validate` / `notion_*` and the `openapi-mcp` CLI removed (zero usage
  measured from the worklog; 4,145 LOC, six runtime deps). At that point the MCP surface was
  `worklog_*` only; the board tools came back with rocky-todo on 2026-09-22.

Dated entries, oldest first:

- **2026-07-25** — rocky-todo extracted to its own repo/plugin `minjun0219/rocky-todo`, served as the
  2nd entry of the same rocky marketplace (github source, `dependencies:["rocky"]`). rocky dropped all
  todo code, the daemon, the web UI, the `notify-todo` hook, and its react/react-dom/zustand deps.
  `rocky.json` still **tolerates** a `todo` block (rocky ignores it; the rocky daemon consumes it)
  because the file is shared across the ecosystem.
- **2026-07-30** — `/rocky:review-pr` renamed to `/rocky:resolve-reviews`. The old name parsed as
  verb + object ("review the PR" — which is what the built-in `/review` does), so it kept getting
  confused with `/rocky:review`. The new name says what it does to what: it resolves review threads.
- **2026-09-22** — rocky-todo absorbed back into this repo with both histories kept: the Rust daemon,
  the web UI and the board tools return, and the `todo` block is read again. The rocky-todo repo was
  archived read-only on 2026-09-28.
- **2026-09-29** — `/rocky:finish` → `/rocky:review-request`, `/rocky:resolve-reviews` → `/rocky:review-fix`.
  `finish` did not say what it finished (it stops at opening the PR), and `resolve-reviews` said the one
  thing it never does — it leaves resolving threads to the owner. The pair now reads as the PR's two sides:
  ask for review, then act on it. No aliases are kept (personal plugin).
- **2026-09-29** — `/rocky:review` removed. The built-in `/code-review` now does the bug hunt on the same diff
  (background subagent, effort levels, `--fix` / `--comment`), so the only part it did not cover — checking the
  implementation against the requirements — moved into `/rocky:finish` step 2.5, which dispatches the `reviewer`
  subagent for risky changes. Slash commands are not in the usage log, so there is no call count to cite.
- **2026-10-06** — `/rocky:spec-check` added. The requirements check from step 2.5 can now be called on its own —
  mid-work, after review rounds, or on work handed back from another host — and step 2.5 calls it. It also stops
  the session paraphrasing the requirements into one paragraph: sources go to the `reviewer` verbatim, because a
  paraphrase written by the implementer carries the very reading the fresh context exists to avoid. `reviewer` is
  unchanged (still reachable directly for a general review); spec-check scopes it to requirements only.
