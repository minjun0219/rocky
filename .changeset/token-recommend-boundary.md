---
"@minjun0219/rocky": patch
---

토큰 추천의 `switch-to-sonnet` 이 바꾸는 시점을 함께 말한다 — 다음 작업 경계(커밋 직후)나 새 세션. 세션 중간에 모델을 바꾸면 캐시를 새로 쌓아 긴 세션에선 오히려 비싸다. effort 변경은 캐시를 지키므로 `lower-effort` 문구는 그대로다.
