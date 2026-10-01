---
"@minjun0219/rocky": minor
---

PR 감시를 구독한 PR 만으로 좁힌다 — `rocky pr subscribe|unsubscribe N`·`rocky pr subscriptions`, `/rocky:review-request`·`/rocky:review-fix` 가 그 세션으로 구독한다. 알림은 구독한 세션에만 가고(같은 레포의 다른 PR 이 한 세션에 쏟아지지 않는다), 레포 목록 조회를 없애 GitHub API 비용이 구독한 PR 수에만 비례한다. 구독은 머지·닫힘에서 풀린다.
