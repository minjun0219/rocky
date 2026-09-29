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
- **CLI `rocky`** (`crates/rocky-cli`) — thin HTTP client + the three hook entries
  (`hook ensure-daemon` / `notify-todo` / `handoff-stop`). `bin/rocky` is a sh bootstrap that
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
`commands/`, the single `Stop` hook in `hooks/`, bundled skills in `skills/`, and subagents in
`agents/`. This is a wiring
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
├── web/                        ★ 보드 웹 UI (React 19 · zustand · Tailwind v4) — `bun run build:ui` → dist/ (gitignore).
│                                 **UI 를 고치기 전에 `web/DESIGN.md` 를 읽는다**(토큰·정보 우선순위·좁은 패널 규칙의 정본)
│                                 데몬이 실행 파일 옆 dist/ 를 `/` 에 서빙. types.ts 는 Rust 응답 타입의 사본
├── bridges/                    수집함 어댑터 — `todo.inbox[]` 에 등록되는 명령(stdout JSON 규약, docs/board.md "수집함").
│                                 외부 태스크 서비스 코드는 여기에만. file/ 은 규약의 참조 구현
├── crates/                     ★ the daemon, CLI (incl. worklog MCP + hooks) and core (see docs/rewrite/)
├── rocky.schema.json           `rocky.json` JSON Schema — lockstep with crates/rocky-core/src/config.rs
├── biome.json                  lint / format (excludes .sisyphus, .claude)
├── agents/                     ★ subagents — reviewer (fresh-context diff review; /rocky:review
│                                 dispatches it, and it is callable directly). Read-only role.
├── docs/                       architecture, codex, opencode, hosts, backlog, board, rewrite/ (port record)
│   └── design/{specs,plans}/   설계·계획 산출물 (구 docs/superpowers/) — 과거분은 그대로 보존
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
  *데몬/설치 모델* below. Do not confuse the two.)
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
bun install         # 의존성 설치
bun run check       # Biome verify (no write)
bun run fix         # Biome safe fix + format
bun run typecheck   # tsc --noEmit
bun test            # test:unit(scripts·plugin/scripts·bridges·web *.test.ts) + test:dom(web *.test.tsx, happy-dom)
bun run build:ui    # web/ → dist/ (데몬이 서빙)
bunx changeset      # user-facing 변경의 버전 의도 선언 (patch/minor/major)

cargo fmt --all --check                                   # Rust 포맷
cargo clippy --workspace --all-targets -- -D warnings     # Rust 린트 (경고 = 실패)
cargo test --workspace                                    # Rust 테스트
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

1. `bun run check`, `bun run typecheck`, `bun test` and `cargo fmt --all --check`,
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

## 데몬/설치 모델 (핵심)

> 옛 rocky-todo 의 설계 기록을 Rust 모듈 경로로 옮겨 적은 것이다. 순수 판정은 `rocky_core::*`,
> HTTP·프로세스 배선은 `rockyd::*`, 훅·CLI 는 `rocky_cli::*`. 이 레포에는 웹 UI 가 없다 —
> `issueCreateAllowed` 같은 UI 용 필드는 GUI(Swift/TUI, 미정)가 쓸 **API 계약**으로 남아 있고,
> `rockyd` 는 `ui_dist` 가 주어질 때만 정적 파일을 서빙한다.

- **설치 = 활성화**: `todo.enabled` 스위치 없음. `claude plugin disable rocky` 로 끈다.
- **터미널의 `rocky`**: 부트스트랩(`plugin/bin/rocky`)이 SessionStart 에서 `~/.local/bin/rocky` →
  `$XDG_DATA_HOME|~/.local/share/rocky/current/rocky` 링크를 건다(`link_cli`; 남의 실제 파일이면 안
  건드림). `~/.local/bin` 이 PATH 에 있는지는 셸 몫 — `rocky config show` 의 `cli` 행이 링크 없음 /
  PATH 없음을 가르고, `rocky config link` 가 지금 건다. 링크로 불렸을 때 형제 바이너리(`rockyd`·
  `rocky-tui`)는 `sibling_binary` 가 `canonicalize` 한 실제 파일 옆에서 찾는다.
- **데몬 기동**: SessionStart(startup) 훅 `rocky hook ensure-daemon`(`rocky_cli::hooks`)이
  health→없으면 detached spawn.
  CLI 도 온디맨드 spawn. 상시 상주는 `rocky daemon install`(launchd KeepAlive).
