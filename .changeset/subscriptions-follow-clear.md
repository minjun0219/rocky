---
"@minjun0219/rocky": patch
---

`/clear` 된 세션은 더 이상 옛 세션의 PR·수집함 알림으로 깨우지 않는다. 남은 구독은 감시만 이어 가고, 새 세션으로 넘길지·지켜보기만 할지·해지할지는 `POST /api/sessions/cleared` 로 정한다(웹 화면은 다음 변경).
