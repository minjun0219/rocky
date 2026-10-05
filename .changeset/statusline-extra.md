---
"@minjun0219/rocky": minor
---

`rocky statusline --full` 이 다른 도구의 statusline 줄을 덧붙인다. `rocky.json` 의 `statusline.extraCommands[]`(`command` argv · `timeoutMs` 기본 300)에 적은 명령을 나란히 돌려, rocky 줄과 보드 줄 사이에 설정 순서대로 붙인다. `{{cwd}}` · `{{session_id}}` 를 치환하고(쓰인 값이 비면 건너뜀), 늦거나 실패한 명령은 그 줄만 빠진다. cc-usage 의 `extra_commands` 와 같은 출력이다.
