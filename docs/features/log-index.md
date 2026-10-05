# 로그 색인 — JSONL 을 logs.db 로

> rocky 를 **고치는** 에이전트용 개발 문서. 설계: [`2026-10-02-log-index-design.md`](../design/specs/2026-10-02-log-index-design.md).

## 규칙

- **JSONL 이 진실이다.** 작업로그·사용 로그는 파일에 쓰고, 데몬은 그걸 `logs.db`(todo 폴더, `todo.db` 와 별도 파일)로 옮겨 읽기만
  한다 — 쓰기 경로는 그대로다(데몬이 꺼져도 기록이 남는다). *EN: never route writes through the daemon.*
- **전용 OS 스레드**가 기동 때와 1분마다 파일별 바이트 위치로 새 줄만 옮긴다(tokio 워커·보드 DB 잠금과 겹치지 않게). 작업로그는
  `id`, 사용 로그는 `(파일, 위치)` 가 키라 다시 읽어도 중복이 없다. 마지막 줄이 `\n` 없이 끝나면 아직 쓰는 중이라 다음 바퀴로 미룬다.
- **지워도 다시 만든다** — 색인은 파생물이다.
- 조회는 `GET /api/logs/worklog`·`/api/logs/stats` — `spawn_blocking` + 자기 연결.
- 보드 ↔ 레포는 보드 `path` 로 `default_project_key` 를 계산한다(워크트리는 레포 루트로 접힌다).
- 같은 스레드가 [토큰 색인](tokens.md)도 돈다.

## 코드

| 무엇 | 어디 |
| --- | --- |
| 색인·조회 | `crates/rocky-core/src/logindex.rs` |
| 스레드 | `crates/rockyd/src/logindex.rs` |
| 작업로그 쓰기(진실) | `crates/rocky-core/src/worklog.rs`, 사용 로그 `crates/rocky-core/src/usage.rs` |

테스트: `crates/rocky-core/tests/it/logindex_test.rs`, `crates/rockyd/tests/it/logindex_test.rs`.
