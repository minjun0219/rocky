# AGENTS.md

**이 레포에서** 일하는 AI 코딩 에이전트(Claude Code, opencode, codex)를 위한 안내.

> **어디에 무엇이 있나.** 사람은 [`README.md`](./README.md)(표면·설정·환경 변수·빠른 시작)를, 에이전트는
> 이 파일(레이아웃·범위·규칙·체크리스트·리뷰 기준)을 읽는다. 코드만 봐서는 알 수 없는 설계 근거는
> [`docs/architecture.md`](./docs/architecture.md)와 [`docs/daemon.md`](./docs/daemon.md)에 있다 — 기본으로
> 읽지 말고 필요할 때 연다. **도구별 입력·출력은 산문으로 적지 않는다** — 도구 정의(`crates/rockyd/src/mcp.rs`,
> `crates/rocky-cli/src/worklog_mcp.rs`의 `#[tool]`)가 유일한 정본이니 그걸 직접 읽는다.
> 프로젝트를 가리지 않는 관례(언어·커밋 형식·주석 정책)는 사용자 범위 `AGENTS.md`에 있다.

## rocky가 무엇인가

**rocky**(프로젝트 헤일메리의 로키에서 딴 이름) — 오너의 **에이전트 운용 툴킷**이다. 보드·하네스·워크로그에서
시작해, 에이전트를 부리는 데 쓰이는 기능이면 무엇이든 받는다(들이는 방식은 *범위* 참고). 지금 형태는 Rust 데몬 +
CLI(`crates/`)와 그 위의 얇은 Claude Code 플러그인. 옛 `rocky-todo` 레포를 흡수했다(2026-09-22, 두 히스토리를 모두 보존).

- **데몬 `rockyd`**(`crates/rockyd`) — 머신 전체에 하나, `127.0.0.1:8636`, SQLite는 `~/.config/rocky/todo/`.
  보드 REST + SSE, `todo_list` / `todo_write` / `todo_status` / `note_list` / `note_write`와 토큰 색인을 읽는
  `token_summary` / `token_current_session`을 담은 streamable HTTP MCP, **웹 UI**(`web/`, 릴리스 때 `dist/`로 빌드해 바이너리 옆 `dist/`를 `/`에 서빙)를 낸다.
- **CLI `rocky`**(`crates/rocky-cli`) — 얇은 HTTP 클라이언트 + 훅 입구 네 개(`hook ensure-daemon` /
  `notify-todo` / `handoff-stop` / `log-turn`). `bin/rocky`는 플러그인 버전에 맞는 릴리스 tarball을 받아
  바이너리를 실행하는 sh 부트스트랩이다.
- **worklog stdio MCP 서버**(`rocky mcp worklog`, `crates/rocky-cli/src/worklog_mcp.rs`) — 프로젝트별
  `worklog_*` 도구 4개. 워크로그는 호출자의 레포 루트가 키인데 데몬은 호출자의 cwd를 모르므로 데몬이
  아니라 CLI에 있다; 플러그인 stdio 서버가 세션의 프로젝트 디렉터리에서 뜬다. `hook log-turn`(Stop)이 같은
  크레이트로 턴을 덧붙인다. 같은 서버가 **rocky 채널**(`crates/rocky-cli/src/channel.rs`)이기도 하다:
  `claude/channel` 실험 capability를 선언하고 데몬의 `pr-ready` / `pr-conflict` 전이(SSE `/api/events` →
  `/api/changes` 차분, 워터마크는 프로세스 시작 시점)를 `notifications/claude/channel`로 넘겨 세션을 깨운다.
  Claude Code는 `--dangerously-load-development-channels plugin:rocky@rocky-marketplace`로 띄운 세션에만
  전달한다(리서치 프리뷰, 우리 마켓플레이스는 허용 목록에 없다) — 그 밖에서는 알림이 조용히 버려지므로
  capability는 조건 없이 선언한다. 이벤트를 만드는 순수 판정은 `rocky_core::notify::pr_channel_events`
  (훅 주입과 같은 ready·conflict 규칙).
- **TypeScript 서버 코드는 없다.** `package.json` 에는 개발 도구(biome, changesets, `scripts/`·`plugin/scripts/`
  의 릴리스·부트스트랩·permalink 스크립트)와 `web/`의 **브라우저 번들 빌드**(React + zustand + Tailwind v4,
  `bun run build:ui` → `dist/`)만 있다. 머신에서 도는 것은 전부 Rust이고, UI는 데몬이 서빙하는 정적 번들이다.

> **v0.23에서 제거**: `openapi_*`(7), `seo_validate`, `notion_*`(4), 독립 CLI `openapi-mcp`. 39개 레포 ·
> 5,216 턴 로그를 세어 보니 호출이 0이었다. git 히스토리에만 있다 — *범위 → 밖* 참고.

**Claude Code 전용 표면**(MCP 도구가 아니라 Codex/opencode 에는 안 보인다): `plugin/commands/`의 슬래시
커맨드, `plugin/hooks/hooks.json`의 훅(SessionStart · UserPromptSubmit · Stop), `plugin/skills/`의 번들
스킬, `plugin/agents/`의 서브에이전트. 호스트의 한계가 아니라 연결 방식의 선택이다 — `docs/architecture.md`.

> **범위 판단 전에 먼저 읽을 것.** rocky는 지금 가진 도구로 범위가 정해진 제품이 아니라 개인 플러그인이다.
> 지금 표면은 오늘의 기준선이지 **천장이 아니다**. 오너가 어떤 영역이나 기능을 요청하면 **만든다**. 아래
> "선 지키기" 규율은 *요청받지 않은* 범위 확장만 막고, 오너의 명시적 요청을 넘어서지 않는다.
> *EN: The current surface is a baseline, not a ceiling — build what the owner asks for; "hold the line" only blocks unrequested scope creep.*

## 레이아웃

