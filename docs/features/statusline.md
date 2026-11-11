# statusline — 줄 렌더

> rocky 를 **고치는** 에이전트용 개발 문서. 설정 방법은 [`docs/board.md`](../board.md) "statusline",
> cc-usage 에서 옮겨 온 설계(기록 — 그때 이름은 `--full`)는 [`2026-10-05-cc-usage-mirror-design.md`](../design/specs/2026-10-05-cc-usage-mirror-design.md).
>
> **구현은 cc-usage 에 있다(2026-11-11 오너 결정).** rocky 는 [`minjun0219/cc-usage`](https://github.com/minjun0219/cc-usage)
> 를 Cargo git dependency(`rev` 고정, `default-features = false`)로 싣고, 설정 블록을 읽어 넘기고 이름만 건다
> (`crates/rocky-cli/src/statusline_profile.rs`). 아래 규칙은 cc-usage 코드가 지키는 것이고, 렌더·판정·캐시를 고치려면 cc-usage 에
> PR 을 내고 rocky 에서 `Cargo.toml` 의 `rev` 를 올린다. 아래의 `limits::…`·`bounded::…` 같은 경로는 `cc_usage::` 아래다.

## 규칙

- **`rocky statusline` 이 줄 전체를 CLI 에서 그린다** — 경로·git 줄과 모델·ctx·한도 줄(한도가 소진되면 크레딧 줄). 1초마다 도는
  자리라 사용 로그를 거치지 않고 데몬에 묻지 않는다 — 데몬이 없어도 줄은 그대로다. `--full` 은 예전 설정을 위해 받기만 한다.
  데몬이 그리던 보드 줄(`GET /api/statusline`·`todo.statusline` 템플릿·`statusline.board`)은 걷었다(오너 결정 2026-10-15 — 늘 full
  만 썼다). 걷은 라우트는 사용 로그에서 계속 건너뛴다 — 옛 조각이 남은 기기가 1초마다 404 로 로그를 채우지 않게.
- **다른 statusline 안에서 불린 `rocky statusline` 은 아무것도 그리지 않는다** — `extraCommands` 실행기가 자식에
  `ROCKY_STATUSLINE_NESTED` 를 걸고(`bounded::nested_env()`), 보드 줄을 끼우던 옛 꼴(`--cwd`·`--session`)도 빈 출력이다. 안 그러면
  extra 안의 `rocky statusline` 이 자기를 끝없이 다시 띄운다(단계마다 새 프로세스 그룹이라 마감 kill 로 끊기지 않는다). 렌더
  분기는 `build_cli_context` 앞에서 끝난다 — 1초마다 도는 자리에서 쓰지 않는 설정을 읽지 않는다. 표식은 rocky 끼리의 약속이라
  README 표에 올리지 않는다.
- 이 렌더는 cc-usage 에서 rocky 로 옮겨 왔다가(2026-10-05, rocky 가 정본이던 때 2026-10-13) **다시 cc-usage 가 정본이 됐다**
  (2026-11-11). 출력은 cc-usage 의 골든 픽스처(`tests/fixtures/statusline-full/`)가 ANSI 까지 고정하고, rocky 에는 골든이 없다 —
  `rev` 를 올리면 rocky 출력도 그만큼 바뀐다(PR 본문에 무엇이 바뀌는지 적는다). 한도(5h/7d) 판정은 순수 함수, "지금"은 인자로 받는다.
- 설정은 `rocky.json` 최상위 `statusline` 블록이다(`rocky_core::config::load_statusline_block` → `cc_usage::config::from_block`).
  블록 안 모양은 cc-usage `config.json` 최상위와 같다 — `rocky.schema.json` 의 `statusline` 과 cc-usage `config.schema.json` 을 같이 본다.
- **이름은 rocky 것이다**(`statusline_profile::ROCKY`): `ROCKY_STATUSLINE_SOURCE`·`_NESTED`·`_NOW`·`_USAGE_URL`, 캐시
  `<cache>/rocky/statusline/`, 머리 `[rocky]`, 갱신 `rocky statusline refresh`, 해제 안내 `rocky statusline allow`. `rocky` 의
  `main` 첫머리에서 `cc_usage::profile::set` 으로 건다 — 그보다 먼저 cc-usage 함수를 부르면 cc-usage 이름으로 굳는다. 바꾸면 셸
  설정과 캐시를 잃는다. 통합 테스트도 같은 이름으로 캐시 자리를 계산한다(`tests/it/common::statusline_slot`).
  테스트 전용 `ROCKY_STATUSLINE_NOW`(RFC3339)가 "지금" 을 고정한다 — 사용자 표면이 아니라 README 표에 올리지 않는다.
- git 표시는 브랜치 이름 바로 뒤의 기호다(cc-usage #18 · #20): unstaged `*` · staged `+` · 충돌 `!`, 띄우고 ahead/behind
  (`⇡1⇣2`), 원격 브랜치가 지워졌으면 `gone`. 숫자는 커밋 수에만 쓴다.
- git 세그먼트는 `git status --porcelain=v2` 한 번, 500ms. 넘으면 **프로세스 그룹째** 끊고 그 세그먼트만 뺀다 — git 만 죽이면
  git 이 띄운 자식이 고아로 남아 1초마다 쌓인다. git 과 `extraCommands` 는 같은 실행기를 쓴다(cc-usage `src/bounded.rs`).
- `extraCommands` 는 나란히 돌리고 출력은 **설정 순서대로** 붙인다. rocky 는 그 명령이 무엇인지 모른다 — 특정 도구를 아는
  코드가 들어오면 그 도구의 변경이 rocky 를 깬다. 본 프로세스가 끝난 뒤 100ms 안에 파이프가 닫히지 않으면(백그라운드 자식이
  stdout·stderr 를 붙잡음) 그 출력은 버린다. 출력은 바이트 그대로 붙인다(UTF-8 이 아니어도). spawn 만 뮤텍스로 한 번에
  하나씩 — macOS 에는 `pipe2` 가 없어 동시 spawn 이 파이프 끝을 남의 자식에게 넘길 수 있다. 순서: rocky 줄 → extras.

- **usage API 갱신은 데몬이 아니라 statusline 이 띄운다**(`rocky statusline refresh`, detached + `setsid`). 계정을 세션 환경
  (`CLAUDE_CONFIG_DIR`)으로 판단하는데 데몬은 세션 환경을 볼 수 없어서다 — 자식이 환경을 물려받아 그 계정의 토큰을 쓴다.
  같은 계정의 갱신은 `refresh.lock`(flock)으로 하나만, 실패는 1→30분 backoff(429 면 `Retry-After`). 토큰은 읽기만 한다.
  keychain 이름은 Claude Code 규칙(`CLAUDE_CONFIG_DIR` 이 있으면 `-<sha256(값)[:8]>` 접미사, `claude_account::keychain_service`)이고,
  keychain 토큰이 만료됐으면 유효한 파일 토큰이 이긴다(`file_beats_expired_keychain` — 낡은 keychain 항목이 실측으로 있었다).
- **계정은 세션 환경으로 정한다**(`cc_usage::claude_account`): 설정 폴더 = `CLAUDE_CONFIG_DIR` > `statusline.configDir` >
  `~/.claude`. 계정 파일은 `CLAUDE_CONFIG_DIR` 이면 그 폴더의 `.claude.json` 하나뿐이고(못 읽어도 다른 후보로 넘어가지 않는다 —
  다른 계정의 이메일을 집는다), 아니면 `<설정 폴더>/.claude.json` → 기본 설치일 때만 `~/.claude.json`. 이 우선순위를 바꾸면 다른
  계정의 한도를 그리는 회귀가 난다.
- **캐시는 `<cache>/rocky/statusline/<설정 폴더 해시>/<이메일 해시>/`** — `account.json`(이메일 캐시, 1분 TTL·한도가 바뀌면
  재확인, 못 읽으면 덮지 않음)은 설정 폴더에 하나, `state.json`(writer = statusline)·`usage.json`(writer = 갱신)은 계정마다.
  JSON 모양은 cc-usage 와 같다(같은 코드다 — 폴더만 다르다).
- **테스트는 실제 토큰·API 에 닿으면 안 된다.** macOS keychain 은 HOME 과 무관하므로 statusline 을 그리는 테스트는 없는
  `keychainService` 와 테스트 전용 `ROCKY_STATUSLINE_USAGE_URL`(가짜·죽은 주소)을 함께 건다.

- **경보 깜빡임은 `state.json` 의 `alert_key`(`<단계>@<창 키>`)·`alert_at` 으로 센다** — 키가 바뀌면(단계가 오르거나 창이 리셋)
  지금을 적고, 거기서 6초 동안 0.5초마다 배지 ↔ 굵은 빨강(여백은 유지). 프레임은 벽시계에서 고른다(카운터를 저장하지 않는다).
  writer 는 statusline 하나다. 캐시 자리가 없으면(HOME 없음) 깜빡이지 않고 배지로 고정.
- **계정 배지**는 계정 캐시의 이메일로 `statusline.badges` 를 찾는다 — 배지는 세그먼트가 아니라 머리표라 구분자 없이 맨 앞에 붙고,
  폭(+1)은 크레딧 배치 판단에 든다. 나머지가 다 비어도 배지는 낸다.

- **경로 링크(`dirLink`)도 rocky 만의 표시다** — 설정이 있을 때만 경로를 OSC 8(`ESC ]8;;URL BEL`)로 감싸고, 없으면 링크 없이
  경로만 낸다. `{{cwd}}` 는 퍼센트 인코딩(`/` 는 그대로)하고, 템플릿에 `{{cwd}}` 가 없거나 결과가 출력 가능한 ASCII 가 아니면
  링크를 빼고 경로만 낸다 — 섞인 제어 문자가 줄 전체를 깨뜨린다. 경로 줄은 폭 판단(`credit_standalone`)에 들지 않아
  `display_width` 는 OSC 를 모른다 — 링크를 상태 줄에 걸게 되면 그때 OSC 를 0칸으로 세야 한다.
  지금 폴더가 `workspace.project_dir`(세션을 띄운 프로젝트 — 세션이 다른 폴더로 옮겨 가도 그대로) 안이면 경로를 둘로 나눠 건다:
  앞부분(프로젝트)은 프로젝트를, 뒤의 하위 경로는 지금 폴더를 연다 — 누른 글자가 곧 여는 경로라 글자와 대상이 어긋나지 않고
  이름도 겹치지 않는다. 프로젝트 밖(다른 워크트리 등 — 경로 경계로 판정해 `rocky-wt` 는 `rocky` 안이 아니다)이면 경로 전체가
  지금 폴더를 열고 줄 끝에 프로젝트를 여는 `↗ 이름` 버튼을 단다. 프로젝트를 모르면(빈 값 — agy 등) 경로 전체가 지금 폴더를 연다.
  프로젝트를 모르면(빈 값 — agy 등) 버튼을 뺀다.
  링크가 살아남는지는 Claude Code 가 정한다(2.1.295·2.1.296 실측). 터미널이 링크를 지원한다고 판정하지 못하면 statusline 출력의
  OSC 8 을 지우고 글자만 그린다 — 판정은 `TERM_PROGRAM`(`ghostty`·`iTerm.app`·`WezTerm` 등)이고 `TERM`(`xterm-ghostty`)은 보지
  않는다. tmux 흉내 환경이 여기 걸린다: cmux 의 Claude teams 세션은 `TMUX` 를 걸고 `TERM=screen-256color` 에 `TERM_PROGRAM`
  없이 Claude Code 를 띄워 링크가 지워지고, 남은 경로 글자를 cmux 가 스스로 알아봐 Finder 로 연다. 이때는 `FORCE_HYPERLINK=1`
  (셸 또는 Claude Code `settings.json` 의 `env` — 둘 다 실측으로 된다). 링크가 살아 있으면 fullscreen(`tui: "fullscreen"`)에서는
  Claude Code 가 클릭을 받아 직접 `open` 하고, 허용 스킴(`vscode:`·`cursor:`·`zed:`·`windsurf:`·`idea:`·`jetbrains:`·`http(s):`
  등) 밖이면 열지 않는다 — 템플릿 예시는 이 안에서 고른다. 그 밖에서는 터미널이 연다.

- **크레딧 페이드는 rocky 만의 표시다**. 안 쓸 때 금액 색을 회색 쪽으로 반 섞고, 쓰기 시작하면
  그 시각(`state.json` 의 `credits_spending_at`)부터 3초에 걸쳐 원래 색으로 — 다시 안 쓰면 바로 옅게. 색을 섞을 수 없는 터미널은
  회색 ↔ 원래 색. 끄면(`creditFade: false`) 늘 원래 색.

- **ctx 는 그리는 숫자가 75~90% 면 노랑, 91% 이상이면 굵은 빨강이다**(`CTX_YELLOW_AT`·`CTX_RED_AT`, 16색 터미널은 `93`·`1;91`).
  Claude 화면의 컨텍스트 경고와 같은 구간이다(공식 문서에는 기준이 없고 오너가 화면에서 본 구간이다). 구간 안에서는 차오를수록
  진해지고(노랑 → 금색, 주홍 → 진한 빨강), 74% 까지는 차오를수록 밝아지는 파랑 한 축(한도의 초록 → 빨강과 다른 축)이다.
  구간은 그리는 숫자로 가른다 — `{:.0}` 은 짝수 쪽으로 반올림해서(90.5 → `90%`) `round_ties_even` 으로 견준다. `f64::round` 로
  견주면 `90%` 로 보이는데 빨강이 된다. 1M 창의 자동 압축은 97% 언저리(실측 967k~974k 토큰)라 빨강이 압축 직전 신호이고,
  200k 창은 83% 언저리에서 압축돼 노랑까지만 본다. 고정하는 테스트는 `full.rs` 의
  `ctx_is_yellow_from_the_shown_75_and_bold_red_from_the_shown_91` 와 골든 `ctx-near`·`ctx-near-256`·`term-256`·`truecolor`.

- **`source` 는 그 실행에서만 덮을 수 있다** — 설정 < `ROCKY_STATUSLINE_SOURCE` < `--source`(`limits::override_source`). 모르는
  환경 변수 값은 무시, 모르는 플래그 값은 `[rocky] --source "…": …` 한 줄만 낸다. 갱신 자식에게는 **같은 환경 변수로** 넘긴다
  (`statusline_refresh::spawn_detached`) — 안 넘기면 자식이 설정·물려받은 값으로 다시 판단해 부모와 다른 모드로 돈다.
- **`none` 과 agy 는 Claude 쪽을 건드리지 않는다**(`limits::local_limits`) — 토큰·API·캐시·계정 파일을 읽지도 쓰지도 않는다.
  같은 머신 Claude 세션의 캐시를 읽으면 그 숫자가 다른 호스트의 줄에 그려진다. agy(stdin `product: "antigravity"`)는 `source`
  와 무관하게 이쪽이고, 한도는 `quota` 의 모델 버킷(`gemini-*` / `3p-*`, 모델 이름으로 고름)이다. 고른 버킷이 없거나 모델
  이름을 모르면 창을 비운다 — 다른 버킷의 숫자를 대신 그리지 않는다. 경보는 배지로 고정(깜빡임은 캐시가 있어야 센다).

- **guard 는 fail-open 이고 설정 파일의 `source` 만 본다**(`limits::guard`, cc-usage `src/statusline_guard.rs`). 꺼져 있거나
  `none` 이거나 홈·캐시를 모르면 막지 않는다. `ROCKY_STATUSLINE_SOURCE` 를 보면 다른 호스트 때문에 셸에 걸어 둔 `none` 이 같은
  셸에서 띄운 Claude Code 의 guard 를 조용히 끈다. 크레딧 활성은 usage 관측값이 계정 파일 힌트를 이기고, 둘 다 모르면 막는다.
  계정은 statusline 과 같은 규칙(계정 파일을 prompt 마다 직접 읽고, 못 읽으면 계정 캐시). `allow.json` 은 계정마다가 아니라
  `<cache>/rocky/statusline/` 에 하나다.

- **doctor 는 실제 동작과 같은 함수로 계산한다**(cc-usage `src/statusline_doctor.rs`) — 계정·토큰 자리(`Slot`·
  `statusline_refresh::find_token`), extra 실행(`bounded::run_extra` — statusline 도 이걸 탄다), 크레딧 판단(`limits::credits_enabled`).
  따로 계산하면 둘이 어긋났을 때 진단이 거짓말을 한다. keychain 후보는 `security dump-keychain`(`-d` 없이 — 비밀을 찍지 않는다).

## 코드

| 무엇 | 어디 |
| --- | --- |
| CLI 입구 | `crates/rocky-cli/src/{main,commands}.rs`(`rocky statusline`) — 설정을 읽어 cc-usage 를 부른다 |
| 이름(환경 변수·캐시 폴더·머리) | `crates/rocky-cli/src/statusline_profile.rs` |
| 설정 블록 | `crates/rocky-core/src/config.rs`(`load_statusline_block`), 스키마 `rocky.schema.json` 의 `statusline` |
| 렌더·판정·캐시·갱신·guard·doctor·하위 프로세스 | cc-usage `src/`(`statusline/` · `limits` · `claude_account` · `statusline_*` · `bounded` · `git_status`) |
| 버전 | 루트 `Cargo.toml` 의 `cc-usage = { git, rev }` |

테스트: rocky 쪽은 배선을 본다 — rocky 실행 파일을 띄워 이름·설정 블록·캐시 자리를 확인한다. 계정 분리·전환·관측 기록
`crates/rocky-cli/tests/it/statusline_cmd_test.rs`(`full_keeps_caches_apart_per_config_dir` · `full_follows_an_account_switch_inside_one_config_dir`),
갱신·probe·doctor `statusline_refresh_test.rs`(가짜 usage API 서버), guard `statusline_guard_test.rs`, 인자 없는 호출 = `--full`
`statusline_cmd_test.rs::statusline_without_flag_draws_the_full_lines`, 설정 블록 `crates/rocky-core/tests/it/config_test.rs`.
순수 판정·렌더와 골든은 cc-usage `tests/it/` 에 있다.
