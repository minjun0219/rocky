---
"@minjun0219/rocky": minor
---

`rc.supervise` 를 켜면 데몬이 2분마다 고정 rc 서버를 보고 꺼져 있으면 스스로 띄운다(연달아 못 뜨면 30분까지 쉬었다 다시). 데몬 맥락의 claude 자격이 끊기거나 돌아오면 macOS 배너로 한 번씩 알리고, 회복 뒤에는 끊기기 전에 뜬 서버를 "자격 의심"으로 표시한다. `rocky rc` 와 현황 API 에 감시 상태 · `authSuspect` 가 실린다.
