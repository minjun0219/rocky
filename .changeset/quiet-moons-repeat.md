---
"@minjun0219/rocky": minor
---

`delegating-to-codex` 번들 스킬을 제거하고 `/rocky:codex` 를 자기완결화

공식 [`openai/codex-plugin-cc`](https://github.com/openai/codex-plugin-cc) 플러그인이 Codex 위임
영역을 공유 app-server 런타임 기반으로 더 넓게 덮으면서(`codex-cli-runtime` / `codex-result-handling`
/ `gpt-5-4-prompting` 스킬 + `/codex:rescue` · `/codex:review` · `/codex:transfer` 커맨드), rocky 가
같은 메커니즘을 스킬로 중복 배포할 근거가 사라졌다.

`/rocky:codex` 에는 공식 플러그인에 없는 고유 가치가 남아 있어 유지한다 — **격리 git worktree** 와
**rocky 플러그인 표면 무결 검증**(MCP 도구 개수/이름 + `.claude-plugin/plugin.json` 의 `mcpServers`).
스킬에 있던 자기완결 프롬프트 원칙, 감독자 규칙, 샌드박스·모델 선택 가드레일은 커맨드 본문으로
흡수했고, 공식 플러그인을 써야 할 상황을 커맨드 상단에 명시했다.
