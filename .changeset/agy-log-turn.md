---
"@minjun0219/rocky": minor
---

Antigravity(`agy`)의 턴도 워크로그에 저절로 남긴다. 번들 `hooks.json` 의 `Stop` 이 `rocky hook log-turn agy` 를 불러, agy 트랜스크립트에서 이번 턴(요청 · 쓴 도구 · 마지막 답)을 뽑아 Claude Code 와 같은 `kind: "turn"` 한 줄로 쓴다. 프로젝트는 agy 의 작업 폴더(`workspacePaths[0]`)라 워크로그 MCP(`--roots`)와 같은 칸에 쌓이고, 태그는 `turn` · `agy` 다. MCP 도구는 `mcp__<서버>__<도구>` 로 적는다. 이미 번들을 깐 기기는 `agy plugin install <rocky 레포>/antigravity` 를 다시 돌려야 훅이 들어간다.
