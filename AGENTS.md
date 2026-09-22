# AGENTS.md

Guide for AI coding agents (Claude Code, opencode, codex) working in **this repository**.

> **Where things live.** Humans read [`README.md`](./README.md) (Korean — surface, config, env vars,
> quick start). Agents read this file (English — layout, scope, rules, checklist, review bar). Design
> rationale that isn't derivable from the code lives in [`docs/architecture.md`](./docs/architecture.md)
> — read it on demand, not by default. **Per-tool input/output is not documented in prose** — the tool
> definitions (`#[tool]` in `crates/rocky-todod/src/mcp.rs` and `crates/rocky-todo-cli/src/worklog_mcp.rs`)
> are the single source; read them directly.
> Cross-project conventions (language, commit style, comment policy) live in the user-scope `AGENTS.md`.

## What rocky is

**rocky** (named after Project Hail Mary's Rocky) — the owner's **personal agent tool**: a Rust
daemon + CLI (`crates/`) and this thin Claude Code plugin on top. Absorbed the former `rocky-todo`
repo (hail-mary D-046, 2026-09-22); the merge kept both histories.

- **Daemon `rocky-todod`** (`crates/rocky-todod`) — system-wide single instance on `127.0.0.1:8636`,
  SQLite at `~/.config/rocky/todo/`. Serves the board REST + SSE and a streamable HTTP MCP with
  `todo_list` / `todo_write` / `todo_status` / `note_list` / `note_write`. **No web UI in this repo**
  — the React/Tauri UI stayed in rocky-todo's history; a Swift or TUI app comes later.
- **CLI `rocky-todo`** (`crates/rocky-todo-cli`) — thin HTTP client + the three hook entries
  (`hook ensure-daemon` / `notify-todo` / `handoff-stop`). `bin/rocky-todo` is a sh bootstrap that
  downloads the release tarball for the plugin's version and execs the binary.
- **worklog stdio MCP server** (`rocky-todo mcp worklog`, `crates/rocky-todo-cli/src/worklog_mcp.rs`) —
  4 `worklog_*` tools, per project. It lives in the CLI, not the daemon, because the worklog is keyed
  by the caller's repo root and the daemon cannot see the caller's cwd; a plugin stdio server is spawned
  in the session's project directory. `hook log-turn` (Stop) appends the turn from the same crate.
- **No TypeScript runtime code.** `package.json` only carries dev tooling (biome, changesets, the
  release / bootstrap / permalink scripts under `scripts/` and `plugin/scripts/`).
- Names are pre-rename (`rocky-todo`, `rocky-todod`, crate names) on purpose — a separate rename PR.

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
│   ├── bin/rocky-todo          sh bootstrap → release tarball → native binary (hooks + CLI + MCP entry)
│   ├── hooks/hooks.json        SessionStart (ensure-daemon), UserPromptSubmit (notify-todo), Stop (handoff-stop → log-turn)
│   ├── commands/ skills/ agents/   slash commands, bundled skills, reviewer subagent
│   └── scripts/permalink.ts    /rocky:finish uses it — must live inside the plugin to exist after install
├── Cargo.toml · Cargo.lock     Rust workspace — crates/rocky-todo-core · rocky-todod · rocky-todo-cli
├── crates/                     ★ the daemon, CLI (incl. worklog MCP + hooks) and core (see docs/rewrite/)
├── rocky.schema.json           `rocky.json` JSON Schema — lockstep with crates/rocky-todo-core/src/config.rs
├── biome.json                  lint / format (excludes .sisyphus, .claude)
├── agents/                     ★ subagents — reviewer (fresh-context diff review; /rocky:review
│                                 dispatches it, and it is callable directly). Read-only role.
├── docs/                       architecture, codex, opencode, hosts, backlog, rocky-todo (board), rewrite/ (port record)
│   └── design/{specs,plans}/   설계·계획 산출물 (구 docs/superpowers/) — 과거분은 그대로 보존
└── scripts/                    Bun dev scripts — release-github, sync-plugin-version, check-changesets, bootstrap.test
```

## Scope (hold the line)

**In** — everything under *What rocky is* above, plus the config surface (`rocky.json`, project > user)
and the Claude Code-only surfaces. Surface details are in `README.md`; rationale in
`docs/architecture.md`.

**Out** — do not re-add without an explicit request:

- mysql / spec-pact / pr-watch / the old agents & skills — archived on `archive/pre-openapi-only-slim`.
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
- The rocky-todo **web UI and Tauri app** (`src/ui/`, `app/`, `DESIGN.md`) — left in rocky-todo's
  history when the repo was absorbed. The GUI will be Swift or a TUI, decided separately.
- The TypeScript reference implementation of the daemon (rocky-todo's `src/*.ts`) — the Rust
  crates are the implementation; the contract is `docs/rewrite/contract.md`.
- **Any external task-service integration** (Todoist, Linear, Jira, …) — the `todoist` bundled skill
  was removed and moved to the owner's private plugin repo. The owner's task list is the
  rocky-todo board and the record is `worklog_*`; rocky ships nothing else. This holds even for a
  skill that only borrows a connected MCP and ships no credentials — the point is that rocky's public
  surface names one task system. Do not name such a service in docs, manifest keywords, or PR titles.
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
bun test            # 모든 src/**/*.test.ts
bunx changeset      # user-facing 변경의 버전 의도 선언 (patch/minor/major)

cargo fmt --all --check                                   # Rust 포맷
cargo clippy --workspace --all-targets -- -D warnings     # Rust 린트 (경고 = 실패)
cargo test --workspace                                    # Rust 테스트
cargo build --workspace                                   # target/debug/{rocky-todo,rocky-todod}
```

**Version lockstep.** `package.json` = `.claude-plugin/plugin.json` = `Cargo.toml` (workspace) =
`Cargo.lock` members. `ensure-daemon` compares the daemon's reported `CARGO_PKG_VERSION` with its
own by exact string, and `bin/rocky-todo` picks the release tarball by `plugin.json`'s version.
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
  (clippy sees tests too). Pure decision logic goes in `rocky-todo-core` with integration tests in
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
  (`crates/rocky-todo-core/tests/worklog_test.rs::project_key_matches_ts_golden`).

## Change checklist

1. `bun run check`, `bun run typecheck`, `bun test` and `cargo fmt --all --check`,
   `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` all pass.
2. If the user-facing surface (tools / env vars) changed, sync `README.md` (humans) and this file
   (agents), and `.claude-plugin/plugin.json` when the Claude Code surface changed.
3. New env var → update its reading site (`crates/rocky-todo-core/src/config.rs` /
   `crates/rocky-todo-cli/src/hooks.rs` / `worklog_mcp.rs`) and the `README.md` env-var table.
4. Tool contract change → update the `#[tool]` definition (`crates/rocky-todod/src/mcp.rs` for the
   board, `crates/rocky-todo-cli/src/worklog_mcp.rs` for worklog) and the matching test
   (`mcp_test.rs` / `worklog_mcp_test.rs`).
