---
"@minjun0219/rocky": minor
---

`/rocky:brainstorm` · `/rocky:review` 슬래시 커맨드 추가 — superpowers 플러그인을 걷어내면서 실제로 쓰던 두 발상만 rocky 자체 커맨드로 재작성했다. `/rocky:brainstorm` 은 아이디어를 설계로 다듬는다(맥락 파악 → 한 번에 하나씩 질문 → 접근안 2~3개 → 설계 → 규모가 클 때만 스펙 문서). `/rocky:review` 는 완료 선언 전 신선한 컨텍스트의 서브에이전트로 현재 작업 diff 를 검토시킨다(이미 열린 PR 의 스레드 대응인 `/rocky:review-pr` 과 별개). 원본과 달리 **강제 게이트가 아니다** — 사용자가 부를 때만 돌고, 작은 수정에는 요구하지 않는다. 설계·계획 산출물 디렉터리는 `docs/superpowers/` 에서 `docs/design/` 으로 개명(기존 문서는 경로만 갱신해 보존).
