---
"@minjun0219/rocky": patch
---

`claude agents --json` 의 pid 없는 background 세션(사람 답을 기다리며 잠든 `blocked`)을 세션 목록에서 버리지 않는다. 버리면 그 세션이 든 할 일이 "세션 없음" 으로 판정되어 24시간 뒤 자동으로 풀렸다.