```
rocky/                          단일 패키지 — @minjun0219/rocky
├── .claude-plugin/marketplace.json  ★ 이 레포가 곧 마켓플레이스 — 플러그인 source 는 "./plugin"
├── plugin/                     ★ Claude Code 플러그인 — 플러그인 캐시로 복사되는 유일한 것
│   ├── .claude-plugin/plugin.json  플러그인 메타데이터 + MCP 서버 두 개(rocky = 데몬 http, worklog = stdio)
│   ├── bin/rocky          sh 부트스트랩 → 릴리스 tarball → 네이티브 바이너리(훅 + CLI + MCP 입구)
│   ├── hooks/hooks.json        SessionStart(ensure-daemon), UserPromptSubmit(notify-todo), Stop(handoff-stop → log-turn)
│   ├── commands/ skills/ agents/   슬래시 커맨드, 번들 스킬, 서브에이전트(reviewer · quick-fix(Sonnet) · merge-cleanup(Haiku))
│   └── scripts/permalink.ts    /rocky:review-request 가 쓴다 — 설치 후에도 있으려면 플러그인 안에 있어야 한다
├── Cargo.toml · Cargo.lock     Rust 워크스페이스 — crates/rocky-core · rockyd · rocky-cli
├── web/                        ★ 보드 웹 UI(React 19 · zustand · Tailwind v4) — `bun run build:ui` → dist/(gitignore).
│                                 **UI 를 고치기 전에 `web/DESIGN.md` 를 읽는다**(토큰·정보 우선순위·좁은 패널 규칙의 정본).
│                                 데몬이 바이너리 옆 dist/ 를 `/` 에 서빙한다. types.ts 는 Rust 응답 타입의 사본.
├── bridges/                    수집함 어댑터와 알림 브릿지 — `todo.inbox[]` / `pr.notifiers[]` 에 등록하는 명령
│                                 (stdout JSON 규약, docs/board.md). 외부 서비스 코드는 여기에만; file/ 이 참조 구현.
├── crates/                     ★ 데몬·CLI(worklog MCP + 훅 포함)·코어(docs/rewrite/ 참고)
├── rocky.schema.json           `rocky.json` JSON Schema — crates/rocky-core/src/config.rs 와 함께 움직인다
├── biome.json                  린트·포맷(.sisyphus, .claude 제외)
├── docs/                       architecture, daemon(데몬 모델의 근거), codex, opencode, hosts, backlog, board, rewrite/(포팅 기록)
│   └── design/{specs,plans}/   설계·계획 산출물(구 docs/superpowers/) — 과거분은 그대로 보존
└── scripts/                    Bun 개발 스크립트 — release-github, sync-plugin-version, check-changesets, bootstrap.test
```

## 범위 (선 지키기)

**안** — 위 *rocky가 무엇인가*의 전부, 설정 표면(`rocky.json`, 프로젝트 > 사용자), Claude Code 전용 표면.
표면 세부는 `README.md`, 근거는 `docs/architecture.md`.

**들이는 방식** — 다른 곳(다른 레포·플러그인·스크립트)에 있는 기능을 일괄로 옮기지 않는다. 하나씩, 오너가 정한
것만 들인다. "가져올 만해 보인다"는 근거가 되지 않는다 — 선 지키기와 같은 규율이다.
*EN: Never bulk-migrate features from elsewhere — the owner brings them in one at a time.*

**밖** — 명시적 요청 없이 다시 넣지 않는다:

- mysql / spec-pact / 옛 `pr-watch` **플러그인** / 옛 에이전트·스킬 — `archive/pre-openapi-only-slim`에
  보관. (2026-09-28의 데몬 쪽 PR 감시 — `rockyd::prwatch`, `rocky.json`의 `pr` 블록 — 는 오너의 명시적
  결정으로 만든 다른 것이다; 아래 *데몬·설치 모델* 참고. 둘을 헷갈리지 않는다.) 옛 에이전트(`rocky` /
  `grace` / `mindy`)는 opencode 형식의 **페르소나·라우팅** 에이전트였다; 지금의 `agents/reviewer.md`는
  `/rocky:review-request`가 쓰는 요구사항 점검 역할이지 그들의 부활이 아니다.
- 옛 네이티브 `@opencode-ai/plugin` 표면 — 한때 `.archive/`에 두었다가 제거(필요하면 git 히스토리에서).
  지금의 opencode 지원은 stdio MCP 등록이고 그 부활이 **아니다**.
- `/rocky:opencode` 위임 런타임(`opencode-companion.ts`, `opencode-{jobs,cli,runner,render}.ts`,
  `session-jobs` 훅, `opencode` 설정 블록). v0.19에서 제거 — 잡을 한 번 돌린 1,737 LOC. 제 몫을 할 때만
  git 히스토리에서 되살린다.
- 소울(`souls/`, `soul.ts`, `inject-soul` 훅, `rocky.json`의 `soul` / `callsign`)과 statusline(`statusline/`,
  `statusline.ts`, `sync-statusline`) — v0.19에서 제거. 재미는 있었지만 필수는 아니었다; 소울은 rocky가
  세션 컨텍스트에 넣던 유일한 것이었다(압축 후 605자, 그 뒤 통째로 뺐다).
- `/rocky:codex`와 `/rocky:issue` — v0.19에서 제거. Codex 위임은 공식 `openai/codex-plugin-cc`가 맡는다.
- rocky-todo의 **Tauri 앱**(`app/`, 그 루트 `DESIGN.md`) — 레포를 흡수할 때 rocky-todo 히스토리에 남겼다.
  (`web/DESIGN.md`는 웹 UI의 다른 현행 문서다.) (**웹 UI**는 2026-09-28 오너 요청으로 `web/`에 되살렸다 —
  데몬의 클라이언트이지 런타임이 아니고, 테일넷 없이 Cloudflare Tunnel + Access로 보드에 닿는 것이
  목적이며 그게 다음 조각이다.)
- 데몬의 TypeScript 참조 구현(rocky-todo의 `src/*.ts`) — Rust 크레이트가 구현이고 계약은
  `docs/rewrite/contract.md`.
- **데몬·CLI·훅·스킬·MCP 도구 안의 외부 태스크 서비스 연동.** 오너의 할 일 목록은 rocky 보드이고 기록은
  `worklog_*` 다. 외부 앱(Todoist, Google Tasks, …)은 **별도의 수집함이고 동기화하지 않는다**: rocky는 수집함
  어댑터 규약(`todo.inbox[]` → 명령 → stdout JSON → `GET /api/inbox`)으로 읽기만 하고 링크로 참조한다.
  어댑터 코드는 `bridges/<name>/` 에만 둔다(오너 결정 2026-09-27,
  `docs/design/specs/2026-09-27-bridges-and-tui-design.md`); 서비스 이름이 `crates/`, `plugin/`, 매니페스트
  keywords, MCP 도구에 나오면 위반이다. *EN: External task apps are read-only inboxes, never synced; a service
  name in `crates/`, `plugin/`, manifest keywords or an MCP tool is a violation — adapter code lives only in `bridges/<name>/`.* 옛 `todoist` 번들 스킬은 오너의 비공개 플러그인 레포에 있다.
