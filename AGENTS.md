# AGENTS.md

**이 레포에서** 일하는 AI 코딩 에이전트(Claude Code, opencode, codex)를 위한 안내.

> **어디에 무엇이 있나.** 사람은 [`README.md`](./README.md)(표면·설정·환경 변수·빠른 시작)를, 에이전트는
> 이 파일(레이아웃·범위·규칙·체크리스트·리뷰 기준)을 읽고, 기능을 고칠 땐 그 기능의 [`docs/features/`](./docs/features/)
> 문서를 연다. 코드만 봐서는 알 수 없는 설계 근거는
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
  (훅 주입과 같은 ready·conflict 규칙, 이 세션이 구독한 PR만 — 세션은 서버의 부모 pid로 `/api/sessions`에서
  찾는다; env의 `CLAUDE_CODE_SESSION_ID`는 `/clear` 뒤 낡는다).
- **TypeScript 서버 코드는 없다.** `package.json` 에는 개발 도구(biome, changesets, `scripts/`·`plugin/scripts/`
  의 릴리스·부트스트랩·permalink 스크립트)와 `web/`의 **브라우저 번들 빌드**(React + zustand + Tailwind v4,
  `bun run build:ui` → `dist/`)만 있다. 머신에서 도는 것은 전부 Rust이고, UI는 데몬이 서빙하는 정적 번들이다.
  예외 하나: `plugin/hooks/lab/`은 Claude Code 프로세스 안에서 도는 function hooks 모듈(실험, 기본 꺼짐)이다 — 데몬을
  읽어 그리기만 하는 가장자리 클라이언트라 웹 UI 와 같은 자리다([lab](./docs/features/lab.md)).

> **v0.23에서 제거**: `openapi_*`(7), `seo_validate`, `notion_*`(4), 독립 CLI `openapi-mcp`. 39개 레포 ·
> 5,216 턴 로그를 세어 보니 호출이 0이었다. git 히스토리에만 있다 — *범위 → 밖* 참고.

