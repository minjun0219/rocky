# 보드 모델 — 보드 메타·번호 참조·노트 CRDT

> rocky 를 **고치는** 에이전트용 개발 문서. 보드를 **쓰는** 법은 `plugin/skills/board`, 사람용 설명은
> [`docs/board.md`](../board.md). 계약(REST·MCP 모양)은 [`docs/rewrite/contract.md`](../rewrite/contract.md) — 골든 테스트가 고정한다.

## 규칙

- **보드 메타**(`update_board`)는 key/title/description/repo/path/reviewFix/prAuthors 를 한 트랜잭션으로 고친다. `null` 은 지우기,
  빈 문자열은 400. key 를 바꾸면 옛 key 를 `board_aliases` 에 남긴다(입력 전용 — 출력은 늘 새 key, 쓴 key 는 은퇴).
  `match_board` 는 현재 key 만 본다. 별칭까지 풀어야 하는 조회는 `get_board`.
- **번호 참조(ref).** todo/note 는 보드별 번호를 갖는다: `rocky-12` → `12`(보드 맥락) → id 정확 일치 → id prefix 순으로 푼다
  (`resolve_ref_id`), 가장 오른쪽 `-` 에서 가른다. 옛 `#12` 표기는 입력 전용. `note-N` 은 늘 전역 메모. 번호는 재사용하지
  않는다. 댓글에는 번호가 없다.
- **노트는 CRDT 문서다**(`yrs`): 데몬이 CRDT 피어라 에이전트와 CLI 는 Yjs 를 모른다 — `set` 은 최소 편집, `append` 는 끝에 삽입.
  - `notes.content` 는 읽는 쪽의 진실, `note_docs.state` 는 합치는 쪽의 진실. state 가 전진했을 때만 저장하고, 본문이 바뀌었을
    때만 content·히스토리를 고친다. 셋은 한 트랜잭션.
  - 스토어가 `NoteDocEvent` 를 내고 서버가 노트별로 방송한다(라우트는 방송하지 않는다). 웹은 노트 소켓 하나(`GET /api/ws`)로
    오가고 밀리면 `lag` 로 차분을 다시 받게 한다. HTTP 라우트·노트별 SSE(밀리면 끊는다)는 폴백.
  - 제목은 CRDT 가 아니다. 고정(`pinned_at`, REST `pin`/`unpin`·CLI)은 보여 주는 방식이라 MCP 도구로는 바꾸지 않는다(출력의
    `pinnedAt` 은 실린다).
- 삭제는 없다 — 보관(archive)만. 모든 변경은 히스토리에 남는다.

## 코드

| 무엇 | 어디 |
| --- | --- |
| 스토어(보드·todo·note·ref·히스토리) | `crates/rocky-core/src/store.rs`, 스키마 `crates/rocky-core/src/migrations.rs` |
| ref 표시 | `crates/rocky-core/src/refs.rs` |
| 노트 CRDT | `crates/rocky-core/src/note_doc.rs`, 소켓 `crates/rockyd/src/ws.rs` |
| REST·MCP 표면 | `crates/rockyd/src/server.rs`, `crates/rockyd/src/mcp.rs` |

테스트: `crates/rocky-core/tests/it/{store_test,refs_test,note_doc_test,migrations_test}.rs`,
`crates/rockyd/tests/it/{server_rest_test,mcp_test,note_doc_test,ws_test}.rs`. MCP 도구 목록은 `mcp_test.rs` 의 `TOOLS` 가 정확히
고정한다. 설계: [`docs/design/specs/2026-09-28-note-crdt-design.md`](../design/specs/2026-09-28-note-crdt-design.md).