5. `rocky.json` shape change → update `rocky.schema.json` **and** `crates/rocky-todo-core/src/config.rs`
   in lockstep.
6. Tool name surfacing again → the surface tests pin the exact tool lists (`TOOLS` in
   `crates/rocky-todod/tests/mcp_test.rs`, `crates/rocky-todo-cli/tests/worklog_mcp_test.rs`); removed
   names (openapi / seo / notion / mysql / spec-pact / pr-watch) must not reappear.
7. User-facing change → `bunx changeset`. Tooling-only chores need none.

## 데몬/설치 모델 (핵심)

> rocky-todo `AGENTS.md` 에서 그대로 옮겨 왔다. 파일 경로가 `src/*.ts` 로 적힌 곳은 지금은 `crates/` 의 같은 이름 모듈이고(`src/local-request.ts` → `rocky_todo_core::local_request`), 웹 UI 서술은 이 레포에 웹 UI 가 없으므로 계약 설명으로만 읽는다. 이름·경로 정리는 개명 PR 에서 한다.

- **설치 = 활성화**: `todo.enabled` 스위치 없음. `claude plugin disable rocky-todo` 로 끈다.
- **데몬 기동**: SessionStart(startup) 훅 `ensure-daemon.ts` 가 health→없으면 detached spawn.
  CLI 도 온디맨드 spawn. 상시 상주는 `rocky-todo daemon install`(launchd KeepAlive).
