---
"@minjun0219/rocky": minor
---

수집함 — 외부 투두 앱을 읽기 전용으로 보드 옆에 띄운다. `rocky.json` 의 `todo.inbox[]` 에
어댑터 명령을 등록하면 데몬이 실행해 stdout JSON(`{ "items": [...] }`)을 읽고
`GET /api/inbox?refresh=true` 로 소스별로 합쳐 준다(동시 실행, 소스별 60초 캐시, 실패는 그 소스만
`available:false`). 동기화가 아니다 — 보드로 올리는 건 클라이언트가 링크를 달아 한다. 참조 구현
`bridges/file/inbox.sh`. MCP 도구는 그대로 5개.