- **버전 인식 재기동**: 데몬은 부트스트랩이 받아 둔 **버전 디렉터리**(`~/.local/share/rocky/v<v>/rockyd`)
  에서 실행되고 프로세스는 그 설치본보다 오래 산다. 그래서 훅은 health 유무만 보지 않고
  `/api/health` 의 `version` 을 자기 `CARGO_PKG_VERSION` 과 정확 문자열로 비교해, 다르면 `pid` 로
  SIGTERM → 종료 확인 → 현재 버전으로 재기동한다 (version 미보고 데몬 ≤0.1.0 도 stale 취급).
  launchd 상주면 PID kill 이 무의미하므로(KeepAlive 가 되살린다) job 자체를 현재 경로로
  다시 설치한다. 못 내리면 재기동하지 않는다 — 보드가 없는 것보다 구버전이라도 있는 게 낫다.
  health 의 `name` 이 `"rocky"` 가 아니면 우리 데몬으로 보지 않는다(0.23.0 이하 `rocky-todo` 포함).
  한계: **버전이 같으면 경로가 달라도 재기동하지 않는다** — 로컬 레포 데몬과 설치본 버전이
  같을 때(개발 중) 서로 갈아치우지 않는 건 의도된 동작. 강제 교체는 `rocky daemon stop`.
  **`/reload-plugins` 경로**: SessionStart 가 다시 돌지 않으므로 매 턴의 `UserPromptSubmit`
  훅(`notify-todo`)이 같은 검사를 `RestartPolicy::OnlyIfOlder` 로 한다 — 도는 데몬이 자기보다
  **오래됐을 때만** 올리고, 없거나 더 새 데몬은 건드리지 않는다(옛 플러그인으로 도는 세션과
  새 세션이 턴마다 서로 뒤집는 걸 막는다). 비교는 `rocky_core::version::is_older`(세 자리 +
  `-next.N`, 못 읽으면 false). 그 전 단계로, 새 버전 바이너리가 아직 없으면 `bin/rocky`
  부트스트랩이 SessionStart 가 아닌 훅에서 `hook ensure-daemon` 을 **백그라운드로** 한 번
  띄워 받게 한다(마커 디렉터리로 중복 방지) — 그 끝에서 데몬도 올라간다.
  **교체의 각 단계는 실패를 삼키지 않는다**(0.27→0.28 사고: worklog MCP 기동이 새 버전을
  받았는데 `current` 링크는 옛 버전에 남고, 다음 턴의 launchd 교체가 bootout 뒤 bootstrap 에
  실패한 채 결과가 버려져 plist 만 새 경로이고 서비스도 데몬도 없는 상태가 남았다). 지금은
  (1) 부트스트랩이 **새 버전을 받은 직후엔 입구와 무관하게** `current` 를 건다, (2)
  `rocky_cli::launchd::register_job` 이 bootout 뒤 서비스가 사라지길 기다린 뒤 bootstrap 을
  재시도하고(`5: Input/output error` 는 bootout 이 비동기라 나는 레이스) `print` 로 로드를
  확인한다, (3) 그래도 실패하면 `ensure_daemon_with_policy` 가 health 를 다시 봐서 데몬이
  사라졌으면 launchd **밖에서라도** 띄우고 경고 문자열을 돌려준다 — SessionStart 는 stdout
  (세션 컨텍스트)에, `notify-todo` 는 additionalContext 에 `⚠ rocky 데몬: …` 로 싣는다.
  `rocky daemon status`/`config show` 는 "plist 는 있으나 로드되지 않음" 을 `launchd_loaded`
  로 가르고 `rocky daemon install` 을 고치는 명령으로 붙인다; `daemon start` 는 띄운
  프로세스가 launchd 상주인지 밖인지를 출력에 적는다.
- **첫 세션 순서 미보장**: SessionStart 데몬 기동 ↔ http MCP 초기화 순서는 보장 안 됨. 첫 세션
  MCP `failed` 는 `/mcp` retry / 다음 세션 / launchd 로 해소 — 감안 사항.
- **전역 단일 인스턴스**: 포트가 락. project rocky.json 무시, user rocky.json 의 todo 블록만.
- **데모/개발 인스턴스는 전역 설정을 상속하지 않게 띄운다** — `ROCKY_CONFIG=<전용 파일>
  cargo run -p rockyd` 로 데몬이 user `rocky.json` 을 **아예 안 읽게** 만든다(그 파일에 전용
  포트·`dir`·`expose: "off"` 를 적는다). 예전 TS 판의 `bun run demo` 가 이 모양이었다.
  `cargo run -p rockyd` 를 맨손으로 부르면 포트·디렉터리를 env(`ROCKY_TODO_PORT`/`_DIR`)로
  갈라놔도 **`expose` 는 전역 설정에서 딸려온다**. 실제로 그렇게 뜬 데모가 user config 의 `tailscale-serve` 를 물려받아
  기동 시 `tailscale serve` 를 자기 포트로 잡았고, 설치본이 열어둔 테일넷 노출을 빼앗아
  폰에서 빈 데모 보드가 보이는 사고가 났다 (`serve` 의 노출 지점은 443 의 `/` 하나뿐인
  머신 공유 자원인데, 단일 인스턴스 보장은 *같은 포트* 기준이라 둘은 공존한다).
  전역 설정을 일부러 태우고 싶을 때만 맨손으로 부른다.
