---
"@minjun0219/rocky": minor
---

세션이 시작할 때 보드 요약 뒤에 이 작업 경로(워크트리 루트)에서 남긴 워크로그 기억(마지막 `/rocky:recall` 다이제스트 앵커, 그 뒤의 decision·blocker) 몇 줄을 싣는다. 이를 위해 `worklog_append` 와 Stop 의 턴 기록이 태그 `path:<워크트리 루트>` 를 자동으로 붙인다. `worklog.sessionBrief: false`(프로젝트 `rocky.json` 에서도) 로 끈다.
