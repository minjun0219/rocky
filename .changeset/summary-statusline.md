---
"@minjun0219/rocky": minor
---

보드 요약 세 자리 — `rocky today`(마감·진행중·핸드오프·수집함 미올림 + 항목 4개, Claude Code 의
`! rocky today` 로 LLM 없이), SessionStart 훅이 같은 요약을 세션 컨텍스트에 넣기
(`todo.sessionSummary: false` 로 끔), statusline 템플릿 변수 `{due}`·`{collect}`. 뒤에서
`GET /api/summary` 와 `GET /api/inbox?cached=true`(기다리지 않는 수집함 조회 — 캐시만, 없으면
백그라운드 갱신)가 생겼다.
