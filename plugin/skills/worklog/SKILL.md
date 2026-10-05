---
name: worklog
description: Use when a session should leave or consult a durable per-repo record across turns and sessions — recording a decision, a blocker, the user's answer to a question, or a note worth finding later ("이거 기록해 둬", "지난번에 왜 이렇게 했지?", "전에 비슷한 거 결정한 적 있어?"). Covers rocky's worklog MCP tools (worklog_append / worklog_read / worklog_search / worklog_status), what the Stop hook already captures automatically, and the split between the board (what to do) and the worklog (what happened).
---

# 워크로그 — 무엇이 있었나를 남긴다

**보드는 "무엇을 할까", 워크로그는 "무엇이 있었나"** 다. 워크로그는 레포마다(워크트리는 레포 루트로 접힌다) append-only JSONL 이다.

## 저절로 남는 것

`Stop` 훅이 매 턴을 `kind: "turn"` 한 줄로 남긴다(요청·쓴 도구·마지막 답의 앞부분, LLM 없이). 이 세션이 할 일을 들고 있으면
`todo:<ref>` 태그가 붙는다. 그러니 턴 요약을 손으로 쓰지 않는다.

## 손으로 남길 것

턴 기록에서 다시 찾기 어려운 **의미 있는 순간**만 `worklog_append` 로:

| kind | 언제 |
| --- | --- |
| `decision` | 갈래 중 하나를 골랐다 — 무엇을, 왜, 버린 쪽 |
| `blocker` | 막혔다 — 무엇이, 무엇을 기다리나 |
| `answer` | 사용자가 질문에 답했다 — 다음에 또 묻지 않게 |
| `note` | 나중에 찾을 사실(측정값·경로·명령) |

한 줄은 짧게, 다시 읽을 사람이 맥락 없이 알아듣게 쓴다. 기존 줄은 고치거나 지우지 않는다.

## 찾기

- `worklog_search { query }` — 같은 주제의 과거 결정을 먼저 찾는다(설계를 다시 하기 전에).
- `worklog_read { kind, since, limit }` — 최근 결정·blocker.
- `worklog_status` — 이 레포의 키·항목 수.
- 쌓인 턴을 정리하는 건 `/rocky:recall`(다이제스트 한 줄을 남긴다).

## 하지 않는 것

- 할 일 관리를 워크로그로 하지 않는다 — 그건 보드(`todo_*`).
- 비밀(토큰·비밀번호)을 남기지 않는다.
