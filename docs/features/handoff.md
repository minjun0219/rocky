# 핸드오프와 doing 귀속 — 보드 → 세션

> rocky 를 **고치는** 에이전트용 개발 문서. 받은 세션이 **어떻게 행동하나**는 사용자 스킬
> [`plugin/skills/handoff`](../../plugin/skills/handoff/SKILL.md), 근거는
> [`docs/daemon.md`](../daemon.md).

## 규칙

- **큐.** 데몬은 `handoffs` 큐에 쌓고, `Stop` 훅이 한 번에 하나씩 집고 `UserPromptSubmit` 이 턴 시작에 본다. 보관된 todo 는
  건너뛴다. TTL 은 없다. 핸드오프는 MCP 도구를 늘리지 않는다.
- **쉬는 세션은 깨워야 한다.** 대상 세션이 받은편지함 소켓을 등록했으면 데몬이 그 문구(`poke`)를 바로 꽂아 턴을 연다(응답
  `woke: true`, "보내지 않기" 와 무관). 아니면 라우트가 `poke` 를 돌려준다 — 그 문구를 늘리지 않는다.
- **대상** = cwd 가 보드와 맞는 세션이 하나일 때 그 세션. 아니면 사용자가 고른다. 잠든(`blocked`)·끝난(`done`) background
  세션은 자동 대상에서 뺀다(`takes_handoff`) — 잠든 세션의 cwd 는 레포 루트라 보드와 맞지만 넘긴 일을 집지 않는다.
- **doing 귀속.** `start` 가 가장 오래된 배달 건을 수락하고 `doing_session_id` 를 귀속시킨다. `done` 이 완료하고 비운다. 사람이
  누른 `start` 는 귀속하지 않는다. 세션이 MCP `todo_status` 로 **스스로** `start` 한 것은 PostToolUse 훅 `claim-doing` 이
  `POST /api/sessions/doing`(로컬 전용)으로 귀속시키고 `doing_session_claimed` 로 표시한다 — 응답의 doing 할 일 id 만, 데몬이 다시
  조건(에이전트가 10분 안에 시작·귀속 없음·훅이 준 `doingSince` 가 지금 착수와 같음)을 본다. 이 귀속은 doing 상태 판정
  (`resolve_doing_state` — 방치·멈춤·자동 해제·이어받기 추천)에 **쓰지 않는다** — 그 세션은 Stop 에서 묻지 않으므로 턴마다 `idle`,
  `/clear` 뒤엔 `gone` 이 된다. 판정은 귀속 없는 doing 과 같은 보드 근사. HTTP MCP 는 호출한 세션을 모르므로(헤더 없음, 환경 변수의 세션 id 는 `/clear`
  뒤 낡는다) 훅 입력의 `session_id` 가 유일한 정확한 단서다. `resolve_doing_state` → `live` / `idle` / `gone` / `unknown`.
  - 세션 목록의 **pid 없는 background 행도 세션이다.** 사람 답을 기다리며 잠든(`state: blocked`) 세션은 `pid`·`status` 없이
    온다(Claude Code 2.1.289, cwd 는 워크트리가 아니라 레포 루트) — 버리면 그 doing 이 `gone` 이 되어 자동 해제된다. `blocked` 는
    `idle` 로 읽는다.
- **자동 해제.** `rockyd::sweep` 는 에이전트가 든 `gone` doing 중 24시간 지난 것만 자동으로 멈추고 이유를 댓글로 남긴다
  (`should_auto_release`). 사람이 든 것·`idle`·`unknown` 은 건드리지 않는다.
- **닫았는지 묻기.** 핸드오프 주입문은 착수(`start`)와 함께 닫는 법(`done`/`stop`)을 말한다. `Stop` 훅(`handoff-stop`)은 이 세션에
  귀속된 doing 이 있으면 턴을 한 번 막고 닫았는지 묻는다(`held_todo_reminder`). 스스로 든 것(`doing_session_claimed`)은 묻지 않는다
  — 오너 결정(2026-10-06): 그 귀속은 statusline ⏺ 와 턴 태그에만 쓴다.
  - `stop_hook_active` 인 턴은 다시 막지 않는다(루프 없음).
  - GitHub PR 을 링크한 할 일은 머지를 기다리는 중이라 묻지 않는다(그 PR 이 머지 없이 닫혀도 다시 묻지 않는다 — 보드 댓글로만 안다).
- **턴 기록과 잇기.** 같은 귀속으로 `log-turn` 은 턴 기록 태그에 `todo:<ref>` 를 붙인다(보드 할 일 상세가 작업 흐름을 이 태그로
  모은다). 하네스가 넣은 메시지(`<task-notification>`·셸 출력)는 턴을 나누되 요청 칸엔 짧은 이름만 남긴다(`label_injected`).

## 코드

| 무엇 | 어디 |
| --- | --- |
| 큐·배달·주입문 | `crates/rocky-core/src/handoff.rs`, 받은편지함 문구 `crates/rocky-core/src/peer_inbox.rs` |
| doing 상태 판정 | `crates/rocky-core/src/doing.rs` |
| 자동 해제 잡 | `crates/rockyd/src/sweep.rs` |
| 훅(`handoff-stop`·`notify-todo`·`log-turn`·`claim-doing`) | `crates/rocky-cli/src/hooks.rs`, 턴 추출 `crates/rocky-core/src/transcript.rs` |

테스트: `crates/rocky-core/tests/it/{handoff_test,doing_test,peer_inbox_test,transcript_test}.rs`,
`crates/rockyd/tests/it/{server_handoff_test,sweep_test}.rs`, `crates/rocky-cli/tests/it/hooks_test.rs`.
