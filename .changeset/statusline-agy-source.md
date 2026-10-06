---
"@minjun0219/rocky": minor
---

`rocky statusline --full` 을 Antigravity(`agy`)의 statusLine 에도 걸 수 있다. stdin 의 `product: "antigravity"` 로 알아보고, 한도는 agy 가 주는 `quota`(Gemini 모델이면 `gemini-*`, 그 밖이면 `3p-*` 버킷)로 그린다 — Claude 쪽 토큰·API·캐시·계정 배지는 보지 않고, 경보는 배지로 고정된다. `--source auto|stdin|api|none` 플래그와 환경 변수 `ROCKY_STATUSLINE_SOURCE` 로 설정의 `source` 를 그 실행에서만 바꿀 수 있다(플래그가 이긴다, 갱신 프로세스에도 같은 값이 간다).
