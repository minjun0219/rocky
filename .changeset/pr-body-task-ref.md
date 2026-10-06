---
"@minjun0219/rocky": patch
---

`/rocky:review-request` 가 PR 본문 첫 줄에 `할 일: <ref>` 를 쓰지 않는다. 할 일과 PR 은 할 일의 `links` 로만 잇는다 — 데몬은 그 링크로 머지 뒤 완료를 판정하고 본문 줄은 읽지 않았다. 보드 번호를 레포에 남기지 않는다는 board 스킬 규칙과 어긋나던 것을 맞췄다.
