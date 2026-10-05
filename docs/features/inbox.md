# 수집함 — 외부 투두 읽기·이미 올라감·구독

> rocky 를 **고치는** 에이전트용 개발 문서. 어댑터 규약(stdout JSON)은 [`docs/board.md`](../board.md), 설계는
> [`2026-09-27-bridges-and-tui-design.md`](../design/specs/2026-09-27-bridges-and-tui-design.md).

## 규칙

- **외부 앱은 동기화하지 않는다.** rocky 는 수집함 어댑터 규약(`todo.inbox[]` → 명령 → stdout JSON → `GET /api/inbox`)으로 읽기만
  하고 링크로 참조한다. 어댑터 코드는 `bridges/<name>/` 에만 — 서비스 이름이 `crates/`, `plugin/`, 매니페스트 keywords, MCP 도구에
  나오면 범위 위반이다(AGENTS.md *범위*).
- **"이미 올라감"은 데몬 한 곳에서 판정한다**(`mark_promoted`): 항목 url 이 **어느 보드든**(보관 포함) todo 링크에 있으면
  `promoted`. 요약·웹·`rocky inbox` 가 이 값을 본다 — 소비자마다 다시 판정하지 않는다.
- 세션 시작 요약은 **수집함 캐시만** 본다(어댑터를 기다리지 않는다). 📥 제목은 외부 글이라 한 줄로 펴서 자른다.
- **보드 수집함은 "명령은 설정 파일(`todo.inboxAdapters[]`), 값은 화면"** 이다 — 화면 값은 어댑터의 `--describe` 칸으로만 검증해
  받고(`validate_params`), 명령 자체를 화면이 바꾸게 하지 않는다. 쓰기는 로컬 전용([security](security.md)).
- **수집함 구독**(`rocky inbox subscribe`)은 세션을 소스의 구독자로 적고(`inbox_subscriptions`, 기준선은 `inbox_seen`),
  `rockyd::inbox_watch` 가 구독된 소스만 5분마다 읽어 새 항목을 그 세션 받은편지함에 보낸다 — 알리기만, 착수는 사람.

## 코드

| 무엇 | 어디 |
| --- | --- |
| 규약·검증·이미 올라감 | `crates/rocky-core/src/inbox.rs`, 요약 `crates/rocky-core/src/summary.rs` |
| 어댑터 실행 | `crates/rockyd/src/inbox_exec.rs` |
| 구독 감시 | `crates/rockyd/src/inbox_watch.rs` |
| 참조 어댑터 | `bridges/file/` |

테스트: `crates/rocky-core/tests/it/{inbox_test,summary_test}.rs`,
`crates/rockyd/tests/it/{server_inbox_test,server_board_inbox_test,inbox_subscribe_test}.rs`, 어댑터는 `bridges/*/*.test.ts`.
