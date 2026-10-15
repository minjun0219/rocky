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
- **깨우기는 SQLite 훅이다.** `update_hook`이 대상 테이블의 변경을 표시하고 `commit_hook`이 커밋마다 한 번 구독자를 부른다
  (`subscribe_rows`, 롤백이면 표시를 지운다). 데몬은 `watch` 값만 올린다. 변경 이벤트(`ChangeEvent`)에 기대지 않는다. 모든
  쓰기가 그 이벤트를 내지는 않는다 — 같은 노트를 2초 안에 다시 고치면 히스토리도 이벤트도 남지 않는다. 그쪽에 기대면 바뀐
  행이 다음 이벤트까지 묶인다. 훅은 커밋 도중(연결 락 안)에 불리므로 리스너는 신호만 남기고 스토어를 다시 부르지 않는다.
- **`/api/ws`의 `sync` 프레임을 받은 연결에만 민다.** 신호를 받으면 20ms 모았다가 그 연결에 보낸 순번 이후를 읽는다(읽기는
  `spawn_blocking`). 이미 보낸 변경이면 보내지 않는다. 방송 채널이 아니므로 밀린 연결도 다음 차례에 한꺼번에 받는다. 읽기
  전에 신호를 본 것으로 표시해서, 읽은 뒤의 변경은 다음 신호가 다시 읽는다. 밀다가 읽기가 실패하면 `err`를 한 번 보내고
  밀기를 멈춘다(클라이언트가 `sync`로 다시 시작한다).

## 코드

| 무엇 | 어디 |
| --- | --- |
| 대상 테이블 · 행 타입 · 통째 판정(순수) | `crates/rocky-core/src/sync.rs` |
| `sync_state` · `seq` 열 · 트리거 | `crates/rocky-core/src/migrations.rs`의 `add_sync_seq`(19) |
| 머리 · 바뀐 행 읽기 · `epoch` 갱신 | `crates/rocky-core/src/store.rs`의 `sync_head` · `sync_rows_since` · `renew_sync_epoch` |
| 행 변경 구독(`update_hook`) | `crates/rocky-core/src/store.rs`의 `with_conn` · `subscribe_rows` |
| 변경 신호 · 소켓 프레임 | `crates/rockyd/src/server.rs`의 `sync_rows`, `crates/rockyd/src/ws.rs` |

테스트: `crates/rocky-core/tests/it/sync_test.rs`, `crates/rockyd/tests/it/ws_test.rs`(보드 싱크 절).
