---
"@minjun0219/rocky": patch
---

GitHub 프로젝트 보드 수집함 어댑터(`bridges/github-project/inbox.ts`) — 보드 필터(`assignee:@me type:Bug component/s:Web`)를 인자로 옮겨, 조건에 맞는 열린 이슈를 rocky 수집함에 띄운다. 로그인된 `gh`(`read:project`)를 쓰고, 이슈 타입이 없는 개인 계정 레포는 같은 이름의 라벨로 본다.
