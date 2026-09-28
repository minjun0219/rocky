---
"@minjun0219/rocky": minor
---

`/rocky:config` 와 `rocky config` — 새 기기 셋업을 손으로 하지 않는다. `rocky config show` 가
설정 파일·설치본·데몬·launchd 상주·세션 요약·노출·수집함·Claude Code statusline 연결·보드 ↔
레포 경로를 한 번에 점검해 `다음 할 일` 을 내고(`--json` 가능), `rocky config init` 은 기본
`rocky.json`(expose off · sessionSummary on · `$schema`)을 없을 때만 만든다. 슬래시 커맨드는 그
결과를 보고 빠진 항목을 하나씩 물어 채우고, `expose off` 같은 값 변경도 같은 자리에서 한다.
사용자 파일(`settings.json` 의 statusLine)은 덮어쓰지 않는다.
