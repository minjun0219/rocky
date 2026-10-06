---
"@minjun0219/rocky": minor
---

`rocky statusline --full` 이 한도 경보를 깜빡이고 계정 배지를 붙인다. 5h/7d 가 임박(`alertPercent`)·소진으로 오르면 그 창이 6초 동안 0.5초마다 배지와 굵은 빨강을 오간 뒤 배지로 남는다(창이 리셋되면 다시 무장). `rocky.json` 의 `statusline.badges` 에 이메일을 키로 `emoji` 또는 `glyph`+`color` 를 적으면 로그인된 계정이 상태 줄 앞에 표시된다. cc-usage 와 같은 출력이다.
