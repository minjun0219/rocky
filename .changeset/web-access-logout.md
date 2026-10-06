---
"@minjun0219/rocky": patch
---

Cloudflare Access 로 들어온 웹 화면의 ⋯ 메뉴 맨 아래에 로그인한 이메일과 **로그아웃**을 보인다(`/cdn-cgi/access/logout` — Access 세션 전체가 끝난다). `/api/health` 가 그 이메일을 `accessUser` 로 알려 준다. 로컬·테일넷 화면에는 없다.
