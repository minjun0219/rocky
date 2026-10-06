# 세션 띄우기 — 보드에서 세션을

> rocky 를 **고치는** 에이전트용 개발 문서.

## 규칙

- `boards.path` 에서 `claude --bg --worktree todo-<n>` 으로 띄운다. **그 워크트리에 살아 있는 세션이 있으면 띄우지 않고 그 세션을 쓴다.**
- 이중 기동은 캐시 없는 세션 목록과 `RecentSpawns` 예약이 막는다 — 예약은 확인과 잡기를 **한 락 안에서**(`try_reserve`, 앞의
  `is_recent` 와 잡기 사이에 세션 목록 await 가 있다), 요청이 끝날 때까지 진행 중으로 잡고 끝난 뒤 60초(창 안의 재요청은 409).
  확실히 아무것도 안 띄웠을 때만 되돌린다(`Reservation::release`). 경로가 키라 자기 예약만 고친다(토큰).
- `boards.path` 는 절대경로여야 하고 canonicalize 한다.
- 실행은 비동기, 30초 timeout, `kill_on_drop`. `--permission-mode` 는 넘기지 않는다.
- 프로세스를 띄우는 동작이라 **로컬 요청 전용**([security](security.md)).

## 코드

| 무엇 | 어디 |
| --- | --- |
| 띄우기·예약 | `crates/rockyd/src/spawnctl.rs` |
| 세션 목록 | `crates/rockyd/src/sessions_exec.rs`, 판정 `crates/rocky-core/src/sessions.rs` |
| 라우트 | `crates/rockyd/src/server.rs` |

테스트: `crates/rockyd/tests/it/{spawnctl_test,server_spawn_test,sessions_swr_test}.rs`.