- 워크로그 다이제스트를 MCP 도구로 노출(`wiki_*`), 독립 CLI의 워크로그, 네이티브 메모리로 자동 승격,
  폴링 기반 자동 다이제스트. 기록 = `worklog_*` + `Stop` 훅; 정리 = `/rocky:recall`만.
- `openapi_*` / `seo_validate` / `notion_*`와 독립 CLI `openapi-mcp` — v0.23에서 사용 집계(39개 레포,
  5,216 턴) 결과 호출 0이라 제거. 4,145 LOC와 런타임 의존성 6개(`swagger-parser`, `swagger2openapi`,
  `js-yaml`, `openapi-types`, `pino`, `ogpeek`)가 같이 빠졌다. git 히스토리에서 되살리고, `ntn` CLI 위임
  형태는 `docs/architecture.md`에 적혀 있다.
- **TUI `rocky-tui`** — 2026-10-02 제거(90일 사용 0회, 오너 결정 "웹 UI에 몰빵"). 보드 화면은 웹 UI 하나다;
  git 히스토리에만 있다. `ratatui`/`crossterm`도 같이 빠졌다.
- npm publish 자동화(GitHub Release ≠ npm publish).

## 자주 쓰는 명령

```bash
bun install         # 의존성 설치
bun run check       # Biome 검증(쓰기 없음)
bun run fix         # Biome 안전 수정 + 포맷
bun run typecheck   # tsc --noEmit
bun run test        # test:unit(scripts·plugin/scripts·bridges·web *.test.ts) + test:dom(web *.test.tsx, happy-dom preload).
                    # 맨 `bun test` 는 preload 가 빠져 DOM 테스트가 실패한다 — 늘 `bun run test`
                    # EN: always `bun run test`; bare `bun test` skips the happy-dom preload and DOM tests fail
bun run build:ui    # web/ → dist/(데몬이 서빙)
bun run e2e         # 웹 UI E2E(`playwright test` — playwright.config.ts, e2e/*.spec.ts) — globalSetup 이 빌드 후 임시 폴더의
                    # 격리 데몬 + 가짜 픽스처를 한 번 띄우고, 폰·cmux·데스크톱(Chromium)과 cmux-webkit(cmux 웹뷰 = Safari
                    # 엔진, `bunx playwright install webkit`) 네 프로젝트가 병렬로 돈다. 화면 점검 발견은
                    # 경고(annotation), `E2E_STRICT=1` 이면 실패. `E2E_NO_BUILD=1` 은 빌드 생략. 브라우저는 받아 둔 Chromium
                    # (`bunx playwright install chromium`)이 맞으면 그것, 아니면 설치된 Chrome(CI 는 Chrome). CI 잡 `e2e (playwright)`.
                    # 테스트는 자기 이름이 든 항목만 만든다. 이슈·세션·핸드오프 버튼은 누르지 않는다
bunx changeset      # 사용자 표면 변경의 버전 의도 선언(patch/minor/major)

cargo fmt --all --check                                   # Rust 포맷
cargo clippy --workspace --all-targets -- -D warnings     # Rust 린트(경고 = 실패)
cargo test --workspace                                    # Rust 테스트
cargo build --workspace                                   # target/debug/{rocky,rockyd}
```

**버전은 함께 움직인다.** `package.json` = `.claude-plugin/plugin.json` = `Cargo.toml`(workspace) =
`Cargo.lock` 멤버. `ensure-daemon`은 데몬이 보고한 `CARGO_PKG_VERSION`을 자기 버전과 정확한 문자열로
비교하고, `bin/rocky`는 `plugin.json` 버전으로 릴리스 tarball을 고른다. `bun run changeset:version`이
`scripts/sync-plugin-version.ts`로 넷을 맞춘다. *EN: The four version fields move together — never bump one by hand.*

더 좁게 돌리는 `lint` / `lint:fix` / `format`도 있다.

**릴리스(changesets).** 사용자 표면이 바뀐 PR은 `bunx changeset`으로 의도를 선언한다(`.changeset/*.md`
커밋). main에 머지되면 `changesets/action`이 `package.json` + `.claude-plugin/plugin.json` 버전 올림과
`CHANGELOG.md` 갱신을 담은 "Version Packages" PR을 연다 — changesets는 `package.json`만 올리므로
plugin.json 동기화는 `bun run changeset:version` 안의 `scripts/sync-plugin-version.ts`가 한다. 그 PR을
머지하면 `scripts/release-github.ts`가 `v<version>` 태그와 GitHub Release를 멱등하게 만든다. npm publish
는 자동화하지 **않는다**.

**Git 훅(husky).** `bun install`이 `prepare: "husky"`로 `core.hooksPath`를 `.husky/_`에 건다.
`.husky/pre-commit`은 `lint-staged`(biome) + 비밀 스캔(`gitleaks protect --staged`, 없으면 내장 grep),
`.husky/pre-push`는 빠른 검사만 돈다 — `typecheck` + `test`(bun) + `cargo fmt --check` + `cargo clippy`(전체 `cargo test`는 CI 몫,
2026-10-02 — 푸시마다 20~30분 걸려 SSH가 끊겼다). `~/.cargo/bin`이 PATH에 있어야 하고, 도는 동안 체크아웃·파일 수정을 하지 않는다(훅의 테스트는 지금 작업 트리를 돈다).
여러 브랜치는 `git push origin a b c` 한 번이면 훅도 한 번이다. `--no-verify`로 건너뛸 수 있다. CI는 같은 게이트에
`gitleaks` 잡을 더해 다시 돌린다. 추적하는 것은 `.husky/pre-commit`과 `.husky/pre-push` 뿐 — `.husky/_`는
husky가 스스로 gitignore 한다.

**typecheck 나 테스트를 도는 Stop/PostToolUse 훅을 추가하지 않는다** — pre-push와 CI가 이미 결정적으로
막고, 턴마다 게이트를 돌리면 모든 턴이 느려질 뿐이다. *EN: Do not add a Stop/PostToolUse hook that runs
typecheck or tests — pre-push and CI already cover it.*

