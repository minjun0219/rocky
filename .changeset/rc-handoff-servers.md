---
"@minjun0219/rocky": minor
---

보드의 새 세션 띄우기가 띄운 rc 서버(핸드오프 서버)를 `rocky rc` 에 "핸드오프:" 로 따로 보이고(`GET /api/rc/servers` 의 `handoffs`), `rocky rc stop <할 일>`(`POST /api/rc/handoffs/:ref/stop`, 로컬 전용)로 닫는다 — 지금 그 폴더에서 그 pid 로 도는 rc 서버일 때만 pid 로 내리고 워크트리는 남긴다. README 의 권장 확인 규칙에 `Bash(rocky rc stop:*)` 를 더했다.