- **serve 자동 보장은 남의 노출을 빼앗지 않는다**(위 사고의 코드 측 방어, `rockyd::tailscale`):
  기동 시 `serve status --json` 의 루트 프록시 포트를 보고 `decide_serve_action` 이 판정한다 —
  빈 자리면 `claim`, 내 포트면 `keep`, **살아 있는 다른 rocky 데몬**이면 `yield`(그
  인스턴스는 노출 없이 뜬다), 아무도 안 듣는 죽은 포트면 `reclaim`. `reclaim` 이 있어야
  한 번 빼앗긴 노출이 정상 데몬 재기동으로 복구된다 — 무조건 양보로 만들면 stale 설정이
  영구화된다. 점유자 판별은 `daemon_health`(신원 검증 포함)라 무관한 서비스가 그 포트를
  물고 있어도 `reclaim` 이 아니라 그쪽을 데몬으로 오인하지 않는다. **수동 경로
  (`rocky tailscale on` → `tailscale_serve_on`)는 가드하지 않는다** — 사용자가 명시적으로
  넘기라고 한 것이다.
- **이슈 생성은 로컬 요청 전용**: 보드는 무인증이고 `todo.expose` 로 노출하는 대상이지만,
  이슈 생성은 데몬 사용자의 `gh` 인증을 빌려 외부에 되돌릴 수 없는 글을 쓴다 — 보드 쓰기
  권한이 GitHub 쓰기 권한으로 확대되는 지점이라 노출 설정과 무관하게 막는다. 판별은
  `rocky_core::local_request::is_local_request` 하나이고, REST(403)와 `/mcp`(도구 에러)가
  같이 쓴다. **peer 주소만으로는 부족하다** — `tailscale serve` 는 tailnet 요청을 루프백으로
  프록시하므로 원격도 `127.0.0.1` 로 보인다. 그래서 루프백 주소 **+ 프록시 헤더 없음**
  (`x-forwarded-*` / `forwarded` / `tailscale-user-*` / `cf-*` — Cloudflare Tunnel·Access)을 함께 본다. 헤더 위조는 요청을 덜
  신뢰하게만 만들 수 있어 우회 수단이 못 된다. peer 주소는 axum 의
  `ConnectInfo<SocketAddr>` 로 넘기고, 안 넘어오면 거부다(fail-closed). `/api/health` 의
  `issueCreateAllowed` 는 UI 가 버튼 대신 이유를 보여주라는 힌트일 뿐 강제는 서버가 한다.
- **cross-site 변경은 라우트 전에 끊는다**(`is_cross_site_request` in `rocky_core::local_request`):
  데몬은 무인증이라 사용자가 방문한 악성 페이지가 `enctype="text/plain"` 폼으로 preflight
  없이 루프백에 POST 하면 `is_local_request` 를 그대로 통과한다(peer 는 `127.0.0.1`, 프록시
  헤더 없음). 그래서 `rockyd::server` 의 REST 입구에서 변경 메서드(POST/PATCH/PUT/DELETE,
  `is_mutating`)만 걸러 403 을 낸다 — `/mcp`(`rockyd::daemon` 의 `mcp_handler`)도 같은 가드를
  자기 앞단에 둔다. 그쪽은 별도 라우트라 REST 입구를 안 타기 때문이다(전송 규약이
  이미 폼 POST 를 걸러내지만 규칙에 예외를 남기지 않는다). 판정 1순위는 **`Sec-Fetch-Site`** 이고 `cross-site` 만 막는다 —
  브라우저가 요청 URL 과 개시자를 비교해 계산한 값이라 프록시가 `Host` 를 바꿔도 흔들리지
  않는다. `Origin` 문자열 비교를 1순위로 삼으면 `tailscale serve` 를 거친 정상 화면
  (브라우저는 `https://<host>.ts.net`, 데몬은 `127.0.0.1:8636`)을 막을 위험이 있어
  `Sec-Fetch-Site` 가 없을 때만 폴백으로 쓴다. 헤더가 **둘 다 없으면 통과** — 브라우저는
  cross-origin 쓰기에 `Origin` 을 반드시 붙이므로 부재는 비브라우저 클라이언트(CLI·훅·
  MCP)라는 뜻이다. 읽기는 막지 않는다(응답을 못 가져가는 cross-origin 읽기를 막을 값이 없다).
