---
"@minjun0219/rocky": minor
---

웹 에이전트 탭에서 살아 있는 background 세션을 **멈추고**(로컬 화면만, 한 번 더 묻는다) background 행의 `claude attach <id>` 명령을 복사한다. 새 라우트 `POST /api/sessions/stop {sessionId}` 가 `claude stop <짧은 id>` 를 돌린다 — 대화·워크트리는 남아 `claude attach` 로 잇는다. 로컬 요청 전용이고, 목록에 없으면 404, interactive·끝난 세션·pid 없이 잠든 세션은 409. 멈추면 데몬이 세션 목록 캐시를 비워 다른 탭·statusline 도 바로 따라온다.
