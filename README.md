# Rocky

OpenAPI / Swagger 명세를 캐시-우선으로 둘러보는 MCP toolkit — 이름은 *Project Hail Mary* 의 Rocky (스펙을 번역해 주는 엔지니어) 에서. v0.4 부터 **단일 패키지** 로 정리되어, 한 저장소가 두 배포 타깃을 노출한다:

| 배포 타깃 | 역할 | 설치 |
| --- | --- | --- |
| **Claude Code plugin** | `.claude-plugin/plugin.json` 의 `mcpServers` 로 stdio MCP 서버를 등록. marketplace 배포. | Claude Code plugin marketplace |
| **`openapi-mcp` 단독 CLI** | 어떤 stdio MCP host (Cursor / Continue / Claude Desktop / …) 든 등록해 쓰는 host-agnostic CLI. `bin/openapi-mcp`. | `bun link` (npm publish 는 별도 PR) |

둘 다 **동일한 7 openapi tool surface** (`openapi_get` / `openapi_refresh` / `openapi_status` / `openapi_search` / `openapi_envs` / `openapi_endpoint` / `openapi_tags`) 를 노출한다. 공유 core 는 [`src/core/`](./src/core) — spec 다운로드 / 디스크 캐시 / `$ref` deref / swagger 2.0 → OpenAPI 3 변환 / endpoint 점수화 검색 / handler 함수.

추가로 **Claude Code plugin 에만** `seo_validate` 도구가 붙는다 — 단일 URL 의 OG / Twitter Card / JSON-LD / favicon 메타를 [`ogpeek`](https://www.npmjs.com/package/ogpeek) 으로 fetch + parse 해 검증한다 (기본 SSRF 가드로 private / loopback 호스트 차단). 단독 `openapi-mcp` CLI 는 OpenAPI 도메인만 다루므로 이 도구는 노출하지 않는다.

v0.5 부터 **Claude Code plugin 에만**, 그리고 **공식 Notion CLI (`ntn`) 가 설치되어 있을 때만** (기동 시 `ntn --version` 탐지) `notion_*` 4 도구 (`notion_get` / `notion_refresh` / `notion_status` / `notion_extract`) 가 붙는다 — Notion 페이지를 캐시-우선으로 읽고 (TTL 이내면 CLI 미호출), `notion_refresh` 는 기존 캐시 대비 heading-section 단위 diff 를 함께 반환한다. rocky 는 Notion 토큰 / OAuth 를 직접 다루지 않는다 — 접근은 전부 `ntn` 위임 (`gh` CLI 위임과 동일 정책). `ntn` 이 없으면 이 도구들은 애초에 등록되지 않고, 설치는 됐지만 미로그인/권한 문제면 도구 호출 시점에 `NotionCliCommandError` 로 표면화된다.

v0.6 부터 **Claude Code plugin 에만** `journal_*` 4 도구 (`journal_append` / `journal_read` / `journal_search` / `journal_status`) 가 붙는다 — **기록(記錄)** 레이어. append-only 로컬 JSONL 에 결정 / blocker / 답변 / 메모를 turn 을 넘겨 남긴다. 외부 의존이 없어(순수 파일시스템) `notion` 처럼 CLI-gate 하지 않고 항상 등록된다. 저장은 프로젝트별 (`~/.config/rocky/journal/<project-key>`, `ROCKY_JOURNAL_DIR` 로 변경). 짝이 되는 **정리(整理)** 레이어는 `/curate` 슬래시 커맨드가 담당한다 — 저널을 읽어 설정된 wiki 위치(Obsidian vault 등, `journal.wikiDir`)로 markdown 을 증류한다. rocky 는 기록·저장만 하고, 증류는 호스트 LLM 의 몫이라 Claude Code 네이티브 메모리와 역할이 겹치지 않는다.

MCP tool 외에, Claude Code plugin 은 `commands/` 의 **슬래시 커맨드** 도 노출한다 — `/finish` (게이트→커밋→푸시→PR 생성) 와 `/pr-watch` (그 PR 을 머지 가능 상태까지 감시·알림) 는 `gh` CLI 기반 한 쌍이고, `/curate` 는 `journal_*` 를 읽어 wiki 로 정리한다. 자세한 건 [`FEATURES.md`](./FEATURES.md#claude-code-커맨드).

> - v0.2 까지의 journal / mysql / spec-pact / pr-watch 도메인은 [`archive/pre-openapi-only-slim`](https://github.com/minjun0219/rocky/tree/archive/pre-openapi-only-slim) 브랜치에 박제되어 있다 (notion 은 v0.5, journal 은 v0.6 에서 재추가됨).
> - opencode plugin 은 [`.archive/agent-toolkit-opencode/`](./.archive/agent-toolkit-opencode) 에 박제되어 있다 (게이트에서 제외).
>
> 활용 패턴이 잡히면 ROADMAP 의 phase 별로 도메인을 재추가한다.

- **사람용 단일 문서**: [`FEATURES.md`](./FEATURES.md) (한국어) — 도구 / 설정 / Quick start / 검증 한 페이지.
- **에이전트용 단일 문서**: [`AGENTS.md`](./AGENTS.md) (영문) — Layout / MVP scope / coding rules / change checklist.

## 진입점

### Claude Code plugin

이 저장소 자체가 플러그인 소스이자 로컬 마켓플레이스다 (`.claude-plugin/marketplace.json`, 별도 파사드 없음). 개인용으로 한 번 설치:

```bash
bun install
claude plugin marketplace add /Users/minjun/dev/workspaces/agent-toolkit-rocky
claude plugin install rocky@rocky-local
```

`directory` 소스 마켓플레이스라 CC 는 저장소를 **제자리에서** 읽는다 (사본 X). 코드나 메타데이터를 고친 뒤 `/reload-plugins` 하면 재시작 없이 반영되고, `openapi_envs` / `openapi_get` 등을 호출한다. 설치본이 쓰는 MCP 서버는 `.claude-plugin/plugin.json` 의 `mcpServers` (`${CLAUDE_PLUGIN_ROOT}/src/index.ts`) 하나뿐 — 저장소에 `.mcp.json` 을 두지 않는 이유는 그게 설치본 MCP 설정으로 새기 때문이다. 개발 중 쓰는 `context7` 은 대신 유저 스코프에 둔다:

```bash
claude mcp add --scope user --transport http context7 https://mcp.context7.com/mcp
```

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
