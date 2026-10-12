---
"@minjun0219/rocky": patch
---

버전 오른쪽 괄호에 빌드한 커밋의 짧은 해시를 붙인다 — 웹 하단(`rocky v0.49.0 (abc1234)`), `rocky version`, `rocky daemon status`·`restart`. 해시는 release 빌드(릴리스 tarball·main 빌드)에만 굽는다. `/api/health` 에 `commit` 필드가 생긴다(훅의 데몬 교체 판단은 지금처럼 버전으로). `daemon restart` 는 버전이 같아도 커밋이 다르면 재시작 실패로 알리고, 웹의 새로고침 권유는 커밋이 바뀌어도 뜬다.
