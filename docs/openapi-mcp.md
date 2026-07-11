# openapi-mcp 단독 진입점

agent-toolkit 의 다른 도메인 없이 OpenAPI / Swagger spec MCP 만 띄우고 싶을 때 쓰는 단독 CLI 진입점. 구 [`openapi-mcp-server`](https://github.com/minjun0219/openapi-mcp-server) 와 동일한 config 형태와 7 tool 표면을 그대로 받는다.

> v0.4 부터 이 CLI 는 단일 패키지 안의 `bin/openapi-mcp` (→ `src/standalone.ts`) 에 자리한다. 공유 core 는 `src/core/`. 저장소 구조는 루트 [`AGENTS.md`](../AGENTS.md).

## 설치

npm publish 는 별도 PR 이라 현재는 로컬 체크아웃 + `bun link`:

```bash
git clone https://github.com/minjun0219/agent-toolkit.git
cd agent-toolkit
bun install
bun link                       # 한 번만 — repo root 에서 `openapi-mcp` 명령어가 PATH 에 등록된다.
openapi-mcp --config ~/.config/openapi-mcp/openapi-mcp.json
```

## 설정 파일

기본 경로 `~/.config/openapi-mcp/openapi-mcp.json` (XDG `$XDG_CONFIG_HOME` 존중). `--config <path>` 로 위치를 바꿀 수 있고, 확장자에 따라 JSON / YAML / YML 어느 것이든 받는다.

최소 형태:

```json
{
  "specs": {
    "payment": {
      "source": {
        "type": "url",
        "url": "https://swagger.dev.internal/payment/v3/api-docs"
      },
      "environments": {
        "dev": { "baseUrl": "https://api.dev.internal/payment" },
        "stage": { "baseUrl": "https://api.stage.internal/payment" }
      }
    }
  }
}
```

핵심 필드:

| 필드 | 설명 |
| ---- | ---- |
| `specs.<name>.source` | `{ type: 'url', url }` 또는 `{ type: 'file', path }`. **상대경로는 config 파일 디렉토리 기준** (CWD 아님). `format` 으로 `openapi3` / `swagger2` / `auto` 강제 가능 (기본 `auto`). |
| `specs.<name>.environments.<env>.baseUrl` | 실제 API base URL. `openapi_endpoint` 가 path 와 합성해 `fullUrl` 로 응답. |
| `specs.<name>.environments.<env>.source` | 환경별 spec source override (옵션). |
| `specs.<name>.cacheTtlSeconds` | 백그라운드 재검증 주기 (기본 300초). |
| `cache.diskCache` | 디스크 캐시 on/off (기본 `true`). |
| `cache.diskCachePath` | 디스크 캐시 디렉토리 (기본 `~/.cache/openapi-mcp`). |
| `http.timeoutMs` | HTTP fetch 타임아웃 (기본 10000ms). |
| `http.insecureTls` | TLS 검증 비활성화. CLI `--insecure-tls` 와 동일. |

## CLI

```text
openapi-mcp [options]

Options:
  -c, --config <path>      설정 파일 경로 (기본: ~/.config/openapi-mcp/openapi-mcp.json)
  -l, --log-level <level>  로그 레벨 trace|debug|info|warn|error|fatal|silent (기본 info)
  --insecure-tls           TLS 인증서 검증 비활성화 (사내 self-signed 환경)
  -V, --version            버전 출력
  -h, --help               도움말
```

stdio transport 이므로 stdout 은 JSON-RPC 전용. 모든 로그는 **stderr** 로 나간다 (pino).

self-signed 인증서 환경에서는 다음 둘 중 하나:

- `openapi-mcp --insecure-tls --config ...`
- `NODE_EXTRA_CA_CERTS=/path/to/ca.pem openapi-mcp --config ...`

## MCP host 에 연결하기

### Claude Desktop

`~/Library/Application Support/Claude/claude_desktop_config.json` (macOS) 또는 `%APPDATA%/Claude/claude_desktop_config.json` (Windows) 에:

```json
{
  "mcpServers": {
    "openapi": {
      "command": "openapi-mcp",
      "args": ["--config", "/absolute/path/to/openapi-mcp.json"]
    }
  }
}
```

### Claude Code (CLI / VS Code)

`.mcp.json` 또는 `~/.claude/mcp.json` 에 동일하게 등록.

### MCP Inspector 로 직접 디버깅

```bash
npx @modelcontextprotocol/inspector openapi-mcp --config /path/to/config.json
```

## 노출되는 MCP tools

| Tool | 입력 | 결과 |
| ---- | ---- | ---- |
| `openapi_envs` | 없음 | configured spec 들과 environments(baseUrl 포함) |
| `openapi_get` | `input` (spec name), `environment?` | 캐시 우선 spec 적재. swagger 2.0 → 3.0 자동 변환 + `$ref` deref. |
| `openapi_status` | `input` | 캐시 메타 (cached / fetchedAt / ttlSeconds) |
| `openapi_refresh` | `input?` | 메모리 + 디스크 캐시 비우고 재다운로드 |
| `openapi_search` | `query?`, `spec?`, `tag?`, `method?`, `limit?` | 점수화된 endpoint 검색 (operationId>path>summary>description) |
| `openapi_endpoint` | `spec`, `environment`, (`operationId` 또는 `method`+`path`) | 풍부한 endpoint detail + `fullUrl` |
| `openapi_tags` | `spec` | OpenAPI tag 목록 + 각 tag 의 endpoint 개수 |

전형적인 흐름: `openapi_envs` → `openapi_search` (필터) → `openapi_endpoint`.

## 캐싱 동작

- 첫 요청 시 spec 을 fetch + parse + dereference + index 후 in-memory 캐시.
- 디스크 캐시 (기본 활성) 에도 동시에 저장. 다음 프로세스 시작 시 hydrate.
- TTL (`cacheTtlSeconds`) 이 지나면 다음 요청은 stale 데이터로 즉시 응답하고, 백그라운드에서 conditional GET (`If-None-Match`, `If-Modified-Since`) 으로 재검증.
- 재검증 실패 시 stale 캐시 유지하고 stderr 에 경고 로그.
- `openapi_refresh` 는 캐시 (메모리 + 디스크) 를 모두 비우고 무조건적으로 재다운로드.

## Claude Code plugin 의 7 tool 과의 차이

이 저장소는 한 패키지에서 두 진입점을 노출한다 — Claude Code plugin (`src/index.ts`) 과 standalone CLI (`src/standalone.ts`). 둘 다 같은 7 tool 을 같은 core (`src/core/`) 위에서 노출하지만, **입력 형태**가 다르다:

- Claude Code plugin: `input` = spec URL **또는** `host:env:spec` 핸들. `agent-toolkit.json` 의 `openapi.registry` 를 adapter 로 평탄화. baseUrl 은 leaf 에 옵션으로 선언.
- `openapi-mcp` 단독: `input` = `specs.<name>` 의 spec name + `environment` 가 별도 파라미터. `openapi-mcp.json` 을 평탄화 없이 그대로 SpecRegistry 에 적재. baseUrl 은 `environments.<env>.baseUrl` 에 필수로 선언.

코어 동작 (deref / swagger2 변환 / TTL / conditional GET / 디스크 캐시) 은 두 진입점 모두 동일 — handler 한 곳에서 정의 (`src/core/handlers.ts`) 되고 (plugin), standalone 은 같은 SpecRegistry 위에서 자체 tool 을 정의한다.
