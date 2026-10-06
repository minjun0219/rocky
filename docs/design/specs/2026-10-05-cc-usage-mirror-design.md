# cc-usage 미러 — statusline 을 rocky 로 옮기고 같은 출력을 낸다

2026-10-05. 오너 요청: cc-usage(Go, 별도 공개 레포)의 기능을 rocky(Rust)로 가져온다. 기존 cc-usage 는 그대로 두고
둘을 나란히 돌리며 기능을 맞추고, rocky 가 대체할 수 있다고 판단되면 그때 cc-usage 아카이브를 고려한다.

2026-10-06. 오너 결정으로 cc-usage 아카이브를 앞당겼다(read-only) — "1주 실사용 뒤 대체 판정" 게이트는 없어졌다. 남은
조각도 cc-usage 와 같은 출력을 목표로 옮기고, 골든은 로컬 체크아웃(main `07a4a86`, `CC_USAGE_NOW` 포함)을 빌드해 뜬다.
설치된 `~/.local/bin/cc-usage` 는 `CC_USAGE_NOW` 이전 빌드라 캡처에 쓰지 않는다.

## 결정

- **cc-usage 는 동결했고(2026-10-05) 이어서 아카이브했다(2026-10-06).** 목표가 움직이지 않아 "같다" 를 판정할 수 있다.
  새 기능은 rocky 에만 넣는다.
- **이식 기준 = 바이트 일치.** 같은 stdin·시각·캐시·환경에서 두 바이너리의 출력이 ANSI 까지 같다. 일부러 다르게 둔 곳은
  픽스처의 허용 목록이나 아래 "의도된 차이" 에 적는다. (처음에는 1주 실사용까지 대체 기준이었으나 아카이브를 앞당기며 뺐다.)
- **rocky 만의 기능**(아카이브 뒤): 크레딧 금액 페이드(`statusline.creditFade`, 기본 켜짐) — 끄면 cc-usage 와 같은 색이라
  골든 대조는 끄고 돈다. 새 기능은 이렇게 끌 수 있게 들여 바이트 대조를 지킨다.
- **의도된 차이**: 모든 세그먼트가 빈 줄의 `[rocky]`(픽스처 `allow`) · 계정별 캐시 자동 분리(아래 "계정") · 에러 문구를
  자를 때 글자 경계를 지킨다(cc-usage 는 40/120 **바이트**에서 잘라 한글이 깨진 바이트를 낸다 — 예: 비기본 폴더의
  `token not found (keychain: 건너뜀 …` 는 40바이트가 '비' 한가운데다) · `keychainService`·`credentialsFile` 은 `configDir`
  세션에만(cc-usage 는 계정마다 설정 파일이 따로라 문제가 없었다) · `HOME` 이 없으면 캐시·계정을 쓰지 않는다(경보는 배지로
  고정, 계정 배지 없음 — cc-usage 는 `CLAUDE_CONFIG_DIR`·`XDG_CACHE_HOME` 만으로도 쓴다) · 스키마가 막는 잘못된 `badges` 항목
  (`null`, 대소문자가 다른 키, 타입이 틀린 값)은 그 항목만 버린다(cc-usage 는 0값 배지로 받거나 설정 전체가 에러) ·
  `--source=none` 처럼 `=` 로 붙인 플래그는 받지 않는다(rocky CLI 파서의 계약 — `--source none`. cc-usage 처럼 무엇이 틀렸는지 `[rocky] …` 한 줄을 낸다) · source 환경 변수 이름은
  `ROCKY_STATUSLINE_SOURCE`(cc-usage `CC_USAGE_SOURCE`) · guard·allow 는 `rocky statusline guard` · `rocky statusline allow`
  (문구의 안내 명령도 그 이름) · `allow.json` 은 계정과 상관없이 하나다(cc-usage 는 캐시가 하나라 같았다 — rocky 는 캐시를
  계정마다 나누지만 막힌 세션과 다른 환경의 터미널에서 불러도 풀리게 둔다) · guard 의 계정은 statusline 과 같은 규칙으로 정한다.
- **미러 대상에서 뺀 것:** cc-usage 의 `update` · `config` · `version` — rocky 에 같은 일을 하는 명령(`rocky update` ·
  `rocky config`)이 있다. guard · agy 는 지금 쓰지 않아도 "기능 동일" 에 넣는다(조각 5).

## 프로세스 모델

**CLI 가 렌더하고 데몬은 갱신만 맡는다.** cc-usage 의 불변 조건을 그대로 옮긴다.

