---
"@minjun0219/rocky": minor
---

기본 브랜치 검증 결과를 구독한 세션에 보낸다. 세션 안에서 `rocky verify subscribe [보드] [--branch B]` 로 구독하면 끝난 실행마다(통과도) 그 세션 받은편지함으로 한 줄(`rocky: <보드> <브랜치> 검증 <sha> 통과 …` / `… 실패(<단계>) …`)이 온다 — 배포를 맡은 세션이 `/api/verify` 를 폴링하지 않아도 된다. 같은 커밋은 한 번, 다시 돌려 결과가 바뀌면 다시. 대상마다 세션 하나(다시 구독하면 넘겨받기), `unsubscribe`·`subscriptions` 와 `GET|POST|DELETE /api/verify/subscriptions`(바꾸기는 로컬 전용). 전달 기록에는 `verify-passed`·`verify-failed` 로 남고, "보내지 않기"·`/clear` 규칙은 PR 알림과 같다.
