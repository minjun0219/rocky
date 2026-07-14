# opencode 에서 rocky 쓰기

rocky 의 전체 MCP 도구를 opencode 에서 쓰는 방법. `src/index.ts` 는 이미 host-agnostic stdio MCP 서버라 opencode 에서도 Claude Code plugin 과 같은 프로세스(`bun run <repo>/src/index.ts`)를 그대로 띄우면 된다.

## 등록

opencode 설정 파일 `opencode.json` 의 `mcp` 섹션에 local stdio 서버로 추가한다. user 스코프는 `~/.config/opencode/opencode.json`, project 스코프는 레포 루트의 `opencode.json` 을 쓴다:

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

`type` 은 `"local"` 고정이며 필수다. `command` 는 문자열 배열이다. Codex 의 `command` + `args` 를 한 배열로 합친 형태라고 보면 된다. 옵션으로 `environment` 객체, `enabled`, `timeout` 을 둘 수 있고 `timeout` 기본값은 5000ms 다. 환경 변수 필드명은 `env` 가 아니라 `environment` 다.

## CLI 등가

동등한 CLI 명령:

```bash
opencode mcp add rocky
```

대화형으로 local 서버를 추가한다. 환경 변수가 필요하면 `--env KEY=VALUE` 옵션으로 `environment` 를 지정할 수 있다.

## 노출되는 도구

opencode 에서는 `src/index.ts` 의 전체 MCP tool surface 를 쓴다.

- `openapi_*` 7개: `openapi_get` / `openapi_refresh` / `openapi_status` / `openapi_search` / `openapi_envs` / `openapi_endpoint` / `openapi_tags`
- `seo_validate` 1개
- `worklog_*` 4개: `worklog_append` / `worklog_read` / `worklog_search` / `worklog_status`
- `notion_*` 4개: `notion_get` / `notion_refresh` / `notion_status` / `notion_extract` (`ntn` CLI 탐지 시에만)

## 주의점

- `/abs/path/to/rocky/src/index.ts` 는 사용자의 rocky 레포 체크아웃 위치에 맞춘 절대경로로 바꾼다. 별도 install 없이 항상 최신 소스를 실행한다.
- `cwd` 가 중요하다. `rocky.json` 의 project scope 해석과 worklog 프로젝트별 기본 저장 경로는 MCP 서버 프로세스의 `cwd` 기준이다. opencode 는 실행 디렉터리 기준으로 서버를 spawn 하므로 프로젝트별로 동작한다.
- 경로는 `environment` 로 오버라이드할 수 있다:

```json
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "rocky": {
      "type": "local",
      "command": ["bun", "run", "/abs/path/to/rocky/src/index.ts"],
      "enabled": true,
      "environment": {
        "ROCKY_WORKLOG_DIR": "...",
        "ROCKY_NOTION_CACHE_DIR": "..."
      }
    }
  }
}
```

- `bun` 이 opencode 가 보는 `PATH` 에 있어야 한다.
- `notion_*` 4 도구는 공식 Notion CLI `ntn` 이 서버 기동 시 탐지될 때만 노출된다.
- `/finish` / `/recall` / `/codex` / `/opencode` 슬래시 커맨드와 `writing-cc-plugin` 스킬은 Claude Code 전용이다. opencode 에서는 MCP 도구만 쓰이고 이들은 노출되지 않는다.
