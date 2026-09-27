---
"@minjun0219/rocky": patch
---

TUI 의 GitHub 이슈·PR 상태를 `gh pr view` 프로세스 대신 GraphQL 한 요청으로 읽는다 — 토큰은
`gh auth token` 으로 한 번 받아 메모리에만(헤더로만 나감), 링크가 N 개여도 요청 하나(보드 rocky-21).
