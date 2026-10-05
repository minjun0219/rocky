---
"@minjun0219/rocky": minor
---

`rocky mcp worklog --roots` 가 생겼다. 서버를 작업 폴더 밖에서 띄우는 호스트(Antigravity)를 위해, 도구를 부를 때마다 클라이언트에 MCP `roots/list` 를 물어 그 폴더를 워크로그의 프로젝트로 쓴다. 답을 못 받으면 서버 cwd 로 물러서지 않고 에러를 낸다. Antigravity 번들에 워크로그와 `worklog` 스킬을 다시 넣었다. 플래그 없이 띄우는 Claude Code·Codex·opencode 는 그대로다.
