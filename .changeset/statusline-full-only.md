---
"@minjun0219/rocky": minor
---

`rocky statusline` 이 늘 경로·git·모델·ctx·한도 줄(예전 `--full`)을 그린다 — `--full` 은 그대로 받는다. 데몬이 그리던 보드 줄을 걷었다: `GET /api/statusline`, `todo.statusline` 템플릿, `ROCKY_TODO_STATUSLINE`, `statusline.board`. 보드 줄을 끼우던 `rocky statusline --cwd/--session` 과 `extraCommands` 안의 `rocky statusline` 은 아무것도 그리지 않는다. statusline 스크립트 안에서 `rocky statusline` 을 끼워 부르던 설정은 이제 줄 전체가 한 번 더 나오니 statusLine 을 바로 `rocky statusline` 으로 건다. rocky.json 에 남은 `todo.statusline` 은 무시되고, `statusline.board` 는 남아 있으면 지운다(실행은 무시하지만 편집기 스키마가 모르는 키로 잡는다). `rocky config show` 는 옛 `curl …/api/statusline` 조각을 짚고 `settings.json` 에 넣을 조각을 보여 준다.
