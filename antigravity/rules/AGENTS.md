# rocky (Antigravity)

- `rocky-12`처럼 `<보드>-<번호>` 모양의 참조를 받으면 rocky 보드 항목이다. `board` 스킬을 읽고
  `todo_list { id: "<참조>" }`로 시작한다 — 할 일의 `description`이 넘겨받은 작업의 브리프다.
- 보드 도구를 부를 때 `actor`는 늘 `antigravity`다.
- 보드 도구가 연결 거부로 실패하면 rocky 데몬이 꺼진 것이다. 진행을 지어내지 말고 `rocky daemon status`를
  안내한 뒤 멈춘다.