- **보드 메타(`update_board` in `rocky_core::store`)**: key(slug)·title·description·repo·path 를
  **한 트랜잭션에** 고친다. `PATCH /api/boards/:key` 가 다섯 필드를 함께 받고(예전의
  "repo 와 path 를 같이 보내면 400" 제약은 부분 적용 위험 때문이었는데 트랜잭션이 그걸
  없앴다), `null` 은 "지운다"·빈 문자열은 400 이다(폼이 실수로 비워 보낸 값이 설정을
  날리지 않게 하는 구분). `set_board_repo`/`set_board_path` 는 이 함수의 얇은 입구다.
  **key 변경은 옛 key 를 `board_aliases` 에 남긴다**(user_version 6) — key 는 참조
  접두사이자 cwd 유추 대상이라, 그냥 바꾸면 히스토리·댓글·GitHub 이슈에 박힌 `gotgan-12`
  와 훅/CLI 가 보내는 옛 `board` 인자가 통째로 죽는다. 별칭은 **입력 전용**이다:
  `board_id_of`/`resolve_ref_id`/`ensure_board` 가 전부 별칭을 보고, `ref_of` 가 내보내는 문자열은
  언제나 새 key 다. `ensure_board` 까지 별칭을 보는 이유는 읽기/쓰기 갈라짐을 막기 위해서다
  — 안 그러면 `todo_list { board: "gotgan" }` 은 이름 바뀐 보드를 읽는데
  `todo_write { board: "gotgan" }` 은 같은 이름의 빈 보드를 새로 만든다. 그 대가로 한 번
  쓴 key 는 은퇴한다(다른 보드가 재사용 불가 — 시도하면 `board key already in use`).
  **별칭이 닿지 않는 곳이 하나 있다**: `match_board`(`rocky_core::sessions`)는 세션 cwd 의
  경로 세그먼트에 **현재 key** 가 있는지만 본다 — 핸드오프 대상 고르기와 `doing` 의
  `gone` 판정이 그걸 쓴다. 즉 key 를 디렉터리 이름과 **어긋나게** 바꾸면 그 두 자리에서
  후보를 못 찾는다(기능은 죽지 않고 사람이 고르게 된다). 이름 변경의 통상 방향은 반대
  (디렉터리에 맞추는 것)라 별칭 매칭까지는 넣지 않았다 — `board_key_for_cwd`(statusline)만
  `boards.path` 를 먼저 보므로 경로가 설정된 보드는 이 어긋남에 영향받지 않는다.
- **노트 본문은 CRDT 문서다**(`rocky_core::note_doc`, Yjs 호환 `yrs`; 설계
  `docs/design/specs/2026-09-28-note-crdt-design.md`, 오너 결정 2026-09-28 — `yrs`/`yjs` 두 런타임
  의존성은 그 결정으로 승인됐다). 사람(웹 `yjs`)과 에이전트(MCP `note_write`·CLI 의 set/append)가
  같은 메모를 동시에 고쳐도 글자 단위로 합쳐진다. **데몬이 CRDT 피어**라 에이전트·CLI·TUI 는
  Yjs 를 모른다 — `update_note` 의 set 은 통째 교체가 아니라 공통 접두·접미를 뺀 **최소 편집**으로
  문서에 들어가고, append 는 끝에 삽입이다. `notes.content` 는 **읽는 쪽의 진실**(목록·TUI·CLI·
  요약은 그대로), `note_docs.state`(user_version 7)는 **합치는 쪽의 진실**이다; 문서를 열 때
  둘이 어긋나면(구버전 데몬이 content 만 고친 경우) content 에 맞추고, 씨앗을 심거나 맞춘 문서는
  **읽기 경로여도 그 자리에서 저장한다** — 안 그러면 열 때마다 다른 client id 의 새 문서가 생겨
  클라이언트가 받은 상태와 다음 요청의 문서가 다른 히스토리가 된다(같은 글자가 두 번 들어간다).
  `NoteDoc::apply` 는 **state 가 앞으로 간 것과 본문이 바뀐 것을 가른다** — 같은 글자를 지웠다
  넣은 편집·의존 update 가 먼저 온 경우는 본문은 그대로인데 state 는 전진하므로, 저장은
  `state_changed`, content·히스토리는 `text_changed` 기준이다(전자를 후자로 판단하면 그 update 가
  버려져 뒤이어 오는 update 가 영영 안 붙는다). state·content·history 는 **한 트랜잭션**이다.
  어느 경로의 편집이든(웹 update·MCP/CLI set/append) 스토어가 `NoteDocEvent`(새로 생긴 조각만)를
  내고(`subscribe_note_docs`) 서버가 그 노트의 스트림에 방송한다 — 라우트가 직접 방송하지
  않는다(에이전트 경로가 방송에서 빠져 열린 웹 편집기가 append 를 못 보던 구멍).
  노트 스트림은 **밀리면 끊는다**(`sse_from` 의 `OnLag::Close`, 채널 256건) — 이 구독자는 refetch 가
  아니라 update 를 하나씩 쌓으므로 한 건이 빠지면 그 연결이 사는 동안 문서가 낡은 채 남는다;
  끊기면 브라우저가 다시 붙어 `GET …/doc?sv=` 로 차분을 받는다(전역 `/api/events` 는 반대로
  건너뛰고 이어 간다). 구독(`subscribe_note`)은 맵 락 안에서 끝낸다 — 보내는 쪽이 "듣는 이 0"
  채널을 걷어 내므로, 채널을 꺼낸 뒤 구독하기 전에 방송이 끼면 걷힌 채널을 구독하게 된다.
  전송은 HTTP + **노트별 SSE**(`GET /api/notes/:ref/doc[?sv=]` · `POST …/doc {update}` ·
  `GET …/doc/events` · `POST …/presence`) — 전역 `/api/events` 에는 싣지 않는다(그 채널의
  구독자는 전부 refetch 한다). 웹 편집의 히스토리는 같은 actor 60초 창으로 **묶는다**
  (`NOTE_EDIT_COALESCE_SECS`; 글자마다 한 줄이면 `/api/changes` → 세션 주입까지 잡음이 된다).
  사용 로그는 여는 `GET …/doc` 만 남기고 편집·프레즌스·스트림은 모양으로 거른다(`SKIPPED_SHAPES`).
  제목은 CRDT 가 아니다(`PATCH` 그대로). MCP 도구 수는 그대로 5.
  웹 쪽은 `web/notedoc.ts`(`NoteSync`: 열기·150ms 배치 POST·노트별 SSE·재접속 시 sv 차분·
  프레즌스; `fetch`/`EventSource` 주입으로 단위 테스트)와 `web/textarea-binding.ts`(textarea ↔
  `Y.Text`, 로컬은 input 마다 최소 diff, 원격은 delta 로 커서 이동, IME 조합 중엔 원격 적용을
  멈춘다). `NoteCard` 는 포커스에 세션을 열고 blur 20초 뒤 닫는다 — 카드마다 늘 SSE 를 물면
  브라우저의 호스트당 연결 한도(HTTP/1.1 6개)에 걸린다. 편집기 후보 (b) `web/codemirror-editor.ts`
  (CodeMirror 6 + `y-codemirror.next`)는 같은 `NoteSync` 위에 올라가고, y-protocols `Awareness`
  를 프레즌스 라우트의 `state`(awareness update 의 base64)에 실어 나른다(`bridgeAwareness`) —
  상대 커서·선택 영역. 헤더의 편집기 스위치(`rocky.noteEditor` in localStorage)는 **둘 중 하나를
  지우기 위한 임시**다(결정 6). 번들 +500KB(minified) 가 (b)의 값이다.
