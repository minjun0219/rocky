---
"@minjun0219/rocky": minor
---

데몬이 기본 브랜치(보통 `main`)의 새 커밋을 받아 게이트를 다시 돈다 — `rocky.json` 의 `verify.targets[]` 로 켜고, 전용 detached 워크트리에서 단계를 차례로 돌며 실패·복구만 배너로 알린다. `rocky verify` · `GET /api/verify` 로 마지막 결과를 본다.