- **버전 인식 재기동**: 데몬은 플러그인 캐시의 **버전 디렉터리**(`.../rocky-todo/<v>/src/daemon.ts`)
  에서 실행되고 프로세스는 그 설치본보다 오래 산다. 그래서 훅은 health 유무만 보지 않고
  `/api/health` 의 `version` 을 자기 `package.json` 버전과 비교해, 다르면 `pid` 로 SIGTERM →
  종료 확인 → 현재 버전으로 재기동한다 (version 미보고 데몬 ≤0.1.0 도 stale 취급). 못 내리면
  재기동하지 않는다 — 보드가 없는 것보다 구버전이라도 있는 게 낫다.
  한계: **버전이 같으면 경로가 달라도 재기동하지 않는다** — 로컬 레포 데몬과 설치본 버전이
  같을 때(개발 중) 서로 갈아치우지 않는 건 의도된 동작. 강제 교체는 `rocky-todo daemon stop`.
- **첫 세션 순서 미보장**: SessionStart 데몬 기동 ↔ http MCP 초기화 순서는 보장 안 됨. 첫 세션
  MCP `failed` 는 `/mcp` retry / 다음 세션 / launchd 로 해소 — 감안 사항.
- **전역 단일 인스턴스**: 포트가 락. project rocky.json 무시, user rocky.json 의 todo 블록만.
- **데모/개발 인스턴스는 전역 설정을 상속하지 않는다** — `bun run demo` 로 띄운다.
  이 스크립트는 `ROCKY_CONFIG=./demo.rocky.json` 을 걸어 데몬이 user `rocky.json` 을 **아예
  안 읽게** 만든다 (전용 포트 8993 / `/tmp/rt-demo` / `expose: "off"`).
  `bun run src/daemon.ts` 를 맨손으로 부르면 포트·디렉터리를 env 로 갈라놔도 **`expose` 는
  전역 설정에서 딸려온다**. 실제로 그렇게 뜬 데모가 user config 의 `tailscale-serve` 를 물려받아
  기동 시 `tailscale serve` 를 자기 포트로 잡았고, 설치본이 열어둔 테일넷 노출을 빼앗아
  폰에서 빈 데모 보드가 보이는 사고가 났다 (`serve` 의 노출 지점은 443 의 `/` 하나뿐인
  머신 공유 자원인데, 단일 인스턴스 보장은 *같은 포트* 기준이라 둘은 공존한다).
  전역 설정을 일부러 태우고 싶을 때만 맨손으로 부른다.
- **serve 자동 보장은 남의 노출을 빼앗지 않는다**(위 사고의 코드 측 방어, `src/tailscale.ts`):
  기동 시 `serve status --json` 의 루트 프록시 포트를 보고 `decideServeAction` 이 판정한다 —
  빈 자리면 `claim`, 내 포트면 `keep`, **살아 있는 다른 rocky-todo 데몬**이면 `yield`(그
  인스턴스는 노출 없이 뜬다), 아무도 안 듣는 죽은 포트면 `reclaim`. `reclaim` 이 있어야
  한 번 빼앗긴 노출이 정상 데몬 재기동으로 복구된다 — 무조건 양보로 만들면 stale 설정이
  영구화된다. 점유자 판별은 `daemonHealth`(신원 검증 포함)라 무관한 서비스가 그 포트를
  물고 있어도 `reclaim` 이 아니라 그쪽을 데몬으로 오인하지 않는다. **수동 경로
  (`rocky-todo tailscale on` → `tailscaleServeOn`)는 가드하지 않는다** — 사용자가 명시적으로
  넘기라고 한 것이다.
- **이슈 생성은 로컬 요청 전용**: 보드는 무인증이고 `todo.expose` 로 노출하는 대상이지만,
  이슈 생성은 데몬 사용자의 `gh` 인증을 빌려 외부에 되돌릴 수 없는 글을 쓴다 — 보드 쓰기
  권한이 GitHub 쓰기 권한으로 확대되는 지점이라 노출 설정과 무관하게 막는다. 판별은
  `src/local-request.ts` 의 `isLocalRequest` 하나이고, REST(403)와 `/mcp`(도구 에러)가
  같이 쓴다. **peer 주소만으로는 부족하다** — `tailscale serve` 는 tailnet 요청을 루프백으로
  프록시하므로 원격도 `127.0.0.1` 로 보인다. 그래서 루프백 주소 **+ 프록시 헤더 없음**
  (`x-forwarded-*` / `forwarded` / `tailscale-user-*`)을 함께 본다. 헤더 위조는 요청을 덜
  신뢰하게만 만들 수 있어 우회 수단이 못 된다. peer 주소는 `daemon.ts` 가
  `server.requestIP(req)` 로 넘기고, 안 넘어오면 거부다(fail-closed). 웹 UI 는
  `/api/health` 의 `issueCreateAllowed` 를 부팅에 한 번 보고 버튼 대신 이유를 보여준다 —
  힌트일 뿐 강제는 서버가 한다.
