---
name: writing-cc-plugin
description: Use when creating, authoring, hardening, debugging, packaging, publishing, or reviewing a Claude Code plugin — anything touching plugin.json / the manifest, hooks/hooks.json, plugin-bundled MCP or LSP servers, monitors, agents, commands, skills, skills-directory (@skills-dir) plugins, local testing (--plugin-dir / --plugin-url / reload-plugins), converting .claude/ config into a plugin, marketplace publishing, install scopes, version management, or `claude plugin` CLI commands. Covers non-obvious rules (component dir placement, hook→MCP tool scoping, version fallback) that are easy to get wrong from memory.
---

# Writing Claude Code Plugins

Authoring guide + authoritative reference for the Claude Code plugin system: how to create,
test, harden, and publish a plugin, plus the full `plugin.json` manifest and component spec
(skills / agents / hooks / MCP / LSP / monitors / themes), caching, scopes, versioning, and
the `claude plugin` CLI.

**Do not answer plugin-structure questions from memory** — several rules below are
counter-intuitive and commonly mis-recalled. Read [reference.md](reference.md) for full
detail (§1–9 spec, §10 authoring/publishing); the tables here are the fast path.

## When to use
- Creating a new plugin, or converting `.claude/` config into one
- Editing/reviewing `.claude-plugin/plugin.json` or `marketplace.json` (manifest side)
- Adding/debugging skills, commands, agents, hooks, bundled MCP/LSP servers, monitors
- A plugin loads but a component silently doesn't appear
- Local dev/test loop (`--plugin-dir`, `--plugin-url`, `/reload-plugins`)
- Deciding install scope, versioning strategy, or publishing to a marketplace

## Authoring workflow (detail → reference.md §10)
1. **Standalone vs plugin**: start standalone in `.claude/` for fast iteration; convert to a plugin when sharing / needing cross-project reuse / versioned releases. Plugin skills are namespaced `/plugin-name:skill`.
2. **Scaffold**: `.claude-plugin/plugin.json` (only `name` required) + components at plugin **root** (`skills/<name>/SKILL.md`, `commands/`, `agents/`, `hooks/hooks.json`, `.mcp.json`, `.lsp.json`, `monitors/monitors.json`). Or `claude plugin init <name>` for a `@skills-dir` plugin under `~/.claude/skills/`.
3. **Test locally**: `claude --plugin-dir ./my-plugin` (loads without installing; overrides an installed same-named plugin for the session; accepts `.zip`). Edit → `/reload-plugins` to apply without restart. Verify skills via `/plugin:skill`, agents in `/context`, hooks by triggering.
4. **Validate**: `claude plugin validate ./my-plugin --strict` (warnings → errors; use in CI).
5. **Publish**: add `README.md`, pick versioning (§9), distribute via a marketplace (`/ko/plugin-marketplaces`); submit to `claude-community` via the in-app forms. — reference.md §10

## Gotchas that bite (verify against reference.md, don't guess)

| Trap | Reality |
| :-- | :-- |
| Component dirs inside `.claude-plugin/` | Only `plugin.json`/`marketplace.json` go in `.claude-plugin/`. `commands/`, `agents/`, `hooks/`, `skills/`, `monitors/`, `themes/`, `output-styles/` live at the plugin **root** (sibling). Wrong placement = plugin loads, components silently missing — no error. |
| Hook matcher on bare server name | To hook the plugin's OWN MCP tools: matcher/`if` = `mcp__plugin_<plugin>_<server>__<tool>`; `mcp_tool` hook `server` = `plugin:<plugin>:<server>`. Bare-server-key matchers **never fire**. |
| Setting `version` for a fast-iterating plugin | `version` set = users update ONLY when you bump it; new commits do nothing. **Omit `version`** → falls back to git commit SHA → every commit is a new version. Omit for actively-developed/personal plugins. |
| `skills` field replaces `skills/` | `skills` **adds to** the always-scanned `skills/`. But `commands`, `agents`, `outputStyles`, `experimental.themes`, `experimental.monitors` **replace** their default dir — list the default explicitly to keep it. |
| `name` optional so skip it | In a root-`SKILL.md` plugin, missing frontmatter `name` falls back to the install-dir basename — a version string that changes every marketplace update. Always set `name`. |
| Plugin agents can declare hooks/mcpServers | Plugin agents forbid `hooks`, `mcpServers`, `permissionMode` (security). Only `isolation: "worktree"` is valid. |
| Absolute paths / `../` in manifest | All manifest paths must be relative and start with `./`. Installed plugins can't reference files outside their dir (`../shared` breaks — not copied to cache). |
| Writing state under `${CLAUDE_PLUGIN_ROOT}` | That path changes on every update. Persistent state → `${CLAUDE_PLUGIN_DATA}` (`~/.claude/plugins/data/{id}/`). |
| Event name casing | Hook events are case-sensitive: `PostToolUse`, not `postToolUse`. |
| `CLAUDE.md` at plugin root as context | A plugin-root `CLAUDE.md` is NOT loaded as context. Ship instructions via a skill instead. |

## Quick reference (full detail → reference.md)

- **Manifest**: only `name` required. Unrecognized fields → warnings (plugin still loads); wrong-typed fields → errors. `claude plugin validate --strict` in CI. — §2
- **Component locations**: `skills/`, `commands/`, `agents/`, `hooks/hooks.json`, `.mcp.json`, `.lsp.json`, `monitors/monitors.json`, `bin/` (added to PATH), `settings.json` (`agent`/`subagentStatusLine` only). — §3
- **Hook events & types**: full lifecycle-event table; types = `command`/`http`/`mcp_tool`/`prompt`/`agent`. — §1
- **Env vars** (written without the `${...}` wrapper here so they don't get substituted when this skill loads): `CLAUDE_PLUGIN_ROOT` (ephemeral, install dir), `CLAUDE_PLUGIN_DATA` (persistent state), `CLAUDE_PROJECT_DIR` (project root), `user_config.*`. Use the `${...}` form in actual manifests. — §2
- **`@skills-dir` plugins**: drop `.claude-plugin/plugin.json` under `~/.claude/skills/` or `<cwd>/.claude/skills/`; project scope has trust + no-monitors restrictions. — §4
- **Scopes**: `user` / `project` / `local` / `managed`. — §5
- **Caching**: marketplace plugins copied to `~/.claude/plugins/cache` (directory-source = in place); symlink rules; 7-day orphan grace. — §6
- **CLI**: `init`/`install`/`uninstall`/`prune`/`enable`/`disable`/`update`/`list`/`details`/`tag`/`validate`. — §7
- **Debugging**: `claude --debug`; common-issue table. — §8
- **Versioning**: resolution order + explicit-vs-SHA table. — §9
- **Authoring & publishing**: standalone↔plugin, quickstart, dev/test loop, config→plugin migration, community marketplaces. — §10

> `marketplace.json` structure (entries, `strict`, sources) lives on a separate docs page
> (`/ko/plugin-marketplaces`), not covered here beyond manifest overlap.

## Source
Distilled from https://code.claude.com/docs/ko/plugins-reference (§1–9) and
https://code.claude.com/docs/ko/plugins (§10). When a version-gated feature or exact field
matters for a shipped change, confirm against the live docs — plugin features are versioned
(many notes cite `v2.1.x` minimums).
