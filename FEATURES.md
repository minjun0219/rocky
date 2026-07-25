# Rocky — Features

> 이 toolkit 이 노출하는 표면을 한 페이지로 정리한 사람용 카탈로그.
> 대상: GitHub 에서 훑어 보는 사람, 그리고 grep / anchor 로 인용하는 에이전트 (Claude Code / Codex / opencode / …).
> 이 파일이 **사람용 단일 source of truth** 다 (한국어). 에이전트용 단일 source 는 [`AGENTS.md`](./AGENTS.md) (영문). 표면이 바뀌면 두 파일을 같이 갱신한다.

## 한눈에

- **단일 패키지 (`@minjun0219/rocky`) — 전체 표면 서버(`src/index.ts`) + 단독 OpenAPI CLI, 공유 7 openapi tool + 전체 표면 전용 `seo_validate` + CLI-gated `notion_*` + 기록 `worklog_*` (`Stop` hook 자동 기록 + 정리 `/rocky:recall`)**:
  | 진입점 / 소비 호스트 | 역할 | 설치 |
  | --- | --- | --- |
  | **전체 표면 MCP 서버** (`src/index.ts`) | Claude Code plugin 이 `.claude-plugin/plugin.json` 의 `mcpServers` 로 실행하고, Codex CLI 와 opencode 도 각각 host 설정으로 같은 stdio MCP 서버를 실행. | Claude Code plugin marketplace, Codex MCP 설정, opencode `opencode.json` |
  | **`openapi-mcp` 단독 CLI** (`bin/openapi-mcp` → `src/standalone.ts`) | host-agnostic subset MCP. 어떤 stdio MCP host (Cursor / Continue / Claude Desktop / …) 든 등록해 쓰는 단독 CLI. | `bun link` (npm publish 는 별도 PR) |
