# Claude Code Plugin Reference (full)

Distilled from https://code.claude.com/docs/ko/plugins-reference (§1–9, the schema/spec)
and https://code.claude.com/docs/ko/plugins (§10, the authoring/publishing workflow).
**`marketplace.json` structure lives on a third page** (`/ko/plugin-marketplaces`) — this
reference only touches marketplace where it interacts with the plugin manifest.

A **plugin** is a self-contained directory of components: skills, agents, hooks, MCP
servers, LSP servers, monitors, themes.

---

## 1. Components

### Skills
- **Location**: `skills/` or `commands/` at plugin root, or a single `SKILL.md` at root.
- `skills/<name>/SKILL.md` (+ optional `reference.md`, `scripts/`). `commands/*.md` are flat markdown skills.
- Auto-discovered on install. Claude can invoke them by context; users invoke as `/name` (plugin skills are namespaced: `/plugin-name:name`).
- If no `skills/` dir and no `skills` manifest field, a root `SKILL.md` loads as a single skill (Claude Code v2.1.142+; no need to set `"skills": ["./"]`). The invocation name comes from frontmatter `name`; **fallback is the install directory basename**, which for marketplace-installed plugins is a version string that changes every update — so always set `name` in frontmatter.
- `disable-model-invocation: true` in frontmatter makes a skill user-invocable only (Claude won't auto-fire it).

### Agents
- **Location**: `agents/` at plugin root. Markdown files with frontmatter.
- Supported frontmatter: `name`, `description`, `model`, `effort`, `maxTurns`, `tools`, `disallowedTools`, `skills`, `memory`, `background`, `isolation`. Only valid `isolation` value is `"worktree"`.
- **NOT supported in plugin agents (security)**: `hooks`, `mcpServers`, `permissionMode`.
- Appear in @-mention typeahead as `my-plugin:code-reviewer`.

### Hooks
- **Location**: `hooks/hooks.json` at plugin root, or inline in `plugin.json`.
- Format: JSON with event → array of `{matcher, hooks: [{type, command|...}]}`.
- **Hook types**: `command` (shell), `http` (POST event JSON to URL), `mcp_tool` (call a tool on a configured MCP server), `prompt` (LLM eval, uses `$ARGUMENTS`), `agent` (run an agent validator with tools).
- **Scoping to the plugin's OWN bundled MCP server (critical, easy to get wrong):**
  - Tool matcher / `if` field must use the **scoped tool name**: `mcp__plugin_<plugin-name>_<server-name>__<tool>`.
  - An `mcp_tool` hook's `server` field must use: `plugin:<plugin-name>:<server-name>`.
  - **Matchers written against the bare server key NEVER fire.**

**Hook lifecycle events (full list):**

| Event | Fires |
| :-- | :-- |
| `SessionStart` | session begins or resumes |
| `Setup` | `--init-only`, or `--init`/`--maintenance` in `-p` mode (one-time CI/script prep) |
| `UserPromptSubmit` | prompt submitted, before processing |
| `UserPromptExpansion` | user command expands into a prompt; can block expansion |
| `PreToolUse` | before a tool call; can block |
| `PermissionRequest` | permission dialog appears |
| `PermissionDenied` | tool denied by auto-mode classifier; return `{retry:true}` to allow retry |
| `PostToolUse` | after a tool call succeeds |
| `PostToolUseFailure` | after a tool call fails |
| `PostToolBatch` | after a batch of parallel tool calls resolves |
| `Notification` | Claude Code sends a notification |
| `MessageDisplay` | while assistant message text is displayed |
| `SubagentStart` / `SubagentStop` | subagent spawned / finished |
| `TaskCreated` / `TaskCompleted` | task created via TaskCreate / marked completed |
| `Stop` | Claude finishes responding |
| `StopFailure` | turn ends due to an API error (output/exit code ignored) |
| `TeammateIdle` | agent-team teammate about to go idle |
| `InstructionsLoaded` | CLAUDE.md or `.claude/rules/*.md` loaded into context |
| `ConfigChange` | config file changes during session |
| `CwdChanged` | working directory changes (e.g. `cd`) |
| `FileChanged` | watched file changes; `matcher` = filenames to watch |
| `WorktreeCreate` / `WorktreeRemove` | worktree created/removed (replaces default git behavior) |
| `PreCompact` / `PostCompact` | before/after context compaction |
| `Elicitation` / `ElicitationResult` | MCP server requests user input / after user responds |
| `SessionEnd` | session terminates |

### MCP servers
- **Location**: `.mcp.json` at plugin root, or inline in `plugin.json` (`mcpServers`).
- Standard MCP server config. Use `${CLAUDE_PLUGIN_ROOT}` in `command`/`args`/`env`.
- Auto-start when the plugin activates; appear as standard MCP tools (namespaced `mcp__plugin_<plugin>_<server>__<tool>`).

### LSP servers
- **Location**: `.lsp.json` at plugin root, or inline `plugin.json` (`lspServers`).
- Required fields: `command` (binary on PATH), `extensionToLanguage` (map ext → language id).
- Optional: `args`, `transport` (`stdio` default / `socket`), `env`, `initializationOptions`, `settings`, `workspaceFolder`, `startupTimeout`, `shutdownTimeout`, `restartOnCrash` (default `true`), `maxRestarts`, `diagnostics` (default `true`; set `false` to keep navigation but suppress auto-diagnostic injection).
- `restartOnCrash`/`shutdownTimeout` need v2.1.205+ (earlier: setting either silently skipped the server).
- Same extension declared by multiple active servers → first-registered wins, others never start.
- **You must install the language-server binary separately** — the plugin only configures the connection. Prefer the official prebuilt LSP plugins (pyright/typescript/rust-analyzer) over rolling your own.

### Monitors (experimental)
- **Location**: `monitors/monitors.json`, or inline `plugin.json` `experimental.monitors` (array, or relative-path string).
- JSON array of `{name, command, description, when?}`. Runs a persistent background shell command; every stdout line is forwarded to Claude as a notification.
- `name` (unique, dedupes on reload), `command` (persistent bg process in session cwd), `description`. Optional `when`: `"always"` (default) or `"on-skill-invoke:<skill-name>"`.
- Only run in interactive CLI sessions; unsandboxed (same trust as hooks); skipped where Monitor tool is unavailable. Needs v2.1.105+.
- Supports the same variable substitution as MCP/LSP: `${CLAUDE_PLUGIN_ROOT}`, `${CLAUDE_PLUGIN_DATA}`, `${CLAUDE_PROJECT_DIR}`, `${user_config.*}`, `${ENV_VAR}`.

### Themes (experimental)
- `themes/*.json`: `{name, base, overrides}`. Selecting persists `custom:<plugin-name>:<slug>`. Read-only; `Ctrl+E` in `/theme` copies to `~/.claude/themes/` for editing.

---

## 2. Manifest schema — `.claude-plugin/plugin.json`

The manifest is **optional**. Omit it and Claude Code auto-discovers components from default
locations and derives the name from the directory. Use it for metadata or custom component paths.

```json
{
  "name": "plugin-name",
  "displayName": "Plugin Name",
  "version": "1.2.0",
  "description": "Brief plugin description",
  "author": { "name": "Author Name", "email": "author@example.com", "url": "https://github.com/author" },
  "homepage": "https://docs.example.com/plugin",
  "repository": "https://github.com/author/plugin",
  "license": "MIT",
  "keywords": ["keyword1", "keyword2"],
  "skills": "./custom/skills/",
  "commands": ["./custom/commands/special.md"],
  "agents": ["./custom/agents/reviewer.md"],
  "hooks": "./config/hooks.json",
  "mcpServers": "./mcp-config.json",
  "outputStyles": "./styles/",
  "lspServers": "./.lsp.json",
  "experimental": { "themes": "./themes/", "monitors": "./monitors.json" },
  "dependencies": ["helper-lib", { "name": "secrets-vault", "version": "~2.1.0" }]
}
```

### Required
- `name` (string, kebab-case, no spaces) — the ONLY required field. Used for component namespacing (`plugin-dev:agent-creator`). If a marketplace entry names the plugin differently, the **marketplace entry name** is what `enabledPlugins` keys and `/plugin` use.

### Unrecognized fields
- Claude Code ignores unrecognized top-level fields (so one file can double as `package.json` / VS Code / MCPB manifest). `claude plugin validate` reports them as **warnings** (suggests intended names on typos); the plugin still loads.
- **Wrong-typed** fields still fail (e.g. `keywords` as a string = load error). `--strict` turns warnings into errors (use in CI).

### Metadata fields
| Field | Type | Notes |
| :-- | :-- | :-- |
| `$schema` | string | editor autocomplete; ignored at load |
| `displayName` | string | human-readable UI name (v2.1.143+); falls back to `name`; not used for namespacing |
| `version` | string | semver. **Setting it freezes the plugin to that string** — users only update when you bump it. Omit → falls back to git commit SHA (every commit = new version). `plugin.json` wins over marketplace entry. See §9. |
| `description` | string | |
| `author` | object | `{name, email, url}` |
| `homepage` / `repository` | string | |
| `license` | string | e.g. `"MIT"` |
| `keywords` | array | discovery tags |
| `defaultEnabled` | boolean | v2.1.154+. `false` = installs disabled; user enables via `claude plugin enable` / `/plugin`. Overridden by the user's `enabledPlugins` setting and by dependency requirements. Marketplace entry's value wins over `plugin.json`. |

### Component path fields
| Field | Type | Behavior |
| :-- | :-- | :-- |
| `skills` | string\|array | **ADDS to** default `skills/` (always scanned). Marketplace-root-source exception: declaring subdirs replaces the default scan. |
| `commands` | string\|array | **REPLACES** default `commands/` |
| `agents` | string\|array | **REPLACES** default `agents/` |
| `hooks` | string\|array\|object | path or inline; own merge rules |
| `mcpServers` | string\|array\|object | path or inline; own merge rules |
| `outputStyles` | string\|array | **REPLACES** default `output-styles/` |
| `lspServers` | string\|array\|object | own merge rules |
| `experimental.themes` | string\|array | **REPLACES** default `themes/` |
| `experimental.monitors` | string\|array | **REPLACES** default `monitors/` |
| `userConfig` | object | prompted config values (see below) |
| `channels` | array | message-injection channels bound to an MCP server (see below) |
| `dependencies` | array | required plugins, optional semver constraint |

**Path behavior rules:**
- **Replace** default dir: `commands`, `agents`, `outputStyles`, `experimental.themes`, `experimental.monitors`. To keep the default and add more, list both: `"commands": ["./commands/", "./extras/"]`.
- **Add to** default: `skills` (default `skills/` always scanned).
- Own merge rules: `hooks`, `mcpServers`, `lspServers`.
- All paths must be **relative to plugin root and start with `./`**.
- v2.1.140+ flags ignored default folders in `claude plugin list` / `/plugin` when both a manifest key and matching default folder exist (unless the manifest key points into that folder).

### `userConfig`
Prompts the user on activation instead of manual `settings.json` editing.
```json
{ "userConfig": { "api_token": { "type": "string", "title": "API Token", "description": "…", "sensitive": true } } }
```
Per-option fields: `type` (`string`|`number`|`boolean`|`directory`|`file`, required), `title` (required), `description` (required), `sensitive` (masks + stores in secure store), `required`, `default`, `multiple` (string arrays), `min`/`max` (number).
- Substitutable as `${user_config.KEY}` in MCP/LSP config, hook commands, monitor commands. Non-sensitive values also substitute in skill/agent content. All exported as `CLAUDE_PLUGIN_OPTION_<KEY>` env vars.
- Non-sensitive → `settings.json` `pluginConfigs[<id>].options`. Sensitive → system keychain (or `~/.claude/.credentials.json`); ~2 KB total limit shared with OAuth tokens, keep small.

### `channels`
```json
{ "channels": [ { "server": "telegram", "userConfig": { "bot_token": { "type": "string", "title": "Bot Token", "sensitive": true } } } ] }
```
`server` (required) must match a key in the plugin's `mcpServers`. Optional per-channel `userConfig` uses the same schema.

### Environment variables (substituted inline everywhere: skill/agent content, hook/monitor commands, MCP/LSP config; also exported to subprocesses)
- **`${CLAUDE_PLUGIN_ROOT}`** — absolute path of the install dir. Reference bundled scripts/binaries/config. In shell-form hooks/monitors wrap in double quotes; in exec-form use `args`. **Changes on update** — treat as ephemeral, don't write state here (old version dirs persist ~7 days). After an in-session update, run `/reload-plugins` to switch hooks/MCP/LSP to the new path; monitors need a session restart.
- **`${CLAUDE_PLUGIN_DATA}`** — persistent dir surviving updates. For installed deps (`node_modules`, venv), generated code, caches. Auto-created on first reference. Resolves to `~/.claude/plugins/data/{id}/` (id = plugin identifier with non-`[A-Za-z0-9_-]` → `-`; e.g. `formatter@my-marketplace` → `formatter-my-marketplace`). Deleted when the plugin is removed from its last scope (`--keep-data` to keep).
- **`${CLAUDE_PROJECT_DIR}`** — project root (same as the hook's `CLAUDE_PROJECT_DIR`). Quote for spaces.

---

## 3. Directory structure & file locations

`.claude-plugin/` holds ONLY `plugin.json` (and `marketplace.json`). **Every other component
dir (commands/, agents/, skills/, output-styles/, themes/, monitors/, hooks/) lives at the
plugin ROOT — a sibling of `.claude-plugin/`, NOT inside it.** Getting this wrong = plugin
loads but components silently don't appear. The plugin root is the directory containing
`.claude-plugin/plugin.json` — never `~/.claude/` itself.

A root-level `CLAUDE.md` is **not** loaded as project context. Provide context via skills, not CLAUDE.md.

| Component | Default location | Purpose |
| :-- | :-- | :-- |
| Manifest | `.claude-plugin/plugin.json` | metadata (optional) |
| Skills | `skills/` | `<name>/SKILL.md` |
| Commands | `commands/` | flat `.md` skills (prefer `skills/` for new plugins) |
| Agents | `agents/` | subagent markdown |
| Output styles | `output-styles/` | |
| Themes | `themes/` | |
| Hooks | `hooks/hooks.json` | |
| MCP servers | `.mcp.json` | |
| LSP servers | `.lsp.json` | |
| Monitors | `monitors/monitors.json` | |
| Executables | `bin/` | added to Bash tool `PATH` while plugin active; callable as bare commands |
| Settings | `settings.json` | default config applied on activation; currently only `agent` + `subagentStatusLine` keys |

---

## 4. Skills-directory plugins (`@skills-dir`)

Any folder under a skills dir containing `.claude-plugin/plugin.json` loads next session as
`<name>@skills-dir` — no marketplace, no install step (discovered in place, not copied to cache).
Scaffold with `claude plugin init`.

A skills-dir tree supports three things:
| You have | It is |
| :-- | :-- |
| `<skills-dir>/foo/SKILL.md` (no manifest) | a plain skill `foo` |
| `<skills-dir>/foo/.claude-plugin/plugin.json` | a `foo@skills-dir` plugin (can bundle its own skills/agents/hooks) |
| `<plugin>/skills/bar/SKILL.md` | a `bar` skill packaged inside a plugin |

Load location & scope:
| Skills dir | Scope | Loads |
| :-- | :-- | :-- |
| `~/.claude/skills/` | personal | from all projects |
| `<cwd>/.claude/skills/` | project | only after accepting the workspace trust dialog |

Project-scope restrictions (beyond trust gate): bundled MCP servers get per-server approval; LSP servers start only after trusting the workspace; **background monitors do NOT load**. Personal scope has none of these restrictions.

**Project-scope `@skills-dir` plugins only load from the `.claude/skills/` of the startup dir** — they don't walk to repo root like plain skills. Start at repo root or `/reload-plugins` after `cd`.

Changes to a skill's `SKILL.md` apply immediately; changes to `hooks/`, `.mcp.json`, `agents/`, `output-styles/` need `/reload-plugins` or restart. Stop loading via `claude plugin disable my-tool@skills-dir` or delete the folder (no uninstall step).

---

## 5. Installation scopes
| Scope | Settings file | Use |
| :-- | :-- | :-- |
| `user` | `~/.claude/settings.json` | personal, all projects (default) |
| `project` | `.claude/settings.json` | team, version-controlled |
| `local` | `.claude/settings.local.json` | project-specific, gitignored |
| `managed` | managed settings | read-only, update-only |

---

## 6. Caching & file resolution
- `--plugin-dir` / `--plugin-url`: for the session duration. Marketplace: installed for future sessions.
- **Marketplace plugins are copied to `~/.claude/plugins/cache`** (for security/verification), NOT used in place. `directory`-source marketplaces are the exception — read in place.
- Each installed version = separate cache dir. Old versions marked orphaned, auto-removed after 7 days (grace period for concurrent sessions). Glob/Grep skip orphaned dirs.
- **Path traversal**: installed plugins CANNOT reference files outside their dir. `../shared-utils` breaks after install (not copied to cache).
- **Symlinks within a marketplace**: link target inside the plugin's own dir → preserved as relative symlink. Elsewhere in the same marketplace → dereferenced (contents copied). Outside the marketplace → skipped (security). For `--plugin-dir`/local installs only own-dir symlinks are preserved.

---

## 7. CLI commands

| Command | Purpose | Key options / notes |
| :-- | :-- | :-- |
| `plugin init <name>` | scaffold `~/.claude/skills/<name>/`, loads as `<name>@skills-dir` | `--description`, `--author`, `--author-email`, `--with <skills\|agents\|hooks\|mcp\|lsp\|output-style\|channel>`, `-f/--force`. Alias `new` |
| `plugin install <plugin>` | install from marketplace | `<plugin>` or `plugin@marketplace`; `-s/--scope user\|project\|local` (default user) |
| `plugin uninstall <plugin>` | remove | `--scope`, `--keep-data`, `--prune`, `-y`. Aliases `remove`, `rm`. Removing last scope deletes `${CLAUDE_PLUGIN_DATA}` unless `--keep-data` |
| `plugin prune` | remove orphaned auto-installed deps | `--scope`, `--dry-run`, `-y`. Alias `autoremove`. v2.1.121+ |
| `plugin enable <plugin>` | enable | `--scope`; transitively enables dependencies (fails if a dep isn't installed) |
| `plugin disable <plugin>` | disable without removing | `--scope`; fails if another enabled plugin depends on it |
| `plugin update <plugin>` | update to latest | `--scope user\|project\|local\|managed` |
| `plugin list` | list installed (version, source, enablement) | `--json`, `--available` (needs `--json`). Interactive `/plugin list` adds `--enabled`/`--disabled`, alias `ls` |
| `plugin details <name>` | component inventory + token cost | shows Always-on vs On-invoke token costs per component |
| `plugin tag` | create a release git tag (run inside plugin folder) | `--push`, `--dry-run`, `-f/--force` |
| `plugin validate [path]` | validate manifest & frontmatter | `--strict` (warnings → errors) |

---

## 8. Debugging & common issues

`claude --debug` shows: plugins loaded, manifest errors, skill/agent/hook registration, MCP init.

| Problem | Cause | Fix |
| :-- | :-- | :-- |
| Plugin doesn't load | bad `plugin.json` | `claude plugin validate` / `/plugin validate` |
| Skills don't appear | wrong dir structure | `skills/`/`commands/` at plugin root, NOT in `.claude-plugin/` |
| Hooks don't run | script not executable | `chmod +x`; check shebang; use `${CLAUDE_PLUGIN_ROOT}`; test manually |
| MCP server fails | missing `${CLAUDE_PLUGIN_ROOT}` | use the variable for all plugin paths |
| Path errors | absolute paths used | all paths relative, start with `./` |
| LSP `Executable not found in $PATH` | language server not installed | install the binary |

Example errors: `name: Required` (missing required field); `conflicting manifests: both plugin.json and marketplace entry specify components` (remove duplicate component defs or `strict: false` from the marketplace entry); `Plugin directory not found at path: …` (marketplace `source` path wrong).

Hook not triggering: event names are **case-sensitive** (`PostToolUse`, not `postToolUse`); matcher must match the tool (`"Write|Edit"`); valid hook types are `command`/`http`/`mcp_tool`/`prompt`/`agent`.

Debug checklist: (1) `claude --debug`, find "loading plugin"; (2) confirm each component dir is listed; (3) test each component (skill/agent/hook) individually.

---

## 9. Versioning

Version is resolved from the first of:
1. `version` in `plugin.json`
2. `version` in the marketplace entry
3. git commit SHA (for `github`/`url`/`git-subdir`/relative-path sources in a git-hosted marketplace)
4. `unknown` (for `npm` sources or local dirs not in a git repo)

| Approach | Set | Update behavior | Best for |
| :-- | :-- | :-- | :-- |
| **Explicit version** | `"version": "2.1.0"` | users update ONLY when you bump it; new commits without a bump do nothing (`update` says "already latest") | published plugins with release cycles |
| **Commit-SHA version** | omit `version` in both manifest and marketplace entry | users update on every new commit to the git source | actively-developed internal/team/personal plugins |

**Gotcha:** setting `version` and pushing new commits WITHOUT bumping = users never get changes (cached copy kept on the same version string). For fast iteration, leave `version` unset. Use semver when explicit; document in `CHANGELOG.md`.

---

## 10. Authoring & publishing workflow

Distilled from https://code.claude.com/docs/ko/plugins — the parts a pure schema reference lacks.

### Standalone (`.claude/`) vs plugin
| | Standalone (`.claude/`) | Plugin |
| :-- | :-- | :-- |
| Skill name | `/hello` | `/plugin-name:hello` (namespaced) |
| Best for | personal/project-only workflow, quick experiments, short names | sharing with team/community, version-controlled releases, cross-project reuse |
Recommended path: start standalone in `.claude/` for fast iteration → convert to a plugin when ready to share.

### Quickstart (create a plugin)
1. `mkdir my-first-plugin` — location doesn't matter during dev (you point at it with `--plugin-dir`).
2. `mkdir my-first-plugin/.claude-plugin` → write `.claude-plugin/plugin.json` (`name`, `description`, `version`, `author`).
3. `mkdir -p my-first-plugin/skills/hello` → write `skills/hello/SKILL.md` with a `description` frontmatter (+ `disable-model-invocation: true` for user-only skills).
4. Test: `claude --plugin-dir ./my-first-plugin` → `/my-first-plugin:hello`. `/help` lists skills under the namespace.
5. `$ARGUMENTS` in SKILL.md captures text after the skill name (`/my-first-plugin:hello Alex`). `/reload-plugins` to apply edits.

### Local dev/test loop
- `claude --plugin-dir ./my-plugin` — load without installing. Also accepts a `.zip` (v2.1.128+). Repeat the flag to load several. A `--plugin-dir` copy **overrides an installed marketplace plugin of the same name** for that session (except managed-forced plugins).
- `claude --plugin-url https://…/my-plugin.zip` — fetch a hosted zip for the session (CI artifacts). Same trust rules; repeat or space-separate for several.
- `/reload-plugins` — apply changes without restart (reloads plugins, skills, agents, hooks, plugin MCP + LSP servers). Verify: skills via `/plugin-name:skill`, agents in `/context` Custom Agents (or @-mention), hooks by triggering them.
- Debug: check structure (dirs at root, not in `.claude-plugin/`), test components individually, `claude plugin validate` / `claude --debug`.

### Default settings & main-agent override
- A plugin-root `settings.json` applies defaults on activation; **only `agent` + `subagentStatusLine` keys** are supported. Setting `agent` activates one of the plugin's `agents/` as the MAIN thread (its system prompt / tool limits / model), so a plugin can change how Claude Code behaves by default. `settings.json` wins over `plugin.json`'s `settings`; unknown keys ignored.

### Convert existing `.claude/` config → plugin
1. `mkdir -p my-plugin/.claude-plugin` + write manifest.
2. `cp -r .claude/commands my-plugin/`, `cp -r .claude/agents my-plugin/`, `cp -r .claude/skills my-plugin/`.
3. Hooks: `mkdir my-plugin/hooks` → copy the `hooks` object from `.claude/settings.json` into `my-plugin/hooks/hooks.json` (same format; commands read hook input as JSON on stdin, e.g. `jq -r '.tool_input.file_path'`).
4. Test with `--plugin-dir`. Then remove the originals from `.claude/` to avoid duplication (project/user `.claude/agents/` override same-named plugin agents; plugin skills are namespaced so both stay available).

### Share & publish
1. Add a `README.md` (install + usage).
2. Choose versioning (explicit `version` vs git SHA — §9).
3. Distribute via a marketplace (`/ko/plugin-marketplaces`); private repo for team-only.
4. Test with others before wide rollout. Run `claude plugin validate` before submitting.

Community marketplaces (Anthropic-maintained):
- `claude-plugins-official` — curated by Anthropic, no application process; auto-registered on first interactive start.
- `claude-community` — third-party submissions after review; add via `/plugin marketplace add anthropics/claude-plugins-community`, install with `@claude-community`.
- Submit for community review: claude.ai/admin-settings/directory/submissions/plugins/new (Team/Enterprise + directory admin) or platform.claude.com/plugins/submit (individual authors). Approved plugins pin to a commit SHA; the public catalog syncs nightly.