- **cross-site 변경은 라우트 전에 끊는다**(`isCrossSiteRequest` in `src/local-request.ts`):
  데몬은 무인증이라 사용자가 방문한 악성 페이지가 `enctype="text/plain"` 폼으로 preflight
  없이 루프백에 POST 하면 `isLocalRequest` 를 그대로 통과한다(peer 는 `127.0.0.1`, 프록시
  헤더 없음). 그래서 `server.ts` 의 `fetch` 맨 앞에서 변경 메서드(POST/PATCH/PUT/DELETE)만
  걸러 403 을 낸다 — `/mcp`(`createMcpFetchHandler`)도 같은 가드를 자기 앞단에 둔다.
  그쪽은 `daemon.ts` 가 별도 라우트로 붙여 `api.fetch` 를 안 타기 때문이다(전송 규약이
  이미 폼 POST 를 걸러내지만 규칙에 예외를 남기지 않는다). 판정 1순위는 **`Sec-Fetch-Site`** 이고 `cross-site` 만 막는다 —
  브라우저가 요청 URL 과 개시자를 비교해 계산한 값이라 프록시가 `Host` 를 바꿔도 흔들리지
  않는다. `Origin` 문자열 비교를 1순위로 삼으면 `tailscale serve` 를 거친 정상 화면
  (브라우저는 `https://<host>.ts.net`, 데몬은 `127.0.0.1:8636`)을 막을 위험이 있어
  `Sec-Fetch-Site` 가 없을 때만 폴백으로 쓴다. 헤더가 **둘 다 없으면 통과** — 브라우저는
  cross-origin 쓰기에 `Origin` 을 반드시 붙이므로 부재는 비브라우저 클라이언트(CLI·훅·
  MCP)라는 뜻이다. 읽기는 막지 않는다(응답을 못 가져가는 cross-origin 읽기를 막을 값이 없다).
- **보드 메타(`updateBoard` in `src/store.ts`)**: key(slug)·title·description·repo·path 를
  **한 트랜잭션에** 고친다. `PATCH /api/boards/:key` 가 다섯 필드를 함께 받고(예전의
  "repo 와 path 를 같이 보내면 400" 제약은 부분 적용 위험 때문이었는데 트랜잭션이 그걸
  없앴다), `null` 은 "지운다"·빈 문자열은 400 이다(폼이 실수로 비워 보낸 값이 설정을
  날리지 않게 하는 구분). `setBoardRepo`/`setBoardPath` 는 이제 이 함수의 얇은 입구다.
  **key 변경은 옛 key 를 `board_aliases` 에 남긴다**(user_version 6) — key 는 참조
  접두사이자 cwd 유추 대상이라, 그냥 바꾸면 히스토리·댓글·GitHub 이슈에 박힌 `gotgan-12`
  와 훅/CLI 가 보내는 옛 `board` 인자가 통째로 죽는다. 별칭은 **입력 전용**이다:
  `boardIdOf`/`resolveRef`/`ensureBoard` 가 전부 별칭을 보고, `refOf` 가 내보내는 문자열은
  언제나 새 key 다. `ensureBoard` 까지 별칭을 보는 이유는 읽기/쓰기 갈라짐을 막기 위해서다
  — 안 그러면 `todo_list { board: "gotgan" }` 은 이름 바뀐 보드를 읽는데
  `todo_write { board: "gotgan" }` 은 같은 이름의 빈 보드를 새로 만든다. 그 대가로 한 번
  쓴 key 는 은퇴한다(다른 보드가 재사용 불가 — 시도하면 `board key already in use`).
  웹 UI 는 보드 목록 위 `BoardHeader` 가 이 전부를 보여주고 편집한다.
  **별칭이 닿지 않는 곳이 하나 있다**: `matchBoard`(`src/sessions.ts`)는 세션 cwd 의
  경로 세그먼트에 **현재 key** 가 있는지만 본다 — 핸드오프 대상 고르기와 `doing` 의
  `gone` 판정이 그걸 쓴다. 즉 key 를 디렉터리 이름과 **어긋나게** 바꾸면 그 두 자리에서
  후보를 못 찾는다(기능은 죽지 않고 사람이 고르게 된다). 이름 변경의 통상 방향은 반대
  (디렉터리에 맞추는 것)라 별칭 매칭까지는 넣지 않았다 — `boardKeyForCwd`(statusline)만
  `boards.path` 를 먼저 보므로 경로가 설정된 보드는 이 어긋남에 영향받지 않는다.