- **번호 참조(ref)**: todo/note 는 랜덤 id(`921gvwnr`, PK 로 유지) 외에 보드별 순번을 갖는다.
  id 를 받는 자리는 어디서든 `rocky-12`(보드 접두사) → `12`(현재 보드 컨텍스트 안의
  번호) → id 정확 일치 → id 유일 prefix 순으로 시도해 해석한다(`resolve_ref_id` in
  `rocky_core::store`). 구분자가 `-` 인 이유는 `#` 가 GitHub 이슈 번호와 겹쳐서다 — 보드는
  이슈를 만들어 붙일 수 있어 한 항목에 두 종류의 `#N` 이 나타날 수 있었다. 파싱은
  **가장 오른쪽** `-` 에서 갈린다(`rocky-todo-1` = 보드 `rocky-todo` 의 1번). 옛 표기
  `rocky#12`/`#12` 는 **입력으로만** 계속 받는다 — 제품이 내보내는 문자열은 전부 `-`
  형태다. notes 만 board 없이도 존재할 수 있어(글로벌 메모) 전역 번호 공간을 따로 갖고
  예약 접두사를 붙여 `note-3` 으로 렌더된다 — `note-N` 은 board 인자와 무관하게 늘
  전역 메모다. `note` 도 board key 로 만들 수 있다(`api`/`mcp` 와 같은 원칙 — board key 는
  레포 이름에서 유추되는 값이라 생성을 막지 않는다). 다만 `is_ref_safe_board_key("note")` 가
  `false` 라 그 보드의 항목은 `ref_of` 가 `note-N` 대신 raw id 로 폴백한다. todos 는 항상
  보드에 속하므로 보드
  컨텍스트 없는 맨숫자는 에러다. 번호는 보드 안에서 `MAX(number)+1` 로 발급되어
  아카이브해도 회수(재사용)되지 않는다. **댓글은 이 번호 체계 밖이다** — 보드별
  순번 없이 댓글 id 로만 지정한다(`PATCH /api/comments/:id` 등). mutation 은 부모 todo 의
  히스토리(`entity: 'todo'`, action `comment`/`comment-edit`/`comment-archive`/
  `comment-unarchive`)로 기록되어 SSE·훅 주입 경로를 그대로 탄다.
