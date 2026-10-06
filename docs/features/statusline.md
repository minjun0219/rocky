# statusline — 한 줄 렌더

> rocky 를 **고치는** 에이전트용 개발 문서. 설정 방법은 [`docs/board.md`](../board.md) "statusline 에 얹기",
> `--full` 의 cc-usage 이식 설계는 [`2026-10-05-cc-usage-mirror-design.md`](../design/specs/2026-10-05-cc-usage-mirror-design.md).

## 규칙

- **`GET /api/statusline` 은 한 줄 전체를 데몬이 렌더링한다.** 이 라우트만 세션 목록을 15초마다 새로 받고, 만료된 값은 기다리지 않고 내주며 뒤에서
  새로 받는다(SWR — 요청이 `claude agents` 를 기다리면 CLI 300ms 마감에 걸린다). 실패하면 빈 문자열. 보드는
  `board_key_for_cwd` 로 정한다.
- **끼워 넣는 쪽은 `rocky statusline`**(`--cwd`·`--session`, 없으면 stdin JSON). 1초마다 도는 자리라 사용 로그·데몬 자동 기동을
  거치지 않고, 300ms 안에 못 받으면 조용히 빈 출력.
- `--full`(경로·git 줄 + 모델·ctx·한도 줄)은 cc-usage 의 이식이다 — 같은 입력이면 ANSI 까지 **같은 바이트**를 낸다(골든 픽스처
  `crates/rocky-core/tests/fixtures/cc-usage/`). 한도(5h/7d) 판정은 순수 함수, "지금"은 인자로 받는다.
- `--full` 의 줄은 **CLI 가 그린다** — 데몬이 없어도 경로·모델·한도 줄은 남고, 보드 줄은 그 아래 한 세그먼트라 실패하면 그 줄만
  빠진다. 설정은 `rocky.json` 최상위 `statusline` 블록(보드 줄 템플릿 `todo.statusline` 과 다른 자리).
- 골든은 cc-usage(아카이브됨 — 로컬 체크아웃을 빌드해 쓴다)에서 `scripts/cc-usage-capture.ts` 로 뜬다. 일부러 다르게 둔 곳은
  케이스의 `allow` 나 스펙의 "의도된 차이" 에 적는다.
  테스트 전용 `ROCKY_STATUSLINE_NOW`(RFC3339)가 "지금" 을 고정한다 — 사용자 표면이 아니라 README 표에 올리지 않는다(cc-usage
  쪽 짝은 `CC_USAGE_NOW`).
- git 세그먼트는 `git status --porcelain=v2` 한 번, 500ms. 넘으면 **프로세스 그룹째** 끊고 그 세그먼트만 뺀다 — git 만 죽이면
  git 이 띄운 자식이 고아로 남아 1초마다 쌓인다. git 과 `extraCommands` 는 같은 실행기를 쓴다(`crates/rocky-cli/src/bounded.rs`).
- `extraCommands` 는 나란히 돌리고 출력은 **설정 순서대로** 붙인다. rocky 는 그 명령이 무엇인지 모른다 — 특정 도구를 아는
  코드가 들어오면 그 도구의 변경이 rocky 를 깬다. 본 프로세스가 끝난 뒤 100ms 안에 파이프가 닫히지 않으면(백그라운드 자식이
  stdout·stderr 를 붙잡음) 그 출력은 버린다. 출력은 바이트 그대로 붙인다(UTF-8 이 아니어도). spawn 만 뮤텍스로 한 번에
  하나씩 — macOS 에는 `pipe2` 가 없어 동시 spawn 이 파이프 끝을 남의 자식에게 넘길 수 있다. 순서: rocky 줄 → extras → 보드 줄.