- **번호 참조(ref)**: todo/note 는 랜덤 id(`921gvwnr`, PK 로 유지) 외에 보드별 순번을 갖는다.
  id 를 받는 자리는 어디서든 `rocky-12`(보드 접두사) → `12`(현재 보드 컨텍스트 안의
  번호) → id 정확 일치 → id 유일 prefix 순으로 시도해 해석한다(`resolveRef` in
  `src/store.ts`). 구분자가 `-` 인 이유는 `#` 가 GitHub 이슈 번호와 겹쳐서다 — 보드는
  이슈를 만들어 붙일 수 있어 한 항목에 두 종류의 `#N` 이 나타날 수 있었다. 파싱은
  **가장 오른쪽** `-` 에서 갈린다(`rocky-todo-1` = 보드 `rocky-todo` 의 1번). 옛 표기
  `rocky#12`/`#12` 는 **입력으로만** 계속 받는다 — 제품이 내보내는 문자열은 전부 `-`
  형태다. notes 만 board 없이도 존재할 수 있어(글로벌 메모) 전역 번호 공간을 따로 갖고
  예약 접두사를 붙여 `note-3` 으로 렌더된다 — `note-N` 은 board 인자와 무관하게 늘
  전역 메모다. `note` 도 board key 로 만들 수 있다(`api`/`mcp` 와 같은 원칙 — board key 는
  레포 이름에서 유추되는 값이라 생성을 막지 않는다). 다만 `isRefSafeBoardKey('note') ===
  false` 라 그 보드의 항목은 `refOf` 가 `note-N` 대신 raw id 로 폴백한다. todos 는 항상
  보드에 속하므로 보드
  컨텍스트 없는 맨숫자는 에러다. 번호는 보드 안에서 `MAX(number)+1` 로 발급되어
  아카이브해도 회수(재사용)되지 않는다. **댓글은 이 번호 체계 밖이다** — 보드별
  순번 없이 댓글 id 로만 지정한다(`PATCH /api/comments/:id` 등). mutation 은 부모 todo 의
  히스토리(`entity: 'todo'`, action `comment`/`comment-edit`/`comment-archive`/
  `comment-unarchive`)로 기록되어 SSE·훅 주입 경로를 그대로 탄다.
  웹 UI 의 번호 버튼은 참조가 아니라 `/rocky-todo:board rocky-12` 슬래시 커맨드를
  복사한다(`boardCommand` in `src/ui/lib.ts`) — 붙여넣기 한 번이 곧 착수 요청이 된다.
- **핸드오프(보드 → 세션)**: 보드에서 todo 를 실행 중인 Claude Code 세션에 넘긴다.
  데몬은 세션에 밀 수 없다 — `handoffs` 큐에 쌓고 세션 훅이 당겨간다. `Stop` 훅이 집으면
  `decision: block` 으로 그 자리에서 착수하고, `UserPromptSubmit` 훅은 턴이 열릴 때 같은
  큐를 본다. 한 번에 한 건만 배달한다.
  **배달은 턴 경계에서만 일어나므로 idle 세션에는 닿지 않는다** — 턴을 여는 건 handoff 를
  호출한 에이전트 몫이다. `POST /api/todos/:ref/handoff` 는 그래서 `poke: { to, message }`
  (`buildHandoffPoke`)를 함께 돌려주고, 호출자가 그대로 `SendMessage` 로 보내면 그 턴의
  `UserPromptSubmit` 훅이 상세 지시를 주입한다. poke 본문을 늘리지 마라 — 같은 턴에
  주입문이 따로 오므로 내용이 겹친다.
  세션 목록은 `claude agents --json` (`src/sessions.ts`, 주입 가능 `RunCommand`) — `claude`
  CLI 가 없으면 이 기능만 비활성되고(`available: false` + `reason`) 보드 나머지는 정상이다.
  대상은 보드 key ↔ 세션 cwd **경로 세그먼트** 매칭 — 후보가 정확히 1개일 때만 자동으로
  보내고 아니면 사용자가 고른다. 대기 중인 요청에 TTL 은 없다 — 대상 세션이 사라지면
  "세션 없음"(stale)으로 표시만 하고 큐에는 남는다. **MCP 도구는 늘리지 않았다(5개 유지)**
  — 사람이 에이전트에게 넘기는 기능이지 에이전트끼리 일을 미루는 경로가 아니다.
