# Rocky

OpenAPI / Swagger 명세를 캐시-우선으로 둘러보는 MCP toolkit — 이름은 *Project Hail Mary* 의 Rocky (스펙을 번역해 주는 엔지니어) 에서. v0.4 부터 **단일 패키지** 로 정리되어, 한 저장소가 두 배포 타깃을 노출한다:

| 배포 타깃 | 역할 | 설치 |
| --- | --- | --- |
| **Claude Code plugin** | `.claude-plugin/plugin.json` 의 `mcpServers` 로 stdio MCP 서버를 등록. marketplace 배포. | Claude Code plugin marketplace |
| **`openapi-mcp` 단독 CLI** | 어떤 stdio MCP host (Cursor / Continue / Claude Desktop / …) 든 등록해 쓰는 host-agnostic CLI. `bin/openapi-mcp`. | `bun link` (npm publish 는 별도 PR) |

둘 다 **동일한 7 openapi tool surface** (`openapi_get` / `openapi_refresh` / `openapi_status` / `openapi_search` / `openapi_envs` / `openapi_endpoint` / `openapi_tags`) 를 노출한다. 공유 core 는 [`src/core/`](./src/core) — spec 다운로드 / 디스크 캐시 / `$ref` deref / swagger 2.0 → OpenAPI 3 변환 / endpoint 점수화 검색 / handler 함수.

추가로 **Claude Code plugin 에만** `seo_validate` 도구가 붙는다 — 단일 URL 의 OG / Twitter Card / JSON-LD / favicon 메타를 [`ogpeek`](https://www.npmjs.com/package/ogpeek) 으로 fetch + parse 해 검증한다 (기본 SSRF 가드로 private / loopback 호스트 차단). 단독 `openapi-mcp` CLI 는 OpenAPI 도메인만 다루므로 이 도구는 노출하지 않는다.

> - v0.2 까지의 journal / mysql / notion / spec-pact / pr-watch 도메인은 [`archive/pre-openapi-only-slim`](https://github.com/minjun0219/rocky/tree/archive/pre-openapi-only-slim) 브랜치에 박제되어 있다.
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
