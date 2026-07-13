# Rocky — Features

> 이 toolkit 이 노출하는 표면을 한 페이지로 정리한 사람용 카탈로그.
> 대상: GitHub 에서 훑어 보는 사람, 그리고 grep / anchor 로 인용하는 에이전트 (Claude Code / codex / …).
> 이 파일이 **사람용 단일 source of truth** 다 (한국어). 에이전트용 단일 source 는 [`AGENTS.md`](./AGENTS.md) (영문). 표면이 바뀌면 두 파일을 같이 갱신한다.

## 한 눈에

- **단일 패키지 (`@minjun0219/rocky`) — 두 배포 타깃, 공유 7 openapi tool + plugin 전용 `seo_validate` + CLI-gated `notion_*` + 기록 `journal_*` (+ 정리 `/curate`)**:
  | 배포 타깃 | 역할 | 설치 |
  | --- | --- | --- |
  | **Claude Code plugin** (`src/index.ts`) | `.claude-plugin/plugin.json` 의 `mcpServers` (`${CLAUDE_PLUGIN_ROOT}/src/index.ts`) 로 stdio MCP 서버를 등록. marketplace 배포. | Claude Code plugin marketplace |
  | **`openapi-mcp` 단독 CLI** (`bin/openapi-mcp` → `src/standalone.ts`) | host-agnostic subset MCP. 어떤 stdio MCP host (Cursor / Continue / Claude Desktop / …) 든 등록해 쓰는 단독 CLI. | `bun link` (npm publish 는 별도 PR) |
- **공유 core**: [`src/core/`](./src/core) — 두 타깃 모두 이 디렉토리의 `handlers.ts` / `registry.ts` / `adapter.ts` 등을 import. plugin 진입점은 barrel (`./core`) 로, standalone 은 `./core/<file>` subpath 로 가져온다.
- **Surface**: 공유 7 openapi tool (두 타깃 동일) — `openapi_get` / `openapi_refresh` / `openapi_status` / `openapi_search` / `openapi_envs` / `openapi_endpoint` / `openapi_tags` — 에 더해 Claude Code plugin 전용 `seo_validate` (OG / Twitter Card / JSON-LD / favicon 메타 검증, `ogpeek` 기반). 단독 `openapi-mcp` CLI 는 OpenAPI 도메인만 다뤄 `seo_validate` 를 노출하지 않는다. v0.5 부터 Claude Code plugin 은 **공식 Notion CLI (`ntn`) 가 탐지될 때만** `notion_*` 4 도구 (`notion_get` / `notion_refresh` / `notion_status` / `notion_extract`) 를 추가 등록한다 — `ntn` 이 없으면 아예 나타나지 않는다. v0.6 부터 Claude Code plugin 은 **기록(記錄)** 레이어인 `journal_*` 4 도구 (`journal_append` / `journal_read` / `journal_search` / `journal_status`) 를 항상 등록한다 (외부 의존 없음) — append-only 로컬 JSONL. 짝이 되는 **정리(整理)** 는 `/curate` 슬래시 커맨드가 저널을 읽어 설정된 wiki 위치(Obsidian 등)로 증류한다 (rocky 는 기록·저장만).
- **설정 파일**:
  - `rocky.json` — plugin 이 읽는다 (project 의 `./rocky.json` 이 user 의 `~/.config/rocky/rocky.json` 을 leaf 단위로 덮어쓴다). v0.3 부터 `openapi.registry` 한 키만 존재.
  - `openapi-mcp.json` — 단독 CLI 가 읽는다. config 형태 (`specs.<name>.environments.<env>.baseUrl`) 가 다르고 평탄화 없이 그대로 SpecRegistry 에 들어간다.
- **런타임**: Bun ≥ 1.0. 빌드 단계 없음 (Bun 이 TS 직접 실행).

