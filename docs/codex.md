# Codex CLI 에서 rocky 쓰기

rocky 의 전체 MCP 도구를 OpenAI Codex CLI 에서 쓰는 방법. `src/index.ts` 는 이미 host-agnostic stdio MCP 서버라 Codex 에서도 Claude Code plugin 과 같은 프로세스(`bun run <repo>/src/index.ts`)를 그대로 띄우면 된다.

## 등록

`~/.codex/config.toml` 에 MCP 서버를 추가한다:

```toml
[mcp_servers.rocky]
command = "/abs/path/to/rocky/target/release/rocky-todo"
args = ["mcp", "worklog"]
```

동등한 CLI 명령:

```bash
codex mcp add rocky -- /abs/path/to/rocky/target/release/rocky-todo mcp worklog
```

## 노출되는 도구

Codex 에서는 `rocky mcp worklog` 의 worklog 도구를 쓴다 (보드 도구는 데몬 `http://127.0.0.1:8636/mcp` 에 따로 등록).

- `worklog_*` 4개: `worklog_append` / `worklog_read` / `worklog_search` / `worklog_status`

## 주의점

- `/abs/path/to/rocky/target/release/rocky-todo` 는 `cargo build --release` 산출물(또는 플러그인 캐시의 `plugin/bin/rocky` 부트스트랩) 경로로 바꾼다.
- `cwd` 가 중요하다. `rocky.json` 의 project scope 해석과 worklog 프로젝트별 기본 저장 경로는 MCP 서버 프로세스의 `cwd` 기준이다. Codex 가 워크스페이스를 `cwd` 로 spawn 하면 프로젝트별로 동작한다. 고정하려면 `[mcp_servers.rocky]` 에 `cwd` 를 추가한다.
- 경로는 `env` 로 오버라이드할 수 있다:

```toml
[mcp_servers.rocky]
command = "/abs/path/to/rocky/target/release/rocky-todo"
args = ["mcp", "worklog"]
env = { ROCKY_WORKLOG_DIR = "..." }
```

- `/rocky:finish` / `/rocky:recall` 등 슬래시 커맨드와 `writing-cc-plugin` 스킬은 Claude Code 전용이다. Codex 에서는 rocky 가 MCP 도구만 등록하므로 이들은 노출되지 않는다. 단 이는 rocky 의 현재 배선 선택이지 Codex 의 한계가 아니다 — Codex 자체는 2026 기준 hooks · skills · subagents · `.codex-plugin/plugin.json` 번들 + 마켓플레이스를 지원하므로 이 표면들은 이식 가능하다(미구현). 호스트별 커버 범위는 [`docs/hosts.md`](./hosts.md) 참고.
