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
- `--full` 의 줄은 **CLI 가 그린다** — 데몬이 없어도 경로·모델·한도 줄은 남고, 보드 줄은 그 아래 한 세그먼트라 실패하면 그 줄만
  빠진다. 설정은 `rocky.json` 최상위 `statusline` 블록(보드 줄 템플릿 `todo.statusline` 과 다른 자리).
- 골든은 cc-usage(기능 동결)에서 `scripts/cc-usage-capture.ts` 로 뜬다. 일부러 다르게 둔 곳은 케이스의 `allow` 에만 적는다.
  테스트 전용 `ROCKY_STATUSLINE_NOW`(RFC3339)가 "지금" 을 고정한다 — 사용자 표면이 아니라 README 표에 올리지 않는다(cc-usage
  쪽 짝은 `CC_USAGE_NOW`).
- git 세그먼트는 `git status --porcelain=v2` 한 번, 500ms. 넘으면 **프로세스 그룹째** 끊고 그 세그먼트만 뺀다 — git 만 죽이면
  git 이 띄운 자식이 고아로 남아 1초마다 쌓인다. git 과 `extraCommands` 는 같은 실행기를 쓴다(`crates/rocky-cli/src/bounded.rs`).
- `extraCommands` 는 나란히 돌리고 출력은 **설정 순서대로** 붙인다. rocky 는 그 명령이 무엇인지 모른다 — 특정 도구를 아는
  코드가 들어오면 그 도구의 변경이 rocky 를 깬다. 본 프로세스가 끝난 뒤 100ms 안에 파이프가 닫히지 않으면(백그라운드 자식이
  stdout·stderr 를 붙잡음) 그 출력은 버린다. 출력은 바이트 그대로 붙인다(UTF-8 이 아니어도). spawn 만 뮤텍스로 한 번에
  하나씩 — macOS 에는 `pipe2` 가 없어 동시 spawn 이 파이프 끝을 남의 자식에게 넘길 수 있다. 순서: rocky 줄 → extras → 보드 줄.

## 코드

| 무엇 | 어디 |
| --- | --- |
| 세그먼트 렌더 | `crates/rocky-core/src/statusline.rs` |
| `--full` 렌더·폭·git·extra | `crates/rocky-core/src/statusline/{full,width,git,extra}.rs` |
| 한도 판정 | `crates/rocky-core/src/limits.rs` |
| CLI 입구 | `crates/rocky-cli/src/commands.rs`·`client.rs`(`rocky statusline`) |
| 하위 프로세스(마감·그룹 kill) | `crates/rocky-cli/src/{bounded,git_status}.rs` |

테스트: `crates/rocky-core/tests/it/statusline_test.rs`, `crates/rocky-cli/tests/it/statusline_cmd_test.rs`,
`crates/rockyd/tests/it/server_statusline_test.rs`, 골든 픽스처 `crates/rocky-core/tests/fixtures/cc-usage/`.
