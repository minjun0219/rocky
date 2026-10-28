---
"@minjun0219/rocky": minor
---

맥 앱(`mac/`, SwiftUI)을 더한다 — 메뉴 막대에 돌고 있는 일과 머지 후보 PR 을 늘 띄우고, 할 일 창에서 보드를 골라 착수·멈춤·완료·추가한다. 데몬 REST/SSE 의 클라이언트(`x-rocky-client: mac`)이고 웹 UI 는 그대로다. `mac/scripts/bundle.sh` 가 `Rocky.app` 을 만들고 `ROCKY_MAC_SIGN_IDENTITY` 의 인증서로 서명한다(없으면 ad-hoc).
