# Rocky — Features

> 이 toolkit 이 노출하는 표면을 한 페이지로 정리한 사람용 카탈로그.
> 대상: GitHub 에서 훑어 보는 사람, 그리고 grep / anchor 로 인용하는 에이전트 (Claude Code / codex / …).
> 이 파일이 **사람용 단일 source of truth** 다 (한국어). 에이전트용 단일 source 는 [`AGENTS.md`](./AGENTS.md) (영문). 표면이 바뀌면 두 파일을 같이 갱신한다.

## 한 눈에

- **단일 패키지 (`@minjun0219/rocky`) — 전체 표면 서버(`src/index.ts`) + 단독 OpenAPI CLI, 공유 7 openapi tool + 전체 표면 전용 `seo_validate` + CLI-gated `notion_*` + 기록 `worklog_*` (`Stop` hook 자동 기록 + 정리 `/recall`)**:
  | 진입점 / 소비 호스트 | 역할 | 설치 |
  | --- | --- | --- |
  | **전체 표면 MCP 서버** (`src/index.ts`) | Claude Code plugin 이 `.claude-plugin/plugin.json` 의 `mcpServers` 로 실행하고, Codex CLI 도 `~/.codex/config.toml` 로 같은 stdio MCP 서버를 실행. | Claude Code plugin marketplace 또는 Codex MCP 설정 |
  | **`openapi-mcp` 단독 CLI** (`bin/openapi-mcp` → `src/standalone.ts`) | host-agnostic subset MCP. 어떤 stdio MCP host (Cursor / Continue / Claude Desktop / …) 든 등록해 쓰는 단독 CLI. | `bun link` (npm publish 는 별도 PR) |
- **공유 core**: [`src/core/`](./src/core) — 두 타깃 모두 이 디렉토리의 `handlers.ts` / `registry.ts` / `adapter.ts` 등을 import. plugin 진입점은 barrel (`./core`) 로, standalone 은 `./core/<file>` subpath 로 가져온다.
- **Surface**: 공유 7 openapi tool (두 타깃 동일) — `openapi_get` / `openapi_refresh` / `openapi_status` / `openapi_search` / `openapi_envs` / `openapi_endpoint` / `openapi_tags` — 에 더해 Claude Code plugin 전용 `seo_validate` (OG / Twitter Card / JSON-LD / favicon 메타 검증, `ogpeek` 기반). 단독 `openapi-mcp` CLI 는 OpenAPI 도메인만 다뤄 `seo_validate` 를 노출하지 않는다. v0.5 부터 Claude Code plugin 은 **공식 Notion CLI (`ntn`) 가 탐지될 때만** `notion_*` 4 도구 (`notion_get` / `notion_refresh` / `notion_status` / `notion_extract`) 를 추가 등록한다 — `ntn` 이 없으면 아예 나타나지 않는다. v0.6 부터 Claude Code plugin 은 **기록(記錄)** 레이어인 `worklog_*` 4 도구 (`worklog_append` / `worklog_read` / `worklog_search` / `worklog_status`, v0.9 에서 `journal_*` 를 개명) 를 항상 등록한다 (외부 의존 없음) — append-only 로컬 JSONL. v0.9 부터 `Stop` hook (`src/hooks/log-turn.ts`) 이 매 턴 종료 시 `kind:"turn"` 워크로그를 자동으로 남기고(`autoCapture`, 기본 on), 짝이 되는 **정리(整理)** 는 `/recall` 슬래시 커맨드가 워크로그를 앵커 히스토리 다이제스트(`kind:"digest"`)로 증분 요약한다 (rocky 는 기록·저장만, 별도 wiki 위치는 없음).
- **소울(페르소나)**: Claude Code plugin 은 `rocky.json` 의 `soul` 필드로 고정한 페르소나를 `SessionStart` 훅이 자동 주입한다 (`matcher: startup|clear|compact` — 새 세션/clear/compact 시, resume 은 건너뜀). 소울은 markdown 파일(frontmatter `name`/`description` + 본문) — 번들 프리셋 3 종은 `souls/rocky.md` / `souls/senior.md` / `souls/terse.md`, 커스텀은 `~/.config/rocky/souls/<name>.md` (같은 이름이면 커스텀이 이김). `/rocky:soul` 로 목록/전환/미리보기/스캐폴딩. MCP tool 은 아니며, 미설정 시 아무 것도 주입하지 않는다(vanilla).
- **설정 파일**:
  - `rocky.json` — plugin 이 읽는다 (project 의 `./rocky.json` 이 user 의 `~/.config/rocky/rocky.json` 을 leaf 단위로 덮어쓴다). v0.3 부터 `openapi.registry` 한 키만 존재, 여기에 `soul` 도 추가.
  - `openapi-mcp.json` — 단독 CLI 가 읽는다. config 형태 (`specs.<name>.environments.<env>.baseUrl`) 가 다르고 평탄화 없이 그대로 SpecRegistry 에 들어간다.
- **런타임**: Bun ≥ 1.0. 빌드 단계 없음 (Bun 이 TS 직접 실행).

