---
"@minjun0219/rocky": patch
---

보관된 todo 의 핸드오프는 더 이상 "열린" 것으로 세지 않는다 — `GET /api/handoffs?open=true` 와
요약(`rocky today` · SessionStart · `/api/summary` 의 `handoffsOpen`)에서 빠진다. 배달만 되고
착수 없이 보관된 옛 테스트 핸드오프가 요약에 "핸드오프 대기 1" 로 영원히 남던 문제. 보관을
해제하면 다시 열린다.
