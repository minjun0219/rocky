# statusline — 한 줄 렌더

> rocky 를 **고치는** 에이전트용 개발 문서. 설정 방법은 [`docs/board.md`](../board.md) "statusline 에 얹기",
> `--full` 의 cc-usage 이식 설계는 [`2026-10-05-cc-usage-mirror-design.md`](../design/specs/2026-10-05-cc-usage-mirror-design.md).

## 규칙

- **`GET /api/statusline` 은 한 줄 전체를 데몬이 렌더링한다.** 이 라우트만 세션 캐시 TTL 이 15초. 실패하면 빈 문자열. 보드는
  `board_key_for_cwd` 로 정한다.
- **끼워 넣는 쪽은 `rocky statusline`**(`--cwd`·`--session`, 없으면 stdin JSON). 1초마다 도는 자리라 사용 로그·데몬 자동 기동을
  거치지 않고, 300ms 안에 못 받으면 조용히 빈 출력.
- `--full`(경로·git 줄 + 모델·ctx·한도 줄)은 cc-usage 의 이식이다 — 같은 입력이면 ANSI 까지 **같은 바이트**를 낸다(골든 픽스처
  `crates/rocky-core/tests/fixtures/cc-usage/`). 한도(5h/7d) 판정은 순수 함수, "지금"은 인자로 받는다.

## 코드

| 무엇 | 어디 |
| --- | --- |
| 세그먼트 렌더 | `crates/rocky-core/src/statusline.rs` |
| `--full` 렌더·폭·git | `crates/rocky-core/src/statusline/{full,width,git}.rs` |
| 한도 판정 | `crates/rocky-core/src/limits.rs` |
| CLI 입구 | `crates/rocky-cli/src/commands.rs`·`client.rs`(`rocky statusline`) |

테스트: `crates/rocky-core/tests/it/statusline_test.rs`, `crates/rocky-cli/tests/it/statusline_cmd_test.rs`,
`crates/rockyd/tests/it/server_statusline_test.rs`, 골든 픽스처 `crates/rocky-core/tests/fixtures/cc-usage/`.
