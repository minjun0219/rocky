# doctor — 설치·설정 + 실행 상태 점검

> rocky 를 **고치는** 에이전트용 개발 문서. 쓰는 법은 [`docs/board.md`](../board.md) 의 `rocky config show` · `rocky doctor` 절.

## 규칙

- **읽기만 한다.** 고치는 명령(`rocky daemon restart` · `rocky rc start` …)은 항목의 `fix` 안내로만 낸다 — doctor 가 직접 실행하지 않는다.
- **꺼진 데몬을 띄우지 않는다.** `request_value` 는 꺼진 데몬을 자동으로 띄우므로, 데몬 상태(`gather` 의 `input.daemon`)를 먼저 보고 떠 있을 때만
  묻는다. 꺼져 있으면 실행 상태는 `null`(JSON)·"닿지 못함"(사람용)으로 남긴다.
- **못 받은 항목은 문제가 아니라 건너뛴다.** 옛 데몬이 싣지 않은 필드, 로컬 전용 라우트의 403(`{ "error" }` 본문), 요청 실패는 `None` 이고
  그 항목은 아예 나오지 않는다 — 모르는 것을 ⚠ 로 치지 않는다.
- **판정은 `rocky_core::doctor` 의 순수 함수**(`runtime_checks`, "지금" 은 인자) — I/O 는 `rocky_cli::doctor_cmd` 가 한다. 설치·설정 점검은
  `rocky config show` 와 같은 `setup::build_report` 를 쓴다(`config_cmd::gather` 공유) — 따로 계산하면 둘이 어긋난다.
- 임계값: PR 감시 마지막 tick 15분(`PR_WATCH_STALE_SECS`, 감시 주기 3분의 5배), 세션 전달 실패 24시간(`DELIVERY_FAILURE_WINDOW_SECS`).
  PR 감시 주기를 바꾸면 앞의 것도 같이 본다.

## 코드

| 무엇 | 어디 |
| --- | --- |
| 실행 상태 판정·렌더 | `crates/rocky-core/src/doctor.rs` |
| 재료 수집·CLI 입구 | `crates/rocky-cli/src/doctor_cmd.rs` |
| 설치·설정 재료(`config show` 와 공유) | `crates/rocky-cli/src/config_cmd.rs`(`gather`) |

테스트: `crates/rocky-core/tests/it/doctor_test.rs`(항목별 판정·건너뛰기·렌더).