| cc-usage | rocky |
|---|---|
| `cc-usage statusline` (매 호출 새 프로세스) | `rocky statusline --full` — 경로·git·모델·ctx·한도를 CLI 가 직접 그린다 |
| `cc-usage refresh` (detached + lock 파일 + backoff) | `rocky statusline refresh` — 같은 방식(조각 3). 데몬이 아닌 이유는 아래 "계정" |
| `usage.json` writer = `refresh`, `state.json` writer = `statusline` | writer 분리 그대로 |
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
| 3 | usage API 갱신 + 계정 구분 | detached `refresh`, keychain(`/usr/bin/security`) 읽기 전용, 한도별 폴링 간격, backoff·`Retry-After`, `source: api` / `auto`, 캐시 판정·상태 문구. 크레딧 렌더·폭 판단은 조각 1 리뷰 반영 때 앞당겼다 |
| 4 | 경보 깜빡임 · 계정 배지 | `state.json` 의 경보 시각(`alert_key`·`alert_at`, 6초·0.5초 프레임), `statusline.badges`(계정 판단은 조각 3) |
| 5 | guard / allow · probe / doctor · agy | guard 는 fail-open |

### 계정 (조각 3, 2026-10-06 결정)

오너는 계정을 둘 쓴다(기본 `~/.claude` 와 `CLAUDE_CONFIG_DIR`). cc-usage 는 계정을 **statusline 을 띄운 세션의 환경**으로
판단한다 — `CLAUDE_CONFIG_DIR` 이 설정값을 이기고, 토큰 위치(기본 dir 만 keychain, 아니면 `<config_dir>/.credentials.json`)와
계정 파일(`.claude.json`)이 그 값을 따른다. rockyd 는 머신에 하나라 세션 환경을 볼 수 없으므로 갱신도 cc-usage 처럼
statusline 이 detached 로 띄운다 — 자식이 세션 환경을 물려받아 맞는 계정의 토큰을 쓴다. rocky 는 계정이 몇 개인지 몰라도 된다.

- **계정 판단은 cc-usage 와 같다**: 진실 원천은 `.claude.json` 의 `oauthAccount.emailAddress`(전환 도구가 무엇이든 결과만 본다).
  `CLAUDE_CONFIG_DIR` 이 있으면 그 dir 의 파일 하나만, 없으면 `<config_dir>/.claude.json` → (기본 설치일 때만) `~/.claude.json`.
  한도 숫자가 바뀌었거나 1분이 지났을 때만 다시 읽고, 읽기 실패는 캐시를 덮지 않는다.
- **캐시는 cc-usage 보다 잘게 나눈다**: `<cache>/rocky/statusline/<config_dir 해시>/account.json`(이메일 캐시) 아래
  `<이메일 해시>/{state,usage}.json·refresh.lock`. cc-usage 는 `XDG_CACHE_HOME` 을 계정마다 나눠 줘야 캐시가 갈리고, usage 캐시가
  계정을 키로 갖지 않아 같은 dir 안의 전환(claude-swap · `/login`) 뒤에 이전 계정의 크레딧·기준선이 남을 수 있다. 출력이
  달라지는 것은 cc-usage 가 남의 숫자를 보이던 경우뿐이다. 이메일을 모르면(API 키 인증 · 첫 읽기 실패) `_` 폴더.
- 캐시 JSON 모양은 cc-usage 와 같다 — 골든에 심는 캐시를 두 바이너리가 함께 읽는다.

## 대조 하네스

- cc-usage 의 테스트 전용 환경 변수 `CC_USAGE_NOW`(RFC3339)로 시각을 고정한다(cc-usage#14). rocky 는 자기 이름의
  테스트 전용 변수 `ROCKY_STATUSLINE_NOW` 를 읽는다 — 픽스처의 `now` 를 각 바이너리의 변수로 넣는다.
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
- statusLine 을 `rocky statusline --full` 로 바꾸는 것은 조각 3(usage API) 머지 뒤 — 크레딧까지 나와야 cc-usage 를 내릴 수
  있다. 경보 깜빡임·배지(조각 4)와 guard 등(조각 5)은 그 뒤에 이어 옮긴다.

## PR

조각 1 은 셋으로 나눈다: cc-usage#14(`CC_USAGE_NOW`) → 캡처 스크립트 + 픽스처 + `rocky_core::limits` · 렌더 →
CLI `--full` 배선 + git + 설정 블록 + changeset. rocky 쪽 둘은 스택이다.
