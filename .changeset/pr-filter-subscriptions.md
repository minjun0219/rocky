---
"@minjun0219/rocky": minor
---

PR 을 GitHub 검색 조건으로도 구독한다 — `rocky pr subscribe --filter "repo:o/r author:@me"`(라벨·`project:org/5`·`review-requested:@me` 등 검색 조건 그대로). 데몬이 3분마다 검색해 걸린 열린 PR 을 그 세션이 받는다. `rocky pr unsubscribe --filter ID` 로 해지하면 그 필터로 들어온 구독도 걷힌다.
