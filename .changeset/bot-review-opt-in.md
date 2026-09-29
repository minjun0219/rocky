---
"@minjun0219/rocky": patch
---

`/rocky:resolve-reviews` 가 봇 리뷰를 기본으로 기다리지 않는다 — 봇 흔적이 보이면 "다음부터 봇 리뷰를 기다릴까" 를 한 번 묻고, 그렇다고 하면 세션 메모리에 남겨 그 레포에서만 `watch --wait-bot` 으로 기다린다. 레포 흔적으로 자동 판단하던 방식(`verdict: "none"`)은 걷어냈다.
