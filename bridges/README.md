# bridges/ — 수집함 어댑터

외부 투두 앱을 **읽는** 명령들. `rocky.json` 의 `todo.inbox[]` 에 등록하면 데몬이 실행해
`GET /api/inbox` 로 합쳐 준다. 동기화가 아니다 — 읽어서 보여주고 링크로 참조만 한다.

규약(입출력 형식·실패 처리)의 정본은 [`docs/board.md` "수집함"](../docs/board.md#수집함--외부-투두-앱-읽기-todoinbox--get-apiinbox).
설계 근거는 [`docs/design/specs/2026-09-27-bridges-and-tui-design.md`](../docs/design/specs/2026-09-27-bridges-and-tui-design.md).

| 디렉터리 | 무엇 |
| --- | --- |
| `file/` | JSON 파일을 그대로 내는 참조 구현 — 테스트·수동 확인용 |

새 어댑터는 `bridges/<name>/` 에 두고, 토큰은 1Password Agent Vault 에서 `op read` 로 읽는다
(홈의 평문 파일 금지). 공개 레포이므로 계정 식별자를 코드에 박지 않는다.