**Claude Code 전용 표면**(MCP 도구가 아니라 Codex/opencode 에는 안 보인다): `plugin/commands/`의 슬래시
커맨드, `plugin/hooks/hooks.json`의 훅(SessionStart · UserPromptSubmit · Stop)과 function hooks 모듈(`modules` →
`plugin/hooks/lab/`, 실험), `plugin/skills/`의 번들 스킬, `plugin/agents/`의 서브에이전트. 호스트의 한계가 아니라 연결 방식의 선택이다 — `docs/architecture.md`.

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
│   ├── hooks/lab/ types/       function hooks 모듈(실험 — rocky.json `lab` 블록일 때만) + 그 `$.state` 계약. tsconfig.json 은 엔진 타입을 extends
│   ├── commands/ skills/ agents/   슬래시 커맨드, 번들 스킬, 서브에이전트(reviewer · quick-fix(Sonnet) · merge-cleanup(Haiku))
│   └── scripts/permalink.ts    /rocky:review-request 가 쓴다 — 설치 후에도 있으려면 플러그인 안에 있어야 한다
├── Cargo.toml · Cargo.lock     Rust 워크스페이스 — crates/rocky-core · rockyd · rocky-cli
├── web/                        ★ 보드 웹 UI(React 19 · zustand · Tailwind v4) — `bun run build:ui` → dist/(gitignore).
│                                 **UI 를 고치기 전에 `web/DESIGN.md` 를 읽는다**(토큰·정보 우선순위·좁은 패널 규칙의 정본).
│                                 데몬이 바이너리 옆 dist/ 를 `/` 에 서빙한다. types.ts 는 Rust 응답 타입의 사본.
├── antigravity/                Antigravity(`agy`) 플러그인 번들 — 보드·워크로그(`--roots`) MCP + board·worklog 스킬(plugin/ 의 링크)
│                                 + 규칙. `agy plugin install` 이 복사한다(링크는 풀림). 근거는 docs/antigravity.md
├── bridges/                    수집함 어댑터와 알림 브릿지 — `todo.inbox[]` / `pr.notifiers[]` 에 등록하는 명령
│                                 (stdout JSON 규약, docs/board.md). 외부 서비스 코드는 여기에만; file/ 이 참조 구현.
├── crates/                     ★ 데몬·CLI(worklog MCP + 훅 포함)·코어(docs/rewrite/ 참고)
├── rocky.schema.json           `rocky.json` JSON Schema — crates/rocky-core/src/config.rs 와 함께 움직인다
├── biome.json                  린트·포맷(.sisyphus, .claude 제외)
├── docs/                       architecture, daemon(데몬 모델의 근거), codex, opencode, antigravity, hosts, backlog, board, rewrite/(포팅 기록)
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
  결정으로 만든 다른 것이다; [`docs/features/pr-watch.md`](./docs/features/pr-watch.md) 참고. 둘을 헷갈리지 않는다.) 옛 에이전트(`rocky` /
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
bun run test        # test:unit(scripts·plugin/scripts·plugin/hooks·bridges·web *.test.ts) + test:dom(web *.test.tsx, happy-dom preload).
                    # 맨 `bun test` 는 preload 가 빠져 DOM 테스트가 실패한다 — 늘 `bun run test`
                    # EN: always `bun run test`; bare `bun test` skips the happy-dom preload and DOM tests fail
bun run build:ui    # web/ → dist/(데몬이 서빙)
bun run test:lab    # plugin/hooks/lab — claude plugin validate + 엔진 테스트(*.test.tsx). claude CLI 가 있어야 해서 CI 밖.
                    # 순수 판정(lib.test.ts)은 bun run test 가 돈다
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
  JSON 규약), 로그 분석(`logs.db`를 읽기만), 개발·릴리스 스크립트(Bun), Claude Code function hooks 모듈(`plugin/hooks/lab/`,
  TS — 데몬을 읽어 그리기만 한다. 판정·쓰기·rocky 의 훅 동작은 가운데에 둔다). 데몬을 다른 언어로 옮기지 않는다: core와 FFI
  경계가 수백 개 생기고, 런타임·의존성 동봉(플러그인 캐시 부분 설치 사고)과 데몬 교체·버전 맞추기가 두 벌이 된다.
  느리면 언어보다 측정이 먼저(`rocky usage`). *EN: Rust core (`crates/`) stays; new work in another language goes to the
  edges (web, bridges, scripts reading logs.db, the read-only function-hooks module).*
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
9. 기능의 규칙·불변식이 바뀜 → 그 기능의 `docs/features/<기능>.md` 를 같이 고친다. 새 기능이면 문서를 만들고 아래 *기능별 개발 문서*
   표에 한 줄을 더한다.

## 기능별 개발 문서

기능마다 지킬 규칙·코드 위치·불변식·고정하는 테스트·함정은 [`docs/features/`](./docs/features/)에 있다 — **그 기능을 고치기 전에
그 문서를 연다.** 여기엔 어기면 사고가 나는 규칙 한 줄씩만 둔다. 설계 근거(왜)는 [`docs/daemon.md`](./docs/daemon.md). 순수 판정은
`rocky_core::*`, HTTP·프로세스 배선은 `rockyd::*`, 훅과 CLI는 `rocky_cli::*`. 규칙을 바꾸면 코드와 그 문서를 같이 고친다.

