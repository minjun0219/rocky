---
"@minjun0219/rocky": patch
---

`/rocky:recall` 이 읽은 프로젝트와 다른 워크로그에 쓰지 못하게 막는다

worklog 의 프로젝트 키는 MCP 서버 프로세스의 `process.cwd()` 에서 나오는데, rocky MCP 서버는
프로젝트마다 따로 뜬다. 세션이 붙은 인스턴스가 실행 도중 갈아끼워지면 **한 번의 recall 안에서도
읽는 프로젝트와 쓰는 프로젝트가 갈린다** — 실제로 A 를 읽고 B 에 digest 를 써서 B 의 watermark 를
오염시키는 사고가 났다. 그 프로젝트의 digest 가 그것 하나뿐이면 이전 항목 전부가 다음 증분에서
영구히 건너뛰어진다.

커맨드 절차에 방어를 넣었다 — 1단계에서 `projectKey` 를 적어두고, append 직전 `worklog_status` 를
다시 불러 대조한 뒤 불일치면 중단한다. 이미 오염된 경우의 복구 절차(정정 note + 전체 범위 재-digest)도
예외 처리에 명시했다.
