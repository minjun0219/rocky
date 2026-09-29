---
"@minjun0219/rocky": minor
---

`/rocky:review` 를 없앤다 — 버그 찾기는 기본 `/code-review` 가 같은 diff 를 더 잘 본다. 이 커맨드만의 몫이던 "요구사항 대비 점검" 은 `/rocky:finish` 의 한 단계(2.5)로 옮겼다: 위험한 변경이면 커밋 전에 `/code-review` 와 `reviewer` 서브에이전트를 돌린다.
