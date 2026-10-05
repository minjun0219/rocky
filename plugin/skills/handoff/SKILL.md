---
name: handoff
description: Use when a session receives a rocky board handoff ("# rocky: 보드에서 도착한 작업 요청") or a held-todo reminder from the Stop hook ("# rocky: 이 세션이 들고 있는 할 일"), or when the user asks to hand a board item to a session. Covers the start → work → done/stop lifecycle that the web board shows as "처리중", linking the resulting PR instead of marking done yourself, and when to stay silent on the reminder.
---

# 핸드오프 — 보드에서 넘겨받은 일

사용자가 웹 보드에서 할 일을 세션에 넘기면, 데몬이 그 세션에 작업 요청을 넣는다(쉬고 있으면 깨워서). 요청 문구에 할 일의 참조
(`rocky-12` 같은)와 메모가 들어 있다.

## 받으면

1. 상세를 읽는다 — `todo_list { id }`(설명·댓글·히스토리).
2. **착수 표시** — `todo_status { id, action: "start" }`. 이걸로 이 세션이 그 할 일을 "들고 있다"고 기록되고, 보드에 처리중으로
   보인다. 이 세션의 턴 기록에도 그 할 일이 붙는다.
3. 일을 한다. 진행·질문은 설명을 덮어쓰지 말고 **댓글**로(`todo_write { id, comment }`).
4. **닫는다**:
   - 끝났으면 `todo_status { id, action: "done" }`.
   - PR 로 끝나는 일이면 `done` 대신 그 PR 주소를 할 일의 `links` 에 더한다 — 머지되면 데몬이 완료한다.
   - 손을 떼면 `action: "stop"`(다른 세션이나 사람이 이어받을 수 있게).

## 턴 끝의 확인

이 세션이 든 할 일이 열린 채 턴이 끝나면 Stop 훅이 한 번 묻는다("이 세션이 들고 있는 할 일"). 끝났으면 `done`/`stop` 으로 닫고,
**아직 하는 중이거나 사람의 답을 기다리는 중이면 아무것도 하지 않고 멈춘다** — 그 확인에 답하지 않는다. PR 을 링크한 할 일은
머지를 기다리는 중이라 묻지 않는다.

## 하지 않는 것

- 착수 표시 없이 일만 하기 — 보드에서 아무도 안 하는 것처럼 보인다.
- 끝낸 일을 doing 으로 남겨 두기 — 에이전트가 든 doing 은 세션이 사라지고 24시간 지나면 데몬이 이유를 댓글로 남기고 멈춘다.
- 할 일을 지우기 — 삭제는 없다(보관만).
