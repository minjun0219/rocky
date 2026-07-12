# Rocky

OpenAPI / Swagger 명세를 캐시-우선으로 둘러보는 MCP toolkit — 이름은 *Project Hail Mary* 의 Rocky (스펙을 번역해 주는 엔지니어) 에서. v0.4 부터 **단일 패키지** 로 정리되어, 한 저장소가 두 배포 타깃을 노출한다:

| 배포 타깃 | 역할 | 설치 |
| --- | --- | --- |
| **Claude Code plugin** | `.claude-plugin/plugin.json` 의 `mcpServers` 로 stdio MCP 서버를 등록. marketplace 배포. | Claude Code plugin marketplace |
| **`openapi-mcp` 단독 CLI** | 어떤 stdio MCP host (Cursor / Continue / Claude Desktop / …) 든 등록해 쓰는 host-agnostic CLI. `bin/openapi-mcp`. | `bun link` (npm publish 는 별도 PR) |

둘 다 **동일한 7 openapi tool surface** (`openapi_get` / `openapi_refresh` / `openapi_status` / `openapi_search` / `openapi_envs` / `openapi_endpoint` / `openapi_tags`) 를 노출한다. 공유 core 는 [`src/core/`](./src/core) — spec 다운로드 / 디스크 캐시 / `$ref` deref / swagger 2.0 → OpenAPI 3 변환 / endpoint 점수화 검색 / handler 함수.

추가로 **Claude Code plugin 에만** `seo_validate` 도구가 붙는다 — 단일 URL 의 OG / Twitter Card / JSON-LD / favicon 메타를 [`ogpeek`](https://www.npmjs.com/package/ogpeek) 으로 fetch + parse 해 검증한다 (기본 SSRF 가드로 private / loopback 호스트 차단). 단독 `openapi-mcp` CLI 는 OpenAPI 도메인만 다루므로 이 도구는 노출하지 않는다.

v0.5 부터 **Claude Code plugin 에만**, 그리고 **공식 Notion CLI (`ntn`) 가 설치되어 있을 때만** (기동 시 `ntn --version` 탐지) `notion_*` 4 도구 (`notion_get` / `notion_refresh` / `notion_status` / `notion_extract`) 가 붙는다 — Notion 페이지를 캐시-우선으로 읽고 (TTL 이내면 CLI 미호출), `notion_refresh` 는 기존 캐시 대비 heading-section 단위 diff 를 함께 반환한다. rocky 는 Notion 토큰 / OAuth 를 직접 다루지 않는다 — 접근은 전부 `ntn` 위임 (`gh` CLI 위임과 동일 정책). `ntn` 이 없으면 이 도구들은 애초에 등록되지 않고, 설치는 됐지만 미로그인/권한 문제면 도구 호출 시점에 `NotionCliCommandError` 로 표면화된다.

MCP tool 외에, Claude Code plugin 은 `commands/` 의 **슬래시 커맨드** 도 노출한다 (`gh` CLI 기반) — `/finish` (게이트→커밋→푸시→PR 생성) 와 `/pr-watch` (그 PR 을 머지 가능 상태까지 감시·알림) 가 한 쌍. 자세한 건 [`FEATURES.md`](./FEATURES.md#claude-code-커맨드).

> - v0.2 까지의 journal / mysql / spec-pact / pr-watch 도메인은 [`archive/pre-openapi-only-slim`](https://github.com/minjun0219/rocky/tree/archive/pre-openapi-only-slim) 브랜치에 박제되어 있다 (notion 은 v0.5 에서 `ntn` CLI 위임으로 재추가됨).
> - opencode plugin 은 [`.archive/agent-toolkit-opencode/`](./.archive/agent-toolkit-opencode) 에 박제되어 있다 (게이트에서 제외).
>
> 활용 패턴이 잡히면 ROADMAP 의 phase 별로 도메인을 재추가한다.

- **사람용 단일 문서**: [`FEATURES.md`](./FEATURES.md) (한국어) — 도구 / 설정 / Quick start / 검증 한 페이지.
- **에이전트용 단일 문서**: [`AGENTS.md`](./AGENTS.md) (영문) — Layout / MVP scope / coding rules / change checklist.

## 진입점

### Claude Code plugin

저장소 root 에서 직접 trust 해 개발할 때:

1. `bun install`
2. Claude Code 가 `.mcp.json` 의 `rocky` stdio 서버 (`bun run ${CLAUDE_PROJECT_DIR}/src/index.ts`) + `context7` 두 서버를 처음 로드할 때 trust prompt 가 뜬다 — 둘 다 승인.
3. `openapi_envs` / `openapi_get` 등 호출.

marketplace 설치 시에는 `.claude-plugin/plugin.json` 의 `mcpServers` (`${CLAUDE_PLUGIN_ROOT}/src/index.ts`) 가 그대로 등록된다 — dev 전용 `context7` 는 딸려가지 않는다.

### `openapi-mcp` 단독 CLI

```bash
bun install
bun link                                          # 한 번만 — openapi-mcp 를 PATH 에 노출
openapi-mcp --config ~/.config/openapi-mcp/openapi-mcp.json
```

config 형태와 host 별 등록 예시는 [`docs/openapi-mcp.md`](./docs/openapi-mcp.md).

## 개발

```bash
bun install        # 의존성
bun run check      # Biome 검증
bun run typecheck  # tsc --noEmit
bun test           # 단위 + smoke 테스트
```
