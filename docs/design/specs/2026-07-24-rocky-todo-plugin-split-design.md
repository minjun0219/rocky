# rocky-todo 를 별도 레포 플러그인으로 분리 — 설계

- 날짜: 2026-07-24 (개정 2026-07-25 — 모노레포안 폐기, 완전 별도 레포로 선회)
- 대상: rocky 레포에서 todo 제거(슬림다운) + 신규 독립 레포 `minjun0219/rocky-todo`
- 상태: 설계 협의 완료 (Logan 승인 방향), 계획 대기
- 대체: `2026-07-24-rocky-todo-mcp-bridge-design.md` (stdio 브릿지 방향)
- 개정 이력:
  - v1 (07-24): 한 레포 2-플러그인 모노레포(`plugins/rocky` + `plugins/rocky-todo`).
  - **v2 (07-25): 모노레포 폐기.** changeset/workspace/husky 를 한 레포에서 2 패키지로
    묶는 마찰(플러그인 install 은 workspace 비호환, changeset 은 workspace 요구)이 컸다. Logan 결정:
    **rocky·rocky-todo 를 완전 별도 GitHub 레포로 분리.** 각자 표준 단일 플러그인 레포, 공유 툴링 0.
  - **v2.1 (07-25, 현재): 마켓플레이스 통합.** rocky-todo 는 **자기 마켓플레이스를 두지 않는다.**
    rocky 레포의 `marketplace.json` 이 rocky-todo 를 2번째 entry 로 등록하되 source 를 외부 repo 로
    가리킨다: `"source": { "source": "github", "repo": "minjun0219/rocky-todo" }` (실측 확인, 공식
    문서 plugin-marketplaces §plugin-sources). 코드 레포는 분리(깔끔한 툴링) + 마켓은 하나(설치 시
    `marketplace add minjun0219/rocky` 하나면 둘 다 노출). **같은 마켓 안이라 `dependencies:["rocky"]`
    자동 해석도 깔끔** — 크로스-마켓 미보장 문제가 사라진다.

## 배경 / 동기

rocky 는 openapi/seo/notion/worklog + 소울/statusline/슬래시커맨드/스킬을 담은 owner 의 개인
Claude Code 플러그인이다. v0.13 에서 rocky-todo(상주 데몬 + 웹UI + MCP + CLI)가 rocky 안에
들어왔는데, 두 가지 문제가 있다.

1. **rocky 본체가 무겁다** — todo 를 안 쓰는 세션도 rocky 설치 시 react/zustand 의존과
   데몬/웹UI 코드를 전부 끌고 온다.
2. **활성화 절차가 복잡하다** — `todo.enabled` 마스터 스위치 + CLI 자동기동 + 훅 게이트로
   opt-in 을 구현했는데, 플러그인 설치 자체가 이미 자연스러운 opt-in 경계다.

**해결**: rocky-todo 를 **완전 별도 레포의 독립 플러그인**으로 분리한다. rocky 는 todo 를 들어내
가벼워지고, **rocky-todo 플러그인 설치가 곧 데몬+MCP+훅 활성화**가 되어 별도 스위치가 필요 없다.
두 레포는 서로의 툴링/릴리스/버전을 공유하지 않는다 — 각자 오늘의 rocky 처럼 표준 단일 플러그인
레포다.

## 확정 사실 (문서 + 실측)

- **`claude plugin install --from` 없음** ✗ (실측 `claude plugin install --help`) — `install <plugin>`
  은 `<name>` / `<name>@marketplace` 만 받는다. marketplace 미등록 플러그인은 설치 불가. →
  rocky-todo 도 **어느 marketplace 엔가는** 등록돼야 설치된다. v2.1 에서는 자기 마켓 대신
  rocky 레포 `marketplace.json` 의 2번째 entry(github source)로 등록한다.
