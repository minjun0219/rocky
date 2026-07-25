> **SUPERSEDED (2026-07-24)**: stdio 브릿지 방향은 폐기되었다. 데몬 `/mcp` 를 plugin.json 에
> http 로 직접 선언하는 방식으로 선회했고(코드 반영 완료), 이어서 rocky-todo 를 별도 하위
> 플러그인으로 분리하기로 했다 → `2026-07-24-rocky-todo-plugin-split-design.md` 참조.
> 이 문서의 웹UI 에셋 chdir 픽스(②)만 유효하게 남아 머지되었다.

# rocky-todo MCP 브릿지 + 활성화 동의 + 웹UI 에셋 경로 — 설계

- 날짜: 2026-07-24
- 대상: `src/todo/` (rocky-todo 데몬 + CLI + 신규 stdio MCP 브릿지)
- 상태: 설계 확정 (Logan 승인), 구현 계획 대기

## 배경 / 문제

rocky-todo 는 시스템 유일 상주 데몬(127.0.0.1:8636, SQLite)이 공유 todo/스크래치패드
보드를 들고, 에이전트는 MCP/CLI 로 쓰고 호출자는 React 웹 UI 로 실시간 편집한다.
현재 세 가지 문제가 있다.

### ① 세션에 MCP 도구가 없어 CLI 로 폴백된다 (근본 원인)

`~/.claude.json` 에 `rocky-todo` 가 `type:"http", url:"http://127.0.0.1:8636/mcp"` 로
등록돼 있다. 세션 시작 시점에 데몬이 죽어 있으면 이 HTTP MCP 서버는 **연결 실패 →
도구가 세션에 아예 없음**. 그래서 `todo` 스킬의 도구 게이트가 CLI 폴백을 탄다. 이후
CLI 가 데몬을 띄워도 그 세션의 MCP 는 되살아나지 않는다. 즉 "CLI 를 먼저 시도"한 게
아니라 **MCP 가 존재하지 않았던 것** — rocky 가 MCP lifecycle 을 쥐고 있지 않은 게 원인.

### ② 데몬 기동 후 웹 UI 의 css/js 경로가 깨진다

타 디렉터리(예: `~/dev/workspaces/logan-agent-kit`)에서 데몬을 spawn 하면 `/` 응답의
에셋 링크가 이렇게 나온다:

```html
<link href="/../../../../../../../dev/workspaces/logan-agent-kit/chunk-bkj5n7w5.css">
```

Bun 의 HTML 번들이 asset public path 를 **HTML 파일 디렉터리 → `process.cwd()` 상대경로**로
계산하기 때문. CLI 가 `Bun.spawn` 할 때 cwd 를 넘기지 않아 호출자 cwd 를 상속받는 게 방아쇠다.
(재현 확인: `src/todo/ui` 로 chdir 하면 `/chunk-*.css` 로 정상화되고 chunk 200 응답.)

### ③ 비활성(todo.enabled=false) 상태에서 대화가 끊긴다

`cli.ts` 는 "기본 비활성이다, 설정해라" 에러만 던지고, `todo` 스킬도 "임의로 켜지 않는다,
안내하고 멈춘다" 로 못 박아 대화가 거기서 끊긴다. 호출자에게 **동의를 묻고 켜주는 경로**가
없다.

## 목표

- 데몬 생사와 무관하게 rocky-todo 도구가 **항상 세션에 존재**하고, rocky 가 그 lifecycle 을 쥔다.
- 비활성 상태에서 사용자에게 노출 범위를 설명하고 **동의를 받은 뒤** 활성화하는 경로를 만든다.
  동의 없는 자동 활성화는 계속 금지.
- 타 cwd 에서 spawn 돼도 웹 UI 에셋 경로가 깨지지 않는다.
- **의존성 0** 원칙 유지 — 새 prod-dep 없음.

## 비목표 (YAGNI)

- launchd 자동 등록 (기존 `rocky-todo daemon install` 로 분리 유지).
- `todo_enable` 의 `scope` 인자.
- `notifications/tools/list_changed` 기반 동적 도구 토글.
- http MCP 등록 자동 마이그레이션 (이 작업에서 직접 1회 제거로 처리).
- gRPC / protobuf 등 새 통신 스택.

## 결정 사항 (Logan 승인)

1. **MCP 경로**: stdio 브릿지. plugin.json 에 stdio MCP 서버로 선언, 브릿지가 유일한
   MCP 서버가 된다. Codex/opencode 도 같은 방식으로 등록.
2. **데몬 `/mcp` 제거**: 데몬은 순수 REST + SSE + 웹 UI. MCP 프로토콜은 브릿지 한 곳에만.
   브릿지↔데몬은 기존 `/api/*` REST 로 통신.
