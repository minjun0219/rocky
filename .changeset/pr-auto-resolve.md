---
"@minjun0219/rocky": minor
---

리뷰가 붙은 PR 을 세션이 알아서 처리하게 한다 — 보드마다 그 레포의 세션이 `rocky board auto-resolve on|off` 로 켠다(기본 끔, `PATCH /api/boards/:key {"autoResolve": true}` 는 로컬 요청 전용). 처음 보는 처리 안 된 리뷰 스레드가 생기면 데몬이 "리뷰 도착" 전이(`pr-review`)를 남기고, 켠 보드의 레포면 그 레포에서 일하는 Claude Code 세션에 `/rocky:resolve-reviews N` 절차대로 처리하라는 메시지를 받은편지함으로 보낸다. 배너·알림 브릿지는 이 전이를 쓰지 않는다.
