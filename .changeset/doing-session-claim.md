---
"@minjun0219/rocky": minor
---

세션이 MCP `todo_status` 로 스스로 시작한 할 일에도 그 세션을 귀속시킨다(PostToolUse 훅 `rocky hook claim-doing`). 그동안 세션 귀속은 핸드오프로 받은 작업에만 붙어 statusline 보드 줄의 `⏺ {mine.ref} {mine.title}`·`💬 {mine.comments}` 와 워크로그 턴의 `todo:<ref>` 태그가 거의 나오지 않았다. Stop 훅의 "닫았나?" 확인은 지금처럼 핸드오프로 받은 할 일에만 한다. 데몬 DB 에 컬럼 하나(`todos.doing_session_claimed`)가 더해진다.
