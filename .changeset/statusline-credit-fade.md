---
"@minjun0219/rocky": minor
---

`rocky statusline --full` 의 크레딧 금액이 쓰는 중인지 색으로 알려 준다. 안 쓸 때는 원래 색(사용률 그라데이션)을 회색 쪽으로 반 섞은 옅은 색이고, 쓰기 시작하면(이번 window 에서 늘었거나 15분 안에 늘어남) 3초에 걸쳐 원래 색으로 페이드한다. 다시 안 쓰면 바로 옅어진다. `rocky.json` 의 `statusline.creditFade: false` 로 끄면 cc-usage 와 같은 색이다.
