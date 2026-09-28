---
"@minjun0219/rocky": minor
---

노트 본문이 CRDT(Yjs 호환) 문서가 된다 — 사람과 에이전트가 같은 메모를 동시에 고쳐도 서로 지우지 않고 글자 단위로 합쳐진다. 에이전트·CLI 의 set/append 는 데몬이 최소 편집으로 문서에 넣고, `notes.content` 는 늘 합쳐진 최신 본문이다(기존 노트는 처음 열 때 지금 본문으로 문서를 만든다). 웹·다른 클라이언트용 라우트 `GET/POST /api/notes/:ref/doc`, `GET …/doc/events`(노트별 SSE), `POST …/presence`. 웹 편집 히스토리는 60초 창으로 묶인다.
