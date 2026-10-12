---
"@minjun0219/rocky": patch
---

부트스트랩이 main 빌드 모드를 따른다 — `$XDG_DATA_HOME/rocky/current` 가 `main` 을 가리키면 릴리스를 받지 않고 그 기기에서 빌드한 `main/rocky` 를 실행한다(`ROCKY_BIN` 이 먼저). main 빌드로 갈아 끼우는 기기가 `settings.json` 의 `ROCKY_BIN` 없이도 돈다.