- **marketplace entry 는 자동 설치 안 됨** ✓ — 명시 `install <name>@<marketplace>` 일 때만 깔린다.
- **`dependencies` 는 자동 해석됨** ✓ (실증: `claude plugin prune` = "auto-installed dependencies
  제거" 커맨드 존재). rocky-todo 가 `dependencies:["rocky"]` 를 선언하면 rocky 자동 동반.
  ~~단 **크로스-마켓플레이스 의존 자동 해석은 불확실**~~ — 이 우려가 v2.1 에서 두 플러그인을
  **한 마켓(rocky-marketplace)** 으로 합친 직접적 이유다. 같은 마켓 안이면 자동 해석이 깔끔해
  크로스-마켓 미보장 문제 자체가 사라진다.
- **설치는 source 서브트리만 캐시로 복사** ✓ — 별도 레포면 이 제약은 자연히 충족(레포 루트가
  곧 플러그인 루트). `../` 참조 문제 없음.
- **설치 시 자동 `bun install`** ✓ (실측) — 각 레포 루트에서 돈다. 표준 단일 패키지면 그대로 동작.
- **`git filter-repo`(또는 subtree split) 로 경로별 history 추출 가능** ✓ — `src/todo` 등의 커밋
  이력을 새 레포로 보존 이동.
- **설치 = 활성화** ✓ — `defaultEnabled` 기본 true. 끄기는 `claude plugin disable rocky-todo`.
- **SessionStart hook ↔ http MCP 초기화 순서 미보장** ⚠ — 첫 세션엔 데몬이 아직 안 떠서 MCP 가
  `failed` 로 뜰 수 있다. `/mcp` 패널 retry / 다음 세션 / launchd 상주로 해결. **감안하기로 함.**
- **marketplace 숨김 필드 없음** ✗ — v2 에서는 별도 마켓이라 자연히 분리 노출됐지만, v2.1 의
  마켓 통합 이후에는 rocky-todo 도 rocky 마켓 브라우즈 목록에 함께 뜬다. 감춤은 불가하며,
  entry 를 명시 `install` 하지 않는 한 설치되지는 않는다는 점으로 갈음한다.

## 목표 / 비목표

**목표**
- rocky 레포에서 todo 코드/의존/훅/설정을 완전히 들어낸다 (경량화, react/zustand 제거).
- 신규 레포 `minjun0219/rocky-todo` 를 표준 단일 플러그인 레포로 만든다 (history 보존 추출).
- rocky-todo 설치 = 데몬+MCP+훅 활성화. `todo.enabled` 스위치 제거.
- rocky-todo 는 자체 경량 config 로더로 self-contained (`../core` 의존 끊기).

**비목표 (YAGNI)**
- 모노레포 / Bun workspace / 공유 changeset (완전 폐기 — 별도 레포라 불필요).
- 데몬 원격화 / launchd 강제 (기존 `daemon install` 선택 유지).
- rocky-todo 의 npm 공개 배포.
- 두 레포 간 버전 동기화 / 공유 CI.

## 결정 사항 (Logan 승인)

1. **토폴로지**: rocky·rocky-todo **완전 별도 GitHub 레포**. 모노레포 폐기.
2. **rocky-todo 위치**: 신규 레포 `minjun0219/rocky-todo` (plugin.json + CI + release). **마켓은 두지
   않는다** — rocky 레포의 `marketplace.json` 이 github source 로 서빙한다(v2.1).
3. **추출 방식**: `git filter-repo`(또는 subtree)로 **git history 보존** 이동. 소스는 현재 브랜치
   `minjun0219/todo` HEAD (http pivot + mcp 복원 + client 추출이 반영된 최신·최선 코드).
4. **rocky 슬림다운 베이스**: `origin/main` 에서 새 브랜치 → todo 삭제 PR. 현재 브랜치의 16개
   브릿지/split 실험 커밋은 main 에 올리지 않는다 (추출 소스로만 사용).
5. **의존성**: rocky-todo → `dependencies:["rocky"]` 선언. v2.1 로 같은 마켓에 들어가면서
   자동 해석이 보장되므로 "rocky 먼저" 수동 안내는 불필요해졌다.
6. **`todo.enabled` 제거**: 설치=활성화. 런타임 끄기는 `claude plugin disable rocky-todo`.
7. **데몬 기동**: rocky-todo 의 SessionStart hook(startup)이 health→spawn. 첫 세션 순서 미보장은 감안.

## 레이아웃 (두 레포)

### 레포 A — rocky (기존, todo 제거 후)

```
rocky/                                (기존 그대로, todo 관련만 제거)
├── .claude-plugin/plugin.json        # mcpServers 에서 rocky-todo(http) 항목 제거, rocky(stdio)만
├── .claude-plugin/marketplace.json   # 기존 rocky 만 (변화 없음)
├── package.json                      # react/react-dom/zustand + @types/react* 제거
├── src/core/                         # rocky-config 에서 TodoConfig/validateTodo/todo 키 제거
├── src/hooks/                        # notify-todo.ts 제거 (나머지 4개 유지)
├── src/index.ts / standalone.ts      # 변화 없음 (todo 표면 없었음)
├── skills/                           # todo/ 제거 (writing-cc-plugin, delegating-to-codex, todoist 유지)
├── hooks/hooks.json                  # UserPromptSubmit(notify-todo) 제거 (SessionStart×2 + Stop 유지)
├── rocky.schema.json                 # todo 키 제거
└── (src/todo/, bin/rocky-todo, docs/rocky-todo.md 삭제)
```

### 레포 B — rocky-todo (신규, 표준 단일 플러그인)

```
rocky-todo/
├── .claude-plugin/plugin.json        # mcpServers(http) + hooks + dependencies:["rocky"]
├── package.json                      # deps: mcp-sdk, react, react-dom, zustand, zod (bun:sqlite 내장)
├── tsconfig.json / biome.json / bun.lock
├── src/                              # 기존 src/todo/* 이동 (daemon/server/store/mcp/cli/… + ui/)
│   ├── rocky-config.ts               # ★ 신규 경량 로더 (todo 블록만, ../core 의존 제거)
│   └── (config.ts 에서 enabled 분기 제거, enable.ts 삭제)
├── bin/rocky-todo                    # 기존 이동
├── hooks/hooks.json                  # SessionStart→ensure-daemon, UserPromptSubmit→notify-todo
├── hooks/ensure-daemon.ts            # ★ 신규 (health→spawn, fail-open)
├── hooks/notify-todo.ts              # 기존 src/hooks/notify-todo.ts 이동 (import 경로 로컬화)
├── skills/todo/SKILL.md              # 설치·활용 가이드로 재작성
├── docs/…                            # 기존 docs/rocky-todo.md 이동·재작성
├── .github/workflows/                # rocky CI/release 미러 (단일 패키지)
├── .husky/                           # 자체 pre-commit/pre-push
├── README.md / AGENTS.md / FEATURES.md  # rocky 미러, todo 전용
└── rocky-todo.schema.json (선택)     # todo 설정 스키마 (원하면)
```

## 컴포넌트 이동 매핑 (rocky → rocky-todo)

- `src/todo/*` → `rocky-todo/src/*` (daemon/server/store/mcp/cli/actor/config/notify/tailscale/launchd/client + ui/ + 각 *.test.ts)
- `bin/rocky-todo` → `rocky-todo/bin/rocky-todo`
- `src/hooks/notify-todo.ts` → `rocky-todo/hooks/notify-todo.ts` (import 로컬화)
- `skills/todo/` → `rocky-todo/skills/todo/` (재작성)
- `docs/rocky-todo.md` → `rocky-todo/docs/rocky-todo.md` (재작성, `todo.enabled` 제거)
- 신규 `rocky-todo/src/rocky-config.ts` (경량 로더), `rocky-todo/hooks/ensure-daemon.ts`

## config 로더 분리 (self-contained 의 핵심)

현재 `src/todo/` 는 `src/core/rocky-config.ts` 의 `loadConfig` 와 `src/core/worklog.ts` 의
`expandTilde` 에 의존한다 (실측 cross-import):
- `daemon.ts` → `loadConfig` (`../core/rocky-config`)
- `cli.ts` → `loadConfig` (`../core/rocky-config`)
- `config.ts` → `TodoConfig` 타입 + `expandTilde` (`../core/worklog`)
- `enable.ts` → `USER_CONFIG_PATH` (`../core/rocky-config`) — **삭제 예정**
- `notify-todo.ts` → `loadConfig` + todo 로컬 모듈

별도 레포엔 `../core` 가 없으므로 rocky-todo 가 **자체 경량 config 로더**를 갖는다.

- 신규 `rocky-todo/src/rocky-config.ts` — user `~/.config/rocky/rocky.json` 만 읽어 `todo` 블록
  (`port` / `dir` / `expose` / `watch`)을 반환. project rocky.json 은 무시(기존 정책 동일).
  **`enabled` 필드는 더 이상 읽지 않는다** (설치=활성화). openapi/seo/worklog/soul 등 rocky 전용
  키는 파싱하지 않는다. `expandTilde` 도 이 파일에 자체 구현(작은 순수 함수라 복제 OK).
  제공 표면: `loadTodoConfig()` → `{ todo?: TodoConfig }`, `TodoConfig` 타입, `USER_CONFIG_PATH`,
  `expandTilde`.
- `config.ts` 의 `resolveTodoRuntimeConfig` 에서 `enabled` 분기(현재 line 74–79)를 제거,
  `TodoRuntimeConfig.enabled` 필드 삭제, `TodoConfig` 타입 import 를 로컬 `./rocky-config` 로 교체.
- `enable.ts` / `enable.test.ts` 삭제. CLI `enable` 커맨드 + `INFO_COMMANDS` 의 `'enable'` +
  enabled 게이트(현재 cli.ts line 270–275) 제거. 데몬 기동은 `daemon start` / SessionStart hook /
  CLI 온디맨드 spawn 이 담당.

## rocky-todo plugin.json

```json
{
  "$schema": "https://json.schemastore.org/claude-code-plugin.json",
  "name": "rocky-todo",
  "version": "0.1.0",
  "description": "공유 todo/스크래치패드 상주 데몬 + 웹 UI + MCP + CLI. rocky 생태계의 동반 플러그인.",
  "author": { "name": "Minjun Kim" },
  "license": "MIT",
  "dependencies": ["rocky"],
  "mcpServers": {
    "rocky-todo": { "type": "http", "url": "http://127.0.0.1:8636/mcp" }
  }
}
```

hooks(`rocky-todo/hooks/hooks.json`):
- `SessionStart`(matcher `startup`) → `ensure-daemon.ts` (health→없으면 spawn, 짧은 대기 루프)
- `UserPromptSubmit` → `notify-todo.ts` (사람 변경 주입, 기존 로직)

## 데몬 기동 (ensure-daemon hook)

`rocky-todo/hooks/ensure-daemon.ts` — `src/todo/client.ts` 의 `health`/`ensureDaemon` 를 재사용:
- config 로드 → `resolveTodoRuntimeConfig` → `buildContext` → `ensureDaemon` (health 확인 후 없으면
  detached spawn, 최대 ~5s 대기).
- fail-open: 실패해도 훅은 조용히 종료(세션을 막지 않는다).
- 첫 세션에서 http MCP 초기화보다 늦으면 그 세션의 도구는 `/mcp` retry 로 붙는다 — 훅 결과 메시지에
  그 안내를 additionalContext 로 얹을 수 있다(옵션).
- 테스트: `health` fake 주입으로 "떠 있으면 no-op / 없으면 spawn 시도" 검증(실제 spawn 없이).

## 설치 / 배포

- **rocky**: 기존과 동일 — `claude plugin marketplace add minjun0219/rocky` →
  `claude plugin install rocky@rocky-marketplace`. source `"./"` 그대로 (레포 구조 불변).
- **rocky-todo**: 자기 마켓 없음. rocky 마켓의 2번째 entry(github source)로 서빙 —
  `claude plugin install rocky-todo@rocky-marketplace` (rocky 마켓만 add 하면 됨).
  같은 마켓 안이라 `dependencies:["rocky"]` 가 자동 해석돼 rocky 동반. rocky 레포
  `marketplace.json` 에 아래 entry 추가(rocky-side 작업):
  ```json
  { "name": "rocky-todo",
    "source": { "source": "github", "repo": "minjun0219/rocky-todo" },
    "author": { "name": "Minjun Kim" },
    "description": "공유 todo/스크래치패드 데몬 — 설치=활성화. rocky 동반 플러그인." }
  ```

## 빌드 / 툴링 (각 레포 독립)

두 레포 모두 오늘의 rocky 와 동일한 표준 단일 패키지 구성 — 모노레포 툴링 일절 없음.
- 각 레포: 자기 `package.json` + `bun.lock` + `tsconfig.json` + `biome.json`.
- 각 레포: 자기 `.husky/pre-commit`(lint-staged + secret scan) / `pre-push`(typecheck + test).
- 각 레포: 자기 `.github/workflows/ci.yml`(check/typecheck/test) + `release.yml`(changeset).
- rocky-todo 의 changeset/release 는 rocky 워크플로를 그대로 미러(단일 패키지라 sync-plugin-version
  스크립트도 그대로 적용 가능).

## 문서

- **rocky (레포 A)**: AGENTS.md / FEATURES.md / README.md 에서 rocky-todo 섹션 전부 제거,
  "별도 레포 minjun0219/rocky-todo 로 분리됨" 한 줄 포인터만 남긴다. `docs/rocky-todo.md` 삭제.
- **rocky-todo (레포 B)**: AGENTS.md / FEATURES.md / README.md 신규(rocky 미러, todo 전용).
  `docs/rocky-todo.md` 재작성(설치=활성화, rocky 마켓에서 설치, `/mcp` retry 안내, CLI 표면).
  `skills/todo/SKILL.md` 재작성(플러그인 설치 가이드 + rocky 선행 확인 + 보드 활용 에티켓).
  `todo.enabled` 언급 전부 제거.

## 마이그레이션 / 리스크

- **추출 순서**: (1) 현재 브랜치 HEAD 에서 `git filter-repo` 로 todo 경로 history 추출 → 새 레포
  `minjun0219/rocky-todo` 초기화. (2) 새 레포에서 레이아웃 재배치(src/todo/* → src/*, 훅/스킬/문서
  이동) + 경량 config 로더 + ensure-daemon + enable 제거 + 자기 툴링. (3) rocky 슬림다운 PR(main
  베이스)로 todo 삭제.
- **기존 rocky 설치 영향 없음** — rocky source 는 `"./"` 그대로, 구조 불변. rocky 마켓만 add 돼
  있으면 `claude plugin install rocky-todo@rocky-marketplace` 로 바로 설치된다(마켓 추가 불필요).
  메모리 [[rocky-local-plugin-facade]] 는 rocky 쪽 변화 없음(포인터만).
- **[[plugin-cache-partial-install]] 재발 주의** — rocky-todo 첫 설치는 캐시에 src/+node_modules 가
  온전히 왔는지 marketplace clone 과 diff 로 검증.
- **첫 세션 MCP failed** — 감안. 문서에 `/mcp` retry / `daemon install` 안내.
- ~~**크로스-마켓 dependency 자동해석 미보장**~~ — v2.1 의 마켓 통합으로 해소.

## 테스트

- rocky 슬림다운 후: `bun run check && typecheck && test` 통과 + `src/index.test.ts` 표면 불변
  (todo 는 애초에 stdio 표면에 없었음) + rocky-config/schema 에서 todo 키 제거 후 기존 테스트 통과.
- rocky-todo 새 레포: `bun run check && typecheck && test` 독립 통과. 신규 `rocky-config.ts`(경량
  로더) 단위 테스트(todo 블록만 읽고 enabled 무시). `ensure-daemon.ts` 단위 테스트(health fake).
  기존 store/server/mcp/cli/notify 테스트는 경로 이동 후 그대로 통과.
- 수동: 두 플러그인 실제 설치 스모크(캐시 node_modules 온전, http MCP 5도구, 데몬 기동).