> - v0.2 까지 존재하던 journal / mysql / notion / spec-pact / pr-watch 5 도메인 + rocky / grace / mindy 3 에이전트 + 5 스킬은 [`archive/pre-openapi-only-slim`](https://github.com/minjun0219/rocky/tree/archive/pre-openapi-only-slim) 브랜치에 박제되어 있다. 이 중 **notion 은 v0.5 에서 `ntn` CLI 위임으로, journal 은 v0.6 에서 재추가되어 v0.9 에서 `worklog` 로 개명됨** (아래 `notion_*` / `worklog_*` 참고).
> - opencode plugin 은 [`.archive/agent-toolkit-opencode/`](./.archive/agent-toolkit-opencode) 에 박제되어 있다 (게이트에서 제외).
>
> 활용 패턴이 잡히면 ROADMAP 의 phase 단위로 재추가. 자세한 절차는 `AGENTS.md` 의 *Reintroduction strategy*.

각 도구 entry 는 한 블록으로 인용할 수 있도록 6-필드 형식을 따른다:

```
What           — 동작 한두 줄
Input          — 필수 + 선택 파라미터
Output         — 반환값의 최상위 shape
Side effects   — 디스크 / 네트워크 영향 (없으면 "none")
Related config — 이 도구가 읽는 env 변수 + rocky.json 키
Hosts          — 어디서 호출되는지 (Claude Code plugin / standalone CLI — openapi_* 는 둘 다, seo_validate 는 plugin 만)
```

## 도구

두 배포 타깃 모두 동일한 7 개 openapi 도구를 노출한다. handler 구현은 `src/core/handlers.ts` 한 곳에 정의되어 있고, Claude Code plugin (`src/index.ts`) 은 그걸 호출만 한다. 단독 CLI (`openapi-mcp`, `src/standalone.ts`) 는 자체 tool 정의를 가지되 같은 `SpecRegistry` 를 사용한다. 여기에 더해 `seo_validate` 는 Claude Code plugin 에만 등록된다 (handler 는 `src/core/seo-validate.ts`).

### `openapi_get`

- **What**: OpenAPI / Swagger spec 캐시 우선 fetch. swagger 2.0 자동 변환 + `$ref` deref. fresh hit 은 remote 호출 없음. stale hit (TTL 경과) 은 즉시 stale 데이터 반환 + 백그라운드 conditional GET (`If-None-Match` / `If-Modified-Since`) 으로 재검증. miss 면 fetch + parse + index.
- **Input**: `input` — spec URL (`http://` / `https://` / `file://`) 또는 `rocky.json` 의 `host:env:spec` 핸들. CLI 모드는 `openapi-mcp.json` 의 spec name + 옵셔널 environment.
- **Output**: `{ spec, environment, fromCache, document, baseUrl? }`. `document` 는 deref 된 OpenAPI 3.x.
- **Side effects**: miss 또는 stale revalidate 시 `<ROCKY_OPENAPI_CACHE_DIR>/<sha1>.json` 작성.
- **Related config**: `ROCKY_OPENAPI_CACHE_DIR`, `ROCKY_OPENAPI_CACHE_TTL`, `rocky.json` 의 `openapi.registry`.
- **Hosts**: 둘 다.

### `openapi_refresh`

- **What**: 메모리 + 디스크 캐시 무시하고 강제 재다운로드.
- **Input**: `input` — spec URL 또는 host:env:spec 핸들 (CLI 모드는 옵셔널 — 비우면 전체 refresh).
- **Output**: `RefreshOutcome[]` — 각 entry 의 success / failure / 캐시 메타.
- **Side effects**: 모든 환경의 캐시 파일 덮어쓰기.
- **Related config**: 동일.
- **Hosts**: 둘 다.

### `openapi_status`

- **What**: spec 의 캐시 메타 (`cached` / `fetchedAt` / `ttlSeconds` / `environments`) 만 조회. remote 호출 없음.
- **Input**: `input` — spec URL 또는 host:env:spec 핸들.
- **Output**: `SpecSummary`.
- **Side effects**: 없음.
- **Related config**: 동일.
- **Hosts**: 둘 다.

### `openapi_search`

- **What**: 캐시 (메모리 또는 디스크) 에 있는 spec 들을 가로질러 endpoint 점수화 검색 (operationId > path > summary > description). remote 호출 없음 — 미캐시 spec 은 검색 대상에서 빠지므로 먼저 `openapi_get` 으로 받아야 한다.
- **Input**: `query` (필수), `limit` (선택, 기본 20), `scope` (선택, `host` / `host:env` / `host:env:spec`).
- **Output**: `SwaggerSearchMatch[]` — `{ spec, operationId, method, path, summary?, tags?, deprecated }`.
- **Side effects**: 없음.
- **Related config**: 동일 + `openapi.registry` (scope 해석).
- **Hosts**: 둘 다.

### `openapi_envs`

- **What**: `rocky.json` 의 `openapi.registry` 를 host:env:spec 평면 리스트로 반환. baseUrl / format leaf 가 있으면 함께. remote 호출 없음. config 가 비면 빈 배열. (CLI 모드는 `openapi-mcp.json` 의 specs 를 평탄화.)
- **Input**: 없음.
- **Output**: `OpenapiRegistryEntry[]`.
- **Side effects**: 없음.
- **Related config**: `rocky.json` 의 `openapi.registry`.
- **Hosts**: 둘 다.

### `openapi_endpoint`

- **What**: 단일 endpoint 의 풍부한 정보 (parameters / requestBody / responses / examples / fullUrl). baseUrl 합성된 `fullUrl` 은 leaf 의 baseUrl 이 비면 path 자체.
- **Input**: `input` — spec URL 또는 host:env:spec 핸들. operationId 단독, 또는 method+path 페어 중 정확히 하나.
- **Output**: `{ spec, environment, endpoint }`.
- **Side effects**: 없음 (spec 이 미캐시면 fetch 가 트리거될 수 있음).
- **Related config**: 동일.
- **Hosts**: 둘 다.

### `openapi_tags`

- **What**: spec 의 OpenAPI tag 목록 + 각 tag 의 endpoint 개수.
- **Input**: `input` — spec URL 또는 host:env:spec 핸들.
- **Output**: `{ spec, environment, tags }` — tags 는 `TagSummary[]`.
- **Side effects**: 없음.
- **Related config**: 동일.
- **Hosts**: 둘 다.

### `seo_validate`

- **What**: 단일 URL 의 OG / Twitter Card / JSON-LD / favicon 메타를 `ogpeek` 으로 fetch + parse 해 검증한다. redirect 를 끝까지 추적하고, ogpeek warnings 를 severity 별 (`errors` / `warnings` / `info`) 로 분리한다. 기본 SSRF 가드가 private / loopback / link-local / IPv6 ULA 호스트를 차단한다 (IP literal 기준 — DNS rebinding 은 범위 밖).
- **Input**: `url` — 검증할 `http` / `https` URL (필수). `timeoutMs?` — fetch timeout (1..30000, 기본 8000). `allowPrivateHosts?` — SSRF 가드 비활성 (기본 `config.seo.allowPrivateHosts ?? false`).
- **Output**: `{ summary, raw }`. `summary` 는 finalUrl / redirects / og:title / og:description / og:image / og:type / og:url / canonical / errors / warnings / info / hasJsonLd / hasFavicon / iconCount. `raw` 는 ogpeek 의 원본 `OgDebugResult`.
- **Side effects**: 대상 URL 로 outbound HTTP GET (SSRF 가드 통과 시). 디스크 캐시 없음.
- **Related config**: `rocky.json` 의 `seo` (`allowPrivateHosts` / `timeoutMs`) — 도구 인자가 우선. env 변수 없음.
- **Hosts**: Claude Code plugin 만 (단독 CLI 미노출).

### `notion_*` (CLI-gated)

`notion_*` 4 도구는 **공식 Notion CLI (`ntn`) 가 탐지될 때만** Claude Code plugin 에 등록된다. rocky 는 Notion 토큰 / OAuth 를 직접 다루지 않는다 — 페이지 접근은 전부 `ntn pages get <id> --json` 위임 (`gh` CLI 위임과 동일 정책). 서버 기동 시 `ntn --version` 이 0 으로 끝나야 등록되고, 없으면 4 도구가 아예 나타나지 않는다. 캐시는 `<ROCKY_NOTION_CACHE_DIR>/<pageId>.{json,md}` 두 파일 (page 당). handler 는 `src/core/notion-handlers.ts`, CLI 위임은 `src/core/notion-cli.ts`.

#### `notion_get`

- **What**: Notion 페이지를 캐시 우선으로 가져온다. 캐시 hit (TTL 이내) 이면 `ntn` 미호출, miss / 만료면 `ntn pages get` 으로 1회 fetch 후 캐시. remote 가 요청과 다른 page id 를 돌려주면 캐시 거부 (오염 방지).
- **Input**: `input` — pageId 또는 Notion URL.
- **Output**: `{ entry, markdown, fromCache }`. `entry` 는 pageId / url / cachedAt / ttlSeconds / contentHash / title.
- **Side effects**: miss 시 `ntn` subprocess 1회 + 캐시 파일 2개 작성.
- **Related config**: `ROCKY_NOTION_CLI`, `ROCKY_NOTION_CLI_TIMEOUT_MS`, `ROCKY_NOTION_CACHE_DIR`, `ROCKY_NOTION_CACHE_TTL`.
- **Hosts**: Claude Code plugin 만 (`ntn` 탐지 시).

#### `notion_refresh`

- **What**: 캐시를 무시하고 강제 재fetch. 기존 캐시가 있으면 heading-section 단위 diff (`added` / `removed` / `modified` + line 수 + compact preview, 문서 등장 순서 정렬) 를 함께 반환해 긴 기획서의 변경 위치를 위에서부터 확인할 수 있다. 외부 diff 의존성 없이 자체 LCS 로 계산하며, 큰 섹션은 preview diff 를 상한으로 제한한다.
- **Input**: `input` — pageId 또는 Notion URL.
- **Output**: `{ entry, markdown, fromCache, diff? }`. `diff` 는 기존 캐시가 있을 때만 (`{ changed, previousHash, currentHash, sections[], truncated }`).
- **Side effects**: `ntn` subprocess 1회 + 캐시 파일 덮어쓰기.
- **Related config**: 동일.
- **Hosts**: Claude Code plugin 만 (`ntn` 탐지 시).

#### `notion_status`

- **What**: 캐시된 페이지의 메타 (`exists` / `expired` / `cachedAt` / `ttlSeconds` / `ageSeconds` / `title`) 만 조회. `ntn` 미호출.
- **Input**: `input` — pageId 또는 Notion URL.
- **Output**: `NotionCacheStatus`.
- **Side effects**: 없음.
- **Related config**: `ROCKY_NOTION_CACHE_DIR`.
- **Hosts**: Claude Code plugin 만 (`ntn` 탐지 시).

#### `notion_extract`

- **What**: 긴 페이지를 캐시 우선으로 읽고 heading 기반 chunk + 구현 액션 후보 (requirements / screens / apis / todos / questions) 를 규칙 기반으로 추출. remote 호출 정책은 `notion_get` 과 동일.
- **Input**: `input` — pageId 또는 URL. `maxCharsPerChunk?` — chunk 최대 문자 수 (기본 1400).
- **Output**: `{ entry, fromCache, chunkCount, chunks[], extracted }`.
- **Side effects**: `notion_get` 과 동일 (miss 시 `ntn` 1회).
- **Related config**: 동일.
- **Hosts**: Claude Code plugin 만 (`ntn` 탐지 시).

### `worklog_*` (기록 레이어, 구 `journal_*`)

`worklog_*` 4 도구는 **기록(記錄)** 레이어 — append-only 로컬 JSONL 에 결정 / blocker / 답변 / 메모를 turn 을 넘겨 남긴다. 외부 의존이 없어(순수 파일시스템) `notion` 처럼 CLI-gate 하지 않고 **항상 등록**된다 (Claude Code plugin 전용, 단독 CLI 미노출). 저장은 `<ROCKY_WORKLOG_DIR>/worklog.jsonl` 한 파일 — 미지정 시 프로젝트별 (`~/.config/rocky/worklog/<project-key>`, `project-key = basename(cwd)-sha1(cwd)[:8]`). handler 는 `src/core/worklog-handlers.ts`, 구현은 `src/core/worklog.ts`. v0.9 부터 `Stop` hook (`src/hooks/log-turn.ts`) 이 매 턴 종료 시 `kind:"turn"` 항목을 자동으로 append 한다 (`worklog.autoCapture`, 기본 on). 짝이 되는 **정리(整理)** 레이어는 rocky 가 아니라 `/recall` 슬래시 커맨드(호스트 LLM)가 담당한다 — 별도 wiki 문서가 아니라 워크로그 자체에 `kind:"digest"` 앵커 항목을 남기는 방식이라, Claude Code 네이티브 메모리와 역할이 겹치지 않는다. v0.9 이전의 `journal_*` 4 도구 + 정리 대상 `wikiDir` 설정은 제거되었다 (이름만 바뀐 rename — MCP tool 개수는 4 개로 그대로).

#### `worklog_append`

- **What**: 워크로그에 한 줄을 append-only 로 기록. `content` 는 trim 후 비면 거부, `pageId` 는 `resolveCacheKey` 로 정규화 후 저장. crash 로 마지막 줄이 `\n` 없이 끝나 있으면 leading `\n` 을 붙여 라인 경계를 강제한다. `Stop` hook 도 이 append 경로로 `kind:"turn"` 항목을 남긴다.
- **Input**: `content` (필수 본문). `kind?` (decision / blocker / answer / note / turn / digest 등, 기본 `note`). `tags?` (문자열 배열). `pageId?` (연결할 Notion page id 또는 URL).
- **Output**: 생성된 `WorklogEntry` (`id` / `timestamp` / `kind` / `content` / `tags` / `pageId?`).
- **Side effects**: JSONL 파일에 한 줄 append (필요 시 디렉터리 생성). remote 호출 없음.
- **Related config**: `ROCKY_WORKLOG_DIR`, `rocky.json` 의 `worklog.dir`.
- **Hosts**: Claude Code plugin 만.

#### `worklog_read`

- **What**: 가장 최근 항목부터 필터 / limit 적용해 반환. 손상된 라인은 자동 skip. 필터는 AND 결합.
- **Input**: `limit?` (기본 20). `kind?` (정확 일치). `tag?` (태그 포함). `pageId?` (정규화 후 일치). `since?` (해당 시각 이후, ISO8601).
- **Output**: `WorklogEntry[]` (최근순).
- **Side effects**: 없음 (read-only).
- **Hosts**: Claude Code plugin 만.

#### `worklog_search`

- **What**: substring (case-insensitive) 검색. `content` / `kind` / `tags` / `pageId` 를 매칭. 빈 query 는 전체 (kind 필터만 적용).
- **Input**: `query` (검색어). `limit?` (기본 20). `kind?` (풀 스코프 필터).
- **Output**: `WorklogEntry[]` (최근순).
- **Side effects**: 없음.
- **Hosts**: Claude Code plugin 만.

#### `worklog_status`

- **What**: 워크로그 메타(`path` / `exists` / `totalEntries` — 손상 라인 제외 / `sizeBytes` / `lastEntryAt`) + 프로젝트 키 `projectKey` (`<basename>-<hash8>`) + 마지막 `kind:"digest"` watermark(`lastDigestAt`) + 경로 출처 힌트 `dirSource`(`env` / `config` / `default`)를 조회. `dirSource` 는 소스를 안 읽어도 저장 위치가 어디서 왔는지 · env / `rocky.json` 으로 바꿀 수 있는지 발견하게 하는 힌트 (`dirSource:"default" ⟺` 기본 경로). `/recall` 이 정리 시작 시 이걸로 증분 기준점(`lastDigestAt`)을 확인한다. 정리 대상 wiki 위치는 더 이상 없다 (v0.9 에서 제거 — 정리 결과는 워크로그 자체의 `kind:"digest"` 항목으로 남는다).
- **Input**: 없음.
- **Output**: `WorklogStatus`.
- **Side effects**: 없음.
- **Related config**: `ROCKY_WORKLOG_DIR`, `rocky.json` 의 `worklog.dir`.
- **Hosts**: Claude Code plugin 만.

## Codex CLI 에서 쓰기

Codex CLI 는 `~/.codex/config.toml` 의 `[mcp_servers.<name>]` 테이블로 stdio MCP 서버를 등록한다. rocky 의 `src/index.ts` 는 이미 host-agnostic stdio MCP 서버라, Claude Code plugin 과 같은 프로세스(`bun run <repo>/src/index.ts`)를 그대로 등록하면 전체 도구(`openapi_*` 7 + `seo_validate` + `journal_*` 4 + `ntn` 설치 시 `notion_*` 4)를 쓴다.

```toml
[mcp_servers.rocky]
command = "bun"
args = ["run", "/abs/path/to/rocky/src/index.ts"]
```

동등한 CLI:

```bash
codex mcp add rocky -- bun run /abs/path/to/rocky/src/index.ts
```

전제는 간단하다. `/abs/path/to/rocky/src/index.ts` 는 실제 rocky 체크아웃 위치의 절대경로로 바꾸고, `bun` 이 Codex 가 보는 `PATH` 에 있어야 한다. 별도 install 없이 항상 최신 소스를 실행한다. `cwd` / env override / `notion_*` CLI-gate / Claude Code 전용 슬래시 커맨드·스킬 차이는 [`docs/codex.md`](./docs/codex.md)에 정리되어 있다.

## Claude Code 커맨드

MCP tool 과 별개로, Claude Code plugin 은 `commands/` 의 슬래시 커맨드를 노출한다. `/finish` 는 `gh` CLI 기반 — 게이트 통과 확인 후 커밋·푸시·PR 생성까지 마무리한다. `/recall` 은 `worklog_*` 를 읽어 앵커 히스토리 다이제스트로 정리하는 짝 커맨드다 (v0.9 에서 구 `/curate` 를 대체). 생성된 PR 의 감시·리뷰 반영은 Claude Code **빌트인 `/autofix-pr`** 에 위임한다 (클라우드 세션 + GitHub App webhook 기반 — rocky 커맨드가 아니며, 구 `/pr-watch` 는 v0.8 에서 제거됨). 그리고 `/codex` 는 task 하나를 Codex(`codex exec`)에 위임해 격리 worktree 에서 구현시키고 Claude 가 게이트·MCP 표면·diff 스코프를 감시하는 위임 커맨드다(자동 병합 없음). `/issue` 는 *다른* 레포에서 rocky 를 쓰다 떠오른 기능 제안·버그를 `minjun0219/rocky` GitHub Issue 로 캡처하는 `gh` 기반 커맨드다 — 현재 세션 맥락을 모으고 유사 이슈를 조회한 뒤 초안을 한 번 확인하고 생성한다(자동 생성 없음). `/rocky:soul` 은 소울(페르소나)을 고르는 커맨드다 — 목록 / 활성 소울 전환(`rocky.json` 의 `soul` 쓰기) / 미리보기 / 커스텀 소울 스캐폴딩.

### `/finish [힌트]`

- **What**: 현재 변경을 마무리한다 — 게이트(`bun run check` / `typecheck` / `test`) 통과 확인 → 변경 요약 → 브랜치 → 커밋 → 푸시 → PR 생성.
- **Input**: (옵션) 커밋/PR 요약에 참고할 힌트.
- **하지 않는 것**: 게이트 실패 시 커밋 금지(우회 X), `main` 직접 커밋 금지(먼저 브랜치), 무관한 파일 싸잡아 스테이지 금지.
- **규칙**: Conventional Commits 한국어 제목, 커밋 `Co-Authored-By` / PR 본문 서명 trailer 부착, 리뷰 요청 시 한국어 코멘트 요청.
- **의존성**: 인증된 `gh` CLI.

### `/recall [주제 힌트]`

- **What**: 워크로그(`worklog_*`)에 쌓인 **기록** — `Stop` hook 이 자동으로 남긴 `kind:"turn"` 항목 + 수동 `decision` / `blocker` / `answer` / `note` — 을 읽어 **앵커 히스토리 다이제스트**로 정리한다. 별도 wiki 문서가 아니라, 워크로그로 **찾아 들어갈 수 있는 앵커** (각 항목이 원본 엔트리 `id` 를 가리킴) 를 `worklog_append` `kind:"digest"` 한 줄로 남긴다 (v0.9 에서 구 `/curate` 를 대체).
- **Input**: (옵션) 이번 정리에서 집중할 주제 힌트.
- **동작**: `worklog_status` 로 `lastDigestAt` watermark 확인 → `worklog_read {since}` 로 새 항목 수집(+ 힌트 있으면 `worklog_search` 로 보강) → 새 항목 수 `n` 이 `rocky.json` 의 `worklog.digestThreshold`(기본 40) 이하면 Haiku, 초과면 Sonnet 서브에이전트(`Task`)로 앵커 다이제스트 생성 → `worklog_append {kind:"digest"}` 로 watermark 겸 결과 기록.
- **하지 않는 것**: raw 나열 금지(의미 있는 결정/전환/blocker/사용자 답변만 앵커로), 기존 워크로그 라인 수정/삭제 금지(append-only), 새 항목 0 이면 no-op(watermark 안 남김), 서브에이전트 실패 시 다이제스트 append 안 함(watermark 오염 방지), Claude Code 네이티브 메모리(글로벌)는 건드리지 않음. rocky 는 기록·저장만 하고 증류는 이 커맨드(호스트 LLM)가 한다.
- **의존성**: `worklog_*` MCP 도구 + `Task` (서브에이전트 dispatch). `gh` 불필요, 별도 wiki 설정 불필요.

## Claude Code 훅 (hooks)

MCP tool · 슬래시 커맨드와 별개로, Claude Code plugin 은 `hooks/hooks.json` 에 두 hook 을 등록한다 — `SessionStart` (소울 자동 주입) 와 `Stop` (v0.9, 턴 자동 기록).

### `SessionStart` — 소울(페르소나) 자동 주입

- **What**: 세션 시작 시 `rocky.json` 의 활성 `soul` 을 읽어, 해당 이름의 소울 파일(커스텀 `~/.config/rocky/souls/<name>.md` 우선, 없으면 번들 프리셋 `souls/<name>.md`) 을 찾아 페르소나 본문을 `additionalContext` 로 주입한다. `soul` 이 비어있거나 파일을 못 찾으면 아무 것도 주입하지 않는다(vanilla, opt-in 기본값). 주입되는 컨텍스트 맨 앞에는 "AGENTS.md/CLAUDE.md 의 게이트·안전 규칙이 항상 이긴다" 는 우선순위 preamble 이 붙는다 — 소울은 그 위의 말투/작업 방식 레이어일 뿐, override 가 아니다.
- **동작**: 세션 cwd 로 `rocky.json` 을 로드(project > user) → `soul` 필드 확인 → 소울 파일 read → frontmatter 제거한 본문 + preamble 로 컨텍스트 조립. 어떤 단계든 실패해도 세션 시작을 막지 않고 항상 exit 0 (fail-open).
- **주입 시점**: hook 은 `matcher: "startup|clear|compact"` 로 등록된다 — 컨텍스트가 새로 시작(`startup`)되거나 `/clear`·compact 로 초기화/축약된 뒤 소울을 (재)주입한다. `resume` 은 기존 컨텍스트(이미 주입된 소울 포함)가 그대로 살아있어 중복 주입을 건너뛴다.
- **Side effects**: 없음 (read-only, remote 호출 없음).
- **Related config**: `rocky.json` 의 `soul`. env 변수 없음.
- **Hosts**: Claude Code plugin 만. 구현은 `src/hooks/inject-soul.ts` (코어 로직은 `src/core/soul.ts`).

### `Stop` — 턴 자동 기록

- **What**: 이번 turn 의 사용자 요청(req)과 에이전트가 한 일(did, 사용한 tool 이름 포함)을 추출해 하나의 `content` 로 합성하고 `worklog_append {kind:"turn", tags:["turn"]}` 로 append 한다.
- **동작**: `rocky.json` 의 `worklog.autoCapture` (기본 true) 를 env `ROCKY_WORKLOG_AUTO_CAPTURE` 가 있으면 그게 이긴다 (`0` / `false` / `off` / `no` 값만 비활성, 그 외는 활성). req/did 는 각각 `worklog.captureMaxChars`(기본 800) 로 truncate.
- **Side effects**: 워크로그 JSONL 에 한 줄 append. remote 호출 없음.
- **Related config**: `ROCKY_WORKLOG_AUTO_CAPTURE`, `rocky.json` 의 `worklog.autoCapture` / `worklog.captureMaxChars`.
- **Hosts**: Claude Code plugin 만.

### `/codex <task>`

- **What**: task 하나를 **Codex(`codex exec`)에 구현자로 위임**하고, Claude 가 **감독자**로서
  결과를 검증하는 오케스트레이션 커맨드. Codex 는 새 git worktree(격리)에서 `-s workspace-write`
  (worktree 범위) 로 구현하고, Claude 는 게이트(`check`/`typecheck`/`test`) + MCP 도구 표면
  무결성(`src/index.test.ts`) + `plugin.json` mcpServers 무결 + diff 스코프를 감시한다.
- **감시 = "플러그인 작동 방해 안 하는지"**: 위 4가지가 모두 통과할 때만 "방해 없음" 으로 보고
  현재 브랜치에 병합한다. 하나라도 어기면 병합하지 않고 무엇을 깼는지 보고·에스컬레이션.
- **하지 않는 것**: 자동 병합·자동 push·PR 없음(승인 하 병합만, 이어서 `/finish`).
  `danger-full-access` 미사용. Claude 가 구현 코드를 직접 쓰지 않음(위임·게이트·판정만).
- **전제**: `codex` CLI 설치(`codex exec` 의 `-s workspace-write` 지원), 워킹 트리 clean.

### `/issue [아이디어/버그 한 줄]`

- **What**: *다른* 레포에서 rocky 를 쓰다 떠오른 **기능 제안**·**버그**를, 작업 흐름을 끊지 않고 `minjun0219/rocky` GitHub Issue 로 캡처한다. 현재 세션 맥락(출처 레포 / 트리거 상황 / 관련 코드·에러)을 자동으로 모아 이슈 본문에 담는다.
- **Input**: (옵션) 아이디어/버그 요지. 비어 있으면 최근 대화에서 유추하고, 모호하면 한 줄만 물어본다.
- **동작**: 타입→라벨 추론(레포에 **존재하는** 라벨만: `bug`/`enhancement`/`documentation`/`question` 등) → `gh issue list --search` 로 유사 열린 이슈 조회 → Conventional Commits 한국어 제목 + 본문(요지/출처/맥락/제안·재현) + 라벨 **초안 제시** → 사용자 확인(`y` / 수정 / 기존 `#N` 에 코멘트) → `gh issue create` 또는 `gh issue comment`.
- **하지 않는 것**: 확인 없이 자동 생성 금지(GitHub 은 외부 산출물), 새 라벨 생성 금지(존재하는 라벨만), 현재 레포 remote 신뢰 금지(항상 `--repo minjun0219/rocky` 명시), rocky 가 토큰 직접 취급 금지(전부 `gh` 위임).
- **의존성**: 인증된 `gh` CLI.
- **Hosts**: Claude Code plugin 만 (rocky 설치된 어느 세션에서든 호출 가능 — 다른 레포 포함).

### `/rocky:soul [list | <name> | show [name] | new <name>] [--project]`

- **What**: 로키의 소울(페르소나 — 말투/성격 + 작업 방식)을 고른다. 인자 없음 또는 `list` 는 프리셋(`${CLAUDE_PLUGIN_ROOT}/souls/`) + 커스텀(`~/.config/rocky/souls/`) 목록과 현재 활성 소울을 보여준다(같은 이름이면 커스텀이 이김). `<name>` 은 이름 검증(`^[a-zA-Z0-9_-]+$`) → 존재 확인 → 사용자 확인 후 `rocky.json` 의 `soul` 키만 갱신(기본 user `~/.config/rocky/rocky.json`, `--project` 면 `./rocky.json`, 다른 필드는 보존). `show [name]` 은 본문 미리보기(생략 시 현재 활성 소울). `new <name>` 은 `~/.config/rocky/souls/<name>.md` 에 frontmatter(`name`/`description`) + 빈 섹션 템플릿을 스캐폴딩(이미 있으면 덮어쓰지 않음).
- **Input**: 서브커맨드 + 옵션 `--project`.
- **하지 않는 것**: 소울로 AGENTS.md/CLAUDE.md 게이트·안전 규칙 override 금지, `rocky.json` 쓸 때 `soul` 외 필드 변경 금지, 확인 없이 활성 소울 전환 금지.
- **적용 시점**: `soul` 변경은 다음 세션부터 `SessionStart` 훅이 자동 주입 — 이번 세션에는 반영되지 않는다.
- **Hosts**: Claude Code plugin 만.

## Claude Code 스킬

MCP tool · 슬래시 커맨드와 별개로, Claude Code plugin 은 `skills/` 에 번들 스킬도 노출한다 (기본 `skills/` 자동 스캔, 플러그인 전용). 스킬은 `/rocky:<이름>` 으로 호출되거나 Claude 가 맥락에 따라 자동 사용한다.

### `writing-cc-plugin`

- **What**: Claude Code **플러그인 작성 가이드 + 레퍼런스**. plugin.json 매니페스트 / 컴포넌트(skills · agents · hooks · MCP · LSP · monitors · themes) / 디렉토리 구조 / 로컬 테스트(`--plugin-dir` · `--plugin-url` · `/reload-plugins`) / `.claude/` config → plugin 변환 / 마켓플레이스 배포 / 설치 scope / 버전 관리 / `claude plugin` CLI 를 다룬다.
- **구성**: `SKILL.md`(작성 워크플로우 + 자주 틀리는 gotcha 표 + quick reference) + `reference.md`(전체 스펙 §1–9 + 작성·배포 워크플로우 §10).
- **출처**: 공식 `/ko/plugins-reference` + `/ko/plugins` 문서 증류. 버전 게이트 기능·정확한 필드는 라이브 문서 재확인 권장.
- **Hosts**: Claude Code plugin 만 (standalone `openapi-mcp` CLI 에는 없음).

## 환경 변수

`ROCKY_*` 변수는 **Claude Code plugin 진입점 (`src/index.ts`) 전용** — standalone `openapi-mcp` CLI 는 인지하지 않는다 (CLI 는 `openapi-mcp.json` config 파일 + XDG 표준 변수만 본다).

| 변수 | 기본값 | 적용 host | 영향 |
| --- | --- | --- | --- |
| `ROCKY_OPENAPI_CACHE_DIR` | `~/.config/rocky/openapi-specs` | plugin | OpenAPI spec 디스크 캐시 위치. |
| `ROCKY_OPENAPI_CACHE_TTL` | `300` (초) | plugin | spec 단위 `cacheTtlSeconds` 기본값으로 주입 — `rocky.json` 의 leaf 에 별도 TTL 이 없을 때 사용. |
| `ROCKY_CONFIG` | `~/.config/rocky/rocky.json` | plugin | user-level `rocky.json` 경로 override. |
| `ROCKY_OPENAPI_DOWNLOAD_TIMEOUT_MS` | `10000` (ms) | plugin | spec 다운로드 HTTP timeout. |
| `ROCKY_OPENAPI_INSECURE_TLS` | (unset) | plugin | `1` / `true` 면 TLS 검증 비활성화 — 사내 self-signed 인증서 / 개발용. production 사용 금지. |
| `ROCKY_OPENAPI_EXTRA_CA_CERTS` | (unset) | plugin | 추가 CA pem 경로 (`:` 구분, Unix `PATH` 형식). insecureTls 보다 안전한 사내 CA 옵션. |
| `ROCKY_NOTION_CLI` | `ntn` | plugin | Notion CLI 바이너리 이름 / 경로. 이 바이너리가 탐지될 때만 `notion_*` 도구가 등록된다. |
| `ROCKY_NOTION_CLI_TIMEOUT_MS` | `15000` (ms) | plugin | `ntn` subprocess 호출 timeout. |
| `ROCKY_NOTION_CACHE_DIR` | `~/.config/rocky/notion-pages` | plugin | Notion 페이지 디스크 캐시 위치 (`<pageId>.json` + `<pageId>.md`). |
| `ROCKY_NOTION_CACHE_TTL` | `86400` (초, 24h) | plugin | Notion 캐시 entry TTL 기본값. |
| `ROCKY_WORKLOG_DIR` | `~/.config/rocky/worklog/<project-key>` | plugin | 워크로그 JSONL 저장 디렉터리. 지정 시 프로젝트별 기본 경로 대신 이 값을 verbatim 사용. `rocky.json` 의 `worklog.dir` 보다 우선. |
| `ROCKY_WORKLOG_AUTO_CAPTURE` | `1` (on) | plugin | `Stop` hook 의 턴 자동 기록 on/off. `0` / `false` / `off` / `no` 값만 비활성, 그 외는 활성. `rocky.json` 의 `worklog.autoCapture` 보다 우선. |
| `XDG_CONFIG_HOME` | `~/.config` | standalone CLI | `openapi-mcp.json` 기본 검색 경로의 prefix. |
| `XDG_CACHE_HOME` | `~/.cache` | standalone CLI | 디스크 캐시 디렉토리의 prefix (`openapi-mcp.json` 의 `cache.diskCachePath` 가 우선). |

standalone CLI 는 위 XDG 변수에 추가로 `openapi-mcp` CLI flag (`--config`, `--log-level`, `--insecure-tls`) 를 사용한다.

## 설정 파일

### `rocky.json` (plugin)

```json
{
  "$schema": "https://raw.githubusercontent.com/minjun0219/rocky/main/rocky.schema.json",
  "soul": "rocky",
  "openapi": {
    "registry": {
      "acme": {
        "dev":  { "users": "https://dev.acme.example/openapi.json" },
        "prod": {
          "users":  { "url": "https://api.acme.example/openapi.json", "baseUrl": "https://api.acme.example" },
          "orders": "https://orders.acme.example/openapi.json"
        }
      }
    }
  },
  "seo": {
    "allowPrivateHosts": false,
    "timeoutMs": 8000
  },
  "worklog": {
    "dir": "~/notes/rocky-worklog",
    "autoCapture": true,
    "captureMaxChars": 800,
    "digestThreshold": 40
  }
}
```

- `soul` (옵션): 활성 소울(페르소나) 이름 — `^[a-zA-Z0-9_-]+$`, 파일명 stem 과 동일한 값. `SessionStart` 훅이 이 값으로 `souls/<name>.md` (번들 프리셋, `${CLAUDE_PLUGIN_ROOT}/souls/`) 또는 `~/.config/rocky/souls/<name>.md` (커스텀, 같은 이름이면 이쪽이 이김) 를 찾아 세션 컨텍스트에 자동 주입한다. project (`./rocky.json`) 가 user 를 덮어쓴다. 미설정 시 주입 없음(vanilla, opt-in). `/rocky:soul <name>` 으로 전환.
- 핸들 규칙: `host:env:spec`. 각 식별자는 `^[a-zA-Z0-9_-]+$` — 콜론은 separator 예약.
- `seo` (옵션): `seo_validate` 도구 기본값. `allowPrivateHosts` (boolean, 기본 false) / `timeoutMs` (1..30000). 두 값 모두 도구 호출 인자가 우선. plugin 전용이며 단독 CLI 는 이 키를 읽지 않는다.
- `worklog` (옵션, v0.9 에서 `journal` 개명): `worklog_*` 기록 저장 위치(`dir`, env `ROCKY_WORKLOG_DIR` 가 우선), `Stop` hook 자동 기록 on/off(`autoCapture`, 기본 true, env `ROCKY_WORKLOG_AUTO_CAPTURE` 가 우선) + turn 항목 truncate 길이(`captureMaxChars`, 기본 800), `/recall` 의 Haiku↔Sonnet 임계(`digestThreshold`, 기본 40). 더 이상 `wikiDir` 는 없다 — 정리 결과는 워크로그 자체의 `kind:"digest"` 항목으로 남는다. plugin 전용이며 단독 CLI 는 이 키를 읽지 않는다.
- leaf 는 string (URL only) 또는 object (`{ url, baseUrl?, format? }`). `baseUrl` 은 `openapi_endpoint` 의 `fullUrl` 합성에 사용. `format` 은 `openapi3` / `swagger2` / `auto` (기본 auto).
- project (`./rocky.json`) 가 user (`~/.config/rocky/rocky.json`) 를 leaf 단위로 덮어쓴다.

미지원 top-level 키는 즉시 reject 된다 (`$schema` / `soul` / `openapi` / `seo` / `worklog` 만 허용 — `rocky.schema.json` 최상위 `additionalProperties:false` 와 런타임 `validateConfig` 둘 다 강제) — 오타 가드. 새 도메인이 재추가될 때는 이 허용 목록과 스키마를 함께 갱신해야 한다.

### `openapi-mcp.json` (단독 CLI)

```json
{
  "specs": {
    "acme-users": {
      "environments": {
        "dev":  { "baseUrl": "https://dev.acme.example", "source": { "type": "url", "url": "https://dev.acme.example/openapi.json" } },
        "prod": { "baseUrl": "https://api.acme.example", "source": { "type": "url", "url": "https://api.acme.example/openapi.json" } }
      }
    }
  }
}
```

자세한 옵션 (TLS, timeout, cache 경로) 은 [`docs/openapi-mcp.md`](./docs/openapi-mcp.md).

## Quick start

### `openapi-mcp` 단독 CLI (모든 stdio MCP host 에서 사용 가능)

```bash
bun install                                       # 의존성
bun link                                          # 한 번만 — repo root 에서 openapi-mcp 를 PATH 에 노출
openapi-mcp --config ~/.config/openapi-mcp/openapi-mcp.json
```

Claude Code / Cursor / Continue / Claude Desktop 등에 stdio MCP 서버로 등록해 사용. 설정 예시는 [`docs/openapi-mcp.md`](./docs/openapi-mcp.md).

### Claude Code plugin

1. 이 저장소를 로컬 마켓플레이스로 설치: `claude plugin marketplace add <repo>` → `claude plugin install rocky@rocky-local` (저장소 자체가 소스, 파사드 없음). 코드 변경은 `/reload-plugins` 로 반영.
2. `rocky.json` 을 user / project 위치에 둔다 (registry 비어 있어도 OK — URL 직접 입력으로도 작동).
3. `openapi_envs` → `openapi_get` → `openapi_search` 흐름으로 spec 둘러보기.

## 검증

```bash
bun install
bun run check
bun run typecheck
bun test
```

플러그인 설치 / dev 루프 (마켓플레이스 · `/reload-plugins` · context7 유저 스코프) 는 `AGENTS.md` 의 *Plugin source & dev loop* 절 참고.
