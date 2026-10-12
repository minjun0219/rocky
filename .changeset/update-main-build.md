---
"@minjun0219/rocky": minor
---

`rocky update --main [SHA]` — 릴리스 대신 GitHub main 을 이 기기에서 release 빌드해 쓴다(main 빌드 모드). 처음이면 main 전용 clone 을 받고, 플러그인 마켓플레이스를 그 폴더로 바꿔도 될지 빌드 전에 묻는다(`--yes`). 그 뒤 `rocky update` 는 main 최신으로 올리고, `--check` 는 설치 커밋과 origin/main 을 비교한다. 버전이 같아도 커밋이 다르면 빌드하고 새 빌드로 데몬을 재시작한다.
