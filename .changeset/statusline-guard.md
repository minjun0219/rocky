---
"@minjun0219/rocky": minor
---

크레딧 guard 를 더한다. `rocky.json` 의 `statusline.guard: true` 와 `UserPromptSubmit` 훅 `rocky statusline guard` 를 걸면, 한도가 소진돼 크레딧이 차감되기 시작할 때(또는 최근 크레딧이 늘었을 때) prompt 를 막는다(exit 2). 설정 오류·데이터 없음·크레딧이 꺼진 계정·`source: none` 에서는 막지 않는다. `rocky statusline allow [30m|2h|off]` 로 잠시 풀거나 다시 켠다 — 계정과 상관없이 하나라 어느 터미널에서 불러도 된다.
