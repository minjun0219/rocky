---
"@minjun0219/rocky": patch
---

PR 감시가 GitHub GraphQL 한도를 다 쓰지 않는다. 비용은 실제가 아니라 `first:` 로 요청한 노드 수라 첫 쿼리가 열린 PR 이 없어도 레포당 263 포인트였고(3분 × 레포 10개 → 두 tick 에 시간당 5,000 소진, 세션의 `gh` 까지 마비), 이제 레포당 상태 목록 한 번(1 포인트) + 실제로 열린 PR 에만 CI·스레드 상세 배치 한 번(PR 당 ~1.5 포인트)으로 묻는다. 응답마다 잔여 예산을 읽어 1,000 밑이거나 한도 에러를 받으면 그 tick 을 멈추고 리셋까지 쉰다. `/api/health` 의 `prWatch` 에 `rateLimit`(cost·remaining·resetAt)·`pausedUntil` 이 실린다.
