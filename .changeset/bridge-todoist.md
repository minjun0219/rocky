---
"@minjun0219/rocky": minor
---

수집함 어댑터 첫 실물 — `bridges/todoist/inbox.py`. Todoist API v1 의 미완료 작업을 규약
`{ "items": [...] }` 로 낸다(`--filter` 로 Todoist 필터 문법, 커서 페이지 전부 수집, 완료·삭제
제외, 링크는 `app.todoist.com/app/task/<id>`). 토큰은 `--op op://…` 로 1Password Agent Vault
에서 읽고 값은 어디에도 찍지 않는다. `python3` stdlib 만, 의존 없음.
