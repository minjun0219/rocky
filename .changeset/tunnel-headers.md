---
"@minjun0219/rocky": patch
---

Cloudflare Tunnel·Access 가 붙이는 헤더(`cf-connecting-ip` / `cf-ray` / `cf-access-*`)를 중계 헤더로
본다 — 터널 경유 요청이 원격으로 분류되어 이슈 생성·spawn·claim 이 막힌다(의도). 테일넷 없이 웹 UI 에
닿는 설정 절차를 `docs/board.md` 에 적었다.