- **usage API 갱신은 데몬이 아니라 statusline 이 띄운다**(`rocky statusline refresh`, detached + `setsid`). 계정을 세션 환경
  (`CLAUDE_CONFIG_DIR`)으로 판단하는데 데몬은 세션 환경을 볼 수 없어서다 — 자식이 환경을 물려받아 그 계정의 토큰을 쓴다.
  같은 계정의 갱신은 `refresh.lock`(flock)으로 하나만, 실패는 1→30분 backoff(429 면 `Retry-After`). 토큰은 읽기만 한다.
  keychain 이름은 Claude Code 규칙(`CLAUDE_CONFIG_DIR` 이 있으면 `-<sha256(값)[:8]>` 접미사, `claude_account::keychain_service`)이고,
  keychain 토큰이 만료됐으면 유효한 파일 토큰이 이긴다(`file_beats_expired_keychain` — 낡은 keychain 항목이 실측으로 있었다).
- **계정은 세션 환경으로 정한다**(`rocky_core::claude_account`): 설정 폴더 = `CLAUDE_CONFIG_DIR` > `statusline.configDir` >
  `~/.claude`. 계정 파일은 `CLAUDE_CONFIG_DIR` 이면 그 폴더의 `.claude.json` 하나뿐이고(못 읽어도 다른 후보로 넘어가지 않는다 —
  다른 계정의 이메일을 집는다), 아니면 `<설정 폴더>/.claude.json` → 기본 설치일 때만 `~/.claude.json`. 이 우선순위를 바꾸면 다른
  계정의 한도를 그리는 회귀가 난다.
- **캐시는 `<cache>/rocky/statusline/<설정 폴더 해시>/<이메일 해시>/`** — `account.json`(이메일 캐시, 1분 TTL·한도가 바뀌면
  재확인, 못 읽으면 덮지 않음)은 설정 폴더에 하나, `state.json`(writer = statusline)·`usage.json`(writer = 갱신)은 계정마다.
  JSON 모양은 cc-usage 와 같다(골든이 캐시를 함께 심는다).
- **테스트는 실제 토큰·API 에 닿으면 안 된다.** macOS keychain 은 HOME 과 무관하므로 `--full` 을 도는 테스트는 없는
  `keychainService` 와 테스트 전용 `ROCKY_STATUSLINE_USAGE_URL`(가짜·죽은 주소)을 함께 건다.

- **경보 깜빡임은 `state.json` 의 `alert_key`(`<단계>@<창 키>`)·`alert_at` 으로 센다** — 키가 바뀌면(단계가 오르거나 창이 리셋)
  지금을 적고, 거기서 6초 동안 0.5초마다 배지 ↔ 굵은 빨강(여백은 유지). 프레임은 벽시계에서 고른다(카운터를 저장하지 않는다).
  writer 는 statusline 하나다. 캐시 자리가 없으면(HOME 없음) 깜빡이지 않고 배지로 고정.
- **계정 배지**는 계정 캐시의 이메일로 `statusline.badges` 를 찾는다 — 배지는 세그먼트가 아니라 머리표라 구분자 없이 맨 앞에 붙고,
  폭(+1)은 크레딧 배치 판단에 든다. 나머지가 다 비어도 배지는 낸다.

- **크레딧 페이드는 rocky 만의 표시다**(cc-usage 아카이브 뒤 첫 갈래). 안 쓸 때 금액 색을 회색 쪽으로 반 섞고, 쓰기 시작하면
  그 시각(`state.json` 의 `credits_spending_at`)부터 3초에 걸쳐 원래 색으로 — 다시 안 쓰면 바로 옅게. 색을 섞을 수 없는 터미널은
  회색 ↔ 원래 색. 골든 대조는 `creditFade: false` 로 돈다(끄면 cc-usage 와 같은 바이트).

- **`source` 는 그 실행에서만 덮을 수 있다** — 설정 < `ROCKY_STATUSLINE_SOURCE` < `--source`(`limits::override_source`). 모르는
  환경 변수 값은 무시, 모르는 플래그 값은 `[rocky] --source "…": …` 한 줄만 낸다. 갱신 자식에게는 **같은 환경 변수로** 넘긴다
  (`statusline_refresh::spawn_detached`) — 안 넘기면 자식이 설정·물려받은 값으로 다시 판단해 부모와 다른 모드로 돈다.
