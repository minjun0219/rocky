---
"@minjun0219/rocky": minor
---

토큰 추천에 `fresh-session` 규칙을 더한다 — 맥락이 무거운데(요청당 캐시 읽기, 기본 20만↑) 출력이 작으면(기본 1만↓) 기계적인 후속은 새 세션이나 가벼운 서브에이전트로 넘기라고 권한다. 근거에 턴 평균 캐시 읽기(`avgCacheReadTokens`)와 요청당 캐시 읽기(`avgContextTokens`)를 싣고, 기준은 `tokens.recommend` 의 `freshSession` · `heavyContextTokens` · `freshSessionOutputTokens` 로 바꾼다.
