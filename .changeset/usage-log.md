---
"@minjun0219/rocky": minor
---

사용 로그 — rocky 의 표면(REST 라우트 · MCP 도구 · `rocky <cmd>` · 훅 · 웹 UI 이벤트)이 얼마나
쓰이는지를 `~/.config/rocky/usage/YYYY-MM.jsonl` 에 한 줄씩 남긴다(내용 없이 이름·누가·클라이언트·
성공·시간만; statusline·SSE·health 는 제외). `rocky usage [--since 30d] [--json]` 이 많이 쓴 표면·
에러·**안 쓴 표면**·시간 분포를 낸다. `rocky.json` `usage` 블록 / `ROCKY_USAGE=0` 으로 끈다.
표면을 빼거나 바꾸는 PR 은 이 수치를 인용한다.