> - v0.2 까지 존재하던 journal / mysql / notion / spec-pact / pr-watch 5 도메인 + rocky / grace / mindy 3 에이전트 + 5 스킬은 [`archive/pre-openapi-only-slim`](https://github.com/minjun0219/rocky/tree/archive/pre-openapi-only-slim) 브랜치에 박제되어 있다. 이 중 **notion 은 v0.5 에서 `ntn` CLI 위임으로 재추가됨** (아래 `notion_*` 참고).
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

### `journal_*` (기록 레이어)

`journal_*` 4 도구는 **기록(記錄)** 레이어 — append-only 로컬 JSONL 에 결정 / blocker / 답변 / 메모를 turn 을 넘겨 남긴다. 외부 의존이 없어(순수 파일시스템) `notion` 처럼 CLI-gate 하지 않고 **항상 등록**된다 (Claude Code plugin 전용, 단독 CLI 미노출). 저장은 `<ROCKY_JOURNAL_DIR>/journal.jsonl` 한 파일 — 미지정 시 프로젝트별 (`~/.config/rocky/journal/<project-key>`, `project-key = basename(cwd)-sha1(cwd)[:8]`). handler 는 `src/core/journal-handlers.ts`, 구현은 `src/core/journal.ts`. 짝이 되는 **정리(整理)** 레이어는 rocky 가 아니라 `/curate` 슬래시 커맨드(호스트 LLM)가 담당한다 — 그래서 Claude Code 네이티브 메모리와 역할이 겹치지 않는다.

#### `journal_append`

- **What**: 저널에 한 줄을 append-only 로 기록. `content` 는 trim 후 비면 거부, `pageId` 는 `resolveCacheKey` 로 정규화 후 저장. crash 로 마지막 줄이 `\n` 없이 끝나 있으면 leading `\n` 을 붙여 라인 경계를 강제한다.
- **Input**: `content` (필수 본문). `kind?` (decision / blocker / answer / note / curate 등, 기본 `note`). `tags?` (문자열 배열). `pageId?` (연결할 Notion page id 또는 URL).
- **Output**: 생성된 `JournalEntry` (`id` / `timestamp` / `kind` / `content` / `tags` / `pageId?`).
- **Side effects**: JSONL 파일에 한 줄 append (필요 시 디렉터리 생성). remote 호출 없음.
- **Related config**: `ROCKY_JOURNAL_DIR`, `rocky.json` 의 `journal.dir`.
- **Hosts**: Claude Code plugin 만.

#### `journal_read`

- **What**: 가장 최근 항목부터 필터 / limit 적용해 반환. 손상된 라인은 자동 skip. 필터는 AND 결합.
- **Input**: `limit?` (기본 20). `kind?` (정확 일치). `tag?` (태그 포함). `pageId?` (정규화 후 일치). `since?` (해당 시각 이후, ISO8601).
- **Output**: `JournalEntry[]` (최근순).
- **Side effects**: 없음 (read-only).
- **Hosts**: Claude Code plugin 만.

#### `journal_search`

- **What**: substring (case-insensitive) 검색. `content` / `kind` / `tags` / `pageId` 를 매칭. 빈 query 는 전체 (kind 필터만 적용).
- **Input**: `query` (검색어). `limit?` (기본 20). `kind?` (풀 스코프 필터).
- **Output**: `JournalEntry[]` (최근순).
- **Side effects**: 없음.
- **Hosts**: Claude Code plugin 만.

#### `journal_status`

- **What**: 저널 메타(`path` / `exists` / `totalEntries` — 손상 라인 제외 / `sizeBytes` / `lastEntryAt`) + 정리 대상 `wikiDir` + 프로젝트 키 `projectKey` (`<basename>-<hash8>`) + 마지막 curate watermark(`lastCurateAt`)를 조회. `/curate` 가 정리 시작 시 이걸로 정리 루트(`<wikiDir>/<projectKey>/`)와 증분 기준점을 확인한다.
- **Input**: 없음.
- **Output**: `JournalStatus`.
- **Side effects**: 없음.
- **Related config**: `ROCKY_JOURNAL_DIR`, `ROCKY_JOURNAL_WIKI_DIR`, `rocky.json` 의 `journal.dir` / `journal.wikiDir`.
- **Hosts**: Claude Code plugin 만.

## Claude Code 커맨드

MCP tool 과 별개로, Claude Code plugin 은 `commands/` 의 슬래시 커맨드를 노출한다. `/finish` → `/pr-watch` 는 `gh` CLI 기반 한 쌍 — 마무리로 PR 을 만들고, 그 PR 을 머지까지 감시한다. `/curate` 는 `journal_*` 를 읽어 wiki 로 정리하는 짝 커맨드다.

### `/finish [힌트]`

- **What**: 현재 변경을 마무리한다 — 게이트(`bun run check` / `typecheck` / `test`) 통과 확인 → 변경 요약 → 브랜치 → 커밋 → 푸시 → PR 생성.
- **Input**: (옵션) 커밋/PR 요약에 참고할 힌트.
- **하지 않는 것**: 게이트 실패 시 커밋 금지(우회 X), `main` 직접 커밋 금지(먼저 브랜치), 무관한 파일 싸잡아 스테이지 금지.
- **규칙**: Conventional Commits 한국어 제목, 커밋 `Co-Authored-By` / PR 본문 서명 trailer 부착, 리뷰 요청 시 한국어 코멘트 요청.
- **의존성**: 인증된 `gh` CLI.

### `/pr-watch [PR]`

- **What**: 열린 GitHub PR 하나의 상태(CI 체크 · 리뷰 · 머지 가능성)를 점검하고, 열린 리뷰 코멘트를 코드와 대조해 타당/반박/보류로 정리한 뒤, **머지 가능한 상태가 되면 알려준다.**
- **Input**: PR 번호 / URL / `owner/repo#123`. 생략하면 현재 브랜치에 연결된 PR 을 자동으로 찾는다.
- **동작**: CI 진행 중이면 `gh pr checks --watch` 로 한 턴 안에서 완료까지 대기 후 재판정. 사람 리뷰 대기로 막히면 재실행 / `/loop 5m /pr-watch <PR>` 폴링을 안내.
- **하지 않는 것**: **자동 머지 금지** (`gh pr merge` 실행 X — 머지 가능 알림까지만), 코드 수정 · 리뷰 답글 자동 게시 금지 (권고까지만).
- **의존성**: 인증된 `gh` CLI. GitHub MCP 는 쓰지 않는다.

### `/curate [주제 힌트]`

- **What**: 저널(`journal_*`)에 쌓인 **기록**을 읽어 설정된 wiki 위치(Obsidian vault 등)의 **프로젝트별 하위 폴더**(`<wikiDir>/<projectKey>/`)로 **정리**한다 — 마지막 curate 이후 항목만 증분 증류해 markdown 페이지로 컴파일(YAML frontmatter + `[[wikilinks]]`)하고, `journal_append` `kind:"curate"` 로 watermark 를 남긴다.
- **Input**: (옵션) 이번 정리에서 집중할 주제 힌트.
- **동작**: `journal_status` 로 `wikiDir` + `projectKey` + `lastCurateAt` 확인 → 정리 루트 `<wikiDir>/<projectKey>/` 결정 → `journal_read {since}` 로 새 항목 수집 → 주제별 markdown 페이지 생성/병합 → watermark append.
- **개인 노트 안전장치 (ownership marker)**: 생성 페이지엔 frontmatter `source: rocky-curate` 를 붙이고, 병합/덮어쓰기는 **이 마커가 있는 파일에만** 한다. 마커 없는(사람이 쓴) 동명 파일은 절대 덮어쓰지 않고 다른 이름으로 쓴다 → vault 를 개인 노트와 공유해도 안전.
- **하지 않는 것**: 정리 루트 **밖으로 쓰지 않음**(다른 프로젝트 폴더 / 개인 노트 영역 X), 마커 없는 파일 덮어쓰기 금지, 저널 삭제/편집 금지(append-only), 새 항목 없으면 no-op, Claude Code 네이티브 메모리(글로벌)는 건드리지 않음. rocky 는 기록·저장만 하고 증류는 이 커맨드(호스트 LLM)가 한다.
- **의존성**: `journal_*` MCP 도구 + 설정된 `journal.wikiDir` (또는 `ROCKY_JOURNAL_WIKI_DIR`). `gh` 불필요.

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
| `ROCKY_JOURNAL_DIR` | `~/.config/rocky/journal/<project-key>` | plugin | 저널 JSONL 저장 디렉터리. 지정 시 프로젝트별 기본 경로 대신 이 값을 verbatim 사용. `rocky.json` 의 `journal.dir` 보다 우선. |
| `ROCKY_JOURNAL_WIKI_DIR` | (unset) | plugin | `/curate` 의 정리 대상 wiki 위치 (Obsidian vault 등). `journal_status` 로 노출. rocky 는 여기에 직접 쓰지 않는다. `rocky.json` 의 `journal.wikiDir` 보다 우선. |
| `XDG_CONFIG_HOME` | `~/.config` | standalone CLI | `openapi-mcp.json` 기본 검색 경로의 prefix. |
| `XDG_CACHE_HOME` | `~/.cache` | standalone CLI | 디스크 캐시 디렉토리의 prefix (`openapi-mcp.json` 의 `cache.diskCachePath` 가 우선). |

standalone CLI 는 위 XDG 변수에 추가로 `openapi-mcp` CLI flag (`--config`, `--log-level`, `--insecure-tls`) 를 사용한다.

## 설정 파일

### `rocky.json` (plugin)

```json
{
  "$schema": "https://raw.githubusercontent.com/minjun0219/rocky/main/rocky.schema.json",
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
  "journal": {
    "dir": "~/notes/rocky-journal",
    "wikiDir": "~/Obsidian/MyVault/rocky"
  }
}
```

- 핸들 규칙: `host:env:spec`. 각 식별자는 `^[a-zA-Z0-9_-]+$` — 콜론은 separator 예약.
- `seo` (옵션): `seo_validate` 도구 기본값. `allowPrivateHosts` (boolean, 기본 false) / `timeoutMs` (1..30000). 두 값 모두 도구 호출 인자가 우선. plugin 전용이며 단독 CLI 는 이 키를 읽지 않는다.
- `journal` (옵션): `journal_*` 기록 저장 위치(`dir`)와 `/curate` 정리 대상 wiki 위치(`wikiDir`). 둘 다 env (`ROCKY_JOURNAL_DIR` / `ROCKY_JOURNAL_WIKI_DIR`) 가 우선. plugin 전용이며 단독 CLI 는 이 키를 읽지 않는다.
- leaf 는 string (URL only) 또는 object (`{ url, baseUrl?, format? }`). `baseUrl` 은 `openapi_endpoint` 의 `fullUrl` 합성에 사용. `format` 은 `openapi3` / `swagger2` / `auto` (기본 auto).
- project (`./rocky.json`) 가 user (`~/.config/rocky/rocky.json`) 를 leaf 단위로 덮어쓴다.

미지원 top-level 키는 forward-compatibility 로 통과 — 도메인 재추가 시 같은 파일에 `mysql` / `spec` / `github` 같은 키가 다시 들어와도 깨지지 않는다.

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
