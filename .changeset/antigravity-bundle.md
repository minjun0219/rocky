---
"@minjun0219/rocky": minor
---

Antigravity(`agy`)에서 rocky 보드를 쓸 수 있다. 레포의 `antigravity/` 번들을 `agy plugin install`로 깔면 데몬 MCP(`todo_*` · `note_*`)와 `board` 스킬, 참조(`rocky-12`)를 보드 항목으로 읽는 규칙이 붙는다. `board` 스킬에는 작업을 Antigravity로 넘기는 절차가 생겼다 — 브리프를 할 일의 `description`에 쓰고, agy가 actor `antigravity`로 착수·댓글·완료를 남기면 Claude Code 세션에 보드 변경으로 주입된다. 워크로그는 agy가 MCP 서버를 플러그인 폴더에서 띄워 프로젝트를 못 가리므로 아직 빠져 있다.