## 코딩 규칙

- **언어 경계(2026-10-02 오너 확인)**: 가운데는 Rust — 데몬·CLI·훅·MCP·저장소(`crates/`, edition 2021,
  `rust-toolchain.toml`의 stable). 가장자리는 언어 자유 — 웹 UI(`web/`, TS), 브릿지·수집함 어댑터(`bridges/`, stdout
  JSON 규약), 로그 분석(`logs.db`를 읽기만), 개발·릴리스 스크립트(Bun). 데몬을 다른 언어로 옮기지 않는다: core와 FFI
  경계가 수백 개 생기고, 런타임·의존성 동봉(플러그인 캐시 부분 설치 사고)과 데몬 교체·버전 맞추기가 두 벌이 된다.
  느리면 언어보다 측정이 먼저(`rocky usage`). *EN: Rust core (`crates/`) stays; new work in another language goes to the
  edges (web, bridges, scripts reading logs.db).*
- **Rust 규칙**: `cargo fmt` + `cargo clippy --workspace --all-targets -- -D warnings`가 게이트다(clippy는
  테스트도 본다). 순수 판정 로직은 `rocky-core`에, 통합 테스트는 `crates/*/tests/it/` 에(크레이트당 실행 파일 하나 — 새 파일은 `tests/it/main.rs`에 `mod`로 단다); 데몬과 CLI는 배선만
  한다. 훅은 fail-open — 훅 입구에서 `Result`를 내보내지 않는다. 에러에는 맥락(입력값·경로·상태 코드)을 담는다.
  *EN: Pure logic in `rocky-core`, wiring in the daemon/CLI; hooks fail open; errors carry the input, path or status.*
- **의존성**: 추가하지 않는 쪽으로. 워크스페이스 의존성은 루트 `Cargo.toml`에 한 번만 선언하고,
  `Cargo.lock`에 이미 있는 크레이트를 먼저 쓴다(예: 새 `sha1` 대신 SHA-1은 `ring`). 새 런타임 의존성은
  별도 범위 논의다. 개발 전용 Bun 도구는 괜찮다. *EN: Avoid new dependencies; a new runtime dep is a separate
  scope decision.*
- **TS 스크립트**: 로컬 import에 `.js` / `.ts` 확장자를 붙이지 않고, `__dirname` 대신 `import.meta.dir`,
  테스트는 스크립트 옆 `*.test.ts`, 파일 시스템 격리는 `mkdtempSync`.
- **계약 충실도**: 디스크의 워크로그(JSONL 모양, 키 순서, 프로젝트 키 `<basename>-<sha1[:8]>`)와 보드
  REST/MCP 표면(`docs/rewrite/contract.md`)은 옛 TypeScript 구현과의 호환 계약이다 — 골든 테스트가 고정한다
  (`crates/rocky-core/tests/it/worklog_test.rs::project_key_matches_ts_golden`). *EN: The on-disk worklog and the
  board REST/MCP surface are compatibility contracts pinned by golden tests — do not change their shape.*

## 변경 체크리스트

1. `bun run check`, `bun run typecheck`, `bun run test`와 `cargo fmt --all --check`,
   `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`가 모두 통과한다.
2. 사용자 표면(도구·환경 변수)이 바뀌면 `README.md`(사람)와 이 파일(에이전트)을 맞추고, Claude Code 표면이
   바뀌면 `.claude-plugin/plugin.json`도.
3. 새 환경 변수 → 읽는 자리(`crates/rocky-core/src/config.rs` / `crates/rocky-cli/src/hooks.rs` /
   `worklog_mcp.rs`)와 `README.md` 환경 변수 표를 갱신.
4. 도구 계약 변경 → `#[tool]` 정의(보드는 `crates/rockyd/src/mcp.rs`, 워크로그는
   `crates/rocky-cli/src/worklog_mcp.rs`)와 짝 테스트(`mcp_test.rs` / `worklog_mcp_test.rs`)를 갱신.
5. `rocky.json` 모양 변경 → `rocky.schema.json` **과** `crates/rocky-core/src/config.rs`를 함께.
6. 도구 이름이 다시 나타남 → 표면 테스트가 도구 목록을 정확히 고정한다(`crates/rockyd/tests/it/mcp_test.rs`,
   `crates/rocky-cli/tests/it/worklog_mcp_test.rs`의 `TOOLS`); 제거한 이름(openapi / seo / notion / mysql /
   spec-pact / pr-watch)은 돌아오면 안 된다.
7. 사용자 표면 변경 → `bunx changeset`. 도구 정비만 하는 잡일은 필요 없다.
8. 표면을 빼거나 모양을 바꿈(도구·라우트·커맨드·훅·웹 동작) → PR 본문에 `rocky usage --since 90d`(횟수,
   마지막 사용)를 인용한다. 사용 로그(`rocky_core::usage`, `~/.config/rocky/usage/*.jsonl`)는 "아무도 안
   쓴다"를 기억이 아니라 숫자로 만들려고 있다. 새 표면은 `KNOWN_SURFACES`에 넣어 쓰이기 전까지 안 쓴
   표면으로 보이게 한다.

## 데몬·설치 모델

규칙만 적는다 — 이유·사고 경위·세부 동작은 [`docs/daemon.md`](./docs/daemon.md)에 있다. 순수 판정은
`rocky_core::*`, HTTP·프로세스 배선은 `rockyd::*`, 훅과 CLI는 `rocky_cli::*`. 규칙을 바꾸면 두 곳을 같이 고친다.

- **설치 = 활성화.** `todo.enabled`는 없다; 끄려면 `claude plugin disable rocky`.
- **터미널의 `rocky`.** 부트스트랩이 `~/.local/bin/rocky` → `~/.local/share/rocky/current/rocky` 링크를 건다
  (`link_cli`, 남의 실제 파일은 덮지 않는다); `rocky config show`가 링크·PATH 누락을 알려 주고 `rocky config
  link` 가 고친다. 옆 바이너리는 canonicalize 한 실제 파일 옆에서 찾는다.
- **데몬 기동.** SessionStart의 `hook ensure-daemon`이 health가 없으면 detached로 띄운다; CLI도 필요할 때
  띄운다; `rocky daemon install`이 상주시킨다(launchd KeepAlive). `rocky daemon restart`는 버전과 상관없이 같은 교체 경로(`RestartPolicy::Always`)를 탄다.
