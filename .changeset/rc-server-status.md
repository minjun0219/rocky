---
"@minjun0219/rocky": minor
---

`claude rc` 서버 현황을 데몬이 보여 준다. `rocky.json` 의 `rc` 블록(`root` · `pinned` · `targets`)에 폴더를 적으면 `GET /api/rc/servers` 와 `rocky rc` 가 대상별 실행 여부 · 열린 세션 수 · 떠 있은 시간, 목록 밖 폴더에서 도는 서버, 로그인 상태, Antigravity 상태를 낸다. 보기만 하고 서버를 띄우거나 내리지 않는다.
