---
"@minjun0219/rocky": minor
---

플러그인 업그레이드 중 데몬이 사라지는 사고를 막고, 실패하면 알린다.

- 부트스트랩이 새 버전을 받은 직후엔 입구(훅/MCP/CLI)와 무관하게 `current` 링크를 건다 — worklog MCP 기동이 받아 놓고 링크는 옛 버전에 남던 구멍.
- launchd 교체가 bootout 뒤 서비스가 내려가길 기다린 뒤 bootstrap 을 재시도하고 로드를 확인한다. 그래도 실패하면 데몬을 launchd 밖에서라도 띄우고 세션 컨텍스트에 `⚠ rocky 데몬: …` 로 알린다.
- `rocky daemon status` / `rocky config show` 가 "plist 는 있으나 로드되지 않음" 을 가르고 `rocky daemon install` 을 고치는 명령으로 보여 준다. `rocky daemon start` 는 띄운 데몬이 launchd 상주인지 밖인지 적는다.
- `rocky version` / `rocky --version` 추가.