- **핸드오프(보드 → 세션)**: 보드에서 todo 를 실행 중인 Claude Code 세션에 넘긴다.
  데몬은 세션에 밀 수 없다 — `handoffs` 큐에 쌓고 세션 훅이 당겨간다. `Stop` 훅이 집으면
  `decision: block` 으로 그 자리에서 착수하고, `UserPromptSubmit` 훅은 턴이 열릴 때 같은
  큐를 본다. 한 번에 한 건만 배달한다. **보관된 todo 의 pending 은 집지 않는다**(`claim_handoff`) —
  open 목록·요약에서 빠진 요청이 훅에서만 튀어나와 접은 일을 세션이 착수하게 되는 걸 막는다.
  **배달은 턴 경계에서만 일어나므로 idle 세션에는 닿지 않는다** — 턴을 여는 건 handoff 를
  호출한 에이전트 몫이다. `POST /api/todos/:ref/handoff` 는 그래서 `poke: { to, message }`
  (`rocky_core::handoff::build_handoff_poke`)를 함께 돌려주고, 호출자가 그대로 `SendMessage` 로 보내면 그 턴의
  `UserPromptSubmit` 훅이 상세 지시를 주입한다. poke 본문을 늘리지 마라 — 같은 턴에
  주입문이 따로 오므로 내용이 겹친다.
  세션 목록은 `claude agents --json` (`rockyd::sessions_exec`, 주입 가능 `Runner`) — `claude`
  CLI 가 없으면 이 기능만 비활성되고(`available: false` + `reason`) 보드 나머지는 정상이다.
  대상은 보드 key ↔ 세션 cwd **경로 세그먼트** 매칭 — 후보가 정확히 1개일 때만 자동으로
  보내고 아니면 사용자가 고른다. 대기 중인 요청에 TTL 은 없다 — 대상 세션이 사라지면
  "세션 없음"(stale)으로 표시만 하고 큐에는 남는다. **MCP 도구는 늘리지 않았다(5개 유지)**
  — 사람이 에이전트에게 넘기는 기능이지 에이전트끼리 일을 미루는 경로가 아니다.
- **핸드오프 라이프사이클 + doing 의 세션 귀속**(user_version 5): 배달(`delivered`)은
  "집어갔다"까지만 말한다. 그 세션이 실제로 착수했는지·끝냈는지는 `set_todo_status` 가
  채운다 — `start` 가 오면 그 todo 의 *미수락 delivered* 중 가장 오래된 건에
  `accepted_at` 을 찍고 그 `session_id` 를 `todos.doing_session_id` 로 물려주며, `done` 은
  `completed_at` 을 찍고 귀속을 비운다(`stop` 도 비우지만 착수 기록은 남긴다).
  `status` enum 은 늘리지 않았다 — accepted/completed 는 타임스탬프뿐이고 단계는
  `handoff_phase` 가 파생한다(`?status=pending` 을 쓰는 기존 코드가 안 깨진다).
  두 예외: **start 없이 바로 done** 이면 `accepted_at` 을 `completed_at` 과 같이 찍고
  (안 그러면 "끝났는데 미착수"라는 모순이 남는다), **사람이 누른 start 는 귀속하지
  않는다**(그 요청은 여전히 세션이 안 집은 것이다).
  귀속이 필요한 이유는 `/mcp` 가 stateless 라 도구 호출에 세션 식별자가 없고 에이전트가
  자기 `session_id` 를 모르기 때문 — 핸드오프가 그걸 아는 유일한 경로다.
  판정은 `rocky_core::doing`(순수): `resolve_doing_state` 는 `live`(세션 busy) / `idle`(세션은 사는데
  턴이 끝나고 완료가 없다 — **가장 흔한 실패**) / `gone` / `unknown`. 귀속이 없는 doing 은
  보드 근사로 본다 — 에이전트 actor 이고 그 보드 경로에 활성 세션이 **0개**일 때만 `gone`,
  하나라도 있으면 `unknown`(모르는 것과 없는 것은 다르다). 세션 조회는 `doing` 이 하나도
  없으면 건너뛴다. 세션 식별자는 full UUID 와 spawn 의 짧은 8자 id 를 **둘 다** 대조한다.
  "배달됐는데 미착수"(`is_unstarted`)에는 **시간 임계값이 없다** — 세션이 `gone`/`idle` 일
  때만 경고이고 `busy` 면 조용하다. 자동 만료·자동 재배달은 없고 표시만 하며, 다시 보낼지는
  사람이 정한다(새 핸드오프가 생기고 원본은 `delivered` 로 보존). **doing 만은 예외로 자동
  해제가 있다**(`rockyd::sweep`, 기동 1분 뒤부터 10분마다): 에이전트 actor 가 든 doing 이
  `gone` 이고 착수 후 24시간(`AUTO_RELEASE_GRACE_SECS`)이 지났으면 데몬이 `stop` 으로 돌리고
  actor `rocky` 로 댓글("세션 없음 — 진행중 자동 해제 …")을 남긴다. 사람이 든 것·`idle`·
  `unknown` 은 절대 건드리지 않는다 — 실제로 56일짜리 doing 이 남아 있던 데서 온 규칙
  (오너 결정 2026-09-28). 판정은 `rocky_core::doing::should_auto_release`. UI 용 목록은
  `/api/handoffs?open=true`(대기 중 + 미완료 배달 — **보관된 todo 의 것은 제외**, 요약의
  `handoffsOpen` 도 같은 규칙).
