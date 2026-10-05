# cc-usage 미러 — statusline 을 rocky 로 옮기고 같은 출력을 낸다

2026-10-05. 오너 요청: cc-usage(Go, 별도 공개 레포)의 기능을 rocky(Rust)로 가져온다. 기존 cc-usage 는 그대로 두고
둘을 나란히 돌리며 기능을 맞추고, rocky 가 대체할 수 있다고 판단되면 그때 cc-usage 아카이브를 고려한다.

## 결정

- **cc-usage 는 동결한다.** 버그 수정만 받고 기능은 더하지 않는다. 목표가 움직이지 않아야 "같다" 를 판정할 수 있다.
  새 기능은 대체 판정 뒤 rocky 에만 넣는다.
- **대체 기준 = 바이트 일치 + 실사용.** 같은 stdin·시각·캐시·환경에서 두 바이너리의 출력이 ANSI 까지 같고, rocky 를
  statusLine 으로 1주 써서 문제가 없으면 대체할 수 있다고 본다. 일부러 다르게 둔 곳은 픽스처의 허용 목록에 적는다.
- **미러 대상에서 뺀 것:** cc-usage 의 `update` · `config` · `version` — rocky 에 같은 일을 하는 명령(`rocky update` ·
  `rocky config`)이 있다. guard · agy 는 지금 쓰지 않아도 "기능 동일" 에 넣는다(조각 5).

## 프로세스 모델

**CLI 가 렌더하고 데몬은 갱신만 맡는다.** cc-usage 의 불변 조건을 그대로 옮긴다.

| cc-usage | rocky |
|---|---|
| `cc-usage statusline` (매 호출 새 프로세스) | `rocky statusline --full` — 경로·git·모델·ctx·한도를 CLI 가 직접 그린다 |
| `cc-usage refresh` (detached + lock 파일 + backoff) | `rockyd` 의 주기 태스크(조각 3). 데몬이 머신마다 하나라 lock 이 필요 없다 |
| `usage.json` writer = `refresh`, `state.json` writer = `statusline` | writer 분리 그대로 — usage 는 데몬, state 는 CLI |
| 판정 로직 `internal/core` (`now` 인자) | `rocky_core::limits` (순수, `now` 인자) |
| 렌더 `internal/render` | `rocky_core::statusline` 확장 |

- statusline 은 네트워크를 기다리지 않는다. 기존 보드 줄(데몬 HTTP, 300ms)은 extra 와 같은 지위의 한 세그먼트다 —
  실패하면 그 줄만 빠진다. 데몬이 죽어도 경로·git·한도 줄은 남는다.
- 하위 프로세스(`git`, extra 명령)는 타임아웃을 걸고 **프로세스 그룹째** 끊는다.
- 입력 필드는 모두 optional — 파싱 실패로 줄이 비지 않는다. 에러 문구를 statusline 에 찍지 않는다.
- **이름**: 사용 로그가 `rocky_core::usage` · `rocky usage` 를 쓰고 있어 한도 쪽은 `limits` 로 부른다.
- **플래그 `--full`**: 인자 없는 `rocky statusline`(보드 줄만)을 쓰는 설정을 깨지 않으려고 따로 둔다. 대체 판정 뒤
  기본값으로 올릴지 다시 정한다.
- **설정**: `rocky.json` 최상위 `statusline` 블록(보드 줄 템플릿 `todo.statusline` 과는 다른 자리). cc-usage 의 `config.json` 은 읽지 않는다(아카이브 뒤에도 남을 자리).
  필드는 조각마다 필요한 것만 더한다 — `rocky.schema.json` 과 `crates/rocky-core/src/config.rs` 를 함께.

## 조각

