---
"@minjun0219/rocky": patch
---

버전 없는 고정 진입점 `~/.local/share/rocky/current/rocky` — SessionStart 훅이 부트스트랩한
버전으로 `current` 링크를 걸어 둔다. 플러그인 밖에서 worklog MCP 를 붙이는 설정(`claude -p
--strict-mcp-config` 배치 잡, Codex, opencode)이 릴리스마다 경로를 고치지 않아도 된다.
