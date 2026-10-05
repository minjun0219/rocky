---
"@minjun0219/rocky": minor
---

Claude Code 세션의 모델·effort·토큰을 데몬이 트랜스크립트에서 색인한다. `rocky tokens`(모델 × effort 합계), `rocky tokens here`(지금 세션과 추천), `/api/tokens/*`, MCP `token_summary` · `token_current_session`으로 보고, effort를 낮추거나 Sonnet으로 바꿀지 규칙 기반으로 권한다. 훅 설정은 필요 없다.
