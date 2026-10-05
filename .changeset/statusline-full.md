---
"@minjun0219/rocky": minor
---

`rocky statusline --full` 이 cc-usage 와 같은 statusline 을 그린다 — 경로 줄, 모델·effort·ctx·5h/7d 줄(남은 비율·리셋 시각·경보 배지), 한도가 소진되면 크레딧 줄, 그 아래 보드 줄. 앞의 줄들은 CLI 가 직접 그려 데몬이 없어도 남는다. 한도 설정은 `rocky.json` 최상위 `statusline` 블록(`source` · `alertPercent`). git 세그먼트·`extra_commands`·usage API 조회는 다음 조각에서 더한다.
