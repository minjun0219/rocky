---
"@minjun0219/rocky": minor
---

`rocky rc add <이름|경로> [--pin]` · `remove` · `pin` · `unpin` 으로 rc 대상 목록을 고친다. 데몬은 `rocky.json` 이 바뀌면 대상 목록(`targets` · `pinned` · `root`)을 다시 읽어, 손으로 고쳐도 다시 띄우지 않고 반영된다(켜고 끄기 · `supervise` · `nightly` 는 여전히 기동 때 값).