- **PR 감시(`rockyd::prwatch`, 설계 `docs/design/specs/2026-09-28-pr-watch-design.md`, 오너 결정
  2026-09-28)**: `repo` 가 설정된 보드의 레포마다 `pr.intervalMinutes`(기본 3분)에 한 번 `gh api
  graphql`(레포당 쿼리 하나, `rocky_core::prwatch::PR_QUERY`)을 돌려 스냅숏을 `pr_watch`
  (user_version 8)에 기억하고, 직전과의 전이를 그 레포를 둔 보드의 히스토리에 actor `rocky`·
  action `pr-*` 로 남긴다 — 그래서 SSE·`/api/changes`·`notify-todo` 훅 주입(`build_pr_context` — ready·conflict 만, merged/closed 는 히스토리에만)
  이 그대로 탄다. 판정은 전부 순수(`is_ready`: OPEN·draft 아님·base 가 기본 브랜치·DIRTY 아님·
  CI 통과·viewer 의 👀/🚀 가 없는 미해결 스레드 0·🚀 0). 사람에게는 `ready`·`conflict` 만
  macOS 알림(osascript, `pr.notify`)·**세션 받은편지함**(`pr.sessionNotify`, 기본 켬 — 훅이 턴마다
  `session_id → CLAUDE_CODE_MESSAGING_SOCKET · cwd` 를 `POST /api/sessions/inbox`(로컬 전용, 경로 모양 검증)로
  등록하고, 데몬이 그 레포 보드에서 일하는 가장 최근 세션 하나의 소켓에 JSON 한 줄
  `{"type":"user","message":{…}}` 을 쓴다 — 쉬던 세션도 턴이 열린다; 순수 판정은 `rocky_core::peer_inbox`,
  등록부는 데몬 수명 상태; 그 레포 보드의 `autoResolve`(보드 속성 · user_version 9 · 기본 끔 — 설정 파일이
  아니라 보드에 두는 이유는 그 레포의 세션이 `rocky board auto-resolve on` 으로 자기 보드를 켜게 하려는 것,
  변경은 로컬 전용)가 켜졌으면 처음 보는 처리 안 된 리뷰 스레드가 생긴 전이
  `pr-review` 때 그 세션에 `/rocky:resolve-reviews N` 처리를 시킨다 — 배너·브릿지는 `pr-review` 를 안 쓴다)과
  **알림 브릿지**(`pr.notifiers[]` — `todo.inbox[]` 와 같은
  `CommandBridge` 모양; 데몬이 argv 그대로 실행하고 stdin 에 `bridge_payload` JSON 을 준다, exit ≠ 0 은
  이름과 stderr 첫 줄을 로그에; 서비스 코드는 `bridges/<name>/` 에만 — `bridges/telegram/notify.ts` 가
  참조 구현이고 토큰은 `op read`), `merged` 는 기록만. **읽기만 한다** — GitHub 에 쓰는 것은
  없다(리액션·코멘트·머지는 여전히 세션/사람 몫). 러너·알림기는 주입 가능이라 테스트가 가짜
  `gh` 로 tick 을 돈다. `/api/health` 의 `prWatch { available, reason, lastTick, repos }`,
  `GET /api/prs[?board=&open=true]`, `rocky pr`. 세션 스크립트 `pr-threads.ts` 의 `ready`/
  `transitions` 는 데몬이 없는 곳의 폴백이다. **예산을 지킨다** — GraphQL 한도(시간당 5,000 포인트)는
  사용자 계정 하나에 걸리므로 데몬이 다 쓰면 세션·터미널의 `gh` 까지 막힌다(2026-09-28 실측: 스레드
  100 × 리액션 30 노드를 요청하던 첫 쿼리가 레포당 263 포인트 — **비용은 실제가 아니라 `first:`
  로 요청한 노드 수**라 열린 PR 이 0개여도 그렇다 — 3분 × 레포 10개에 두 tick 만에 바닥). 그래서
  레포당 호출은 둘이다: 목록(`PR_LIST_QUERY`)은 열린 50·닫힌 30의 **상태 조각(`prState`)만**
  받고, CI·스레드는 **실제로 열린 PR 번호에만** 별칭 배치(`detail_query`, `p<번호>:
  pullRequest`)로 묻는다 — 비용이 현실의 열린 PR 수에 비례한다(열린 것이 없으면 목록 한 번).
  리액션은 노드 대신 `viewerHasReacted` 만 묻는다. 직전엔 열려 있었는데 목록에 없는 번호도
  상세에 끼워 merged/closed 전이를 잃지 않는다. 응답마다
  `rateLimit` 을 읽어 잔여가 `RATE_LIMIT_FLOOR`(1,000) 밑이거나 한도 에러를 받으면 그 tick 을 멈추고
  리셋까지 쉰다(`pause_for`; 리셋 시각을 모르면 15분). health 의 `prWatch.rateLimit`(tick 누계
  cost·마지막 remaining)·`pausedUntil` 이 그 상태다.
