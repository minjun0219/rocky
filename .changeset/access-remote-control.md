---
"@minjun0219/rocky": minor
---

Cloudflare Access 로 들어온 허용 이메일에게 원격 제어 탭(rc 서버 띄우기·재시작·닫기·야간 실행)을 연다 — 사용자 `rocky.json` 의 `access` 블록(`team`·`aud`·`emails`·`remoteControl`). 데몬이 `Cf-Access-Jwt-Assertion` 을 팀 공개키로 검증한다. `/api/health` 에 `rcControlAllowed` 가 생긴다.
