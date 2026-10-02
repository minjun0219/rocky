---
"@minjun0219/rocky": patch
---

`rocky update`·`rocky daemon restart` 가 launchd 밖에서 뜬 옛 데몬(고아)이 포트를 쥔 경우에도 교체한다 — 고아를 pid 로 내리고 launchd 의 새 데몬이 목표 버전으로 응답하는지 확인하며, 버전이 목표와 다르면 ✓ 대신 실패로 끝난다(바이너리를 못 받은 경우 포함). `rocky daemon status` 는 포트를 쥔 데몬이 launchd 의 것이 아니면 ⚠ 로 알린다. launchd job 이 있으면 CLI 가 데몬을 따로 띄우지 않고 launchd 에게 맡기며, launchd 가 띄운 데몬은 포트가 차 있으면 끝나는 대신 기다렸다 이어받는다.
