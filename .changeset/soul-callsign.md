---
'@minjun0219/rocky': minor
---

소울이 사용자를 부르는 호칭(`callsign`) 설정 지원 — `rocky.json` 최상위 `callsign` 키(한 줄, 1~40자, project > user)를 `SessionStart` 훅이 소울 컨텍스트에 함께 주입하고, `/rocky:soul <name>` 세팅 플로우가 호칭을 물어보며, 새 `call` 서브커맨드로 호칭만 조회/변경/제거할 수 있다.