- **statusline 세그먼트(`GET /api/statusline`)**: 보드를 보려고 창을 하나 더 띄우지 않으려는
  표면. `?cwd=&session=` 을 받아 **완성된 한 줄**을 `text/plain` 으로 낸다 — 렌더를 데몬이
  하는 이유는 소비자(Claude Code statusline 명령)를 `curl` 한 줄로 유지하려는 것이다.
  그 자리는 1초마다 × 열어둔 세션 수만큼 도는 유일한 경로라 렌더용 프로세스 기동을 없애는
  값이 크다. 같은 이유로 이 라우트만 **세션 캐시 TTL 이 15초**다(`statusline_sessions`) —
  다른 라우트의 3초를 쓰면 `claude agents --json`(~220ms)이 3초마다 영구히 도는 배경
  부하가 된다. 세션 목록에서 얻는 건 방치 경고 하나뿐이라 15초 지연은 손해가 없다.
  템플릿 문법은 `{name}` 치환과 `[...]` 옵셔널 그룹 둘뿐이고, **ESC 바로 뒤의 `[`/`]` 는
  리터럴**이다 — 색을 별도 DSL 로 만들지 않고 템플릿에 ANSI 이스케이프를 직접 적게 한
  선택의 대가를 한 줄로 치른 것. 판정은 전부 `rocky_core::statusline`(순수)에 있고 라우트는
  재료만 모은다. **실패는 조용하다**(빈 문자열) — 여기서 에러 본문을 내면 사용자
  프롬프트에 JSON 덩어리가 박힌다. 보드 판정은 `board_key_for_cwd` 로 `boards.path` 하위 →
  key 가 경로 세그먼트 순인데, `basename(cwd)` 를 쓰면 워크트리에서 원본 보드를 놓치기
  때문이고 이는 `match_board` 와 같은 규약이다. `{mine.*}` 이 핸드오프로 시작된 작업에만
  붙는 것도 같은 이유다 — `doing_session_id` 귀속이 생기는 유일한 경로다.
- **새 세션 띄우기(보드 → 새 워크트리)**: 실행 중인 세션이 없으면 보드가 `claude --bg
  --worktree todo-<번호>` 로 새 백그라운드 세션을 띄운다(`rockyd::spawnctl`). 워크트리 생성·
  재사용·정리는 전부 Claude Code 몫이고(`<repo>/.claude/worktrees/`, 정리는 `claude rm
  <id>`), 데몬은 이름을 결정론적으로 계산할 뿐이라 "이 todo 의 워크트리" 를 저장하지
  않는다. 대상 레포 경로는 `boards.path`(user_version 4). 그 워크트리에서 이미 도는
  세션이 있으면 **띄우지 않고** 기존 handoff 큐로 넘긴다 — 두 에이전트가 한 워크트리를
  같이 고치는 것을 막는 가드다. 이 가드는 두 겹이다: (1) 이 라우트만 **캐시 없는** 세션
  목록(기본 `rockyd::sessions_exec::list_sessions`)을 본다 — TTL 3초 캐시로 보면 spawn 이전
  스냅샷으로 판정하게 된다, (2) `worktreePath → 띄운 시각` 을 60초 기억해
  (`RecentSpawns`, 데몬 수명 상태) 그 창 안의 재요청은 **409** 다 — 재사용 분기로
  보내면 짧은 8자 id 로 pending 이 만들어져 full UUID 로 claim 하는 `Stop` 훅에 영영
  배달되지 않는다. (2)는 **실행 전에 잡는 예약**이다(`remember` → 실패 시 `forget`) —
  spawn 을 `await` 한 뒤로 미루면 겹쳐 들어온 두 요청이 게이트를 나란히 통과한다.
  `boards.path` 는 절대경로만 받고 `std::fs::canonicalize` 로 정규화해 워크트리
  경로 계산·spawn cwd·보드 저장에 **같은 값**을 쓴다(cwd 비교가 정확 문자열 일치다).
  `claude --bg` 실행은 비동기(`tokio::process`)다 — 최악 30초(`SPAWN_TIMEOUT`)를 데몬 전체가
  멎으면 안 된다. 파이프를 EOF 까지 읽고 끝내지 않는다(detach 된 손자가 fd 를 물면 영원히
  매달린다) — 자식 종료 + 짧은 유예(250ms), 또는 timeout 에서 끊는다(`run_in_dir`).
  `kill_on_drop` 이라 핸들러가 취소돼도 자식이 남지 않는다.
  `--permission-mode` 는 넘기지 않는다(사용자 기본 설정).
  **이슈 생성과 같은 로컬 요청 전용**(`is_local_request`, 403) — 보드 쓰기 권한이 프로세스를
  띄우는 권한으로 확대되는 지점이다. MCP 도구는 여전히 5개다.

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