- **`rocky update [--check]`**는 마켓플레이스 갱신 → `claude plugin update` → **새 버전 폴더의 부트스트랩**
  (`<cache>/rocky/<최신>/bin/rocky hook ensure-daemon`)으로 데몬 교체를 한 번에 한다 — 지금 도는 `rocky`는 옛
  바이너리라 자기 자신으로는 새 바이너리를 못 받는다. 목표 버전은 GitHub 최신 릴리스 태그. 할 일 수정은 `rocky edit`
  이다(예전 이름이 `update` — REF·수정 플래그가 붙은 `rocky update`는 업데이트를 돌리지 않고 `edit`으로 안내한다).
  0.36.0에 나간 `rocky upgrade`는 한 릴리스 동안 숨은 별칭이다 — 다음 릴리스에서 걷는다.
- **버전 인식 재기동.** 훅이 `/api/health`의 `version`을 자기 `CARGO_PKG_VERSION`과 정확한 문자열로 비교해
  낡은 데몬을 교체한다 — pid로 SIGTERM, 상주 중이면 launchd job을 다시 설치. 옛 데몬을 못 내리면 재기동하지
  않는다(구버전 보드가 보드 없음보다 낫다). *EN: If the old daemon cannot be stopped, do not restart — an old
  board beats no board.* `name`은 `"rocky"` 여야 한다. 버전이 같으면 경로가 달라도 두는
  것이 의도다. `UserPromptSubmit`(`notify-todo`)이 `RestartPolicy::OnlyIfOlder`로 같은 검사를 해서
  `/reload-plugins`가 세션끼리 뒤집히지 않고 올린다. 교체의 각 단계는 실패를 삼키지 않고 보고한다; 데몬이
  사라지고 말았으면 launchd 밖에서라도 띄우고 `⚠ rocky 데몬: …` 경고를 주입한다.
- **launchd 밖의 고아를 만들지도 두지도 않는다.** job이 로드돼 있으면 CLI·훅은 따로 띄우지 않고 `launchctl kickstart`
  로 맡긴다; 교체 뒤엔 포트의 pid가 job의 pid와 같고 목표 버전인지 확인하고(다르면 그 고아를 pid로 내린다),
  `daemon restart`·`update`는 버전이 목표와 다르면 실패로 끝난다; `daemon status`는 둘이 다르면 ⚠. launchd가 띄운
  데몬은 포트가 차 있으면 끝나지 않고 기다렸다 이어받는다. 재현은 `ROCKY_LAUNCHD_LABEL` + 전용 `ROCKY_CONFIG`.
- **첫 세션 순서**(SessionStart ↔ http MCP 초기화)는 보장되지 않는다 — 첫 세션의 MCP `failed`는 `/mcp`
  재시도, 다음 세션, launchd로 풀린다.
- **전역 단일 인스턴스.** 포트가 락이다; 사용자 `rocky.json`의 `todo` 블록만 적용된다. 새 데몬은 **포트를 먼저 잡고**
  `daemon.pid`의 옛 rockyd가 끝난 뒤에야 DB를 연다(마이그레이션 포함, 15초 넘게 안 끝나면 DB를 열지 않고 멈춘다) —
  업그레이드 때 종료 중인 옛 데몬과 새 데몬의 마이그레이션이 겹쳐 실제 DB가 손상된 적이 있다(2026-09-30). 기동 때
  `quick_check` 결과는 `/api/health`의 `dbIntegrity`.
- **데모·개발 데몬**은 `ROCKY_CONFIG=<전용 파일> cargo run -p rockyd`로 띄운다(자기 포트·`dir`·
  `expose: "off"`) — 전역 `expose`를 물려받지 않게.
- **tailscale serve 자동 확보는 남의 노출을 빼앗지 않는다.** `decide_serve_action`: `claim`(빈 자리),
  `keep`(내 것), `yield`(살아 있는 다른 rocky 데몬), `reclaim`(죽은 포트). 수동 `rocky tailscale on`은 가드하지
  않는다.
- **로컬 요청 전용 동작.** 이슈 생성, 세션 띄우기, 보드의 `path` / `repo` / `reviewFix` / `prAuthors` 변경, 보드 수집함 설정(`/api/inbox/adapters`·`sources` 쓰기 — 값이 실행 인자가 된다)은
  `is_local_request`가 필요하다: 루프백 peer **이고** 프록시 헤더(`x-forwarded-*`, `forwarded`,
  `tailscale-user-*`, `cf-*`)가 없어야 한다; peer 주소가 없으면 거부(fail-closed). *EN: Anything that writes to GitHub, spawns processes or steers
  sessions is local-only: loopback peer and no proxy headers; fail closed.*
- **cross-site 변경은 라우팅 전에 끊는다**(`is_cross_site_request`): `Sec-Fetch-Site: cross-site`(없으면
  `Origin`으로 판단)인 변경 메서드는 REST와 `/mcp`에서 403; 헤더가 둘 다 없으면 비브라우저 클라이언트로 보고
  통과시킨다. 읽기는 막지 않는다. *EN: Block cross-site mutations before routing (`Sec-Fetch-Site` first); never
  block reads.*
- **보드 메타**(`update_board`)는 key/title/description/repo/path/reviewFix/prAuthors를 한 트랜잭션으로 고친다;
  `null`은 지우기, 빈 문자열은 400. key를 바꾸면 옛 key를 `board_aliases`에 남긴다(입력 전용 — 출력은 늘
  새 key, 쓴 key는 은퇴). `match_board`는 현재 key만 본다.
- **노트는 CRDT 문서다**(`rocky_core::note_doc`, `yrs`): 데몬이 CRDT 피어라 에이전트와 CLI는 Yjs를 모른다 —
  `set`은 최소 편집, `append`는 끝에 삽입. `notes.content`는 읽는 쪽의 진실, `note_docs.state`는 합치는 쪽의
  진실; state가 전진했을 때만 저장하고 본문이 바뀌었을 때만 content·히스토리를 고친다; 셋은 한 트랜잭션.
  스토어가 `NoteDocEvent`를 내고 서버가 노트별로 방송한다(라우트는 방송하지 않는다). 웹은 노트 소켓 하나(`GET /api/ws`, 핸드셰이크에 cross-site 가드)로 오가고
  밀리면 `lag`로 차분을 다시 받게 한다; HTTP 라우트·노트별 SSE(밀리면 끊는다)는 폴백. 제목은 CRDT가 아니다. 고정(`pinned_at`, REST `pin`/`unpin`·CLI)은 보여 주는 방식이라 MCP 도구로는 바꾸지 않는다(출력의 `pinnedAt`은 실린다).
