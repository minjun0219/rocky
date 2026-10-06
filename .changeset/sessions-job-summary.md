---
"@minjun0219/rocky": patch
---

`GET /api/sessions` 의 background 행에 Claude Code 가 `~/.claude/jobs/<id>/state.json` 에 남긴 작업 요약을 `job` 으로 싣는다 — `detail`(지금 하는 일) · `needs`(사람에게 기다리는 것) · `updatedAt`. 내부 파일이라 못 읽거나 형식이 다르면 그 행만 `job` 이 없다.
