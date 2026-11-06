---
"@minjun0219/rocky": minor
---

rc 대상마다 칸(`rc.options`)을 줄 수 있다 — `name` 은 폴더 이름 대신 쓸 라벨, `configDir` 은 그 서버를 `CLAUDE_CONFIG_DIR=<폴더>` 로 띄워 사용자 지침 · 스킬 · 플러그인 · 로그인 · 대화 기록을 그 폴더 것으로 나눈다. 전역 맥락 없이 자기 폴더에서 뜨는 세션을 rc 로 띄울 때 쓴다. `rocky rc targets` 가 설정 폴더와 대상이 아닌 키를 보이고, `rocky rc targets rm` 은 그 칸도 지운다.