- **번호 참조(ref).** todo/note는 보드별 번호를 갖는다: `rocky-12` → `12`(보드 맥락) → id 정확 일치 → id
  prefix 순으로 푼다(`resolve_ref_id`), 가장 오른쪽 `-`에서 가른다. 옛 `#12` 표기는 입력 전용. `note-N`은 늘
  전역 메모. 번호는 재사용하지 않는다. 댓글에는 번호가 없다.
- **핸드오프(보드 → 세션).** 데몬은 `handoffs` 큐에 쌓고, `Stop` 훅이 한 번에 하나씩 집고 `UserPromptSubmit`
  이 턴 시작에 본다. 보관된 todo는 건너뛴다. 쉬는 세션은 깨워야 한다 — 대상 세션이 받은편지함 소켓을
  등록했으면 데몬이 그 문구(`poke`)를 바로 꽂아 턴을 열고(응답 `woke: true`, "보내지 않기" 와 무관), 아니면 라우트가
  `poke`를 돌려준다 — 그 문구를 늘리지 않는다. 대상 = cwd가 보드와 맞는 세션이 하나일 때 그 세션; 아니면 사용자가
  고른다. TTL은 없다. 핸드오프는 MCP 도구를 늘리지 않는다.
- **핸드오프 라이프사이클 / doing 귀속.** `start`가 가장 오래된 배달 건을 수락하고 `doing_session_id`를
  귀속시킨다; `done`이 완료하고 비운다; 사람이 누른 `start`는 귀속하지 않는다. `resolve_doing_state` →
  `live` / `idle` / `gone` / `unknown`. `rockyd::sweep`는 에이전트가 든 `gone` doing 중 24시간 지난 것만
  자동으로 멈추고 이유를 댓글로 남긴다(`should_auto_release`); 사람이 든 것·`idle`·`unknown`은 건드리지 않는다.
  핸드오프 주입문은 착수(`start`)와 함께 닫는 법(`done`/`stop`)을 말한다. `Stop` 훅(`handoff-stop`)은 이 세션에 귀속된
  doing이 있으면 턴을 한 번 막고 닫았는지 묻는다(`held_todo_reminder`) — `stop_hook_active` 인 턴은 다시 막지 않아 루프가 없고, GitHub PR을 링크한 할 일은 머지를 기다리는 중이라 묻지 않는다(그 PR이 머지 없이 닫혀도 다시 묻지 않는다 — 보드의 댓글로만 안다).
  같은 귀속으로 `log-turn`은 턴 기록 태그에 `todo:<ref>`를 붙인다(보드 할 일 상세가 작업 흐름을 이 태그로 모은다).
  하네스가 넣은 메시지(`<task-notification>`·셸 출력)는 턴을 나누되 요청 칸엔 짧은 이름만 남긴다(`label_injected`).
- **PR 감시**(`rockyd::prwatch`)는 **구독한 PR 만**(`pr_subscriptions` — `rocky pr subscribe N`, `/rocky:review-request`·
  `review-fix`가 구독한다, 머지·닫힘에서 풀린다) `pr.intervalMinutes` 마다 상세 쿼리로 보고 `pr-*` 전이(actor `rocky`)를
  그 레포를 둔 보드 히스토리에 남긴다. 레포 목록은 보지 않는다 — 보드의 `repo`는 감시 대상을 정하지 않는다. 구독은
  `POST/DELETE /api/prs/subscriptions`(로컬 전용) · `GET`은 열려 있다. **필터 구독**(`rocky pr subscribe --filter
  "repo:o/r author:@me"`, `/api/prs/filters`)은 GitHub 검색 조건이다 — tick 마다 검색 한 번(`is:pr is:open` 을 데몬이
  붙인다, `gh` 에는 `-f`로 — `-F`는 `@me`를 파일로 읽는다)으로 걸린 열린 PR을 그 세션 구독으로 넣는다. 이미 누가
  구독한 PR은 빼앗지 않고, 필터를 해지하면 그 필터로 들어온 구독도 걷힌다(직접 구독하면 필터 출처가 지워진다). GitHub은 읽기만 한다(*EN: the daemon never writes to GitHub*). 전달: macOS 배너(`pr.notify`), 세션
  받은편지함(`pr.sessionNotify` — 훅이 `CLAUDE_CODE_MESSAGING_SOCKET`을 `POST /api/sessions/inbox`로
  등록하고, 데몬이 **그 PR을 구독한 세션**에만 JSON 한 줄을 쓴다 — 그 세션이 끝났거나 "보내지 않기" 면 보내지 않고 다른 세션으로 넘기지 않는다), 브릿지(`pr.notifiers[]`, 코드는
  `bridges/<name>/` 에만). `pr-review`는 보드의 `reviewFix`가 켜졌을 때만 세션에 간다. 받은편지함 등록은 `session_inboxes`에도 남겨 데몬이 다시 떠도 되살린다(다시 뜬 뒤 첫 tick이 세션의 재등록보다 먼저 돌아 알림이 버려졌다, 2026-10-05) — 되살린 등록은 훅이 다시 등록하기 전까지 그 세션이 살아 있고 소켓 이름의 pid가 그 세션의 것일 때만 쓴다(`restored_registration_live`, 캐시 없는 세션 목록; 못 읽으면 보내지 않는다 — 소켓 경로는 다른 세션이 다시 쓸 수 있다). 세션 전달은 `GET /api/deliveries`(받는 세션·최근 50건, 메모리, 못 보낸 건은 `reason`)로 보이고, `POST /api/deliveries/mute`로 세션별 "보내지 않기"(PR 알림은 버리고 수집함 알림은 미룬다, 메모리) — 둘 다 로컬 전용. `pr-merged`는 배너·브릿지 없이 세션에만 간다(머지 뒤 정리 — `/rocky:review-fix` 11단계). `pr-ci-failed`(CI가 실패로 바뀜 — 같은 head에서 한 번, 재실행이 또 실패하면 또)도 세션에만 간다(원인을 보고 재실행 한 번 또는 수정 — 12단계). 리뷰·충돌·CI 실패 메시지를 받은 세션이 다른 작업 중이면 워크트리 서브에이전트에 맡긴다(13단계, 판단이 필요한 👀 는 메인이 묻는다). 보드 `prAuthors`(`@me`·login)에 걸린 전이는 `quiet`로 기록만 되고 세션·배너·브릿지·훅 주입을 건너뛴다(보기는 넓게, 깨우기는 좁게). 구독은 그 레포를 기준선이 잡힌 레포로 표시해, 구독한 뒤 첫 tick이 지금 상태(머지 후보·CI 실패 등)를 알린다 — 처음 본 PR이 이미 머지·닫힘이면 그 전이(`Merged`/`Closed`)도 첫 tick에 온다(첫 tick 전에 머지된 PR, 머지된 PR을 구독한 경우). `ready`는 **머지
  후보**다 — 세션이 사용자에게 알리기 전에 판단한다(`/rocky:review-fix` 8단계); 머지 뒤에 붙은 리뷰는
  다음 PR로 간다(`after-merge`). **PR ↔ 할 일**: 머지·닫힘 전이가 오면 그 PR 주소를 `links`에 둔 할 일(보관 제외, 전 보드)을 정리한다(`rocky_core::prwatch::linked_todo_action`, `rockyd::prwatch::settle_linked_todos`) — 머지면 완료 + 댓글(링크한 다른 PR이 아직 구독 중이면 완료하지 않고 댓글만), 머지 없이 닫히면 댓글만, 이미 끝낸 할 일은 그대로, 한 tick에 같은 할 일의 PR이 함께 머지돼도 완료는 한 번. 링크는 `/rocky:review-request`가 세션이 든 할 일에 붙인다. **예산:** GraphQL 비용은 돌려받은 노드가 아니라 `first:`로 요청한 노드 수다 —
  레포당 `PR_LIST_QUERY`(상태 조각) 한 번 + 실제로 열린 PR 에만 `detail_query`; 잔여가 `RATE_LIMIT_FLOOR`
  (1,000) 밑이거나 한도 에러면 리셋까지 쉰다(`pause_for`). 주기를 바꾸기 전에 `rateLimit { cost }`를 잰다. *EN: GraphQL cost is the nodes requested, not returned —
  measure `rateLimit { cost }` before changing the cadence; the budget is shared with every session's `gh`.*