| 기능 | 어기면 안 되는 것 | 문서 |
| --- | --- | --- |
| 데몬 수명(설치·기동·업데이트·교체·launchd) | 버전은 정확한 문자열로 비교해 교체하고, 옛 데몬을 못 내리면 재기동하지 않는다 — *an old board beats no board*. 새 데몬은 포트를 먼저 잡고 옛 데몬이 끝난 뒤 DB 를 연다. 프로세스는 pid 로만 | [daemon-lifecycle](./docs/features/daemon-lifecycle.md) |
| 보안 경계 | GitHub 쓰기·프로세스 실행·세션 조종은 로컬 요청 전용(루프백 + 프록시 헤더 없음, fail-closed). cross-site 변경은 라우팅 전에 403, 읽기는 막지 않는다 | [security](./docs/features/security.md) |
| 보드 모델(메타·ref·노트 CRDT) | 옛 key 는 입력 전용 별칭. 번호는 재사용하지 않는다. 노트는 데몬이 CRDT 피어 — content·state·히스토리는 한 트랜잭션. 삭제 없음 | [board-model](./docs/features/board-model.md) |
| 핸드오프·doing 귀속 | 사람이 든 doing 은 자동으로 놓지 않는다. 쉬는 세션은 `poke` 로 깨우고 그 문구를 늘리지 않는다 | [handoff](./docs/features/handoff.md) |
| PR 감시 | 구독한 PR 만 보고 그 세션에만 보낸다(받은편지함·훅·채널 공통). GitHub 은 읽기만. 주기를 바꾸기 전에 `rateLimit { cost }` 를 잰다 — *the budget is shared with every session's `gh`* | [pr-watch](./docs/features/pr-watch.md) |
| 수집함 | 외부 앱은 읽기만, 동기화 안 함, 어댑터 코드는 `bridges/<name>/` 에만. "이미 올라감"은 데몬 한 곳에서 판정 | [inbox](./docs/features/inbox.md) |
| 로그 색인 | JSONL 이 진실, `logs.db` 는 지워도 다시 만드는 파생물 — *never route writes through the daemon* | [log-index](./docs/features/log-index.md) |
| 토큰 색인 | 훅을 걸지 않는다(트랜스크립트만). 같은 `message.id` 는 한 번만 센다 | [tokens](./docs/features/tokens.md) |
| 기본 브랜치 검증 | opt-in, 명령은 설정 파일에만. 보드 레포의 작업 트리·브랜치를 건드리지 않는다(detached 워크트리). 준비 실패는 커밋 탓으로 남기지 않는다 | [verify](./docs/features/verify.md) |
| rc 서버 | `rc` 블록이 없으면 프로브하지 않는다. 서버는 새 프로세스 그룹으로 띄우고 놓는다(`kill_on_drop` 금지), 내리기는 pid 로만, 프로브가 실패하면 손대지 않는다. 감시는 고정과 되살림 표식이 남은 대상만 되살리고 다른 주기 잡과 동시에 켜지 않는다 — *never act on a failed probe* | [rc-servers](./docs/features/rc-servers.md) |
| statusline | 1초마다 도는 자리 — 사용 로그·데몬 자동 기동을 거치지 않고 300ms 안에 못 받으면 빈 출력 | [statusline](./docs/features/statusline.md) |
| 세션 띄우기 | 그 워크트리에 살아 있는 세션이 있으면 띄우지 않는다. `--permission-mode` 는 넘기지 않는다. rc 가 켜진 기기는 rc 서버로(할 일당 하나, 내리기는 사람) | [spawn](./docs/features/spawn.md) |
| lab(function hooks 실험) | 사용자 rocky.json 의 `lab` 블록이 있을 때만. 데몬을 읽어 그리기만 하고 주기 폴링·턴 열기를 하지 않는다. 데몬 요청은 `x-rocky-client: claude-code-lab` 을 달아 `hook lab` 으로 센다 — *every REST call lands in the usage log* | [lab](./docs/features/lab.md) |
| 세션 목록(`claude agents`) | pid 없는 잠든 행을 버리지 않는다. 작업 요약은 `detail`·`needs`·`updatedAt` 만, 못 읽으면 그 행만 비운다 — *the session list is readable remotely* | [sessions](./docs/features/sessions.md) |

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

**단일 정본**: 사람 = `README.md`, 에이전트 = 이 파일, 기능별 개발 규칙 = `docs/features/`, 근거 =
`docs/architecture.md`·`docs/daemon.md`, 도구별 계약 = 도구 정의 자체. 루트에 형제 문서를 새로 만들지 않는다 — `FEATURES.md`와 `REVIEW.md`가 이 셋으로
접히기 전에 바로 그랬다. *EN: Do not add a new root-level sibling doc.*
