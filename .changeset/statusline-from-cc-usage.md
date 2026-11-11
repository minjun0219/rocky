---
"@minjun0219/rocky": minor
---

`rocky statusline` 의 구현을 cc-usage 라이브러리로 바꿨다(복사본 두 벌을 하나로). 설정(`rocky.json` 의 `statusline`)·환경 변수(`ROCKY_STATUSLINE_*`)·캐시 자리는 그대로다. git 표시가 cc-usage 를 따라 바뀐다: 파일 수 대신 브랜치 이름 바로 뒤의 기호(unstaged `*` · staged `+` · 충돌 `!`)를 쓰고, 원격 브랜치가 지워졌으면 `gone` 을 단다.
