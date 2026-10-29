---
"@minjun0219/rocky": minor
---

검증 통과 커밋 자동 배포에 배포 창(`verify.targets[].deployWindow`)을 더했다. 창이 있으면 그 시간대(기본 02:00-04:00)에만 `rocky update --main` 을 띄워, 낮에 데몬·플러그인이 바뀌지 않고 밤에 그때의 마지막 통과 커밋이 올라간다.