- **`none` 과 agy 는 Claude 쪽을 건드리지 않는다**(`limits::local_limits`) — 토큰·API·캐시·계정 파일을 읽지도 쓰지도 않는다.
  같은 머신 Claude 세션의 캐시를 읽으면 그 숫자가 다른 호스트의 줄에 그려진다. agy(stdin `product: "antigravity"`)는 `source`
  와 무관하게 이쪽이고, 한도는 `quota` 의 모델 버킷(`gemini-*` / `3p-*`, 모델 이름으로 고름)이다. 고른 버킷이 없거나 모델
  이름을 모르면 창을 비운다 — 다른 버킷의 숫자를 대신 그리지 않는다. 경보는 배지로 고정(깜빡임은 캐시가 있어야 센다).

- **guard 는 fail-open 이고 설정 파일의 `source` 만 본다**(`limits::guard`, `crates/rocky-cli/src/statusline_guard.rs`). 꺼져 있거나
  `none` 이거나 홈·캐시를 모르면 막지 않는다. `ROCKY_STATUSLINE_SOURCE` 를 보면 다른 호스트 때문에 셸에 걸어 둔 `none` 이 같은
  셸에서 띄운 Claude Code 의 guard 를 조용히 끈다. 크레딧 활성은 usage 관측값이 계정 파일 힌트를 이기고, 둘 다 모르면 막는다.
  계정은 statusline 과 같은 규칙(계정 파일을 prompt 마다 직접 읽고, 못 읽으면 계정 캐시). `allow.json` 은 계정마다가 아니라
  `<cache>/rocky/statusline/` 에 하나다.

- **doctor 는 실제 동작과 같은 함수로 계산한다**(`crates/rocky-cli/src/statusline_doctor.rs`) — 계정·토큰 자리(`Slot`·
  `statusline_refresh::find_token`), extra 실행(`bounded::run_extra` — statusline 도 이걸 탄다), 크레딧 판단(`limits::credits_enabled`).
  따로 계산하면 둘이 어긋났을 때 진단이 거짓말을 한다. keychain 후보는 `security dump-keychain`(`-d` 없이 — 비밀을 찍지 않는다).

## 코드

| 무엇 | 어디 |
| --- | --- |
| 세그먼트 렌더 | `crates/rocky-core/src/statusline.rs` |
| `--full` 렌더·폭·git·extra | `crates/rocky-core/src/statusline/{full,width,git,extra}.rs` |
| 한도 판정 | `crates/rocky-core/src/limits.rs`, agy `limits/agy.rs`, guard `limits/guard.rs` |
| guard·allow | `crates/rocky-cli/src/statusline_guard.rs` |
| probe·doctor | `crates/rocky-cli/src/statusline_doctor.rs` |
| CLI 입구 | `crates/rocky-cli/src/commands.rs`·`client.rs`(`rocky statusline`) |
| 하위 프로세스(마감·그룹 kill) | `crates/rocky-cli/src/{bounded,git_status}.rs` |
| 계정 판단·캐시·갱신 | `crates/rocky-core/src/claude_account.rs`, `crates/rocky-cli/src/{statusline_cache,statusline_refresh}.rs` |

테스트: 계정 규칙 `crates/rocky-core/tests/it/claude_account_test.rs`, 캐시 판정 `limits_test.rs`, 계정 분리·전환·관측 기록
`crates/rocky-cli/tests/it/statusline_cmd_test.rs`(`full_keeps_caches_apart_per_config_dir` · `full_follows_an_account_switch_inside_one_config_dir`),
갱신 `statusline_refresh_test.rs`(가짜 usage API 서버), 보드 줄 `crates/rocky-core/tests/it/statusline_test.rs`,
`crates/rockyd/tests/it/server_statusline_test.rs`, 골든 픽스처 `crates/rocky-core/tests/fixtures/cc-usage/`.