- **공유 core**: [`src/core/`](./src/core) — 두 타깃 모두 이 디렉토리의 `handlers.ts` / `registry.ts` / `adapter.ts` 등을 import. plugin 진입점은 barrel (`./core`) 로, standalone 은 `./core/<file>` subpath 로 가져온다.
- **Surface**: 공유 7 openapi tool (두 타깃 동일) — `openapi_get` / `openapi_refresh` / `openapi_status` / `openapi_search` / `openapi_envs` / `openapi_endpoint` / `openapi_tags` — 에 더해 전체 표면 서버 전용 `seo_validate` (OG / Twitter Card / JSON-LD / favicon 메타 검증, `ogpeek` 기반). 단독 `openapi-mcp` CLI 는 OpenAPI 도메인만 다뤄 `seo_validate` 를 노출하지 않는다. v0.5 부터 전체 표면 서버는 **공식 Notion CLI (`ntn`) 가 탐지될 때만** `notion_*` 4 도구 (`notion_get` / `notion_refresh` / `notion_status` / `notion_extract`) 를 추가 등록한다 — `ntn` 이 없으면 아예 나타나지 않는다. v0.6 부터 전체 표면 서버는 **기록(記錄)** 레이어인 `worklog_*` 4 도구 (`worklog_append` / `worklog_read` / `worklog_search` / `worklog_status`, v0.9 에서 `journal_*` 를 개명) 를 항상 등록한다 (외부 의존 없음) — append-only 로컬 JSONL. v0.9 부터 Claude Code plugin 의 `Stop` hook (`src/hooks/log-turn.ts`) 이 매 턴 종료 시 `kind:"turn"` 워크로그를 자동으로 남기고(`autoCapture`, 기본 on), 짝이 되는 **정리(整理)** 는 `/rocky:recall` 슬래시 커맨드가 워크로그를 앵커 히스토리 다이제스트(`kind:"digest"`)로 증분 요약한다 (rocky 는 기록·저장만, 별도 wiki 위치는 없음).
- **소울(페르소나)**: Claude Code plugin 은 `rocky.json` 의 `soul` 필드로 고정한 페르소나를 `SessionStart` 훅이 자동 주입한다 (`matcher: startup|clear|compact` — 새 세션/clear/compact 시, resume 은 건너뜀). 소울은 markdown 파일(frontmatter `name`/`description` + 본문) — 번들 프리셋 3 종은 `souls/rocky.md` / `souls/senior.md` / `souls/terse.md`, 커스텀은 `~/.config/rocky/souls/<name>.md` (같은 이름이면 커스텀이 이김). `/rocky:soul` 로 목록/전환/미리보기/스캐폴딩. MCP tool 은 아니며, 미설정 시 아무 것도 주입하지 않는다(vanilla).
- **statusline**: Claude Code plugin 은 statusline 템플릿 3종(`statusline/<name>.sh` — `duo` 2줄 기본 / `mini` 1줄 컴팩트 / `full` 3줄+git 상태(dirty/↑↓)·임계값 경고색·세션 비용(+시간당)·변경량·경과)을 번들한다. Claude Code 의 `statusLine` 설정은 user `settings.json` 에만 살 수 있으므로(플러그인 `settings.json` 은 미지원), `/rocky:statusline` 이 고른 템플릿을 안정 경로 `~/.config/rocky/statusline.sh` 로 복사하고 settings 를 1회 지정한다 — 이후 플러그인 업데이트는 `SessionStart` 훅이 설치본 헤더의 템플릿 마커를 보고 같은 템플릿에서 자동 전파. MCP tool 은 아니며, opt-in (설치 전에는 아무 것도 하지 않음). 템플릿별 표시 내용은 [`docs/statusline.md`](./docs/statusline.md) 참고.
- **rocky-todo (공유 작업 보드 데몬)**: 별도 레포/플러그인 [`minjun0219/rocky-todo`](https://github.com/minjun0219/rocky-todo) 로 분리됨 (v0.13 번들 → 2026-07-25 분리). 같은 rocky 마켓플레이스가 서빙 — `claude plugin install rocky-todo@rocky-marketplace` (설치=활성화, rocky 자동 동반). 공유 todo/스크래치패드 보드 + 웹 UI + 5 MCP 도구 + CLI.
- **설정 파일**:
  - `rocky.json` — plugin 이 읽는다 (project 의 `./rocky.json` 이 user 의 `~/.config/rocky/rocky.json` 을 leaf 단위로 덮어쓴다). v0.3 부터 `openapi.registry` 한 키만 존재, 여기에 `soul` 도 추가.
  - `openapi-mcp.json` — 단독 CLI 가 읽는다. config 형태 (`specs.<name>.environments.<env>.baseUrl`) 가 다르고 평탄화 없이 그대로 SpecRegistry 에 들어간다.
- **런타임**: Bun ≥ 1.0. 빌드 단계 없음 (Bun 이 TS 직접 실행).

> - v0.2 까지 존재하던 journal / mysql / notion / spec-pact / pr-watch 5 도메인 + rocky / grace / mindy 3 에이전트 + 5 스킬은 [`archive/pre-openapi-only-slim`](https://github.com/minjun0219/rocky/tree/archive/pre-openapi-only-slim) 브랜치에 박제되어 있다. 이 중 **notion 은 v0.5 에서 `ntn` CLI 위임으로, journal 은 v0.6 에서 재추가되어 v0.9 에서 `worklog` 로 개명됨** (아래 `notion_*` / `worklog_*` 참고).
> - 예전 네이티브 opencode plugin 은 [`.archive/agent-toolkit-opencode/`](./.archive/agent-toolkit-opencode) 에 박제되어 있다 (게이트에서 제외). 현재 opencode 지원은 이 플러그인의 부활이 아니라, `src/index.ts` stdio MCP 서버를 `opencode.json` 에 등록해 전체 표면을 소비하는 방식이다.
>
> 활용 패턴이 잡히면 `docs/backlog.md` 의 후보 단위로 재추가. 자세한 절차는 `AGENTS.md` 의 *Reintroduction strategy*.

각 도구 entry 는 한 블록으로 인용할 수 있도록 6-필드 형식을 따른다:

```
What           — 동작 한두 줄
Input          — 필수 + 선택 파라미터
Output         — 반환값의 최상위 shape
Side effects   — 디스크 / 네트워크 영향 (없으면 "none")
Related config — 이 도구가 읽는 env 변수 + rocky.json 키
Hosts          — 어디서 호출되는지 (전체 표면 서버 `src/index.ts` 실행 호스트 = Claude Code plugin / Codex / opencode  vs  단독 `openapi-mcp` CLI — openapi_* 는 둘 다, seo_validate / worklog_* / notion_* 는 전체 표면 서버 호스트만)
```

## 도구

두 배포 타깃 모두 동일한 7 개 openapi 도구를 노출한다. handler 구현은 `src/core/handlers.ts` 한 곳에 정의되어 있고, Claude Code plugin (`src/index.ts`) 은 그걸 호출만 한다. 단독 CLI (`openapi-mcp`, `src/standalone.ts`) 는 자체 tool 정의를 가지되 같은 `SpecRegistry` 를 사용한다. 여기에 더해 `seo_validate` 는 전체 표면 서버(`src/index.ts`)에만 등록되어 이를 실행하는 모든 호스트(Claude Code plugin / Codex / opencode)에서 쓰이고, 단독 `openapi-mcp` CLI 에는 없다 (handler 는 `src/core/seo-validate.ts`).

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
- **Hosts**: 전체 표면 서버(`src/index.ts`) 실행 호스트 (Claude Code plugin / Codex / opencode). 단독 `openapi-mcp` CLI 미노출.

### `notion_*` (CLI-gated)

`notion_*` 4 도구는 **공식 Notion CLI (`ntn`) 가 탐지될 때만** 전체 표면 서버(`src/index.ts`)에 등록된다 (이를 실행하는 Claude Code plugin / Codex / opencode 에서 사용). rocky 는 Notion 토큰 / OAuth 를 직접 다루지 않는다 — 페이지 접근은 전부 `ntn pages get <id> --json` 위임 (`gh` CLI 위임과 동일 정책). 서버 기동 시 `ntn --version` 이 0 으로 끝나야 등록되고, 없으면 4 도구가 아예 나타나지 않는다. 캐시는 `<ROCKY_NOTION_CACHE_DIR>/<pageId>.{json,md}` 두 파일 (page 당). handler 는 `src/core/notion-handlers.ts`, CLI 위임은 `src/core/notion-cli.ts`.

#### `notion_get`

- **What**: Notion 페이지를 캐시 우선으로 가져온다. 캐시 hit (TTL 이내) 이면 `ntn` 미호출, miss / 만료면 `ntn pages get` 으로 1회 fetch 후 캐시. remote 가 요청과 다른 page id 를 돌려주면 캐시 거부 (오염 방지).
- **Input**: `input` — pageId 또는 Notion URL.
- **Output**: `{ entry, markdown, fromCache }`. `entry` 는 pageId / url / cachedAt / ttlSeconds / contentHash / title.
- **Side effects**: miss 시 `ntn` subprocess 1회 + 캐시 파일 2개 작성.
- **Related config**: `ROCKY_NOTION_CLI`, `ROCKY_NOTION_CLI_TIMEOUT_MS`, `ROCKY_NOTION_CACHE_DIR`, `ROCKY_NOTION_CACHE_TTL`.
- **Hosts**: 전체 표면 서버(`src/index.ts`) 실행 호스트 (Claude Code plugin / Codex / opencode), `ntn` 탐지 시. 단독 `openapi-mcp` CLI 미노출.

#### `notion_refresh`

- **What**: 캐시를 무시하고 강제 재fetch. 기존 캐시가 있으면 heading-section 단위 diff (`added` / `removed` / `modified` + line 수 + compact preview, 문서 등장 순서 정렬) 를 함께 반환해 긴 기획서의 변경 위치를 위에서부터 확인할 수 있다. 외부 diff 의존성 없이 자체 LCS 로 계산하며, 큰 섹션은 preview diff 를 상한으로 제한한다.
- **Input**: `input` — pageId 또는 Notion URL.
- **Output**: `{ entry, markdown, fromCache, diff? }`. `diff` 는 기존 캐시가 있을 때만 (`{ changed, previousHash, currentHash, sections[], truncated }`).
- **Side effects**: `ntn` subprocess 1회 + 캐시 파일 덮어쓰기.
- **Related config**: 동일.
- **Hosts**: 전체 표면 서버(`src/index.ts`) 실행 호스트 (Claude Code plugin / Codex / opencode), `ntn` 탐지 시. 단독 `openapi-mcp` CLI 미노출.

#### `notion_status`

- **What**: 캐시된 페이지의 메타 (`exists` / `expired` / `cachedAt` / `ttlSeconds` / `ageSeconds` / `title`) 만 조회. `ntn` 미호출.
- **Input**: `input` — pageId 또는 Notion URL.
- **Output**: `NotionCacheStatus`.
- **Side effects**: 없음.
- **Related config**: `ROCKY_NOTION_CACHE_DIR`.
- **Hosts**: 전체 표면 서버(`src/index.ts`) 실행 호스트 (Claude Code plugin / Codex / opencode), `ntn` 탐지 시. 단독 `openapi-mcp` CLI 미노출.

#### `notion_extract`

- **What**: 긴 페이지를 캐시 우선으로 읽고 heading 기반 chunk + 구현 액션 후보 (requirements / screens / apis / todos / questions) 를 규칙 기반으로 추출. remote 호출 정책은 `notion_get` 과 동일.
- **Input**: `input` — pageId 또는 URL. `maxCharsPerChunk?` — chunk 최대 문자 수 (기본 1400).
- **Output**: `{ entry, fromCache, chunkCount, chunks[], extracted }`.
- **Side effects**: `notion_get` 과 동일 (miss 시 `ntn` 1회).
- **Related config**: 동일.
- **Hosts**: 전체 표면 서버(`src/index.ts`) 실행 호스트 (Claude Code plugin / Codex / opencode), `ntn` 탐지 시. 단독 `openapi-mcp` CLI 미노출.

### `worklog_*` (기록 레이어, 구 `journal_*`)

`worklog_*` 4 도구는 **기록(記錄)** 레이어 — append-only 로컬 JSONL 에 결정 / blocker / 답변 / 메모를 turn 을 넘겨 남긴다. 외부 의존이 없어(순수 파일시스템) `notion` 처럼 CLI-gate 하지 않고 **항상 등록**된다 (전체 표면 서버 `src/index.ts` 실행 호스트 = Claude Code plugin / Codex / opencode; 단독 `openapi-mcp` CLI 미노출). 저장은 `<ROCKY_WORKLOG_DIR>/worklog.jsonl` 한 파일 — 미지정 시 프로젝트별 (`~/.config/rocky/worklog/<project-key>`, `project-key = basename(cwd)-sha1(cwd)[:8]`). handler 는 `src/core/worklog-handlers.ts`, 구현은 `src/core/worklog.ts`. v0.9 부터 **Claude Code plugin 의** `Stop` hook (`hooks/hooks.json` → `src/hooks/log-turn.ts`) 이 매 턴 종료 시 `kind:"turn"` 항목을 자동으로 append 한다 (`worklog.autoCapture`, 기본 on) — 이 자동 기록만은 Claude Code 전용이다 (Codex/opencode 에는 훅이 없어, `worklog_*` 도구 자체는 세 호스트 모두에서 쓰지만 턴 자동 캡처는 안 된다). 짝이 되는 **정리(整理)** 레이어는 rocky 가 아니라 `/rocky:recall` 슬래시 커맨드(호스트 LLM)가 담당한다 — 별도 wiki 문서가 아니라 워크로그 자체에 `kind:"digest"` 앵커 항목을 남기는 방식이라, Claude Code 네이티브 메모리와 역할이 겹치지 않는다. v0.9 이전의 `journal_*` 4 도구 + 정리 대상 `wikiDir` 설정은 제거되었다 (이름만 바뀐 rename — MCP tool 개수는 4 개로 그대로).

#### `worklog_append`

- **What**: 워크로그에 한 줄을 append-only 로 기록. `content` 는 trim 후 비면 거부, `pageId` 는 `resolveCacheKey` 로 정규화 후 저장. crash 로 마지막 줄이 `\n` 없이 끝나 있으면 leading `\n` 을 붙여 라인 경계를 강제한다. `Stop` hook 도 이 append 경로로 `kind:"turn"` 항목을 남긴다.
- **Input**: `content` (필수 본문). `kind?` (decision / blocker / answer / note / turn / digest 등, 기본 `note`). `tags?` (문자열 배열). `pageId?` (연결할 Notion page id 또는 URL).
- **Output**: 생성된 `WorklogEntry` (`id` / `timestamp` / `kind` / `content` / `tags` / `pageId?`).
- **Side effects**: JSONL 파일에 한 줄 append (필요 시 디렉터리 생성). remote 호출 없음.
- **Related config**: `ROCKY_WORKLOG_DIR`, `rocky.json` 의 `worklog.dir`.
- **Hosts**: 전체 표면 서버(`src/index.ts`) 실행 호스트 (Claude Code plugin / Codex / opencode). 단독 `openapi-mcp` CLI 미노출.

#### `worklog_read`

- **What**: 가장 최근 항목부터 필터 / limit 적용해 반환. 손상된 라인은 자동 skip. 필터는 AND 결합.
- **Input**: `limit?` (기본 20). `kind?` (정확 일치). `tag?` (태그 포함). `pageId?` (정규화 후 일치). `since?` (해당 시각 이후, ISO8601).
- **Output**: `WorklogEntry[]` (최근순).
- **Side effects**: 없음 (read-only).
- **Hosts**: 전체 표면 서버(`src/index.ts`) 실행 호스트 (Claude Code plugin / Codex / opencode). 단독 `openapi-mcp` CLI 미노출.

#### `worklog_search`

- **What**: substring (case-insensitive) 검색. `content` / `kind` / `tags` / `pageId` 를 매칭. 빈 query 는 전체 (kind 필터만 적용).
- **Input**: `query` (검색어). `limit?` (기본 20). `kind?` (풀 스코프 필터).
- **Output**: `WorklogEntry[]` (최근순).
- **Side effects**: 없음.
- **Hosts**: 전체 표면 서버(`src/index.ts`) 실행 호스트 (Claude Code plugin / Codex / opencode). 단독 `openapi-mcp` CLI 미노출.

#### `worklog_status`

- **What**: 워크로그 메타(`path` / `exists` / `totalEntries` — 손상 라인 제외 / `sizeBytes` / `lastEntryAt`) + 프로젝트 키 `projectKey` (`<basename>-<hash8>`) + 마지막 `kind:"digest"` watermark(`lastDigestAt`) + 경로 출처 힌트 `dirSource`(`env` / `config` / `default`)를 조회. `dirSource` 는 소스를 안 읽어도 저장 위치가 어디서 왔는지 · env / `rocky.json` 으로 바꿀 수 있는지 발견하게 하는 힌트 (`dirSource:"default" ⟺` 기본 경로). `/rocky:recall` 이 정리 시작 시 이걸로 증분 기준점(`lastDigestAt`)을 확인한다. 정리 대상 wiki 위치는 더 이상 없다 (v0.9 에서 제거 — 정리 결과는 워크로그 자체의 `kind:"digest"` 항목으로 남는다).
- **Input**: 없음.
- **Output**: `WorklogStatus`.
- **Side effects**: 없음.
- **Related config**: `ROCKY_WORKLOG_DIR`, `rocky.json` 의 `worklog.dir`.
- **Hosts**: 전체 표면 서버(`src/index.ts`) 실행 호스트 (Claude Code plugin / Codex / opencode). 단독 `openapi-mcp` CLI 미노출.

### `rocky-todo` (별도 레포로 분리됨)

공유 todo / 스크래치패드 보드 데몬(`todo_list` / `todo_write` / `todo_status` / `note_list` /
`note_write` 5 도구 + 웹 UI + CLI)은 v0.13 에 rocky 에 번들됐다가 **별도 레포/플러그인
[`minjun0219/rocky-todo`](https://github.com/minjun0219/rocky-todo) 로 분리**됐다. 같은 rocky
마켓플레이스가 서빙하니 `claude plugin install rocky-todo@rocky-marketplace` 로 설치하면 된다
(설치=활성화, `dependencies:["rocky"]`). 도구·설정·설치는 그 레포의 문서를 참고.

## Codex CLI 에서 쓰기

Codex CLI 는 `~/.codex/config.toml` 의 `[mcp_servers.<name>]` 테이블로 stdio MCP 서버를 등록한다. rocky 의 `src/index.ts` 는 이미 host-agnostic stdio MCP 서버라, Claude Code plugin 과 같은 프로세스(`bun run <repo>/src/index.ts`)를 그대로 등록하면 전체 도구(`openapi_*` 7 + `seo_validate` + `worklog_*` 4 + `ntn` 설치 시 `notion_*` 4)를 쓴다.

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

## opencode 에서 쓰기

opencode 는 `opencode.json` 의 `mcp` 섹션으로 local stdio MCP 서버를 등록한다. user 스코프는 `~/.config/opencode/opencode.json`, project 스코프는 레포 루트의 `opencode.json` 이다. rocky 의 `src/index.ts` 는 이미 host-agnostic stdio MCP 서버라, Claude Code plugin 과 같은 프로세스(`bun run <repo>/src/index.ts`)를 그대로 등록하면 전체 도구(`openapi_*` 7 + `seo_validate` + `worklog_*` 4 + `ntn` 설치 시 `notion_*` 4)를 쓴다.

```json
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "rocky": {
      "type": "local",
      "command": ["bun", "run", "/abs/path/to/rocky/src/index.ts"],
      "enabled": true
    }
  }
}
```

동등한 CLI:

```bash
opencode mcp add rocky
```

`type` 은 `"local"` 고정(필수), `command` 는 문자열 배열이다. 환경 변수는 `env` 가 아니라 `environment` 객체로 지정한다. 예: `"environment": { "ROCKY_WORKLOG_DIR": "...", "ROCKY_NOTION_CACHE_DIR": "..." }`. `timeout` 은 옵션이며 기본값은 5000ms 다. `/abs/path/to/rocky/src/index.ts` 는 실제 rocky 체크아웃 위치의 절대경로로 바꾸고, `bun` 이 opencode 가 보는 `PATH` 에 있어야 한다. 자세한 설정과 주의점은 [`docs/opencode.md`](./docs/opencode.md)에 정리되어 있다.

## 호스트 지원 매트릭스

> rocky 는 세 full-surface 호스트(Claude Code plugin / Codex CLI / opencode)에서 **오늘 기준 MCP 도구만** 공유한다. 슬래시 커맨드·훅·스킬·소울·statusline 은 Claude Code plugin 에만 배포돼 있다. **단, 이는 "다른 호스트가 그 확장을 못 한다"는 뜻이 아니다** — Codex 와 opencode 도 2026 기준 커맨드 / 훅 / 스킬 / 서브에이전트 / 번들 플러그인을 네이티브로 지원한다. rocky 가 아직 그 호스트용 버전을 만들지 않았을 뿐이라 대부분 이식 가능하다. 아래 두 표가 (A) 호스트가 네이티브로 뭘 지원하는지 와 (B) rocky 표면이 각 호스트에서 어디까지 커버되는지 를 나눠 보여준다.

### A. 호스트 확장 메커니즘 (네이티브 지원)

| 메커니즘 | Claude Code | Codex CLI | opencode |
| --- | --- | --- | --- |
| MCP stdio 서버 | ✅ | ✅ `[mcp_servers.*]` | ✅ `mcp{type:"local"}` |
| 슬래시 커맨드 | ✅ `commands/` (project+user) | ◐ `~/.codex/prompts/*.md` (user-only, **deprecated → skills**) | ✅ `.opencode/command/*.md` (`$ARGUMENTS`/`$1`/`` !`sh` ``/`@file`) |
| SessionStart 훅 | ✅ | ◐ `SessionStart` hook (**실험적 · 기본 off · no Windows**) 또는 AGENTS.md 정적 병합 | ✅ plugin `session.created` / `instructions` |
| Stop · 턴 훅 | ✅ | ◐ `Stop` hook(실험) 또는 `notify`(agent-turn-complete) | ✅ plugin `session.idle` / `message.updated` |
| Skills (SKILL.md) | ✅ | ✅ 동일 스펙 | ✅ `.claude/skills/` 직접 읽음 |
| Subagents | ✅ | ✅ `.codex/agents/*.toml` | ✅ `.opencode/agent/*.md` |
| AGENTS.md 세션 주입 | ✅ | ✅ (3-scope) | ✅ (3-scope + `CLAUDE.md` fallback) |
| 단일 번들 플러그인 + 마켓플레이스 | ✅ `.claude-plugin/` + `marketplace.json` | ✅ `.codex-plugin/plugin.json` + 마켓플레이스 (**2026-03 신규**) | ✗ 우산 매니페스트 없음 (surface별 개별 / npm plugin) |

범례: ✅ 1급 지원 · ◐ 되지만 제약/실험적 · ✗ 등가물 없음.

### B. rocky 표면별 커버 현황

| rocky 표면 | Claude Code | Codex | opencode | 메모 |
| --- | --- | --- | --- | --- |
| MCP 도구 (openapi 7 + `seo_validate` + worklog 4 + notion 4 = 16) | ✅ 배포됨 | ✅ 배포됨 | ✅ 배포됨 | 공유 코어 — 이미 3-호스트 완결 |
| `/rocky:finish`, `/rocky:issue` | ✅ | ◐ 커버 가능 (skill) | ◐ 커버 가능 (command) | `gh` CLI 의존, 로직은 호스트 중립 |
| `/rocky:recall` | ✅ | ◐ 커버 가능 | ◐ 커버 가능 | 정리는 host-LLM 몫 → 호스트별 모델(Haiku↔Sonnet 상당) 매핑 필요 |
| `/rocky:codex`, `/rocky:opencode` | ✅ | — 무의미 | — 무의미 | 대상 호스트로 위임하는 커맨드라 그 호스트 안에 둘 이유 없음 |
| `/rocky:soul` + 소울·callsign 주입 (SessionStart) | ✅ | ◐ SessionStart hook 또는 AGENTS.md 정적 병합 | ◐ plugin / `instructions` | 정적 병합이면 쉬움, 동적 주입은 훅 필요 |
| `/rocky:statusline` + 번들 템플릿 3종 + 동기화 훅 | ✅ | ✗ 등가물 없음 | ✗ 등가물 없음 | Claude Code 의 `statusLine` 설정 자체가 CC 고유 표면 |
| 턴 자동 기록 (Stop hook → worklog) | ✅ | ◐ Stop hook / notify — **트랜스크립트 포맷 상이** | ◐ plugin `session.idle` — **SDK client 접근, 포맷 상이** | `src/hooks/transcript.ts` 를 호스트별 재작성해야 (실제 비용) |
| skill `writing-cc-plugin` | ✅ | ◐ 스펙 호환하나 내용이 CC 전용 | ✅ `.claude/skills/` 자동 발견 | 메커니즘은 커버, 내용 가치는 CC 한정 |
| skill `todoist` | ✅ | ◐ 커버 가능 | ✅ `.claude/skills/` 자동 발견 | 세션에 연결된 Todoist MCP 에만 의존 — 로직은 호스트 중립 |
| 단일 설치 유닛 | ✅ `.claude-plugin/` + `rocky-marketplace` | ◐ `.codex-plugin/plugin.json` 로 번들화 가능 (`codex plugin` 서브커맨드 실재) | ✗ 우산 없음 → config 트리 / npm plugin | Codex 가 새로 연 길 |
| 동반 플러그인 `rocky-todo` (별도 레포) | ✅ 같은 마켓 2번째 entry | ◐ 데몬 MCP 가 HTTP 라 등록만 하면 됨 | ◐ 동일 | 데몬·웹UI 는 호스트 무관, 플러그인 배선과 훅만 CC 전용 |

범례: ✅ rocky 가 이미 배포 · ◐ 호스트는 지원, rocky 미구현(커버 가능) · ✗ 등가물 없음 · — 무의미.

### 요약

- **이미 완결**: MCP 코어 — 세 호스트 동등.
- **정적으로 쉬운 커버**: 소울 / 규칙을 AGENTS.md 정적 병합으로. 스킬은 opencode 가 `.claude/skills/` 를 이미 자동 발견한다.
- **훅 필요(품이 듦)**: 턴 자동 기록 — 호스트별 트랜스크립트 파서 재작성이 실제 비용.
- **새로 열린 길**: Codex 를 `.codex-plugin/plugin.json` 번들 플러그인으로 (MCP + skills + hooks 한 번에). opencode 는 우산 매니페스트가 없어 config 트리 / npm plugin 로 나눠 배포.
- **등가물 없음**: statusline — Claude Code 고유 설정 표면이라 이식 대상이 아니다.
- **무의미**: `/rocky:codex`·`/rocky:opencode` 를 대상 호스트 안에 넣기.

> **신뢰도 캐비앗**: Codex 확장 스택(hooks · plugins · 마켓플레이스)은 2026 초 신규 + 일부 실험적이다 — hooks 기본 off · no Windows, custom prompts deprecated(→ skills), skills 경로 `.agents/skills` vs `.codex/skills` 유동. 설치본 `codex-cli 0.144.5` 기준으로 `codex plugin` 서브커맨드와 `~/.codex/{skills,plugins}` 존재는 실측 확인했으나, 세부 스펙은 이식 직전에 그때의 `codex --version` 으로 재확인할 것. opencode(실측 `1.18.4`)의 `.opencode/command|agent|plugin` 은 단수 디렉터리명이 정식이다(복수형도 허용).

## Claude Code 커맨드

MCP tool 과 별개로, Claude Code plugin 은 `commands/` 의 슬래시 커맨드를 노출한다. `/rocky:finish` 는 `gh` CLI 기반 — 게이트 통과 확인 후 커밋·푸시·PR 생성까지 마무리한다. `/rocky:recall` 은 `worklog_*` 를 읽어 앵커 히스토리 다이제스트로 정리하는 짝 커맨드다 (v0.9 에서 구 `/curate` 를 대체). 생성된 PR 의 감시·리뷰 반영은 Claude Code **빌트인 `/autofix-pr`** 에 위임한다 (클라우드 세션 + GitHub App webhook 기반 — rocky 커맨드가 아니며, 구 `/pr-watch` 는 v0.8 에서 제거됨). 그리고 `/rocky:codex` 는 task 하나를 Codex(`codex exec`)에 위임해 격리 worktree 에서 구현시키고 Claude 가 게이트·MCP 표면·diff 스코프를 감시하는 위임 커맨드다(자동 병합 없음). `/rocky:opencode` 는 같은 패턴으로 task 하나를 opencode(`opencode run`)에 위임하고 Claude 가 게이트·MCP 표면·diff 스코프를 감시한다(자동 병합 없음) — v0.17 부터 dispatch 를 companion 런타임(`src/opencode-companion.ts`)이 맡아 `--background` 위임이 가능해졌고, 그렇게 띄운 잡의 조회·회수·취소는 짝 커맨드 `/rocky:opencode-jobs` 가 담당한다. `/rocky:issue` 는 *다른* 레포에서 rocky 를 쓰다 떠오른 기능 제안·버그를 `minjun0219/rocky` GitHub Issue 로 캡처하는 `gh` 기반 커맨드다 — 현재 세션 맥락을 모으고 유사 이슈를 조회한 뒤 초안을 한 번 확인하고 생성한다(자동 생성 없음). `/rocky:soul` 은 소울(페르소나)을 고르는 커맨드다 — 목록 / 활성 소울 전환(`rocky.json` 의 `soul` 쓰기) / 미리보기 / 커스텀 소울 스캐폴딩.

### `/rocky:finish [힌트]`

- **What**: 현재 변경을 마무리한다 — 게이트(`bun run check` / `typecheck` / `test`) 통과 확인 → 변경 요약 → 브랜치 → 커밋 → 푸시 → PR 생성.
- **Input**: (옵션) 커밋/PR 요약에 참고할 힌트.
- **하지 않는 것**: 게이트 실패 시 커밋 금지(우회 X), `main` 직접 커밋 금지(먼저 브랜치), 무관한 파일 싸잡아 스테이지 금지.
- **규칙**: Conventional Commits 한국어 제목(제목에 나열·부연 금지 — 핵심 하나, 요약부 대략 50자 초과 금지, 세부는 본문으로), 커밋 `Co-Authored-By` / PR 본문 서명 trailer 부착, 리뷰 요청 시 한국어 코멘트 요청.
- **의존성**: 인증된 `gh` CLI.

### `/rocky:recall [주제 힌트]`

- **What**: 워크로그(`worklog_*`)에 쌓인 **기록** — `Stop` hook 이 자동으로 남긴 `kind:"turn"` 항목 + 수동 `decision` / `blocker` / `answer` / `note` — 을 읽어 **앵커 히스토리 다이제스트**로 정리한다. 별도 wiki 문서가 아니라, 워크로그로 **찾아 들어갈 수 있는 앵커** (각 항목이 원본 엔트리 `id` 를 가리킴) 를 `worklog_append` `kind:"digest"` 한 줄로 남긴다 (v0.9 에서 구 `/curate` 를 대체).
- **Input**: (옵션) 이번 정리에서 집중할 주제 힌트.
- **동작**: `worklog_status` 로 `lastDigestAt` watermark 확인 → `worklog_read {since}` 로 새 항목 수집(+ 힌트 있으면 `worklog_search` 로 보강) → 새 항목 수 `n` 이 `rocky.json` 의 `worklog.digestThreshold`(기본 40) 이하면 Haiku, 초과면 Sonnet 서브에이전트(`Task`)로 앵커 다이제스트 생성 → `worklog_append {kind:"digest"}` 로 watermark 겸 결과 기록.
- **하지 않는 것**: raw 나열 금지(의미 있는 결정/전환/blocker/사용자 답변만 앵커로), 기존 워크로그 라인 수정/삭제 금지(append-only), 새 항목 0 이면 no-op(watermark 안 남김), 서브에이전트 실패 시 다이제스트 append 안 함(watermark 오염 방지), Claude Code 네이티브 메모리(글로벌)는 건드리지 않음. rocky 는 기록·저장만 하고 증류는 이 커맨드(호스트 LLM)가 한다.
- **의존성**: `worklog_*` MCP 도구 + `Task` (서브에이전트 dispatch). `gh` 불필요, 별도 wiki 설정 불필요.

## Claude Code 훅 (hooks)

MCP tool · 슬래시 커맨드와 별개로, Claude Code plugin 은 `hooks/hooks.json` 에 세 hook 을 등록한다 — `SessionStart` (소울 자동 주입 / statusline 자동 동기화) 와 `Stop` (v0.9, 턴 자동 기록).

### `SessionStart` — 소울(페르소나) 자동 주입

- **What**: 세션 시작 시 `rocky.json` 의 활성 `soul` 을 읽어, 해당 이름의 소울 파일(커스텀 `~/.config/rocky/souls/<name>.md` 우선, 없으면 번들 프리셋 `souls/<name>.md`) 을 찾아 페르소나 본문을 `additionalContext` 로 주입한다. `soul` 이 비어있거나 파일을 못 찾으면 아무 것도 주입하지 않는다(vanilla, opt-in 기본값). 주입되는 컨텍스트 맨 앞에는 "AGENTS.md/CLAUDE.md 의 게이트·안전 규칙이 항상 이긴다" 는 우선순위 preamble 이 붙는다 — 소울은 그 위의 말투/작업 방식 레이어일 뿐, override 가 아니다.
- **동작**: 세션 cwd 로 `rocky.json` 을 로드(project > user) → `soul` 필드 확인 → 소울 파일 read → frontmatter 제거한 본문 + preamble 로 컨텍스트 조립. `callsign` 이 설정돼 있으면 본문 끝에 호칭 지시 한 줄("사용자를 `<callsign>` 이라고 부른다")을 덧붙인다 — 소울 본문의 기본 호칭 규칙보다 우선. 어떤 단계든 실패해도 세션 시작을 막지 않고 항상 exit 0 (fail-open).
- **주입 시점**: hook 은 `matcher: "startup|clear|compact"` 로 등록된다 — 컨텍스트가 새로 시작(`startup`)되거나 `/clear`·compact 로 초기화/축약된 뒤 소울을 (재)주입한다. `resume` 은 기존 컨텍스트(이미 주입된 소울 포함)가 그대로 살아있어 중복 주입을 건너뛴다.
- **Side effects**: 없음 (read-only, remote 호출 없음).
- **Related config**: `rocky.json` 의 `soul` / `callsign`. env 변수 없음.
- **Hosts**: Claude Code plugin 만. 구현은 `src/hooks/inject-soul.ts` (코어 로직은 `src/core/soul.ts`).

### `SessionStart` — statusline 자동 동기화

- **What**: 세션 시작(`matcher: "startup"`) 시 설치본(`~/.config/rocky/statusline.sh`) 헤더의 템플릿 마커(`# rocky-statusline-template: <name>`, 없으면 기본 `duo`)를 읽어, 번들의 같은 템플릿(`statusline/<name>.sh`)과 비교해 내용이 다르면 덮어쓴다 (실행 권한 보장). 설치 경로에 파일이 없으면 — 즉 `/rocky:statusline` 로 설치한 적이 없으면 — 아무것도 하지 않는다 (opt-in 유지).
- **왜 필요한가**: user `settings.json` 의 `statusLine.command` 는 안정 경로를 가리킨다 (플러그인 캐시 경로는 버전마다 바뀌므로 직접 가리키면 업데이트 때 깨짐). 이 훅이 플러그인 업데이트를 안정 경로로 전파해, 설치 커맨드 재실행 없이 스크립트 개선이 반영되게 한다.
- **Side effects**: 설치돼 있고 내용이 다를 때만 `~/.config/rocky/statusline.sh` 덮어쓰기. remote 호출 없음. 실패해도 항상 exit 0 (fail-open).
- **Related config**: 없음.
- **Hosts**: Claude Code plugin 만. 구현은 `src/hooks/sync-statusline.ts` (코어 로직은 `src/core/statusline.ts`).

### `Stop` — 턴 자동 기록

- **What**: 이번 turn 의 사용자 요청(req)과 에이전트가 한 일(did, 사용한 tool 이름 포함)을 추출해 하나의 `content` 로 합성하고 `worklog_append {kind:"turn", tags:["turn"]}` 로 append 한다.
- **동작**: `rocky.json` 의 `worklog.autoCapture` (기본 true) 를 env `ROCKY_WORKLOG_AUTO_CAPTURE` 가 있으면 그게 이긴다 (`0` / `false` / `off` / `no` 값만 비활성, 그 외는 활성). req/did 는 각각 `worklog.captureMaxChars`(기본 800) 로 truncate.
- **Side effects**: 워크로그 JSONL 에 한 줄 append. remote 호출 없음.
- **Related config**: `ROCKY_WORKLOG_AUTO_CAPTURE`, `rocky.json` 의 `worklog.autoCapture` / `worklog.captureMaxChars`.
- **Hosts**: Claude Code plugin 만.

### `SessionStart` / `SessionEnd` — opencode 위임 잡 세션 배선

- **What**: `SessionStart` 는 `CLAUDE_ENV_FILE` 에 `ROCKY_SESSION_ID` 를 export 로 append 해,
  이후 슬래시 커맨드의 Bash 호출이 만드는 위임 잡에 세션 id 가 박히게 한다 —
  `/rocky:opencode-jobs` 가 **다른 세션의 잡을 건드리지 않게** 하는 근거다.
  `SessionEnd` 는 이 세션이 띄운 진행 중 잡의 프로세스 그룹을 끊어 고아 워커를 막는다.
- **matcher 없음(의도적)**: 소울 주입 훅과 달리 source 를 가리지 않는다. `resume` 에서 주입이
  빠지면 재개된 세션의 잡이 전부 필터에서 새어 나가기 때문이다.
- **Side effects**: env 파일 append + 진행 중 잡 SIGTERM. **잡 기록은 지우지 않는다**(사후 추적용,
  오래된 것은 `opencode.maxJobs` prune 이 정리). 어떤 실패도 세션을 막지 않는다(fail-open).
- **Related config**: `ROCKY_OPENCODE_JOBS_DIR`, `rocky.json` 의 `opencode.dir` / `opencode.maxJobs`.
- **Hosts**: Claude Code plugin 만.

### `/rocky:codex <task>`

- **What**: task 하나를 **Codex(`codex exec`)에 구현자로 위임**하고, Claude 가 **감독자**로서
  결과를 검증하는 오케스트레이션 커맨드. Codex 는 새 git worktree(격리)에서 `-s workspace-write`
  (worktree 범위) 로 구현하고, Claude 는 게이트(`check`/`typecheck`/`test`) + MCP 도구 표면
  무결성(`src/index.test.ts`) + `plugin.json` mcpServers 무결 + diff 스코프를 감시한다.
- **감시 = "플러그인 작동 방해 안 하는지"**: 위 4가지가 모두 통과할 때만 "방해 없음" 으로 보고
  현재 브랜치에 병합한다. 하나라도 어기면 병합하지 않고 무엇을 깼는지 보고·에스컬레이션.
- **하지 않는 것**: 자동 병합·자동 push·PR 없음(승인 하 병합만, 이어서 `/rocky:finish`).
  `danger-full-access` 미사용. Claude 가 구현 코드를 직접 쓰지 않음(위임·게이트·판정만).
- **전제**: `codex` CLI 설치(`codex exec` 의 `-s workspace-write` 지원), 워킹 트리 clean.

### `/rocky:opencode <task>`

- **What**: task 하나를 **opencode(`opencode run`)에 구현자로 위임**하고, Claude 가 **감독자**로서
  결과를 검증하는 오케스트레이션 커맨드. opencode 는 새 git worktree(격리)에서 구현하고,
  Claude 는 게이트(`check`/`typecheck`/`test`) + MCP 도구 표면 무결성(`src/index.test.ts`) +
  `plugin.json` mcpServers 무결 + diff 스코프를 감시한다.
- **감시 = "플러그인 작동 방해 안 하는지"**: 위 4가지가 모두 통과할 때만 "방해 없음" 으로 보고
  현재 브랜치에 병합한다. 하나라도 어기면 병합하지 않고 무엇을 깼는지 보고·에스컬레이션.
- **하지 않는 것**: 자동 병합·자동 push·PR 없음(승인 하 병합만, 이어서 `/rocky:finish`).
  Claude 가 구현 코드를 직접 쓰지 않음(위임·게이트·판정만).
- **전제**: `opencode` CLI 설치(`opencode run` 지원), 워킹 트리 clean.
- **위임 실행**: v0.17 부터 dispatch 는 companion 런타임(`src/opencode-companion.ts`)이 맡는다.
  프롬프트는 `--prompt-file` 로 넘겨 셸 인용 문제를 피하고, 출력은 `--format json` NDJSON 을
  파싱해 최종 텍스트 + opencode 세션 id 를 뽑는다. `--background` 를 붙이면 detached 워커로
  띄우고 즉시 잡 id 를 돌려준다 (진행/회수는 `/rocky:opencode-jobs`).
- **모델 명시 권장**: `rocky.json` 의 `opencode.model` 도 `--model` 도 없으면 opencode 는
  "마지막에 쓴 모델" 로 조용히 폴백해 위임 결과가 재현되지 않는다.

### `/rocky:opencode-jobs [status|result|cancel] [job-id]`

- **What**: `/rocky:opencode --background` 로 띄운 위임 잡의 수명주기 조회·회수·취소.
  잡을 새로 만들지는 않는다.
- **동작**: `status` — 진행 중 잡(로그 최근 3줄 포함) + 최근 종료 잡. `result` — **종료된** 잡의
  최종 출력(진행 중이면 결과 대신 상태를 알린다). `cancel` — 진행 중 잡의 **프로세스 그룹**을
  SIGTERM 으로 끊고 `cancelled` 로 기록.
- **잡 참조**: 정확한 id → 유일한 prefix → (생략 시) 최신 1건. prefix 가 여러 잡에 걸리면 조용히
  하나를 고르지 않고 후보를 나열하며 에러를 낸다.
- **세션 격리**: 기본적으로 **현재 Claude 세션이 만든 잡만** 보인다 (`SessionStart` 훅이 주입한
  `ROCKY_SESSION_ID` 기준). `--all` 로 전체 조회 가능.
- **저장 위치**: `<ROCKY_OPENCODE_JOBS_DIR>` — 미지정 시 프로젝트별
  `~/.config/rocky/jobs/<project-key>` (worklog 와 같은 키 규칙). 인덱스 `state.json` +
  잡별 `jobs/<id>.json` payload + `jobs/<id>.log` 진행 로그. `opencode.maxJobs`(기본 50) 초과분은
  파일까지 함께 정리된다.
- **Related config**: `ROCKY_OPENCODE_JOBS_DIR`, `ROCKY_OPENCODE_CLI`, `ROCKY_OPENCODE_TIMEOUT_MS`,
  `rocky.json` 의 `opencode.*`.
- **Hosts**: Claude Code plugin 만 (MCP 도구 아님 — 도구 표면 불변).

### `/rocky:issue [아이디어/버그 한 줄]`

- **What**: *다른* 레포에서 rocky 를 쓰다 떠오른 **기능 제안**·**버그**를, 작업 흐름을 끊지 않고 `minjun0219/rocky` GitHub Issue 로 캡처한다. 현재 세션 맥락(출처 레포 / 트리거 상황 / 관련 코드·에러)을 자동으로 모아 이슈 본문에 담는다.
- **Input**: (옵션) 아이디어/버그 요지. 비어 있으면 최근 대화에서 유추하고, 모호하면 한 줄만 물어본다.
- **동작**: 타입→라벨 추론(레포에 **존재하는** 라벨만: `bug`/`enhancement`/`documentation`/`question` 등) → `gh issue list --search` 로 유사 열린 이슈 조회 → Conventional Commits 한국어 제목 + 본문(요지/출처/맥락/제안·재현) + 라벨 **초안 제시** → 사용자 확인(`y` / 수정 / 기존 `#N` 에 코멘트) → `gh issue create` 또는 `gh issue comment`.
- **하지 않는 것**: 확인 없이 자동 생성 금지(GitHub 은 외부 산출물), 새 라벨 생성 금지(존재하는 라벨만), 현재 레포 remote 신뢰 금지(항상 `--repo minjun0219/rocky` 명시), rocky 가 토큰 직접 취급 금지(전부 `gh` 위임).
- **의존성**: 인증된 `gh` CLI.
- **Hosts**: Claude Code plugin 만 (rocky 설치된 어느 세션에서든 호출 가능 — 다른 레포 포함).

### `/rocky:soul [list | <name> | call [<이름>|--clear] | show [name] | new <name>] [--project]`

- **What**: 로키의 소울(페르소나 — 말투/성격 + 작업 방식)을 고른다. 인자 없음 또는 `list` 는 프리셋(`${CLAUDE_PLUGIN_ROOT}/souls/`) + 커스텀(`~/.config/rocky/souls/`) 목록과 현재 활성 소울을 보여준다(같은 이름이면 커스텀이 이김). `<name>` 은 이름 검증(`^[a-zA-Z0-9_-]+$`) → 존재 확인 → **호칭 질문("소울이 뭐라고 불러 드릴까요?", 생략 가능)** → 사용자 확인 후 `rocky.json` 의 `soul` 키(+ 호칭을 받았으면 `callsign` 키)만 갱신(기본 user `~/.config/rocky/rocky.json`, `--project` 면 `./rocky.json`, 다른 필드는 보존). `call` 은 호칭만 다룬다 — 인자 없으면 현재 `callsign` 표시(user/project + 병합 결과), `<이름>` 이면 검증(한 줄, 공백만은 불가, 최대 40자) + 확인 후 `callsign` 키만 갱신, `--clear` 면 확인 후 키 제거. `show [name]` 은 본문 미리보기(생략 시 현재 활성 소울). `new <name>` 은 `~/.config/rocky/souls/<name>.md` 에 frontmatter(`name`/`description`) + 빈 섹션 템플릿을 스캐폴딩(이미 있으면 덮어쓰지 않음).
- **Input**: 서브커맨드 + 옵션 `--project`.
- **하지 않는 것**: 소울로 AGENTS.md/CLAUDE.md 게이트·안전 규칙 override 금지, `rocky.json` 쓸 때 `soul` / `callsign` 외 필드 변경 금지, 확인 없이 활성 소울 전환·호칭 변경 금지.
- **적용 시점**: `soul` 변경은 다음 세션부터 `SessionStart` 훅이 자동 주입 — 이번 세션에는 반영되지 않는다.
- **Hosts**: Claude Code plugin 만.

### `/rocky:statusline [install [<template>] | list | status | off]`

- **What**: rocky statusline 을 설치/점검/해제한다. 인자 없음 또는 `install` 은 템플릿 선택(인자로 지정 가능, 미지정 시 목록에서 고름 — `duo` 2줄 기본 / `mini` 1줄 / `full` 3줄+비용) → `jq` 확인 → 현재 `~/.claude/settings.json` 의 `statusLine` 표시(다른 statusline 이 있으면 교체 여부 확인) → **초안 확인 후** 고른 템플릿을 `~/.config/rocky/statusline.sh` 로 복사(`chmod +x`)하고 settings 를 타임스탬프 백업 뒤 `statusLine` 키만 `sh ~/.config/rocky/statusline.sh` 로 갱신 (템플릿 교체만이면 settings 는 그대로). `list` 는 번들 템플릿 목록 + 현재 설치본의 템플릿을 보여준다. `status` 는 settings 값 / 설치본 존재·템플릿 / 번들과의 동기화 여부를 보여준다. `off` 는 확인 후 백업을 남기고 `statusLine` 키만 제거한다 (설치본 파일은 남긴다).
- **Input**: 서브커맨드 (기본 `install`) + 템플릿 이름 (옵션, `^[a-zA-Z0-9_-]+$`).
- **하지 않는 것**: 확인 없이 settings 쓰기 금지, `statusLine` 외 필드 변경 금지, 플러그인 캐시 경로를 settings 에 직접 쓰기 금지 (버전마다 바뀜 — 반드시 안정 경로 간접화).
- **적용 시점**: 새 세션부터. 이후 스크립트 커스터마이징은 번들 `statusline/<template>.sh` 를 고치면 `SessionStart` 훅이 다음 세션에 전파한다 (설치본 직접 수정은 sync 때 덮임).
- **Hosts**: Claude Code plugin 만.

## Claude Code 스킬

MCP tool · 슬래시 커맨드와 별개로, Claude Code plugin 은 `skills/` 에 번들 스킬도 노출한다 (기본 `skills/` 자동 스캔, 플러그인 전용). 스킬은 `/rocky:<이름>` 으로 호출되거나 Claude 가 맥락에 따라 자동 사용한다.

### `writing-cc-plugin`

- **What**: Claude Code **플러그인 작성 가이드 + 레퍼런스**. plugin.json 매니페스트 / 컴포넌트(skills · agents · hooks · MCP · LSP · monitors · themes) / 디렉토리 구조 / 로컬 테스트(`--plugin-dir` · `--plugin-url` · `/reload-plugins`) / `.claude/` config → plugin 변환 / 마켓플레이스 배포 / 설치 scope / 버전 관리 / `claude plugin` CLI 를 다룬다.
- **구성**: `SKILL.md`(작성 워크플로우 + 자주 틀리는 gotcha 표 + quick reference) + `reference.md`(전체 스펙 §1–9 + 작성·배포 워크플로우 §10).
- **출처**: 공식 `/ko/plugins-reference` + `/ko/plugins` 문서 증류. 버전 게이트 기능·정확한 필드는 라이브 문서 재확인 권장.
- **Hosts**: Claude Code plugin 만 (standalone `openapi-mcp` CLI 에는 없음).

### `todoist`

- **What**: 사용자의 Todoist 를 현재 레포의 **작업 장부**로 쓰는 연동 스킬. ① **다음 작업 파악** — Todoist(우선순위·기한) + git(미병합 브랜치·미커밋 변경) + worklog(최근 턴·다이제스트, `worklog_*` 도구 없으면 조용히 생략)를 교차해 후보 2-3개 + 추천 1개 제시 ② **등록/정리** — 명시 요청은 즉시 실행, 스킬 제안·기존 태스크 수정은 초안 확인 후. 등록 전 중복 검색 + 컨벤션 고정(제목 간결·실행형 / description 은 다른 세션이 읽어도 착수 가능하게 자기완결 / priority 의미 고정: p1 블로커 · p2 다음 착수 · p3 준비된 백로그 · p4 아이디어·보류 / 기한은 실제 기한만) ③ **완료 처리** — 출하된 작업의 태스크를 찾아 확인 후 complete, 파생 후속 작업은 신규 초안으로 제안.
- **전제**: 세션에 연결된 Todoist MCP (claude.ai 커넥터 등) — rocky 는 Todoist 접근(도구·토큰·API)을 일절 배포하지 않으며, 도구가 없으면 멈추고 연결 방법을 안내한다. 레포↔프로젝트 매핑은 휴리스틱(프로젝트 이름/설명의 레포 경로, `레포: <경로>` 설명 관례가 최우선 신호) + 애매하면 사용자 확인.
- **가드레일**: 완료·수정·일정 이동은 항상 확인 후, 삭제는 명시 요청만, 일정 이동은 reschedule 계열 도구(recurrence 보존), 대량 일괄 변경 금지, 매핑된 프로젝트 밖은 불가침.
- **Hosts**: Claude Code plugin 만 (standalone `openapi-mcp` CLI 에는 없음).

> `todo` 스킬(공유 보드 사용 가이드)은 rocky-todo 분리와 함께 그 레포로 이동했다 —
> `rocky-todo:board` 스킬로 제공된다.
>
> `delegating-to-codex` 스킬(Codex 위임 메커니즘 + 가드레일)은 v0.16 에서 제거했다. 공식
> [`openai/codex-plugin-cc`](https://github.com/openai/codex-plugin-cc) 플러그인이 같은 영역을
> 공유 app-server 런타임 기반으로 더 넓게 덮기 때문이다 — 스킬 3종(`codex-cli-runtime`,
> `codex-result-handling`, `gpt-5-4-prompting`)과 커맨드(`/codex:rescue`, `/codex:review`).
> rocky 의 `/rocky:codex` 는 그쪽에 없는 **격리 worktree + 플러그인 표면 무결 검증**만 남기고
> 위임 메커니즘을 커맨드 본문에 흡수해 자기완결로 만들었다.

## 환경 변수

`ROCKY_*` 변수는 **전체 표면 서버 진입점 (`src/index.ts`) 전용** — 이를 실행하는 모든 호스트(Claude Code plugin / Codex / opencode)에 적용되고, standalone `openapi-mcp` CLI 는 인지하지 않는다 (CLI 는 `openapi-mcp.json` config 파일 + XDG 표준 변수만 본다). **예외**: 아래 표에서 적용 host 가 "Claude Code plugin" 인 변수들은 서버가 아니라 plugin 의 hook / companion 스크립트에서만 읽히므로 Claude Code 전용이다 (Codex/opencode 에는 이 훅·커맨드가 없어 설정해도 무효) — `ROCKY_WORKLOG_AUTO_CAPTURE`(`Stop` hook)와 `ROCKY_SESSION_ID` / `ROCKY_OPENCODE_*`(`/rocky:opencode` 위임 런타임)가 여기 해당한다.

| 변수 | 기본값 | 적용 host | 영향 |
| --- | --- | --- | --- |
| `ROCKY_OPENAPI_CACHE_DIR` | `~/.config/rocky/openapi-specs` | 전체 표면 서버 | OpenAPI spec 디스크 캐시 위치. |
| `ROCKY_OPENAPI_CACHE_TTL` | `300` (초) | 전체 표면 서버 | spec 단위 `cacheTtlSeconds` 기본값으로 주입 — `rocky.json` 의 leaf 에 별도 TTL 이 없을 때 사용. |
| `ROCKY_CONFIG` | `~/.config/rocky/rocky.json` | 전체 표면 서버 | user-level `rocky.json` 경로 override. |
| `ROCKY_OPENAPI_DOWNLOAD_TIMEOUT_MS` | `10000` (ms) | 전체 표면 서버 | spec 다운로드 HTTP timeout. |
| `ROCKY_OPENAPI_INSECURE_TLS` | (unset) | 전체 표면 서버 | `1` / `true` 면 TLS 검증 비활성화 — 사내 self-signed 인증서 / 개발용. production 사용 금지. |
| `ROCKY_OPENAPI_EXTRA_CA_CERTS` | (unset) | 전체 표면 서버 | 추가 CA pem 경로 (`:` 구분, Unix `PATH` 형식). insecureTls 보다 안전한 사내 CA 옵션. |
| `ROCKY_NOTION_CLI` | `ntn` | 전체 표면 서버 | Notion CLI 바이너리 이름 / 경로. 이 바이너리가 탐지될 때만 `notion_*` 도구가 등록된다. |
| `ROCKY_NOTION_CLI_TIMEOUT_MS` | `15000` (ms) | 전체 표면 서버 | `ntn` subprocess 호출 timeout. |
| `ROCKY_NOTION_CACHE_DIR` | `~/.config/rocky/notion-pages` | 전체 표면 서버 | Notion 페이지 디스크 캐시 위치 (`<pageId>.json` + `<pageId>.md`). |
| `ROCKY_NOTION_CACHE_TTL` | `86400` (초, 24h) | 전체 표면 서버 | Notion 캐시 entry TTL 기본값. |
| `ROCKY_WORKLOG_DIR` | `~/.config/rocky/worklog/<project-key>` | 전체 표면 서버 | 워크로그 JSONL 저장 디렉터리. 지정 시 프로젝트별 기본 경로 대신 이 값을 verbatim 사용. `rocky.json` 의 `worklog.dir` 보다 우선. |
| `ROCKY_WORKLOG_AUTO_CAPTURE` | `1` (on) | Claude Code plugin (`Stop` hook) | `Stop` hook 의 턴 자동 기록 on/off. `0` / `false` / `off` / `no` 값만 비활성, 그 외는 활성. `rocky.json` 의 `worklog.autoCapture` 보다 우선. |
| `ROCKY_SESSION_ID` | (unset) | Claude Code plugin (`SessionStart` hook 주입) | 현재 Claude 세션 id. 위임 잡에 박혀 `/rocky:opencode-jobs` 의 세션 격리 기준이 된다. 직접 설정할 일은 없다. |
| `ROCKY_OPENCODE_JOBS_DIR` | `~/.config/rocky/jobs/<project-key>` | Claude Code plugin (companion) | 위임 잡 상태 저장 디렉터리. 지정 시 verbatim 사용하며 `rocky.json` 의 `opencode.dir` 보다 우선. |
| `ROCKY_OPENCODE_CLI` | `opencode` | Claude Code plugin (companion) | opencode CLI 바이너리 이름 / 경로. |
| `ROCKY_OPENCODE_TIMEOUT_MS` | `1800000` (ms, 30분) | Claude Code plugin (companion) | 위임 1회의 hard timeout. 초과 시 SIGKILL 후 잡을 `failed` 로 기록. |
| `XDG_CONFIG_HOME` | `~/.config` | standalone CLI | `openapi-mcp.json` 기본 검색 경로의 prefix. |
| `XDG_CACHE_HOME` | `~/.cache` | standalone CLI | 디스크 캐시 디렉토리의 prefix (`openapi-mcp.json` 의 `cache.diskCachePath` 가 우선). |

standalone CLI 는 위 XDG 변수에 추가로 `openapi-mcp` CLI flag (`--config`, `--log-level`, `--insecure-tls`) 를 사용한다.

## 설정 파일

### `rocky.json` (plugin)

```json
{
  "$schema": "https://raw.githubusercontent.com/minjun0219/rocky/main/rocky.schema.json",
  "soul": "rocky",
  "callsign": "민준",
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
  },
  "opencode": {
    "model": "anthropic/claude-sonnet-5",
    "agent": "build",
    "maxJobs": 50
  },
  "todo": {
    "port": 8636,
    "dir": "~/.config/rocky/todo"
  }
}
```

- `soul` (옵션): 활성 소울(페르소나) 이름 — `^[a-zA-Z0-9_-]+$`, 파일명 stem 과 동일한 값. `SessionStart` 훅이 이 값으로 `souls/<name>.md` (번들 프리셋, `${CLAUDE_PLUGIN_ROOT}/souls/`) 또는 `~/.config/rocky/souls/<name>.md` (커스텀, 같은 이름이면 이쪽이 이김) 를 찾아 세션 컨텍스트에 자동 주입한다. project (`./rocky.json`) 가 user 를 덮어쓴다. 미설정 시 주입 없음(vanilla, opt-in). `/rocky:soul <name>` 으로 전환.
- `callsign` (옵션): 소울이 사용자를 부르는 호칭 — 한 줄, 공백만은 불가, 최대 40자 (한글/공백 OK, 파일명이 아니라 `soul` 의 `[a-zA-Z0-9_-]` 제약 없음). `SessionStart` 훅이 활성 소울 컨텍스트 끝에 호칭 지시 한 줄로 함께 주입하며, 소울 본문의 기본 호칭 규칙(예: rocky 의 "친구")보다 우선한다. `soul` 미설정 시에는 주입 대상이 없어 무시된다. project 가 user 를 덮어쓴다. `/rocky:soul <name>` 세팅 때 물어보거나 `/rocky:soul call <이름>` 으로 변경.
- 핸들 규칙: `host:env:spec`. 각 식별자는 `^[a-zA-Z0-9_-]+$` — 콜론은 separator 예약.
- `seo` (옵션): `seo_validate` 도구 기본값. `allowPrivateHosts` (boolean, 기본 false) / `timeoutMs` (1..30000). 두 값 모두 도구 호출 인자가 우선. plugin 전용이며 단독 CLI 는 이 키를 읽지 않는다.
- `worklog` (옵션, v0.9 에서 `journal` 개명): `worklog_*` 기록 저장 위치(`dir`, env `ROCKY_WORKLOG_DIR` 가 우선), `Stop` hook 자동 기록 on/off(`autoCapture`, 기본 true, env `ROCKY_WORKLOG_AUTO_CAPTURE` 가 우선) + turn 항목 truncate 길이(`captureMaxChars`, 기본 800), `/rocky:recall` 의 Haiku↔Sonnet 임계(`digestThreshold`, 기본 40). 더 이상 `wikiDir` 는 없다 — 정리 결과는 워크로그 자체의 `kind:"digest"` 항목으로 남는다. plugin 전용이며 단독 CLI 는 이 키를 읽지 않는다.
- `opencode` (옵션, v0.17): `/rocky:opencode` 위임 런타임 설정. 잡 저장 위치(`dir`, env `ROCKY_OPENCODE_JOBS_DIR` 가 우선), 보관 잡 수(`maxJobs`, 기본 50), 기본 위임 모델(`model`, `provider/model`), 기본 agent(`agent`). **`model` 명시를 권장** — 없으면 opencode 가 "마지막에 쓴 모델" 로 조용히 폴백해 위임이 재현되지 않는다. MCP 도구가 아니라 슬래시 커맨드 + companion 스크립트가 소비하므로 이 블록이 비어도 도구 표면은 달라지지 않는다. plugin 전용이며 단독 CLI 는 이 키를 읽지 않는다.
- `todo` (옵션): **rocky-todo 동반 플러그인**(별도 레포 `minjun0219/rocky-todo`)의 설정 블록. rocky 본체는 이 키를 **관용만** 하고(파싱/검증/소비하지 않음 — 공유 rocky.json 이라 거부하지 않을 뿐) 실제 소비는 rocky-todo 데몬 몫이다. 키 모양(`port` / `dir` / `expose` / `watch`)은 그 레포 문서 참고.
- leaf 는 string (URL only) 또는 object (`{ url, baseUrl?, format? }`). `baseUrl` 은 `openapi_endpoint` 의 `fullUrl` 합성에 사용. `format` 은 `openapi3` / `swagger2` / `auto` (기본 auto).
- project (`./rocky.json`) 가 user (`~/.config/rocky/rocky.json`) 를 leaf 단위로 덮어쓴다.

미지원 top-level 키는 즉시 reject 된다 (`$schema` / `soul` / `callsign` / `openapi` / `seo` / `worklog` / `opencode` / `todo` 만 허용 — `rocky.schema.json` 최상위 `additionalProperties:false` 와 런타임 `validateConfig` 둘 다 강제) — 오타 가드. 새 도메인이 재추가될 때는 이 허용 목록과 스키마를 함께 갱신해야 한다.

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

1. 이 저장소를 GitHub 소스 마켓플레이스로 설치: `claude plugin marketplace add minjun0219/rocky` → `claude plugin install rocky@rocky-marketplace` (저장소 자체가 소스, 파사드 없음; 원격 세션에서는 `/plugin` 슬래시 커맨드로 동일). claude.ai 웹 UI 에서는 CLI 명령 대신 플러그인/마켓플레이스 설정에 저장소 URL (`https://github.com/minjun0219/rocky.git`) 을 등록 — 플러그인 source 가 명시적 git URL 이라 웹 서버 사이드 동기화에서도 해석된다. 설치본은 GitHub `main` clone 이라 코드 변경 반영은 push 후 `claude plugin update rocky`.
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

## 릴리스 (changesets)

user-facing 변경이 있는 PR 은 `bunx changeset` 으로 버전 의도(patch/minor/major)를 선언한다. main 에 병합되면 GitHub Action(`.github/workflows/release.yml`)이 "Version Packages" PR 을 자동으로 열어 `package.json` + `.claude-plugin/plugin.json` 버전 범프와 `CHANGELOG.md` 갱신을 모아준다. 그 PR 을 병합해 버전이 오르면 `v<version>` 태그와 GitHub Release(노트는 `CHANGELOG.md` 해당 섹션)까지 자동으로 생성된다. (npm publish 는 자동화하지 않는다 — GitHub Release 는 태그+릴리스 노트일 뿐 npm 과 무관.)
