---
"@minjun0219/rocky": minor
---

이름을 `rocky` 로 통일한다 — CLI `rocky-todo` → `rocky`, 데몬 `rocky-todod` → `rockyd`, 크레이트
`rocky-core` / `rockyd` / `rocky-cli`, 릴리스 자산 `rocky-v<버전>-<target>.tar.gz`, 설치 디렉터리
`~/.local/share/rocky/v<버전>/`, 부트스트랩 `plugin/bin/rocky` 와 env `ROCKY_BIN` /
`ROCKY_RELEASE_BASE`, launchd 라벨 `com.rocky.daemon`, 데몬 health 의 `name: "rocky"`,
훅 주입 블록 제목 `# rocky: …`. 문서는 `docs/board.md` 로 옮기고 웹 UI·데스크톱 앱 절을 걷어냈다.

그대로 두는 것: 보드 key(데이터), `~/.config/rocky/todo`, 설정 env `ROCKY_TODO_*`(`todo` 블록의
키), MCP 도구명 `todo_*` / `note_*`, 역사 문서.

**업그레이드 주의**: 새 CLI 는 옛 이름(`name: "rocky-todo"`)으로 응답하는 0.23.0 이하 데몬도
자기 데몬으로 알아보고 버전 불일치로 재기동한다 — 안 그러면 포트 충돌로 옛 데몬이 영영 남는다.
`~/.local/share/rocky-todo/` 의 옛 설치본은 더 이상 안 쓰이니 지워도 된다.
