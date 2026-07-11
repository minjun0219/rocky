# @minjun0219/agent-toolkit-opencode 설치

> Claude Code 사용자라면 [`packages/agent-toolkit-claude-code`](../agent-toolkit-claude-code) 를, OpenAPI 만 단독 stdio MCP 로 띄우고 싶다면 [`packages/openapi-mcp`](../openapi-mcp) 를 보세요. 이 파일은 **opencode 만** 다룹니다.

## 설치

opencode 의 `opencode.json` (또는 user-level 설정) 의 `plugin` 배열에 등록 후 opencode 재시작:

```json
{
  "plugin": [
    "@minjun0219/agent-toolkit-opencode@git+https://github.com/minjun0219/agent-toolkit.git"
  ]
}
```

로컬 체크아웃을 직접 쓸 때:

```json
{
  "plugin": ["./path/to/agent-toolkit/packages/agent-toolkit-opencode"]
}
```

(npm 으로 publish 된 뒤에는 `"@minjun0219/agent-toolkit-opencode"` 한 줄로도 등록 가능.)

## 검증

opencode 가 실행 중일 때:

```
> use openapi_envs tool
> use openapi_get tool with input "<spec URL or host:env:spec handle>"
> use openapi_search tool with query "/pets"
```

7 개 tool (`openapi_get` / `openapi_refresh` / `openapi_status` / `openapi_search` / `openapi_envs` / `openapi_endpoint` / `openapi_tags`) 가 모두 노출되면 정상.

## 환경 변수

| 변수 | 기본값 | 영향 |
| --- | --- | --- |
| `AGENT_TOOLKIT_OPENAPI_CACHE_DIR` | `$XDG_CACHE_HOME/openapi-mcp` 또는 `~/.cache/openapi-mcp` | 디스크 캐시 위치. |
| `AGENT_TOOLKIT_OPENAPI_CACHE_TTL` | `300` | OpenAPI 캐시 TTL (초). |
| `AGENT_TOOLKIT_CONFIG` | `~/.config/opencode/agent-toolkit/agent-toolkit.json` | user-level `agent-toolkit.json` 경로 override. project-level `./.opencode/agent-toolkit.json` 이 leaf 단위로 덮어쓴다. |

## OpenAPI registry (`agent-toolkit.json`)

설정을 두면 OpenAPI 도구가 `host:env:spec` 짧은 핸들을 받는다 (URL 직접 입력도 그대로 작동).

```jsonc
{
  "$schema": "https://raw.githubusercontent.com/minjun0219/agent-toolkit/main/agent-toolkit.schema.json",
  "openapi": {
    "registry": {
      "acme": {
        "dev":  { "users": "https://dev.acme/users.json",
                  "orders": "https://dev.acme/orders.json" },
        "prod": { "users": "https://api.acme/users.json" }
      }
    }
  }
}
```

식별자 패턴은 `^[a-zA-Z0-9_-]+$` — 콜론은 핸들 separator 예약. URL 은 `http` / `https` / `file` 스킴만 허용. config 가 스키마를 위반하면 plugin 이 한 줄 에러 로그 후 빈 registry 로 fallback — 도구 자체는 계속 작동한다.

```
> use openapi_get tool with input "acme:dev:users"
> use openapi_search tool with query "/pets" scope "acme:dev"
```

## 트러블슈팅

opencode 가 plugin 을 cache 하는 위치는 `~/.cache/opencode/packages/@minjun0219/agent-toolkit-opencode@*` (정확한 경로는 opencode 버전에 따라 다름). plugin 로드가 안 되면 캐시 디렉터리를 지운 뒤 재시작.

## v0.2 와의 차이

| 영역 | v0.2 | v0.3 |
| --- | --- | --- |
| Tool 개수 | 27 | 7 |
| 빠진 도메인 | — | journal / mysql / notion / spec-pact / pr-watch (전부 archive) |
| 빠진 에이전트 | — | rocky / grace / mindy |
| 빠진 skill | — | 5 종 전부 |
| 패키지 형태 | 단일 `package.json` (`agent-toolkit`) | monorepo 의 한 패키지 (`@minjun0219/agent-toolkit-opencode`) |
| Git URL 식별자 | `agent-toolkit@git+...` | `@minjun0219/agent-toolkit-opencode@git+...` |
