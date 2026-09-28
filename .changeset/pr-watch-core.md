---
"@minjun0219/rocky": minor
---

PR 감시의 바탕 — `rocky.json` 에 `pr` 블록(`enabled` / `intervalMinutes` / `notify`), PR 스냅숏을 기억하는 `pr_watch` 테이블, "확인·머지해도 된다"·충돌·머지·닫힘 전이를 보드 히스토리(actor `rocky`, action `pr-*`)로 남기는 스토어. 데몬의 주기 조회와 알림은 다음 층.
