---
"@minjun0219/rocky": minor
---

`rocky rc --activity`(`GET /api/rc/servers?activity=1`, 로컬 전용)가 대상마다 최근 활동(마지막 커밋 시각 · 제목, 작업 중, 곁가지 브랜치)을 보이고 꺼진 비고정 대상 중 오래 조용한 것을 "정박"으로 가른다. `rocky rc start --all` 은 꺼진 대상 전부를 띄운다 — 비고정은 세션 없이 서버만(start 본문 `serverOnly`), 고정은 세션과 함께.
