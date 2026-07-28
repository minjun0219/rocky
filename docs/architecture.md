# Architecture notes

Design rationale that is **not** derivable from reading the code. Load this on demand — `AGENTS.md`
stays short and points here. If you are touching worklog, notion, souls, or the statusline, read the
matching section first.

## MCP tools are nearly free in context — do not "slim the surface" to save tokens

Claude Code's [Tool Search](https://code.claude.com/docs/ko/mcp#scale-with-mcp-tool-search) is **on by
default**: at session start only tool *names* and server instructions load; a tool's full definition
enters context when the model calls `ToolSearch` for it. rocky's 16 tool definitions are ~9,000
characters, but the standing cost is 16 names — roughly 200 tokens.

This was measured the hard way in 2026-07: a slimming pass got as far as deleting the entire MCP
surface on the premise that it cost ~2,900 tokens per session, then reverted. If you propose removing
tools, the argument must be **maintenance cost or absence of real use** — never context savings.
(Tool Search is off under a non-first-party `ANTHROPIC_BASE_URL`, on Bedrock / Vertex / Foundry, or
with `ENABLE_TOOL_SEARCH=false`. rocky's owner runs none of those.)

What *does* cost context on every session is the `SessionStart` soul injection — measured at 1,310
characters (≈400-600 tokens), re-injected on `clear` and `compact`. That is the surface to trim if
the session budget ever matters.

## Host support: what is a rocky choice vs a host limitation

rocky exposes the full MCP tool surface to all three hosts (Claude Code / Codex CLI / opencode), but
ships slash commands, hooks, skills, souls, and the statusline **only for Claude Code**.

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

The `Stop` hook (`src/hooks/log-turn.ts`) auto-appends a `kind:"turn"` entry per turn, deterministically.
Auto-capture is Claude Code-only because rocky ships no Codex/opencode hooks — but the `worklog_*`
tools themselves work on all three hosts.

## notion: external CLI delegation, no tokens in rocky

All Notion page access is delegated to `ntn pages get <id> --json`. rocky never touches Notion
tokens or OAuth. The tools are registered only when `ntn` is detected at startup (CLI-gated), the
same policy as the `gh`-based slash commands.

This shape — external CLI delegation instead of in-process auth — is the **template for any future
auth-bearing domain**. Inject the executor via `buildServer({ notionCli })` so tests can fake it.

## souls & statusline

**Souls** are a layer over AGENTS.md/CLAUDE.md's gates and safety rules, **never an override** — on
conflict those rules win. Injection is fail-open. Default (no `soul` set) is vanilla, no injection.
The `SessionStart` matcher is `startup|clear|compact` so it re-injects on a fresh/cleared/compacted
context but skips `resume`.

`callsign` names what the soul calls the user; the hook appends it as one directive line that beats
the soul body's default form of address. Ignored when no `soul` is set.

**Statusline**: Claude Code's `statusLine` setting only lives in user `settings.json` (plugin
`settings.json` supports only `agent`/`subagentStatusLine`). So `/rocky:statusline` copies the chosen
template to the stable path `~/.config/rocky/statusline.sh` and points `statusLine.command` there —
**never at the per-version plugin cache path**, which breaks on update. The `SessionStart` hook reads
the installed copy's `# rocky-statusline-template: <name>` header marker (fallback `duo`) to propagate
plugin updates from the same template. No-op until installed; fail-open.

Neither souls nor the statusline add MCP tools.

## Reintroduction strategy (archive → main)

Previous toolkit surfaces (mysql / spec-pact / pr-watch + rocky / grace / mindy agents + 5 skills)
live on [`archive/pre-openapi-only-slim`](https://github.com/minjun0219/rocky/tree/archive/pre-openapi-only-slim).
The former native opencode plugin used to sit in-tree at `.archive/agent-toolkit-opencode/`; it has
been removed and lives only in git history now. It was an in-process `@opencode-ai/plugin` surface,
**not** the ancestor of current opencode support, which is plain stdio MCP registration.

Re-adding a domain is **always a separate PR** following this template:

1. **Decision**: (a) join the plugin directly (`src/core/` code + `src/index.ts` registration) or
   (b) a separate CLI entry alongside `openapi-mcp` (`bin/<domain>-mcp` + `src/<domain>.ts`, when host
   independence is high). Record the decision in one line in the PR description.
2. **Port from archive**: `git checkout archive/pre-openapi-only-slim -- <files>`. Old `lib/<domain>.ts`
   becomes `src/core/<domain>.ts`.
3. **Shared handler**: put the domain handler next to `src/core/handlers.ts` — entry points register only.
4. **Config shape**: if `rocky.json` gains a domain key, update `src/core/rocky-config.ts` and
   `rocky.schema.json` in lockstep.
5. **Surface**: register tools in `src/index.ts` and update the `REMOVED_TOOLS` leak guard in
   `src/index.test.ts`.
6. **Docs**: `README.md` surface / config / env tables, `AGENTS.md` Layout + scope.

Reference shapes already re-added: **notion** (v0.5, plugin-bound + `ntn` CLI-gated — the auth-bearing
template) and **journal** (v0.6, plugin-bound, always-on — the memory-shaped template; renamed
`worklog` in v0.9).

## Version history (why things look the way they do)

`CHANGELOG.md` covers 0.11+. Earlier structural moves, for context:

- **v0.5** — notion re-added, first domain back from the archive. CLI-gated on `ntn`.
- **v0.6** — journal re-added (record layer `journal_*` + organize layer `/curate` writing to a wiki dir).
- **v0.8** — `/pr-watch` removed. PR review handling later became `/rocky:review-pr`; Claude Code's
  built-in `/autofix-pr` remains a separate option for CI-failure autofixes.
- **v0.9** — `journal_*` → `worklog_*`; `Stop` hook turn auto-capture added; organize layer moved from
  an external wiki (`/curate`) to in-worklog `kind:"digest"` entries (`/rocky:recall`). `wikiDir` dropped.
- **v0.13** — rocky-todo shared board daemon bundled here.
- **v0.16** — `/rocky:codex` became self-contained; the `delegating-to-codex` skill was removed because
  the official `openai/codex-plugin-cc` plugin now covers general Codex delegation. rocky's remaining
  value there is worktree isolation + the plugin-surface integrity check.
- **v0.17** — opencode delegation runtime (companion CLI + job store + session hooks). **Removed in
  v0.19** — 1,737 LOC that ran a single job across its whole life; `/rocky:codex` covers delegation.
- **2026-07-25** — rocky-todo extracted to its own repo/plugin `minjun0219/rocky-todo`, served as the
  2nd entry of the same rocky marketplace (github source, `dependencies:["rocky"]`). rocky dropped all
  todo code, the daemon, the web UI, the `notify-todo` hook, and its react/react-dom/zustand deps.
  `rocky.json` still **tolerates** a `todo` block (rocky ignores it; the rocky-todo daemon consumes it)
  because the file is shared across the ecosystem.
