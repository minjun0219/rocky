---
"@minjun0219/rocky": patch
---

launchd 밖에서 데몬을 띄우는 마지막 수단이 plist 에 구운 PATH 를 물려준다 — 좁은 PATH 의 앱 아래서 뜬 데몬이 `claude` 를 못 찾아 rc 기동이 실패하던 일. rc 기동이 `os error 2` 로 실패하면 폴더가 없는지, 프로그램이 데몬의 PATH 에 없는지(PATH 포함) 알린다.
