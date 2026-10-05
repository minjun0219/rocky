---
"@minjun0219/rocky": minor
---

웹 UI에 **에이전트** 탭을 더한다. `claude agents` 가 보는 세션을 내 차례(사람 답을 기다리는 background 세션) → 실행 중 → 쉬는 중으로 묶어 보이고, background 세션에는 Claude 가 남긴 요약(기다리는 것 · 지금 하는 일)을 한 줄 싣는다. `GET /api/sessions` 의 background 행에 그 요약(`job`)이 붙는다. ⋯ 메뉴에서 탭을 끌 수 있다.
