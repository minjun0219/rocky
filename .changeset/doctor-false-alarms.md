---
"@minjun0219/rocky": patch
---

`rocky doctor` · `rocky config show` 의 오탐 둘을 고친다. 데몬이 막 떠 PR 감시의 첫 tick 전이면 "멈춤 — 이유 모름"(+ `gh auth status` 안내) 대신 "첫 바퀴 전" 으로 낸다. statusLine 이 `rocky statusline`(·`--full`)을 직접 부르면 연결된 것으로 본다 — 전에는 "rocky 세그먼트 없음" 이라며 `/api/statusline` 조각을 붙이라고 해, 따르면 보드 줄이 두 번 나왔다.
