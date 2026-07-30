# Rocky

[![CI](https://github.com/minjun0219/rocky/actions/workflows/ci.yml/badge.svg)](https://github.com/minjun0219/rocky/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](./LICENSE)
[![Runtime: Bun](https://img.shields.io/badge/Runtime-Bun%20%E2%89%A5%201.0-black)](https://bun.sh)

에이전트 코딩 세션을 위한 **개인용 MCP toolkit** — OpenAPI / Swagger 명세 캐시-우선 탐색, SEO 메타 검증, Notion 페이지 캐시, 에이전트 워크로그(기록↔정리)를 하나의 Bun 패키지로 묶었다. 이름은 *Project Hail Mary* 의 Rocky (스펙을 번역해 주는 엔지니어) 에서. 공유 todo 보드는 동반 플러그인 [`minjun0219/rocky-todo`](https://github.com/minjun0219/rocky-todo) 로 분리돼 있다.

> **공개에 관하여** — 이 저장소는 소유자가 혼자 쓰려고 만든 개인 플러그인이다. 누구나 참고·포크·설치할 수 있도록 MIT로 공개하지만, 범용 제품이 아니라서 표면과 규칙이 소유자의 워크플로우에 맞춰 바뀐다. 구조와 패턴(단일 패키지 MCP 서버, CLI 위임, 기록↔정리 분리 등)을 참고 자료로 보는 쪽을 권한다.

## 한눈에

한 저장소가 두 stdio MCP 진입점을 노출한다:

| 진입점 | 역할 | 소비 호스트 |
| --- | --- | --- |
| **전체 표면 MCP 서버** (`src/index.ts`) | 아래 도구 표면 전부. | Claude Code plugin (`.claude-plugin/plugin.json` 의 `mcpServers`) |
| **`openapi-mcp` 단독 CLI** (`bin/openapi-mcp`) | OpenAPI 도구 7 개만 담은 host-agnostic subset. | 모든 stdio MCP host (Cursor / Continue / Claude Desktop / …) — [설정 가이드](./docs/openapi-mcp.md) |

공유 todo / 스크래치패드 보드 데몬은 **동반 플러그인 [`rocky-todo`](https://github.com/minjun0219/rocky-todo)** 로 분리됐다 (v0.13 번들 → 별도 레포). 같은 rocky 마켓플레이스가 서빙하니 `claude plugin install rocky-todo@rocky-marketplace` 로 설치한다 (설치=활성화, rocky 자동 동반).

### MCP 도구 표면

| 도구군 | 개수 | 하는 일 | 등록 조건 | 전체 표면 서버 | 단독 CLI |
| --- | --- | --- | --- | :---: | :---: |
| `openapi_*` | 7 | OpenAPI / Swagger spec 캐시-우선 fetch (`get` / `refresh` / `status`), endpoint 점수화 검색 (`search`), 레지스트리 조회 (`envs`), 단일 endpoint 상세 (`endpoint`), tag 목록 (`tags`). swagger 2.0 자동 변환 + `$ref` deref + TTL 디스크 캐시 + 백그라운드 재검증. | 항상 | ✅ | ✅ |
| `seo_validate` | 1 | 단일 URL 의 OG / Twitter Card / JSON-LD / favicon 메타 검증 ([`ogpeek`](https://www.npmjs.com/package/ogpeek) 기반, 기본 SSRF 가드). | 항상 | ✅ | — |
| `worklog_*` | 4 | append-only 로컬 JSONL **기록(記錄)** 레이어 — 결정 / blocker / 답변 / 메모를 turn 을 넘겨 남긴다 (`append` / `read` / `search` / `status`). 외부 의존 0. | 항상 | ✅ | — |
| `notion_*` | 4 | Notion 페이지 캐시-우선 읽기 + refresh 시 heading-section diff (`get` / `refresh` / `status` / `extract`). 토큰 / OAuth 는 rocky 가 다루지 않고 전부 공식 Notion CLI (`ntn`) 위임. | `ntn` 탐지 시에만 | ✅ | — |

동반 플러그인 [`rocky-todo`](https://github.com/minjun0219/rocky-todo) 는 별도 데몬 프로세스로 `todo_list` / `todo_write` / `todo_status` / `note_list` / `note_write` 5 도구를 자신의 `/mcp` 엔드포인트에 노출한다 (별도 레포 — 설치/도구/설정은 그 레포 문서 참고).

각 도구의 입출력과 side effect 는 별도 문서가 아니라 **도구 정의 자체**가 단일 소스다 — `src/index.ts` (전체 표면) / `src/standalone.ts` (단독 CLI) 의 등록부를 읽으면 된다.

### Claude Code 전용 표면 (MCP tool 아님)

아래는 Claude Code plugin 으로 설치했을 때만 붙는다 (MCP tool 표면과 별개):

- **슬래시 커맨드** (`commands/`) — `/rocky:brainstorm` (아이디어를 설계로 — 맥락 파악 → 한 번에 하나씩 질문 → 접근안 2~3개 → 설계; **게이트가 아니라 도구**), `/rocky:review` (완료 선언 전 신선한 컨텍스트 서브에이전트로 현재 작업 diff 셀프 리뷰 — PR 스레드 대응인 `/rocky:resolve-reviews` 과 별개), `/rocky:finish` (게이트 → 커밋 → 푸시 → PR 생성), `/rocky:resolve-reviews` (PR 에 붙은 리뷰를 해소 — 판단이 필요 없는 명백한 오류는 즉시 고치고 코멘트 없이 resolve, 호출자가 확인해야 하는 건은 열어 둔 채 보고. GitHub 코멘트는 승인할 때만. 머지 가능 시 알림, 머지는 하지 않는다. 재리뷰를 기다리지 않는다), `/rocky:recall` (워크로그를 앵커 히스토리 다이제스트 `kind:"digest"` 로 증분 정리 — 기록의 짝인 **정리(整理)** 레이어). CI 실패 자동 수정은 Claude Code 빌트인 `/autofix-pr` 이 별도 선택지.
- **훅** (`hooks/hooks.json`) — `Stop` 하나뿐이다. 매 턴 종료 시 `kind:"turn"` 워크로그를 자동 기록한다 (결정론적, LLM 미사용; `worklog.autoCapture` 로 토글). fail-open — 실패해도 세션을 막지 않는다. **세션 컨텍스트에 얹히는 것은 아무것도 없다.**
- **스킬** (`skills/`) — `writing-cc-plugin`: Claude Code 플러그인 작성 가이드 + 매니페스트·컴포넌트·배포 레퍼런스. `todoist`: 세션에 연결된 Todoist MCP 로 현재 레포의 작업 목록을 파악·등록·마감하는 연동 스킬 — 다음 작업 제안은 Todoist + git 교차, 쓰기는 컨벤션 + 확인 게이트.

> **v0.19 에서 걷어낸 것** — 소울(페르소나) 주입과 `SessionStart` 훅, statusline 템플릿 3종과 동기화 훅, opencode 위임 런타임, `/rocky:codex` · `/rocky:issue` · `/rocky:opencode` · `/rocky:opencode-jobs` 커맨드. 재미로 넣었거나 실사용이 없던 것들이라 정리했다 — 전부 git 히스토리에서 꺼낼 수 있다. `rocky.json` 의 `soul` / `callsign` / `opencode` 키도 함께 사라져 이제 거부되니, 예전 설정 파일에 남아 있으면 지워야 한다.

## 시작하기

요구사항: [Bun](https://bun.sh) ≥ 1.0 (빌드 단계 없음 — Bun이 TS를 직접 실행한다).

### Claude Code plugin

이 저장소 자체가 플러그인 소스이자 마켓플레이스다 (`.claude-plugin/marketplace.json`, 별도 파사드 없음). 일반 설치는 GitHub 소스로:

```bash
claude plugin marketplace add minjun0219/rocky
claude plugin install rocky@rocky-marketplace
```

원격 세션 안에서는 `/plugin` 슬래시 커맨드로 동일하게 설치한다. 설치본은 GitHub `main`에서 clone되므로 코드 변경은 push 후 `claude plugin update rocky`로 반영된다.

설치본이 쓰는 MCP 서버는 `.claude-plugin/plugin.json`의 `mcpServers` (`${CLAUDE_PLUGIN_ROOT}/src/index.ts`) 하나뿐 — 저장소에 `.mcp.json`을 두지 않는 이유는 그게 설치본 MCP 설정으로 새기 때문이다.

설치 후 `openapi_envs` → `openapi_get` → `openapi_search` 흐름으로 spec을 둘러보면 된다. 레지스트리 (`rocky.json`)는 비어 있어도 URL 직접 입력으로 작동한다.

### `openapi-mcp` 단독 CLI

```bash
bun install
bun link                                          # 한 번만 — openapi-mcp 를 PATH 에 노출
openapi-mcp --config ~/.config/openapi-mcp/openapi-mcp.json
```

npm publish 는 아직 안 되어 있어 로컬 체크아웃 + `bun link` 로 쓴다. config 형태와 host 별 등록 예시는 [`docs/openapi-mcp.md`](./docs/openapi-mcp.md).

## 설정

전체 표면 서버는 `rocky.json` (project `./rocky.json` > user `~/.config/rocky/rocky.json`, [JSON Schema](./rocky.schema.json) 로 IDE 자동완성 지원)을, 단독 CLI 는 `openapi-mcp.json`을 읽는다:

```json
{
  "$schema": "https://raw.githubusercontent.com/minjun0219/rocky/main/rocky.schema.json",
  "openapi": {
    "registry": {
      "acme": {
        "dev": { "users": "https://dev.acme.example/openapi.json" }
      }
    }
  }
}
```

허용 키는 아래 넷뿐이다 (그 외 top-level 키는 즉시 reject — 오타 가드). 정확한 모양은 [`rocky.schema.json`](./rocky.schema.json) 과 `src/core/rocky-config.ts` 가 lockstep 으로 들고 있다.

| 키 | 내용 |
| --- | --- |
| `openapi.registry` | `host → env → spec → leaf` 평면 트리. 핸들 규칙은 `host:env:spec`, 각 식별자는 `^[a-zA-Z0-9_-]+$` (콜론은 separator 예약). leaf 는 URL 문자열 또는 `{ url, baseUrl?, format? }` |
| `seo` | `seo_validate` 기본값 — `allowPrivateHosts` (기본 false) / `timeoutMs` (1..30000). 도구 호출 인자가 우선 |
| `worklog` | `dir` (env `ROCKY_WORKLOG_DIR` 우선) / `autoCapture` (기본 true) / `captureMaxChars` (기본 800) / `digestThreshold` (기본 40) |
| `todo` | 형제 플러그인 rocky-todo 몫. rocky 는 **관용만** 하고 읽지 않는다 (공유 파일이라 거부하지 않을 뿐) |

### 환경 변수

`ROCKY_*` 는 전체 표면 서버(`src/index.ts`) 전용이라 단독 `openapi-mcp` CLI 는 인지하지 않는다 (CLI 는 `openapi-mcp.json` + XDG 변수만 본다). 예외로 `ROCKY_WORKLOG_AUTO_CAPTURE` 는 서버가 아니라 `Stop` 훅이 읽으므로 Claude Code 전용이다.

| 변수 | 기본값 | 영향 |
| --- | --- | --- |
| `ROCKY_CONFIG` | `~/.config/rocky/rocky.json` | user-level `rocky.json` 경로 override |
| `ROCKY_OPENAPI_CACHE_DIR` | `~/.config/rocky/openapi-specs` | OpenAPI spec 디스크 캐시 위치 |
| `ROCKY_OPENAPI_CACHE_TTL` | `300` (초) | spec 단위 `cacheTtlSeconds` 기본값 |
| `ROCKY_OPENAPI_DOWNLOAD_TIMEOUT_MS` | `10000` | spec 다운로드 HTTP timeout |
| `ROCKY_OPENAPI_INSECURE_TLS` | (unset) | `1`/`true` 면 TLS 검증 비활성 — 사내 self-signed 용, production 금지 |
| `ROCKY_OPENAPI_EXTRA_CA_CERTS` | (unset) | 추가 CA pem 경로 (`:` 구분). insecureTls 보다 안전한 사내 CA 옵션 |
| `ROCKY_NOTION_CLI` | `ntn` | Notion CLI 경로. 탐지될 때만 `notion_*` 4 도구가 등록된다 |
| `ROCKY_NOTION_CLI_TIMEOUT_MS` | `15000` | `ntn` subprocess timeout |
| `ROCKY_NOTION_CACHE_DIR` | `~/.config/rocky/notion-pages` | Notion 페이지 캐시 위치 |
| `ROCKY_NOTION_CACHE_TTL` | `86400` (24h) | Notion 캐시 TTL |
| `ROCKY_WORKLOG_DIR` | `~/.config/rocky/worklog/<project-key>` | 워크로그 JSONL 위치. `worklog.dir` 보다 우선 |
| `ROCKY_WORKLOG_AUTO_CAPTURE` | `1` | `Stop` 훅 턴 자동 기록 on/off. `0`/`false`/`off`/`no` 만 비활성 |
| `XDG_CONFIG_HOME` / `XDG_CACHE_HOME` | `~/.config` / `~/.cache` | 단독 CLI 의 config 검색 · 디스크 캐시 prefix |

## 문서 맵

이 README 가 사람용 진입점이고, 그보다 깊이 들어가는 문서는 아래가 전부다. 도구 하나하나의 입출력은 문서가 아니라 **도구 정의 자체**(`src/index.ts` / `src/standalone.ts`)가 단일 소스다 — 에이전트는 그걸 직접 읽는다.

| 문서 | 대상 | 내용 |
| --- | --- | --- |
| [`AGENTS.md`](./AGENTS.md) | 에이전트 (영문) | **단일 source of truth** — Layout / Scope / coding rules / change checklist / 리뷰 규약 |
| [`docs/architecture.md`](./docs/architecture.md) | 에이전트 (영문) | 코드만 봐선 안 나오는 설계 근거 — 필요할 때만 읽는 심화 레퍼런스 |
| [`docs/hosts.md`](./docs/hosts.md) | 사람 | 호스트 지원 매트릭스 — 세 호스트의 확장 메커니즘 + rocky 표면 커버 현황 (실측) |
| [`docs/backlog.md`](./docs/backlog.md) | 사람 | 백로그 — 보류 항목 + 도메인 재추가 후보 |
| [`docs/openapi-mcp.md`](./docs/openapi-mcp.md) | 사람 | 단독 CLI 설정 + host 별 등록 예시 |
| [`docs/codex.md`](./docs/codex.md) / [`docs/opencode.md`](./docs/opencode.md) | 사람 | 다른 host 에서 전체 표면 서버를 쓰고 싶을 때 |

## 역사 / 아카이브

v0.2 까지의 journal / mysql / spec-pact / pr-watch 도메인 + 에이전트 + 스킬은 [`archive/pre-openapi-only-slim`](https://github.com/minjun0219/rocky/tree/archive/pre-openapi-only-slim) 브랜치에 박제되어 있고, 활용 패턴이 잡히는 대로 [`docs/backlog.md`](./docs/backlog.md)의 후보 단위로 재추가한다 — notion은 v0.5 (`ntn` CLI 위임), journal은 v0.6 에 재추가되어 v0.9 에서 `worklog` 로 개명됐다. 예전 네이티브 opencode plugin 은 in-tree `.archive/` 에 두었다가 걷어냈다 — 필요하면 git 히스토리에서 꺼낸다. 현재 opencode 지원은 이 플러그인의 부활이 아니라 stdio MCP 등록 방식이다.

> 세 호스트에서 rocky 표면이 어디까지 커버되는지 (슬래시 커맨드·훅·스킬·소울 이식 가능 범위 포함) 는 [`docs/hosts.md`](./docs/hosts.md) 참고.

## 개발

```bash
bun install        # 의존성 (husky pre-commit / pre-push 훅도 함께 배선)
bun run check      # Biome 검증
bun run typecheck  # tsc --noEmit
bun test           # 단위 + smoke 테스트
```

같은 게이트를 `.husky/pre-commit` (lint-staged + 시크릿 스캔) / `.husky/pre-push` (typecheck + test) 와 CI ([`ci.yml`](./.github/workflows/ci.yml)) 가 반복 실행한다. 기여 규칙·레이아웃은 [`AGENTS.md`](./AGENTS.md).

개발 중 외부 라이브러리 문서용 `context7` MCP 는 유저 스코프에 둔다 (레포 `.mcp.json` 은 설치본으로 새므로 두지 않는다):

```bash
claude mcp add --scope user --transport http context7 https://mcp.context7.com/mcp
```

## 라이선스

[MIT](./LICENSE) © Minjun Kim