- **핸드오프 라이프사이클 + doing 의 세션 귀속**(user_version 5): 배달(`delivered`)은
  "집어갔다"까지만 말한다. 그 세션이 실제로 착수했는지·끝냈는지는 `setTodoStatus` 가
  채운다 — `start` 가 오면 그 todo 의 *미수락 delivered* 중 가장 오래된 건에
  `accepted_at` 을 찍고 그 `session_id` 를 `todos.doing_session_id` 로 물려주며, `done` 은
  `completed_at` 을 찍고 귀속을 비운다(`stop` 도 비우지만 착수 기록은 남긴다).
  `status` enum 은 늘리지 않았다 — accepted/completed 는 타임스탬프뿐이고 단계는
  `handoffPhase` 가 파생한다(`?status=pending` 을 쓰는 기존 코드가 안 깨진다).
  두 예외: **start 없이 바로 done** 이면 `accepted_at` 을 `completed_at` 과 같이 찍고
  (안 그러면 "끝났는데 미착수"라는 모순이 남는다), **사람이 누른 start 는 귀속하지
  않는다**(그 요청은 여전히 세션이 안 집은 것이다).
  귀속이 필요한 이유는 `/mcp` 가 stateless 라 도구 호출에 세션 식별자가 없고 에이전트가
  자기 `session_id` 를 모르기 때문 — 핸드오프가 그걸 아는 유일한 경로다.
  판정은 `src/doing.ts`(순수): `doingState` 는 `live`(세션 busy) / `idle`(세션은 사는데
  턴이 끝나고 완료가 없다 — **가장 흔한 실패**) / `gone` / `unknown`. 귀속이 없는 doing 은
  보드 근사로 본다 — 에이전트 actor 이고 그 보드 경로에 활성 세션이 **0개**일 때만 `gone`,
  하나라도 있으면 `unknown`(모르는 것과 없는 것은 다르다). 세션 조회는 `doing` 이 하나도
  없으면 건너뛴다. 세션 식별자는 full UUID 와 spawn 의 짧은 8자 id 를 **둘 다** 대조한다.
  "배달됐는데 미착수"(`isUnstarted`)에는 **시간 임계값이 없다** — 세션이 `gone`/`idle` 일
  때만 경고이고 `busy` 면 조용하다. 자동 만료·자동 재배달은 없고 표시만 하며, 다시 보낼지는
  사람이 정한다(새 핸드오프가 생기고 원본은 `delivered` 로 보존). 웹 UI 는
  `/api/handoffs?open=true`(대기 중 + 미완료 배달)로 받는다.
- **statusline 세그먼트(`GET /api/statusline`)**: 보드를 보려고 창을 하나 더 띄우지 않으려는
  표면. `?cwd=&session=` 을 받아 **완성된 한 줄**을 `text/plain` 으로 낸다 — 렌더를 데몬이
  하는 이유는 소비자(Claude Code statusline 명령)를 `curl` 한 줄로 유지하려는 것이다.
  그 자리는 1초마다 × 열어둔 세션 수만큼 도는 유일한 경로라 bun 기동(~30–50ms)을 없애는
  값이 크다. 같은 이유로 이 라우트만 **세션 캐시 TTL 이 15초**다(`statuslineSessions`) —
  다른 라우트의 3초를 쓰면 `claude agents --json`(~220ms)이 3초마다 영구히 도는 배경
  부하가 된다. 세션 목록에서 얻는 건 방치 경고 하나뿐이라 15초 지연은 손해가 없다.
  템플릿 문법은 `{name}` 치환과 `[...]` 옵셔널 그룹 둘뿐이고, **ESC 바로 뒤의 `[`/`]` 는
  리터럴**이다 — 색을 별도 DSL 로 만들지 않고 템플릿에 ANSI 이스케이프를 직접 적게 한
  선택의 대가를 한 줄로 치른 것. 판정은 전부 `src/statusline.ts`(순수)에 있고 라우트는
  재료만 모은다. **실패는 조용하다**(빈 문자열) — 여기서 에러 본문을 내면 사용자
  프롬프트에 JSON 덩어리가 박힌다. 보드 판정은 `boardKeyForCwd` 로 `boards.path` 하위 →
  key 가 경로 세그먼트 순인데, `basename(cwd)` 를 쓰면 워크트리에서 원본 보드를 놓치기
  때문이고 이는 `matchBoard` 와 같은 규약이다. `{mine.*}` 이 핸드오프로 시작된 작업에만
  붙는 것도 같은 이유다 — `doing_session_id` 귀속이 생기는 유일한 경로다.
