---
"@minjun0219/rocky": minor
---

`rocky tui` — 보드를 터미널에 띄운다(cmux 에서 웹 패널 대신). Ink 로 만든 화면이 데몬 REST/SSE 에 붙어 "지금"(전 보드의 내 차례·돌고 있음)과 보고 있는 보드의 섹션별 트리를 보여 준다. `j`/`k` 이동, `Enter` 상세(100열 이상이면 목록 옆에 늘), `s`·`x`·`d` 상태 변경, `a` 빠른 추가, `[` `]` 보드 전환. 릴리스 tarball 과 `rocky update --main` 이 실행 파일 옆에 `tui.js` 를 함께 깔고, 실행에는 bun 이 필요하다.
