---
"@minjun0219/rocky": minor
---

데몬이 `claude rc` 서버를 띄우고 다시 띄운다. `rocky rc start|restart <라벨>`(`--fresh` · `--wait`)과 로컬 전용 `POST /api/rc/servers/:label/{start,restart}` 가 생겼다. 재시작은 열린 세션이 있으면 이어받고(`-c`), `already served` 면 45·90초 뒤 다시 띄운다. 서버는 데몬과 다른 프로세스 그룹으로 떠서 데몬을 재시작·업데이트해도 살아 있다.
