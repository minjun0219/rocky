# 싱크 — 행 순번과 바뀐 행 주고받기

> rocky 를 **고치는** 에이전트용 개발 문서. 설계와 근거는
> [`docs/design/specs/2026-10-15-dual-structure-sync-design.md`](../design/specs/2026-10-15-dual-structure-sync-design.md).

## 규칙

- **순번은 트리거가 매긴다.** `sync::SYNC_TABLES`(boards · sections · todos · comments · notes · history · handoffs)마다
  `AFTER INSERT`·`AFTER UPDATE` 트리거가 `sync_state.seq`를 1 올려 그 행의 `seq`에 적는다(마이그레이션 19). 쓰기 코드는
  `seq`를 직접 쓰지 않는다. 새 쓰기 경로를 더해도 순번은 저절로 붙는다.
- `seq`를 바꾸는 `UPDATE`는 트리거가 건너뛴다(`WHEN NEW.seq IS OLD.seq`). 그래서 트리거가 자기를 다시 부르지 않는다. 같은
  이유로 `seq`를 손으로 고친 변경은 퍼지지 않는다.
- **대상 테이블을 늘리면 새 마이그레이션으로 열과 트리거를 더한다.** 마이그레이션 19 는 고치지 않는다(이미 19 까지 간 DB 는
  다시 돌지 않는다). `SYNC_TABLES`, `SyncTables`, `TodoStore::sync_rows_since`도 함께 고친다. 트리거가 빠지면
  `sync_test`의 이름 검사가 실패한다.
- **`epoch`는 스토어가 DB 를 열 때마다 새로 정한다.** 데몬이 다시 뜨면(DB 를 백업에서 되살린 경우 포함) 클라이언트가 옛
  사본에 순번을 이어 붙이지 않고 통째로 다시 받는다. 클라이언트가 다른 `epoch`, 음수 `since`, 머리보다 큰 `since`를 들고
  오면 통째로 준다(`sync::needs_full`). 마이그레이션 전부터 있던 행은 순번이 0이라 통째로 줄 때는 0도 넣는다.
- **통째 사본의 히스토리는 최근 `FULL_HISTORY_LIMIT`(2,000)줄까지다.** 히스토리는 쓰기마다 늘어 끝이 없다. 이어 받기는
  자르지 않는다.
- 행 모양은 지금 타입의 JSON에 `seq`를 더한 것이다(`Synced<T>`, `serde(flatten)`). 행 밖에서 세는 값(`ref`, `commentCount`)과
  세션에서 계산하는 값(`doingState`, `stale`)은 싣지 않는다. 보드의 `previousKeys`는 싣는다. 별칭은 키를 바꿀 때만 생기고
  그때 보드 행도 함께 바뀐다(`update_board`).
- 머리와 행은 같은 락 안에서 읽는다. 돌려준 `seq`까지의 변경이 빠짐없이 들어 있다.

## 코드

| 무엇 | 어디 |
| --- | --- |
| 대상 테이블 · 행 타입 · 통째 판정(순수) | `crates/rocky-core/src/sync.rs` |
| `sync_state` · `seq` 열 · 트리거 | `crates/rocky-core/src/migrations.rs`의 `add_sync_seq`(19) |
| 머리 · 바뀐 행 읽기 · `epoch` 갱신 | `crates/rocky-core/src/store.rs`의 `sync_head` · `sync_rows_since` · `renew_sync_epoch` |

테스트: `crates/rocky-core/tests/it/sync_test.rs`.
