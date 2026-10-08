# 노트 동시 편집 (CRDT) — 설계

> **현행 여부(2026-10-07)**: 구현됐다. 지금 규칙의 정본은 `docs/features/board-model.md`.

날짜: 2026-09-28 · 결정자: 오너 · 상태: 구현 중 (3 PR 스택)

## 왜

노트는 사람과 에이전트가 같이 쓰는 **스크래치 패드**다. 지금은 웹의 textarea 가 blur 때 본문
전체를 `PATCH` 로 덮어쓰고, 에이전트는 `note_write` 로 set/append 한다. 사람이 타이핑하는
동안 에이전트가 append 하면 사람의 blur 가 그것을 지운다 — 마지막에 쓴 쪽이 이긴다.
오너 결정: **실시간 공동 편집(CRDT)** 으로 간다. 글자 단위로 합쳐지고, 누가 고치고 있는지
보인다.

## 결정

1. **CRDT 는 Yjs 계열** — 웹은 `yjs`, 데몬은 `yrs`(같은 바이너리 규약). 새 런타임 의존성
   둘은 이 결정으로 승인된 것이다(레포 규칙 "의존성 추가는 별도 논의" 의 그 논의).
2. **데몬이 CRDT 피어다.** 에이전트(MCP `note_write`)·CLI(`note edit/append`)·TUI 는 Yjs 를
   모른다 — 데몬이 그들의 set/append 를 문서에 **최소 편집**(공통 접두/접미를 뺀 한 구간
   삭제+삽입)으로 넣는다. 그래서 사람이 같은 순간 다른 줄을 고쳐도 살아남는다.
3. **`notes.content` 는 읽는 쪽의 진실, `note_docs.state` 는 합치는 쪽의 진실.** 목록·TUI·
   CLI·요약은 전부 `content` 를 그대로 읽는다(변경 없음). 문서를 열 때 둘이 어긋나 있으면
   (구버전 데몬이 `content` 만 고친 경우) 문서를 `content` 에 맞춘다 — 기존 노트는 첫 열기에
   `content` 로 씨앗을 심는다. 마이그레이션은 테이블 하나(user_version 7).
4. **전송은 HTTP + 노트별 SSE.** WebSocket 을 들이지 않는다 — 이미 있는 SSE 모양을 따르고
   `tailscale serve`/Cloudflare Tunnel 을 그대로 통과한다. 갱신은 150ms 로 묶어 POST.
   - `GET  /api/notes/:ref/doc[?sv=<b64>]` → `{ update, sv }` (sv 없으면 전체 상태)
   - `POST /api/notes/:ref/doc` `{ update, client }` → 적용·저장·같은 노트 구독자에 방송
   - `GET  /api/notes/:ref/doc/events` → SSE `{kind:"update"|"presence", …}` (노트별 채널)
   - `POST /api/notes/:ref/presence` `{ client, actor, state? }` → 방송만(저장 없음)
   `/api/events`(전역)에는 문서 갱신을 싣지 않는다 — 구독자 전부가 refetch 하는 채널이라
   글자마다 보드 전체를 다시 읽게 된다.
5. **히스토리는 묶는다.** 웹 편집(문서 갱신)은 같은 actor 의 직전 `edit` 기록이 60초 안에
   있으면 새 줄을 남기지 않는다. set/append(에이전트·CLI)는 지금처럼 `update` + 본문 diff.
   `/api/changes` → `notify-todo` 주입도 그 규칙을 따른다(글자마다 세션에 알리지 않는다).
6. **웹 편집기는 둘 다 만들어 보고 하나를 지운다**(오너 결정): (a) textarea 유지 + 직접 짠
   바인딩 + 프레즌스 표시, (b) CodeMirror 6 + `y-codemirror.next` + 상대 커서. 카드의
   토글로 번갈아 써 본 뒤 결정.

## 스택

| PR | 층 | 내용 |
|---|---|---|
| 1 | 데몬 | `rocky_core::note_doc`(순수, yrs) · `note_docs` 테이블 · set/append 가 문서를 거침 · 4 라우트 · 노트별 SSE · 사용 로그 표면 |
| 2 | 웹 (a) | `yjs` · `web/notedoc.ts`(동기화 클라이언트) · textarea 바인딩 · 프레즌스 |
| 3 | 웹 (b) | CodeMirror 6 + `y-codemirror.next` · awareness 를 presence 라우트에 실음 · 편집기 토글 |

## 경계

- 제목(`title`)은 CRDT 가 아니다 — 짧고 드물어 지금의 `PATCH` 로 충분하다.
- 오프라인 편집·되돌리기(undo 스택)는 범위 밖. Yjs 가 가능하게는 하지만 지금 필요 없다.
- MCP 도구 수는 그대로(5) — 에이전트 표면은 바뀌지 않는다.

## 실패 처리

- 저장된 state 가 깨져 못 읽으면 `content` 로 새 문서를 만든다(데이터 손실 없음 — content 가 진실).
- 클라이언트의 update 가 깨지면 400. SSE 가 끊기면 클라이언트가 자기 state vector 로 `GET doc?sv=`
  를 다시 불러 빠진 것을 받는다(Yjs 의 diff 동기화).
