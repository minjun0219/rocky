---
"@minjun0219/rocky": patch
---

터미널에서 `rocky` 가 바로 불린다 — SessionStart 가 `~/.local/bin/rocky` 를 `current/rocky` 로 걸어
두고(릴리스마다 따라온다), `rocky config show` 가 링크·PATH 상태를 `cli` 행으로 알리며, `rocky config link`
로 지금 바로 걸 수 있다. 심볼릭 링크로 불려도 `rocky tui`·`rocky daemon start` 가 형제 바이너리를
제대로 찾는다.
