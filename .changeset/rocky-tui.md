---
"@minjun0219/rocky": minor
---

TUI — `rocky tui [--board K]` 가 보드를 터미널 화면으로 띄운다(새 바이너리 `rocky-tui`, 릴리스
tarball 동봉). 섹션별 목록 + 선택 항목 상세(설명·링크·댓글), SSE 로 자동 갱신(끊기면 백오프
재연결 + 전체 refetch), `s`/`x`/`d`/`o`/`a` 로 상태 변경, `Tab` 으로 보드 전환. 보드는 CLI 와
같은 규약으로 cwd 에서 유추한다. 수집함 탭·핸드오프·GitHub 상태는 다음 조각.