- **새 세션 띄우기(보드 → 새 워크트리)**: 실행 중인 세션이 없으면 보드가 `claude --bg
  --worktree todo-<번호>` 로 새 백그라운드 세션을 띄운다(`src/spawn.ts`). 워크트리 생성·
  재사용·정리는 전부 Claude Code 몫이고(`<repo>/.claude/worktrees/`, 정리는 `claude rm
  <id>`), 데몬은 이름을 결정론적으로 계산할 뿐이라 "이 todo 의 워크트리" 를 저장하지
  않는다. 대상 레포 경로는 `boards.path`(user_version 4). 그 워크트리에서 이미 도는
  세션이 있으면 **띄우지 않고** 기존 handoff 큐로 넘긴다 — 두 에이전트가 한 워크트리를
  같이 고치는 것을 막는 가드다. 이 가드는 두 겹이다: (1) 이 라우트만 **캐시 없는** 세션
  목록(`spawnSessions` 기본 `listSessions`)을 본다 — TTL 3초 캐시로 보면 spawn 이전
  스냅샷으로 판정하게 된다, (2) `worktreePath → 띄운 시각` 을 60초 기억해
  (`createRecentSpawns`, 데몬 수명 클로저) 그 창 안의 재요청은 **409** 다 — 재사용 분기로
  보내면 짧은 8자 id 로 pending 이 만들어져 full UUID 로 claim 하는 `Stop` 훅에 영영
  배달되지 않는다. (2)는 **실행 전에 잡는 예약**이다(`remember` → 실패 시 `forget`) —
  `await spawnSession` 뒤로 미루면 겹쳐 들어온 두 요청이 게이트를 나란히 통과한다.
  `boards.path` 는 절대경로만 받고 `realpathSync` 로 정규화해 워크트리
  경로 계산·spawn cwd·보드 저장에 **같은 값**을 쓴다(cwd 비교가 정확 문자열 일치다).
  `claude --bg` 실행은 비동기(`Bun.spawn` + await)다 — 최악 30초를 데몬 전체가 멎으면
  안 된다. 파이프는 `new Response(stream).text()` 로 읽지 않는다(detach 된 손자가 fd 를
  물면 영원히 매달린다) — 자식 종료 + 짧은 유예, 또는 timeout 에서 끊는다(`runInDir`).
  `--permission-mode` 는 넘기지 않는다(사용자 기본 설정).
  **이슈 생성과 같은 로컬 요청 전용**(`isLocalRequest`, 403) — 보드 쓰기 권한이 프로세스를
  띄우는 권한으로 확대되는 지점이다. MCP 도구는 여전히 5개다.

## Code review bar

Applies to PR review on this repo (Claude Code Code Review included). **Write every comment in
Korean**; keep identifiers, paths, and commands in English.

**Format** — open the summary with a tally: `🔴 N important / 🟡 M nit / 🟣 K pre-existing`, or
"중요 이슈 없음" when there are no Important findings. Each inline comment is three bullets:
`문제 / 영향 / 제안`, with an applicable fix snippet where possible.

**🔴 Important** — reserved for: a change pulling in anything under *Scope → Out* without an explicit
request; an input/output break in the `worklog_*` tools; `rocky.json` shape changed without `rocky.schema.json` **and**
`crates/rocky-todo-core/src/config.rs` moving in lockstep; a user-facing surface changed without the
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