| # | 조각 | 내용 |
|---|---|---|
| 1 | 대조 하네스 + 1~2줄 렌더 | `source: stdin` / `none`. 입력 파싱, git 세그먼트, 남은 비율·리셋 표기, 7d 70% 임계, `alert_percent` 고정 배지, 3단 색, 폭 판단(`COLUMNS - 40`) |
| 2 | `extra_commands` | argv 배열, placeholder(`{{cwd}}` · `{{session_id}}`), 타임아웃. rocky 보드 줄은 내부 세그먼트 |
| 3 | 크레딧 + usage API 갱신 | `rockyd` 태스크, keychain(`/usr/bin/security`) 읽기 전용, 한도별 폴링 간격, backoff·`Retry-After`, `source: api` / `auto` |
| 4 | 경보 깜빡임 · 계정 배지 | `state.json` 의 경보 시각, `.claude.json` 이메일 → 배지 |
| 5 | guard / allow · probe / doctor · agy | guard 는 fail-open |

다중 계정(cc-usage 는 계정마다 프로세스·설정·캐시를 나눈다)은 조각 3 에서 정한다 — rockyd 는 전역에 하나라
`config_dir` 별로 상태를 나눠 들어야 한다.

## 대조 하네스

- cc-usage 의 테스트 전용 환경 변수 `CC_USAGE_NOW`(RFC3339)로 시각을 고정한다(cc-usage#14). rocky 도 같은 이름의
  테스트 전용 변수를 읽는다 — 한 픽스처를 두 바이너리에 그대로 넣기 위해서다.
- 픽스처 한 건 = `stdin.json` + 환경(`CC_USAGE_NOW` · `TZ` · `COLUMNS` · `TERM` · `NO_COLOR`) + 설정(두 바이너리용) +
  캐시 파일(조각 3 부터) + `expected`(cc-usage 출력). 위치는 `crates/rocky-core/tests/fixtures/cc-usage/<케이스>/`.
- `scripts/cc-usage-capture.ts`(Bun)가 로컬 cc-usage 바이너리로 `expected` 를 한 번 떠서 커밋한다. 격리는 cc-usage 의
  `examples/preview.sh` 와 같다 — 임시 `HOME` · `XDG_CACHE_HOME`, 없는 keychain 항목, 없는 credentials 파일.
- `crates/rocky-core/tests/it/cc_usage_parity_test.rs` 가 판정·렌더를 다시 돌려 `expected` 와 바이트 단위로 비교한다.
  CI 에는 Go 가 필요 없다. cc-usage 버그 수정으로 출력이 바뀌면 다시 뜬다. 하위 프로세스가 끼는 것(git 세그먼트,
  extra)은 CLI 를 프로세스째 돌리는 테스트(`crates/rocky-cli/tests/it/`)가 맡는다.
- 케이스의 정본은 캡처 스크립트의 `CASES` 다 — 기준 시각에서 몇 분 뒤 같은 상대값으로 쓰고, 스크립트가 절대값으로 풀어 적는다.
- Go 의 버릇도 따라간다. 예: 타입이 어긋난 숫자 필드(`"used_percentage": "41"`)는 비지 않고 0 이 된다 — Go 디코더가
  포인터를 먼저 할당하고 나서 타입 에러를 내기 때문이다.
- 케이스는 cc-usage 의 Go 테스트와 `examples/preview.sh` 의 입력에서 가져온다.

## 검증

- 조각마다: 골든 diff 전부 통과, `rocky_core` 단위 테스트, 시간 측정 — cc-usage `AGENTS.md` 의 기준선(전체 23.1ms,
  코어 14.4ms)보다 눈에 띄게 느리면 이유를 찾는다.
- 조각 1~4 가 끝나면 statusLine 을 `rocky statusline --full` 로 바꿔 1주를 쓴다. 그 뒤 대체 판정과 cc-usage 아카이브를
  따로 정한다.

## PR

조각 1 은 셋으로 나눈다: cc-usage#14(`CC_USAGE_NOW`) → 캡처 스크립트 + 픽스처 + `rocky_core::limits` · 렌더 →
CLI `--full` 배선 + git + 설정 블록 + changeset. rocky 쪽 둘은 스택이다.