- **수집함 "이미 올라감"**은 데몬 한 곳에서 판정한다(`mark_promoted`): 항목 url이 **어느 보드든**(보관 포함)
  todo 링크에 있으면 `promoted`. 요약·웹·`rocky inbox`가 이 값을 본다 — 소비자마다 다시 판정하지 않는다.
  세션 시작 요약은 수집함 캐시만 본다(어댑터를 기다리지 않는다); 📥 제목은 외부 글이라 한 줄로 펴서 자른다.
  보드 수집함은 **실행할 명령은 설정 파일(`todo.inboxAdapters[]`), 값은 화면**이다 — 화면 값은 어댑터의
  `--describe` 칸으로만 검증해 받고(`rocky_core::inbox::validate_params`), 명령 자체를 화면이 바꾸게 하지 않는다.
  **수집함 구독**(`rocky inbox subscribe`)은 세션을 소스의 구독자로 적고(`inbox_subscriptions`, 기준선은 `inbox_seen`),
  `rockyd::inbox_watch`가 구독된 소스만 5분마다 읽어 새 항목을 그 세션 받은편지함에 보낸다 — 알리기만, 착수는 사람.
- **로그 색인**(`rocky_core::logindex`, `rockyd::logindex`): 작업로그·사용 로그는 **JSONL이 진실**이고 데몬은 그걸
  `logs.db`(todo 폴더, `todo.db`와 별도 파일)로 옮겨 읽기만 한다 — 쓰기 경로는 그대로다(데몬이 꺼져도 기록이 남는다).
  **전용 OS 스레드**가 기동 때와 1분마다 파일별 바이트 위치로 새 줄만 옮긴다(작업로그는 `id`, 사용 로그는 `(파일, 위치)`
  가 키라 다시 읽어도 중복이 없다). 지워도 다시 만든다. 조회는 `GET /api/logs/worklog` — `spawn_blocking` + 자기 연결.
  보드 ↔ 레포는 보드 `path`로 `default_project_key`를 계산한다. 근거·범위는 `docs/design/specs/2026-10-02-log-index-design.md`.
  *EN: JSONL is the source of truth; logs.db is a rebuildable index written by a dedicated thread — never route writes through the daemon.*
- **토큰 색인**(`rocky_core::tokens`): 같은 스레드가 Claude Code 트랜스크립트(`~/.claude/projects/**/*.jsonl`,
  `rocky.json`의 `tokens.dir`)도 `cc_*` 표로 옮긴다 — 모델·effort·토큰은 훅 입력에 없고(토큰은 아예 없고 모델은
  `SessionStart`에만) 트랜스크립트 줄마다 있어서, **훅을 걸지 않는다**. 같은 `message.id`가 content 블록마다 반복되므로
  메시지 id로 한 번만 세고, 턴 경계는 사람이 쓴 프롬프트(`isMeta`·압축 요약 제외), 서브에이전트는 토큰엔 넣고 턴 수·추천에서
  뺀다. 조회는 `/api/tokens/{summary,current,sessions/:id,recommendation}`, MCP 두 도구, `rocky tokens`. 추천은 규칙 v1
  세 가지(`tokens.recommend`로 조정)이고, 색인 스레드가 **낸 규칙이 바뀐** 세션만 `GET /api/tokens/events`로 민다(첫 바퀴는
  과거 가져오기라 기준선만). 이 스트림을 전역 `/api/events`와 나눈 것은 그쪽 구독자가 `data:`마다 보드를 다시 읽기 때문이다.
