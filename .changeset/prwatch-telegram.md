---
"@minjun0219/rocky": minor
---

PR 감시의 ready·충돌 알림을 **알림 브릿지**로도 보낸다 — `rocky.json` `pr.notifiers[]`(수집함 `todo.inbox[]` 와 같은 명령 규약). 데몬이 argv 그대로 실행하고 stdin 에 전이 JSON 을 준다; 어느 서비스인지는 데몬이 모른다. 참조 구현 `bridges/telegram/notify.ts`(Bot API, 토큰은 `op read`). macOS 배너(`pr.notify`)와 독립.
