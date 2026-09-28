---
"@minjun0219/rocky": minor
---

rocky 채널 — worklog stdio MCP 서버가 Claude Code channels(`claude/channel`)를 선언하고 데몬의 PR 전이(확인·머지 가능 / 충돌)를 `notifications/claude/channel` 로 세션에 밀어 넣는다. 세션이 그 자리에서 깨어나므로 훅 주입과 달리 사람이 타이핑하기를 기다리지 않는다. 리서치 프리뷰라 `claude --dangerously-load-development-channels plugin:rocky@rocky-marketplace` 로 띄운 세션만 받는다. `plugin.json` 에 `channels` 항목 추가.