- **기본 브랜치 검증**(`rocky_core::verify`, `rockyd::verify`, opt-in `rocky.json` `verify.targets[]`): 원격 브랜치를 `git ls-remote`로 보고
  새 커밋이면 `<todo dir>/verify/<board>/<branch>/tree`(보드 레포의 **detached** 워크트리 — 사람·세션의 작업 트리·브랜치를 건드리지 않고,
  레포 전체 `worktree prune` 은 하지 않는다)에서 단계(argv, 설정 파일에만)를 차례로 돈다. 보드 레포의 git 훅은 돌리지 않는다(`core.hooksPath=/dev/null`).
  잡 하나가 대상을 차례로 — 동시 1개, 몰린 커밋은 최신 하나. **fetch·워크트리 준비 실패는 커밋을 빨강으로 남기지 않고** 다음 바퀴에 다시.
  단계는 자기 프로세스 그룹으로 띄우고, 남지 않게 세 겹: 시간 초과면 TERM → 5초 → KILL, 데몬이 작업을 버리면 가드가 KILL, 데몬이 죽어
  남은 그룹은 `running.pgid` 로 다음 실행 전에 끝낸다. 기록은 `last.json`(도는 중이면 다음 기동에 같은 커밋을 다시)과 `finished.json`(알림
  기준 — 끊겼다 다시 돈 실행도 복구를 알린다). 실패·복구만 배너. 조회는 `GET /api/verify`·`rocky verify`. 히스토리·웹 "지금"·세션
  받은편지함에는 아직 싣지 않는다.
- **statusline 세그먼트**(`GET /api/statusline`)는 한 줄 전체를 데몬이 렌더링한다; 이 라우트만 세션 캐시
  TTL이 15초; 실패하면 빈 문자열. 보드는 `board_key_for_cwd`로 정한다. 끼워 넣는 쪽은 `rocky statusline`(`--cwd`·`--session`, 없으면 stdin JSON) — 1초마다 도는
  자리라 사용 로그·데몬 자동 기동을 거치지 않고, 300ms 안에 못 받으면 조용히 빈 출력.
- **세션 띄우기**(`boards.path`에서 `claude --bg --worktree todo-<n>`, `rockyd::spawnctl`)는 그 워크트리에
  살아 있는 세션이 있으면 띄우지 않고 그 세션을 쓴다; 캐시 없는 세션 목록과 60초 `RecentSpawns` 예약(창 안의
  재요청은 409)이 이중 기동을 막는다. `boards.path`는 절대경로여야 하고 canonicalize 한다. 실행은 비동기,
  30초 timeout, `kill_on_drop`; `--permission-mode`는 넘기지 않는다.

## 리뷰 기준

이 레포의 PR 리뷰(Claude Code Code Review 포함)에 적용한다. **모든 코멘트는 한국어로** 쓰고 식별자·경로·
명령은 영어 그대로 둔다.

**형식** — 요약 첫 줄에 집계를 쓴다: `🔴 N important / 🟡 M nit / 🟣 K pre-existing`, Important가 없으면
"중요 이슈 없음". 인라인 코멘트는 `문제 / 영향 / 제안` 세 불릿으로, 가능하면 적용할 수 있는 수정 스니펫과 함께.

**🔴 Important** — 이것에만 쓴다: 명시적 요청 없이 *범위 → 밖*의 것을 들여오는 변경; `worklog_*` 도구의
입출력 파손; `rocky.schema.json` **과** `crates/rocky-core/src/config.rs`가 함께 움직이지 않은 `rocky.json`
모양 변경; *변경 체크리스트*의 문서 동기화 없이 바뀐 사용자 표면; `__dirname`, 로컬 import의 `.js`/`.ts`
확장자, Bun 전제를 깨는 Node 전용 API; 로그 속 비밀, 식별 맥락이 빠진 에러 메시지, 검증 안 된 외부 입력으로
만든 파일 경로; 표준 라이브러리나 Bun 내장으로 될 일에 새 런타임 의존성. 그 밖은 기껏해야 Nit.

**🟡 Nit** — 인라인은 5개까지; 나머지는 요약에 `유사 항목 N 개 더`로 접는다. 스타일, 이름, JSDoc 누락,
테스트 파일 위치(소스 옆 `*.test.ts`), `mkdtempSync` 격리 누락.

**보고하지 않는다** — `bun.lock`과 `.gitignore` 된 모든 것; `cargo clippy` / `cargo test` / `bun run test`가
이미 잡는 타입 에러와 테스트 실패(예외: `crates/*/tests/it/`에 테스트가 없는 새 `crates/*/src/` 모듈은 Nit);
`docs/backlog.md` 항목을 들여오라고 *명시적으로 요청받은* PR은 그것만으로 범위 위반이 아니다.

**인용 기준** — 동작에 대한 주장("이 코드는 X를 한다")에는 이름에서 추론한 것이 아니라 `path:line` 인용이
필요하다. 이름만 보고 Important를 올리지 않는다.

**재리뷰 수렴** — 같은 PR의 두 번째 리뷰부터는 새 Nit를 올리지 않는다: Important와 새로 생긴
Pre-existing만.

## 플러그인 소스와 개발 루프

**이 레포가 플러그인 소스이자 자기 마켓플레이스다** — 별도 파사드 디렉터리는 없다.
`.claude-plugin/marketplace.json`이 유일한 마켓플레이스이고 플러그인 `source`는 상대 경로 `"./plugin"`이다.
알려진 한계: claude.ai 웹 UI의 서버 쪽 마켓플레이스 동기화는 레포를 clone 하지 않아 상대 source가 실패한다 —
받아들인 트레이드오프이고 CLI로 설치한다.

```bash
claude plugin marketplace add minjun0219/rocky
claude plugin install rocky@rocky-marketplace
```

설치는 GitHub `main`을 플러그인 캐시로 clone 한다 — 작업 트리에서 읽지 **않는다**. 개발 루프는 푸시 기반:
고치기 → `main`에 푸시 → `claude plugin update rocky@rocky-marketplace`. `/reload-plugins`는 커밋하지 않은
수정을 보지 못한다. 작업 트리로 세션을 돌리려면 `claude --plugin-dir <repo>`.

**여기에 `.mcp.json`이 없는 이유:** 설치된 플러그인 루트가 이 레포 루트의 clone이라, 레포 루트의
`.mcp.json`은 `plugin.json`의 `mcpServers` 위에 *설치된* 플러그인의 MCP 설정으로 새어 들어간다. 그런
서버는 사용자 범위에 둔다.

**단일 정본**: 사람 = `README.md`, 에이전트 = 이 파일, 근거 = `docs/architecture.md`·`docs/daemon.md`, 도구별
계약 = 도구 정의 자체. 루트에 형제 문서를 새로 만들지 않는다 — `FEATURES.md`와 `REVIEW.md`가 이 셋으로
접히기 전에 바로 그랬다. *EN: Do not add a new root-level sibling doc.*
