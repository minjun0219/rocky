---
"@minjun0219/rocky": patch
---

데몬 재시작(버전 교체 훅 · `rocky daemon restart` · `rocky update`)이 LaunchAgent plist 를 다시 쓰지 않는다. plist 가 그대로면 `launchctl kickstart -k` 로 데몬만 다시 띄우고, 데몬 경로가 바뀌었거나 job 이 로드되지 않았을 때만 plist 를 쓰고 job 을 다시 등록한다. 다시 등록할 때는 그 사유를 stderr 에 한 줄 남긴다. 보안 솔루션(EDR)이 데몬 재시작을 악성코드로 탐지한 일이 있었는데, 재시작마다 서명 없는 새 바이너리가 plist 를 다시 쓰고 `launchctl bootout` · `bootstrap` 하던 흐름이 원인으로 보인다. 릴리스 설치본의 plist 는 `current/rockyd` 를 가리켜 버전이 바뀌어도 그대로다(이 버전으로 올리는 재시작에서 한 번 다시 쓴다). main 빌드는 원래 `main/rockyd` 라 그대로다. plist 의 PATH 는 `rocky daemon install` 때 구운 것을 지킨다 — 다시 구우려면 `rocky daemon install` 을 실행한다.
