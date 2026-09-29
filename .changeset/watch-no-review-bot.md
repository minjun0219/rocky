---
"@minjun0219/rocky": patch
---

`/rocky:resolve-reviews` 의 봇 리뷰 대기가 리뷰 봇이 없는 레포에서 timeout 을 꽉 채우지 않는다 — 최근 PR 20개에 봇 흔적이 없으면 CI 만 기다리고 `verdict: "none"` 으로 바로 돌아온다.
