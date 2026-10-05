---
"@minjun0219/rocky": minor
---

Antigravity 원격 제어(`agy remote-control`)를 rocky 에서 켜고 끈다 — `rocky rc agy [start|stop]` 과 `POST /api/rc/antigravity/{start,stop}`(로컬 전용). `GET /api/rc/servers` 의 `antigravity` 는 이제 `rc` 블록과 상관없이 `agy` 가 설치돼 있으면 실린다.