3. **내부 통신**: REST 재사용 + 공유 TS 타입 계약 (gRPC 아님). `client.ts` 가 store 의
   `Todo`/`Note` 타입을 import 하고 zod 로 입력을 검증해 dep 0 로 계약 안전을 얻는다.
4. **비활성 표면**: 도구 목록은 항상 6개(5 + `todo_enable`). 비활성 시 5개 도구 호출은
   구조화된 안내 에러를 반환한다.
5. **`todo_enable` 범위**: user rocky.json 에 `todo.enabled=true` 병합 기록 + 데몬 기동.
   launchd 는 건드리지 않는다.

## 아키텍처

```
Claude Code ──stdio──▶ mcp-stdio.ts  (유일한 MCP 서버, 5+1 도구)
Codex/opencode ─┘         │ 도구 핸들러 = 데몬 REST 호출 (client.ts)
                          ▼
데몬:  /api/*  (REST) + /api/events (SSE) + /  (웹 UI)     ← /mcp 삭제
        └─ store (SQLite, 단일 writer)
```

브릿지의 요청 흐름:

- `initialize` — 로컬 응답 (데몬 불필요).
- `tools/list` — 항상 6개 반환 (데몬 불필요).
- `tools/call`
  - enabled → `ensureDaemon()` (health→spawn→대기) 후 REST 로 포워딩.
  - disabled → 구조화된 안내 에러 (아래 §비활성 동의).
  - `todo_enable` → 로컬 처리 (`enable.ts`).

데몬의 독립 lifecycle 전제는 유지된다 — plugin.json 에 선언되는 건 데몬이 아니라
**브릿지**이고, 브릿지가 데몬을 온디맨드로 health→spawn 한다.

## 컴포넌트 (신규 / 변경)

### 신규

- **`src/todo/mcp-tools.ts`** — 5개 도구 스펙(name / description / zod inputSchema)의 단일
  출처. 데몬 제거 후 이 스펙은 브릿지가 소비한다. (drift 방지 축 ①)
- **`src/todo/client.ts`** — 얇은 REST 클라이언트 + `ensureDaemon`/`health`. 지금 `cli.ts` 에
  인라인된 `request`/`ensureDaemon`/`health` 를 여기로 뽑아 CLI 와 브릿지가 공유.
  store 의 `Todo`/`Note` 타입을 import 해 반환 타입을 강타입으로 준다. (drift 방지 축 ②)
- **`src/todo/mcp-stdio.ts`** — stdio MCP 서버 진입점. `mcp-tools.ts` 스펙에 REST 포워딩
  핸들러를 바인딩 + `todo_enable` 등록. 비활성 시 안내 에러. `#!/usr/bin/env bun` 불필요
  (plugin.json 이 `bun run` 으로 실행).
- **`src/todo/enable.ts`** — 활성화 코어. user rocky.json 에 `todo.enabled=true` 병합
  기록(다른 키 보존) → 데몬 spawn → health 대기 → `{ ok, url, hint }`. CLI `enable` 과
  `todo_enable` 이 공유.

### 변경

- **`src/todo/daemon.ts`**
  - `Bun.serve` 앞에 `process.chdir(join(import.meta.dir, 'ui'))` 1줄 (②).
  - `/mcp` 라우트 + `createMcpFetchHandler` import 제거 (②③ 무관, 결정 2).
- **`src/todo/cli.ts`** — `request`/`ensureDaemon`/`health` 를 `client.ts` 로 이동해 재사용.
  신규 `enable` 커맨드 추가. `mcp setup` 안내를 stdio 브릿지 등록(Codex/opencode)으로 재작성.
- **`.claude-plugin/plugin.json`** — `mcpServers.rocky-todo` 를 stdio 로 추가
  (`command: "bun"`, `args: ["run", "${CLAUDE_PLUGIN_ROOT}/src/todo/mcp-stdio.ts"]`).

### 삭제

- **`src/todo/mcp.ts`** + **`src/todo/mcp.test.ts`** — 데몬 내부 MCP 표면 제거.

## 데이터 흐름 — 도구 → REST 매핑 (커버리지 검증됨)

| MCP 도구 | REST |
|---|---|
| `todo_list` | GET `/api/todos`, GET `/api/boards`, GET `/api/todos/:id`, GET `/api/sections` |
| `todo_write` | POST `/api/todos`, PATCH `/api/todos/:id` |
| `todo_status` | POST `/api/todos/:id/status` |
| `note_list` | GET `/api/notes`, GET `/api/notes/:id` |
| `note_write` | POST `/api/notes`, PATCH `/api/notes/:id`, POST `/api/notes/:id/(archive|unarchive)` |

REST 표면이 5개 도구를 100% 커버함을 `server.ts` 라우트로 확인했다.

actor 는 브릿지가 호스트 env(`detectActor()`)로 채워 `x-rocky-actor` 헤더로 전달한다 —
지금 `'agent'` 로 뭉개지던 히스토리가 정확해진다(보너스).

