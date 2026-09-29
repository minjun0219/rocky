---
"@minjun0219/rocky": minor
---

리뷰가 붙은 PR 을 세션이 알아서 처리하게 한다 — `rocky.json` `pr.autoResolve`(true = 전 레포, `["owner/name", …]` = 그 레포만, 기본 끔). 처리 안 된 리뷰 스레드가 늘면 데몬이 "리뷰 도착" 전이(`pr-review`)를 남기고, 켜 둔 레포면 그 레포에서 일하는 Claude Code 세션에 `/rocky:resolve-reviews N` 절차대로 처리하라는 메시지를 받은편지함으로 보낸다. 배너·알림 브릿지는 이 전이를 쓰지 않는다.
