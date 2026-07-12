# ROADMAP

본 toolkit 의 장기 비전 메모. 현재 출하된 MVP 는 [`AGENTS.md`](./AGENTS.md) 의 *MVP scope* 에 한정한다 — 이 문서는 그 너머의 목표를 정리한다. 새 기능은 항상 별도 PR 로, 한 번에 한 항목씩.

## 현재 (v0.5, openapi + seo + notion)

- 단일 패키지 (`@minjun0219/rocky`) — 두 배포 타깃이 동일 7-tool openapi surface 공유 (+ plugin 전용 `seo_validate`, `ntn` 탐지 시 `notion_*` 4 도구):
  - **Claude Code plugin** (`src/index.ts`, marketplace) — `.claude-plugin/plugin.json` 의 `mcpServers` 로 stdio MCP 등록.
  - **`openapi-mcp` 단독 CLI** (`bin/openapi-mcp` → `src/standalone.ts`, npm publish 는 별도 PR) — host-agnostic stdio MCP.
  - **공유 core** (`src/core/` — handlers / registry / cache / fetcher / parser / indexer / filter / adapter / config / schema). plugin 은 barrel (`./core`), standalone 은 `./core/<file>` subpath 로 import.
- archive 브랜치:
  - [`archive/pre-openapi-only-slim`](https://github.com/minjun0219/rocky/tree/archive/pre-openapi-only-slim) — v0.2 의 journal / mysql / notion / spec-pact / pr-watch + rocky / grace / mindy + 5 skills 박제. 도메인 재추가 작업의 포팅 기준.
  - [`.archive/agent-toolkit-opencode/`](./.archive/agent-toolkit-opencode) — 제거된 opencode plugin 배포 타깃 in-tree 박제 (게이트 제외).

## 재추가 완료

- **`notion-context` → v0.5 (2026-07)**: `ntn` (공식 Notion CLI) 위임으로 재추가. shape (a) plugin 직접 합류 + **CLI-gate** (`ntn` 탐지 시에만 도구 등록). 아카이브의 원안 (외부 Notion MCP OAuth 직접 호출) 대신 CLI 위임으로 인증 경로를 우회 — rocky 는 토큰 / OAuth 를 직접 다루지 않는다. `lib/notion-context.ts` → `src/core/notion-cache.ts`, `lib/notion-chunking.ts` → `src/core/notion-chunking.ts`, 신규 `src/core/notion-cli.ts` (executor + `pages get --json` 파서) + `src/core/notion-handlers.ts` + PR #60 의 `notion-diff.ts` POC 포팅. env 는 `ROCKY_NOTION_CLI` / `ROCKY_NOTION_CLI_TIMEOUT_MS` / `ROCKY_NOTION_CACHE_DIR` / `ROCKY_NOTION_CACHE_TTL`. 토큰/OAuth env (`ROCKY_NOTION_MCP_*`) 는 부활하지 않았다. 이 CLI 위임 shape 가 이후 auth-bearing 도메인의 참고 템플릿.

## 도메인 재추가 후보

각 도메인은 별도 PR. 재추가 시점에 다음 둘 중 하나의 shape 를 정한다:

- **(a) plugin 직접 합류** — 도메인 코드를 `src/core/` 에 두고, Claude Code plugin 진입점 (`src/index.ts`) 의 surface 에 도구를 등록한다. 별도 CLI 진입점은 만들지 않음.
- **(b) 별도 CLI 진입점 분리** — `openapi-mcp` 옆에 `bin/<domain>-mcp` + `src/<domain>.ts` standalone 진입점을 추가한다. 도메인이 plugin 외 host (Cursor / Continue / Claude Desktop) 에서도 자주 쓰일 때.

결정 기준은 활용 패턴 — host 독립성이 높으면 (b), plugin 안에서만 쓰이면 (a).

| 도메인 | archive 위치 (v0.2 경로) | 후보 shape | 비고 |
| --- | --- | --- | --- |
| `journal` (agent journal — append-only JSONL) | `lib/agent-journal.ts` + 4 tool (`journal_*`) | (a) plugin 합류 우선. 다른 host 에 노출 필요 없음. | turn-spanning memory; 재추가 우선순위 높음. |
| `mysql` (read-only inspection) | `lib/mysql-*.ts` + 5 tool (`mysql_*`) + `skills/mysql-query/` | (b) 별도 CLI 진입점 강력 후보 — DB inspector 는 host 독립적. | `mysql2` prod-dep 부활. `rocky.json` 의 `mysql.connections` 키 + `passwordEnv` / `dsnEnv` 정책. |
| `spec-pact` (DRAFT / VERIFY / DRIFT-CHECK / AMEND lifecycle) | `lib/spec-pact-fragments.ts` + 1 tool (`spec_pact_fragment`) + `skills/spec-pact/` + `agents/grace.md` | (a) plugin 합류. fragment loader 자체는 가벼움. | INDEX / SPEC 파일 lifecycle 은 `grace` sub-agent 책임. |
| `pr-review-watch` (polling-only, journal-backed) | `lib/pr-watch.ts` + 6 tool (`pr_*`) + `skills/pr-review-watch/` + `agents/mindy.md` | (a) plugin 합류. 외부 GitHub MCP 의존. | journal 도메인 재추가 후에. |

재추가 절차의 자세한 단계는 `AGENTS.md` 의 *Reintroduction strategy* 절.

## 능력 목표 (원본 메모, 분리 단위 유지)

1. 에이전트가 작업하거나 기억해야 하는 사항을 **자동으로 기억 / 기록** 해야 한다 — journal 도메인 재추가가 1차.
2. 작성한 코드와 관련하여 **주석을 상세하게** 작성 — runtime project comment guidance 로 이미 출하 (in this repo as `AGENTS.md` 의 동일 절).
3. 주석 / 설명을 **한글** 로 작성 — 동일.
4. **Notion MCP** 를 활용해 노션 문서를 캐싱하고, 일정 시간 내 같은 문서를 참고할 때 캐싱 사용 — notion-context 도메인 재추가 후.
5. 개발 기획 문서를 바탕으로 **명확한 개발 스펙으로 분해** — spec-pact 도메인 재추가 후.
6. 분해된 스펙을 **GitHub Issue / Project** 로 관리 / 추적 — rocky 안에서 GitHub 쓰기 surface 를 두지 않는다는 v0.2 결정 유지. 사용자 / Claude Code / 외부 GitHub MCP 책임.
7. 공유된 **Swagger / OpenAPI JSON** 을 로컬 캐시 → 빠르게 탐색 → `fetch` / `axios` 같은 API client 로 작성 — **이미 출하 (v0.3 main surface)**.

## 비전 (도메인 재추가 이후)

작업 컨텍스트를 들고 코드까지 굴리는 에이전트 오케스트레이션 toolkit. 세 갈래 방향이 있었다:

1. **업무 / 코딩 파트너로 단독 충분한 토대** — agent / skill / command / MCP / tool 다섯 종 primitive 을 적재적소에 섞어 쓰는 composition foundation.
2. **외부 primary 와의 시너지** — OmO Sisyphus / Superpowers 같은 외부 primary agent 가 동일 host 에 있을 때 description-driven routing 이 깨지지 않고 자연스럽게 위임이 흐른다.
3. **회사 맞춤 토킷의 base** — plugin (현재 형태) + library (`src/core/` exports) 두 형태로 패키징해 의존성으로 가져다 쓰는 토대.

세 방향 모두 도메인이 다시 모인 뒤 다시 본격적으로 추진. 그 전까지 이 문서의 우선순위는 **archive → main 재추가** 와 **두 배포 타깃의 openapi surface 유지**.

## 인프라 후보 (별도 PR)

- **npm publish 자동화** — `openapi-mcp` CLI 를 npm 에 올릴 시점에 changeset (`@changesets/cli`) 도입 + GitHub Actions release workflow. 첫 publish 필요 시점에 시작.
- **Project references 도입** — 현재 단일 패키지 (`tsconfig.json` 하나) 라 `tsc -b` project references 는 N/A. 도메인 재추가로 별도 CLI 진입점이 늘어나 빌드 그래프가 복잡해지면 재검토.
- **Repo rename** — ✅ 완료 (2026-07): `agent-toolkit` → `rocky` (Project Hail Mary 의 Rocky). GitHub repo / npm 패키지명 / plugin 명 / config (`rocky.json`, `~/.config/rocky/`) / env prefix (`ROCKY_*`) 일괄 전환.

## Out of scope

- OpenAPI YAML 스트림 파싱 (현재 전체 in-memory deref).
- Full SDK code generation.
- Multi-spec merge.
- Mock server.
- UI / dashboard.
- 자동 polling / 백그라운드 fetch (모든 캐시 갱신은 사용자 요청 시점 또는 stale-revalidate 백그라운드 1-shot).
