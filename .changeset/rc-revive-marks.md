---
"@minjun0219/rocky": minor
---

데몬이 rc 서버를 띄울 때마다 그때 설치된 claude 버전을 `rc/<라벨>.version` 에 남기고, 감시(`rc.supervise`)는 야간 재시작이 내리고 못 띄워 되살림 표식(`rc/<라벨>.revive`)이 남은 서버를 비고정이어도 서버만 띄운다.
