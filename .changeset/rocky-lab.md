---
"@minjun0219/rocky": minor
---

실험 기능 `lab` — Claude Code function hooks(early access) 모듈을 플러그인에 싣는다. 사용자 `rocky.json` 에 `"lab": {}` 를 두면(기본 꺼짐) 데몬이 받은편지함으로 보낸 PR·리뷰·수집함·핸드오프 메시지가 화면 toast 로도 뜨고, 프롬프트 위에 보드 요약 · PR 감시 · 받은편지함 등록 줄이, status 줄에 엔진이 주는 5h/7d·ctx 가 붙는다. `/rocky-lab` 이 세션 id(엔진 값과 env 값)·받은편지함·스위치를 진단한다. 칸마다 `toast`·`band`·`limits: false` 로 끈다.
