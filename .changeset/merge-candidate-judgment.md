---
"@minjun0219/rocky": patch
---

"머지 가능" 을 기계 판정(머지 후보)과 세션 판단으로 나눈다 — 데몬 알림 문구가 "머지 후보" 가 되고, 세션은 `/rocky:resolve-reviews` 8단계(응답 안 한 리뷰 요청·봇 리뷰 필수 레포의 봇 신호·방금 한 푸시·작업 중 표시)를 본 뒤에 알린다. `pr-threads.ts ready` 가 응답 안 한 리뷰 요청을 이유로 낸다. 머지 뒤에 붙은 리뷰는 `pr-threads.ts after-merge` 로 찾아 다음 PR 에 고치고 링크를 건다(`/rocky:finish` 도 PR 을 만들기 전에 본다).