## 비활성 동의 절차 (③)

비활성 상태에서 5개 도구 중 하나를 호출하면 브릿지가 반환:

```json
{ "error": "rocky-todo disabled",
  "guidance": "켜기 전에 사용자에게 알리고 동의를 받아라: (1) 127.0.0.1:8636 에 상주 데몬이 뜬다 (2) 보드 데이터는 ~/.config/rocky/todo/todo.db 에 저장된다 (3) user rocky.json 에 todo.enabled=true 가 기록된다. 동의를 받은 뒤에만 todo_enable 을 호출한다." }
```

`todo_enable` (입력 없음):
1. user rocky.json 로드 → `todo.enabled=true` 병합 (soul/callsign 등 기존 키 보존).
2. 데몬 spawn + health 대기.
3. `{ ok: true, url, hint: "재부팅 후에도 상시 기동하려면 rocky-todo daemon install" }`.

CLI `rocky-todo enable` 은 같은 `enable.ts` 코어를 부른다.

`skills/todo/SKILL.md` 의 도구 게이트를 "임의로 켜지 않는다, 안내하고 멈춘다" →
"노출 범위를 설명하고 동의를 받은 뒤 `todo_enable`" 로 수정. 가드레일에 "동의 없는 자동
활성화 금지" 명시.

## 에러 처리

- 브릿지가 데몬 spawn 에 실패(활성인데 안 뜸)하면 → MCP 도구 호출이 에러를 반환하고
  `rocky-todo daemon status` 안내를 담는다. 가짜 진행 금지.
- 브릿지의 로컬 응답(initialize/tools/list)은 데몬 상태와 무관하게 항상 성공 — 도구가
  세션에서 사라지지 않는다.
- `enable.ts` 의 rocky.json 병합은 파싱 실패 시 기존 파일을 덮지 않고 에러를 올린다.

## 테스트

- `src/todo/mcp-tools.test.ts` — 스펙이 정확히 5개 도구를 정의(이름/필수 인자).
- `src/todo/mcp-stdio.test.ts` — 브릿지 단위: 데몬 fetch 를 DI 로 주입해 실제 spawn 없이,
  비활성 시 `tools/list` 6개 + `tools/call` 이 구조화된 안내 에러, 활성 시 포워딩 검증.
- `src/todo/enable.test.ts` — `mkdtempSync` 격리 rocky.json 에 `todo.enabled=true` 병합,
  **기존 키 보존** 확인.
- `src/todo/client.test.ts` — REST 클라이언트 매핑 (fake fetch).
- 에셋 경로(②)는 Bun 번들 런타임 동작이라 유닛으로 잡기 애매 → 문서에 수동 검증 절차:
  "타 cwd 에서 데몬 spawn 후 `curl /` 의 chunk href 가 `/chunk-` 로 시작하는지".

## 문서 갱신 (체크리스트 항목 4~8)

- **plugin.json** — `mcpServers.rocky-todo` stdio 추가.
- **AGENTS.md** — Layout 에 `mcp-stdio.ts`/`mcp-tools.ts`/`client.ts`/`enable.ts` 추가,
  `mcp.ts` 제거. *Project in one line* 의 "데몬 MCP 는 plugin.json 에 선언하지 않는다"를
  "브릿지를 plugin.json 에 선언(데몬 lifecycle 은 독립, 브릿지가 health→spawn 으로 온디맨드
  기동), 데몬은 `/mcp` 없이 REST/SSE/웹UI 만" 으로 수정. `todo_enable` 6번째 도구 반영.
- **FEATURES.md** — 도구 표에 `todo_enable`, 호스트별 등록(Claude Code 자동 / Codex·opencode
  stdio) 갱신.
- **README.md** — surface 카운트.
- **docs/rocky-todo.md** — 호스트별 등록 재작성: Claude Code 자동, 기존 http 수동등록은
  `claude mcp remove rocky-todo` 로 제거 안내(이미 이 작업에서 1회 실행함), Codex/opencode
  stdio 예시. "데몬 `/mcp`" 언급 전부 삭제.
- **skills/todo/SKILL.md** — 도구 게이트 + 가드레일 수정 (위 §비활성 동의).
- **changeset** — minor (user-facing: 새 도구 + 등록 방식 변경).

## 이미 수행한 부수 작업

- `~/.claude.json` 의 http `rocky-todo` MCP 등록을 `claude mcp remove --scope user rocky-todo`
  로 제거함 (브릿지가 유일 등록 경로가 되도록).
- ② 재현 중 프로브 데몬이 rocky.json 의 `expose:["tailscale-serve"]` 를 읽어 `tailscale serve`
  를 프로브 포트로 덮어썼던 것을 `tailscale serve --bg 8636` 으로 원상복구함.
```
