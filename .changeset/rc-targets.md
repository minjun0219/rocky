---
"@minjun0219/rocky": minor
---

rc 대상 목록을 CLI 로 고친다. `rocky rc targets` 로 rocky.json 의 대상(이름 · 고정 · 폴더)을 보고, `rocky rc targets add <이름|경로> [--pin]` · `rocky rc targets rm <이름|경로>` 로 더하고 뺀다. 고친 목록은 떠 있는 데몬이 바로 다시 읽는다. 전에는 손으로 고친 대상을 데몬을 다시 띄울 때까지 "rc 대상이 아니다" 로 거절했다. 손으로 고쳤으면 `rocky rc reload`. 바로 바뀌는 것은 대상 목록(`root` · `pinned` · `targets`)뿐이고 `enabled` · `supervise` · `nightly` 는 데몬을 다시 띄워야 반영된다(명령이 알려 준다). 파일이 JSON 이 아니면 데몬은 하던 목록을 그대로 쓴다.
