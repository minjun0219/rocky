# Rocky

[![CI](https://github.com/minjun0219/rocky/actions/workflows/ci.yml/badge.svg)](https://github.com/minjun0219/rocky/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](./LICENSE)
[![Runtime: Bun](https://img.shields.io/badge/Runtime-Bun%20%E2%89%A5%201.0-black)](https://bun.sh)

에이전트 코딩 세션을 위한 **개인용 MCP toolkit** — OpenAPI / Swagger 명세 캐시-우선 탐색, SEO 메타 검증, Notion 페이지 캐시, 에이전트 워크로그(기록↔정리)를 하나의 Bun 패키지로 묶었다. 이름은 *Project Hail Mary* 의 Rocky (스펙을 번역해 주는 엔지니어) 에서.

> **공개에 관하여** — 이 저장소는 소유자가 혼자 쓰려고 만든 개인 플러그인이다. 누구나 참고·포크·설치할 수 있도록 MIT로 공개하지만, 범용 제품이 아니라서 표면과 규칙이 소유자의 워크플로우에 맞춰 바뀐다. 구조와 패턴(단일 패키지 MCP 서버, CLI 위임, 기록↔정리 분리 등)을 참고 자료로 보는 쪽을 권한다.

## 한눈에

한 저장소가 두 stdio MCP 진입점을 노출한다:

| 진입점 | 역할 | 소비 호스트 |
| --- | --- | --- |
| **전체 표면 MCP 서버** (`src/index.ts`) | 아래 도구 표면 전부. | Claude Code plugin (`.claude-plugin/plugin.json` 의 `mcpServers`) |
| **`openapi-mcp` 단독 CLI** (`bin/openapi-mcp`) | OpenAPI 도구 7 개만 담은 host-agnostic subset. | 모든 stdio MCP host (Cursor / Continue / Claude Desktop / …) — [설정 가이드](./docs/openapi-mcp.md) |

### MCP 도구 표면

| 도구군 | 개수 | 하는 일 | 등록 조건 | 전체 표면 서버 | 단독 CLI |
| --- | --- | --- | --- | :---: | :---: |
| `openapi_*` | 7 | OpenAPI / Swagger spec 캐시-우선 fetch (`get` / `refresh` / `status`), endpoint 점수화 검색 (`search`), 레지스트리 조회 (`envs`), 단일 endpoint 상세 (`endpoint`), tag 목록 (`tags`). swagger 2.0 자동 변환 + `$ref` deref + TTL 디스크 캐시 + 백그라운드 재검증. | 항상 | ✅ | ✅ |
| `seo_validate` | 1 | 단일 URL 의 OG / Twitter Card / JSON-LD / favicon 메타 검증 ([`ogpeek`](https://www.npmjs.com/package/ogpeek) 기반, 기본 SSRF 가드). | 항상 | ✅ | — |
| `worklog_*` | 4 | append-only 로컬 JSONL **기록(記錄)** 레이어 — 결정 / blocker / 답변 / 메모를 turn 을 넘겨 남긴다 (`append` / `read` / `search` / `status`). 외부 의존 0. | 항상 | ✅ | — |
| `notion_*` | 4 | Notion 페이지 캐시-우선 읽기 + refresh 시 heading-section diff (`get` / `refresh` / `status` / `extract`). 토큰 / OAuth 는 rocky 가 다루지 않고 전부 공식 Notion CLI (`ntn`) 위임. | `ntn` 탐지 시에만 | ✅ | — |

전 도구의 입출력 / side effect / 관련 설정은 [`FEATURES.md`](./FEATURES.md) 가 도구별 6-필드 형식으로 정리한 **사람용 단일 source of truth** 다.

### Claude Code 전용 표면 (MCP tool 아님)

아래는 Claude Code plugin 으로 설치했을 때만 붙는다 (MCP tool 표면과 별개):

- **슬래시 커맨드** (`commands/`) — `/finish` (게이트 → 커밋 → 푸시 → PR 생성), `/recall` (워크로그를 앵커 히스토리 다이제스트 `kind:"digest"` 로 증분 정리 — 기록의 짝인 **정리(整理)** 레이어), `/codex` · `/opencode` (task 를 각 CLI 에 위임해 격리 worktree 에서 구현시키고 Claude 가 게이트·표면·diff 스코프 감시, 자동 병합 없음), `/issue` (다른 레포에서 떠오른 rocky 개선안을 GitHub Issue 로 캡처), `/rocky:soul` (소울 전환), `/rocky:statusline` (번들 statusline 설치/점검/해제). PR 감시·리뷰 반영은 Claude Code 빌트인 `/autofix-pr` 에 위임.
- **훅** (`hooks/hooks.json`) — `SessionStart`: 활성 소울(페르소나) 자동 주입 + 설치된 statusline 스크립트 자동 동기화. `Stop`: 매 턴 종료 시 `kind:"turn"` 워크로그 자동 기록 (결정론적, LLM 미사용; `worklog.autoCapture` 로 토글).
- **소울(페르소나)** (`souls/`) — `rocky.json` 의 `soul` 필드로 고정하는 말투/작업 방식 레이어. 번들 프리셋 `rocky` / `senior` / `terse` + 커스텀 (`~/.config/rocky/souls/`). 게이트·안전 규칙을 덮어쓰지 않으며, 미설정 시 아무 것도 주입하지 않는다.
- **스킬** (`skills/`) — `writing-cc-plugin`: Claude Code 플러그인 작성 가이드 + 매니페스트·컴포넌트·배포 레퍼런스. `delegating-to-codex`: 자기완결 task 를 headless OpenAI 모델 (`codex` CLI) 에 위임하는 패턴 + 가드레일 (`/codex` 커맨드가 얹히는 메커니즘 레이어). `todoist`: 세션에 연결된 Todoist MCP 로 현재 레포의 작업 목록을 파악·등록·마감하는 연동 스킬 — 다음 작업 제안은 Todoist + git + worklog 교차, 쓰기는 컨벤션 + 확인 게이트.
- **statusline** (`statusline/`) — statusLine 템플릿 3종: `duo` (2줄 — cwd+branch / model+ctx+세션 잔여율+리셋 타이머, 기본), `mini` (1줄 컴팩트), `full` (3줄 — 세션 비용·변경량·경과 추가). `/rocky:statusline` 이 고른 템플릿을 `~/.config/rocky/statusline.sh` 로 설치하고 user `settings.json` 을 1회 지정 — 이후 플러그인 업데이트는 `SessionStart` 훅이 같은 템플릿에서 자동 전파 (opt-in, 미설치 시 아무 것도 안 함).

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

`host:env:spec` 핸들 규칙, `soul` / `seo` / `worklog` 키, `ROCKY_*` 환경 변수 전체 표는 [`FEATURES.md`](./FEATURES.md#설정-파일) 참고.

## 문서 맵

| 문서 | 대상 | 내용 |
| --- | --- | --- |
| [`FEATURES.md`](./FEATURES.md) | 사람 (한국어) | **단일 source of truth** — 전 도구 카탈로그 / 환경 변수 / 설정 파일 / Quick start |
| [`AGENTS.md`](./AGENTS.md) | 에이전트 (영문) | **단일 source of truth** — Layout / MVP scope / coding rules / change checklist |
| [`docs/backlog.md`](./docs/backlog.md) | 사람 | 백로그 — 보류 항목 + 도메인 재추가 후보 + 비전 메모 |
| [`docs/openapi-mcp.md`](./docs/openapi-mcp.md) | 사람 | 단독 CLI 설정 + host 별 등록 예시 |
| [`docs/codex.md`](./docs/codex.md) / [`docs/opencode.md`](./docs/opencode.md) | 사람 | 다른 host 에서 전체 표면 서버를 쓰고 싶을 때 |
| [`REVIEW.md`](./REVIEW.md) | 리뷰 에이전트 | 이 레포의 코드 리뷰 규칙 |

## 역사 / 아카이브

v0.2 까지의 journal / mysql / spec-pact / pr-watch 도메인 + 에이전트 + 스킬은 [`archive/pre-openapi-only-slim`](https://github.com/minjun0219/rocky/tree/archive/pre-openapi-only-slim) 브랜치에 박제되어 있고, 활용 패턴이 잡히는 대로 [`docs/backlog.md`](./docs/backlog.md)의 후보 단위로 재추가한다 — notion은 v0.5 (`ntn` CLI 위임), journal은 v0.6 에 재추가되어 v0.9 에서 `worklog` 로 개명됐다. 예전 네이티브 opencode plugin 은 [`.archive/agent-toolkit-opencode/`](./.archive/agent-toolkit-opencode) 에 박제 (게이트 제외) — 현재 opencode 지원은 이 플러그인의 부활이 아니라 stdio MCP 등록 방식이다.

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
