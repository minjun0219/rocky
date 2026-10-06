---
"@minjun0219/rocky": patch
---

statusline 보드 줄이 가끔(1~2%) 비던 것을 고친다. 데몬의 `/api/statusline` 이 세션 목록 캐시가 만료될 때마다 그 요청에서 `claude agents --json` 을 기다려 CLI 의 300ms 마감을 넘겼다 — 이제 낡은 값을 바로 내주고 뒤에서 새로 받으며(15초마다, 배경 부하는 그대로), 데몬이 뜰 때 미리 데운다.
