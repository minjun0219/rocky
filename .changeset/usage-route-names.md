---
"@minjun0219/rocky": patch
---

`rocky usage` 가 `POST /api/verify/rerun` · `POST /api/rc/handoffs/:ref/stop` · `POST /api/rc/strays/:ref/stop` 를 쓰여도 늘 "안 쓴 표면" 으로 보이던 것을 고친다 — 기록 이름이 `:ref` 로 접혀 표면 목록의 이름과 달랐다. 훅이 부르는 `POST /api/sessions/doing` 도 표면 목록에 넣는다.
