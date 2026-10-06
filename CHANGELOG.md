# @minjun0219/rocky

## 0.46.0

### Minor Changes

- [#406](https://github.com/minjun0219/rocky/pull/406) [`cca0ee4`](https://github.com/minjun0219/rocky/commit/cca0ee48d7f39ede80c23c7ac2d8cf222f4ca44c) Thanks [@minjun0219](https://github.com/minjun0219)! - 세션이 MCP `todo_status` 로 스스로 시작한 할 일에도 그 세션을 귀속시킨다(PostToolUse 훅 `rocky hook claim-doing`). 그동안 세션 귀속은 핸드오프로 받은 작업에만 붙어 statusline 보드 줄의 `⏺ {mine.ref} {mine.title}`·`💬 {mine.comments}` 와 워크로그 턴의 `todo:<ref>` 태그가 거의 나오지 않았다. Stop 훅의 "닫았나?" 확인은 지금처럼 핸드오프로 받은 할 일에만 한다. 데몬 DB 에 컬럼 하나(`todos.doing_session_claimed`)가 더해진다.

- [#400](https://github.com/minjun0219/rocky/pull/400) [`4d62ae0`](https://github.com/minjun0219/rocky/commit/4d62ae02e19f975bc7dc57d3451774045ae99e94) Thanks [@minjun0219](https://github.com/minjun0219)! - 보드의 새 세션 띄우기가 띄운 rc 서버(핸드오프 서버)를 `rocky rc` 에 "핸드오프:" 로 따로 보이고(`GET /api/rc/servers` 의 `handoffs`), `rocky rc stop <할 일>`(`POST /api/rc/handoffs/:ref/stop`, 로컬 전용)로 닫는다 — 지금 그 폴더에서 그 pid 로 도는 rc 서버일 때만 pid 로 내리고 워크트리는 남긴다. README 의 권장 확인 규칙에 `Bash(rocky rc stop:*)` 를 더했다.

- [#407](https://github.com/minjun0219/rocky/pull/407) [`20aa72e`](https://github.com/minjun0219/rocky/commit/20aa72ed6a0a22274ed58e68ae4dcd2b6635a329) Thanks [@minjun0219](https://github.com/minjun0219)! - `rocky rc stop` 이 대상 밖 서버(rocky.json 목록에 없는 폴더의 rc 서버)도 닫는다 — 폴더 이름이나 pid 로 고르고(`POST /api/rc/strays/:ref/stop`, 로컬 전용), 지금 그 폴더의 rc 서버일 때만 pid 로 내린다. 웹 원격 제어 탭의 "대상 밖" 줄에도 닫기를 둔다(한 번 더 묻는다). 설정 대상은 닫지 않는다.

- [#408](https://github.com/minjun0219/rocky/pull/408) [`c8a95bd`](https://github.com/minjun0219/rocky/commit/c8a95bd01f880eb8ab98bba52d7d21c14ebe7efb) Thanks [@minjun0219](https://github.com/minjun0219)! - 실험 기능 `lab`(0.44.0, Claude Code function hooks 모듈)을 뺀다 — 90일 사용 0회. `rocky.json` 에 `lab` 블록이 남아 있으면 지운다(이제 스키마가 모르는 키로 잡는다).

- [#403](https://github.com/minjun0219/rocky/pull/403) [`e357d2e`](https://github.com/minjun0219/rocky/commit/e357d2e03d946942f4d71810daba07774955ba21) Thanks [@minjun0219](https://github.com/minjun0219)! - statusline 보드 줄 템플릿에 `{pr.ready}`(이 세션이 구독한 열린 PR 중 머지 후보 수)와 `{pr.conflict}`(충돌 수)를 더한다. 기본 템플릿도 바꾼다 — 핸드오프 수 `{inbox}` 의 표시를 받은편지함으로 오해하게 하던 `✉` 에서 `🤝` 로, 그리고 `✅ {pr.ready}` · `⛔ {pr.conflict}` 를 넣었다. 템플릿을 직접 적어 둔 설정은 그대로이니 필요하면 새 변수를 넣는다.

- [#401](https://github.com/minjun0219/rocky/pull/401) [`3f4bdc6`](https://github.com/minjun0219/rocky/commit/3f4bdc60b99ba9acebe1aab39115977a90608ebd) Thanks [@minjun0219](https://github.com/minjun0219)! - 할 일 상세의 "에이전트에게 보내기" 와 "새 세션 띄우기" 를 한 패널로 합쳤다 — 대상 첫 줄이 "새 세션 — 이 할 일의 워크트리(원격 제어)"(로컬 화면에서만, 기본 선택)이고 그 아래 기존 세션들이다. 원격 제어 탭에 "핸드오프" 절을 두어 새 세션 띄우기가 띄운 서버를 보이고, 한 번 더 물은 뒤 닫는다(워크트리는 남는다).

### Patch Changes

- [#405](https://github.com/minjun0219/rocky/pull/405) [`c60bff0`](https://github.com/minjun0219/rocky/commit/c60bff0745939480c2a9c19ced1d49027745c8dc) Thanks [@minjun0219](https://github.com/minjun0219)! - `rocky rc restart`(와 야간 재시작)가 열려 있던 대화가 아니라 서버가 처음 만든 세션을 이어받던 것을 고쳤다 — `claude rc -c` 는 "그 폴더에서 마지막 서버가 처음 만든 세션" 을 되살린다. 이제 내리기 전에 그 서버의 열린 세션 id 를 읽어 `--session-id` 로 못 박는다(여럿이면 가장 최근에 대화한 것).

## 0.45.0

### Minor Changes

- [#399](https://github.com/minjun0219/rocky/pull/399) [`91a5d75`](https://github.com/minjun0219/rocky/commit/91a5d75c280f32411e013bab58990d36b71b8080) Thanks [@minjun0219](https://github.com/minjun0219)! - 실험 기능 `lab` — Claude Code function hooks(early access) 모듈을 플러그인에 싣는다. 사용자 `rocky.json` 에 `"lab": {}` 를 두면(기본 꺼짐) 데몬이 받은편지함으로 보낸 PR·리뷰·수집함·핸드오프 메시지가 화면 toast 로도 뜨고, 프롬프트 위에 보드 요약 · PR 감시 · 받은편지함 등록 줄이, status 줄에 엔진이 주는 5h/7d·ctx 가 붙는다. `/rocky-lab` 이 세션 id(엔진 값과 env 값)·받은편지함·스위치를 진단한다. 칸마다 `toast`·`band`·`limits: false` 로 끈다.

### Patch Changes

- [#396](https://github.com/minjun0219/rocky/pull/396) [`d92021d`](https://github.com/minjun0219/rocky/commit/d92021d63167d517d3084d060725dd91a1266a4d) Thanks [@minjun0219](https://github.com/minjun0219)! - `rocky statusline --full` 의 갱신이 `CLAUDE_CONFIG_DIR` 로 띄운 세션의 토큰도 keychain 에서 찾는다 — Claude Code 의 이름 규칙(`Claude Code-credentials-<sha256(그 값) 앞 8자리>`)을 따르므로 설정 없이 두 번째 계정의 크레딧·api 숫자가 나온다. keychain 토큰이 만료됐으면 유효한 `.credentials.json` 을 대신 쓴다. `rocky statusline doctor` 의 keychain 줄도 그 이름을 보인다.

- [#397](https://github.com/minjun0219/rocky/pull/397) [`425dff0`](https://github.com/minjun0219/rocky/commit/425dff0314e4c83d71f49a7123046f813fd0dae0) Thanks [@minjun0219](https://github.com/minjun0219)! - statusline 보드 줄이 가끔(1~2%) 비던 것을 고친다. 데몬의 `/api/statusline` 이 세션 목록 캐시가 만료될 때마다 그 요청에서 `claude agents --json` 을 기다려 CLI 의 300ms 마감을 넘겼다 — 이제 낡은 값을 바로 내주고 뒤에서 새로 받으며(15초마다, 배경 부하는 그대로), 데몬이 뜰 때 미리 데운다.

## 0.44.0

### Minor Changes

- [#384](https://github.com/minjun0219/rocky/pull/384) [`f0da646`](https://github.com/minjun0219/rocky/commit/f0da6460412e57d6a9d6ecb91c44b11a066c8d71) Thanks [@minjun0219](https://github.com/minjun0219)! - `rocky rc --activity`(`GET /api/rc/servers?activity=1`, 로컬 전용)가 대상마다 최근 활동(마지막 커밋 시각 · 제목, 작업 중, 곁가지 브랜치)을 보이고 꺼진 비고정 대상 중 오래 조용한 것을 "정박"으로 가른다. `rocky rc start --all` 은 꺼진 대상 전부를 띄운다 — 비고정은 세션 없이 서버만(start 본문 `serverOnly`), 고정은 세션과 함께.

- [#385](https://github.com/minjun0219/rocky/pull/385) [`7520df6`](https://github.com/minjun0219/rocky/commit/7520df6fd1939fc6cea77da8b27c1b310ec9b672) Thanks [@minjun0219](https://github.com/minjun0219)! - `rocky rc report` 로 마지막 야간 재시작 보고를 본다. 야간 보고에 rocky 세 층(플러그인 · CLI · 데몬)의 버전과 최신 릴리스 태그, agy 버전과 원격 제어 데몬 상태(업데이트 전 바이너리로 도는지 포함)가 남는다(설치 · 재시작은 하지 않는다). README 에 에이전트가 rc 서버를 바꿀 때 사람 확인을 받는 `permissions.ask` 권장 규칙을 적었다.

- [#383](https://github.com/minjun0219/rocky/pull/383) [`ea4193d`](https://github.com/minjun0219/rocky/commit/ea4193dd8b5c87bfc277f5a1cb9cb345744e2818) Thanks [@minjun0219](https://github.com/minjun0219)! - rc 서버 재시작이 열린 세션이 막 대화하는 중(2분 안)이면 턴이 끝날 때까지 15초마다 보며 10분까지 기다렸다 내린다 — 끝내 안 끝나면 손대지 않는다(행 상태 `waiting`). 이 세션이 붙은 서버는 CLI 가 재시작을 거절한다(재시작이 이 턴을 끊는다). `rocky rc restart <라벨> --session <cse_…>`(본문 `session`)로 이어받을 세션을 못 박을 수 있다. `already served` 재시도 간격을 45·90초에서 60·120초로 늘렸다.

- [#394](https://github.com/minjun0219/rocky/pull/394) [`c8d31b5`](https://github.com/minjun0219/rocky/commit/c8d31b5e39a9258ff1010ead1ba2d44424cb794e) Thanks [@minjun0219](https://github.com/minjun0219)! - 보드의 "새 세션 띄우기"(`rocky spawn`, `POST /api/todos/:ref/spawn`)가 rc 가 켜진 기기에서는 할 일의 워크트리에 단일 세션 rc 서버(`<보드>-<n>: <요약>`)를 띄우고 그 세션에 핸드오프를 넣어 깨운다 — 폰 · 웹의 원격 제어 목록에서 이어 보고, 끝나면 사람이 닫는다. `claude --bg` 세션은 로그인 세션 밖이라 ssh · 자격이 끊겨 PR 로 끝나는 일을 끝내지 못했다. rc 가 꺼진 기기는 지금처럼 `claude --bg` 로 띄우고 응답에 그 경고를 단다. 이미 도는 세션에 넘길 때도 받은편지함이 있으면 깨운다.

- [#389](https://github.com/minjun0219/rocky/pull/389) [`5ac4723`](https://github.com/minjun0219/rocky/commit/5ac4723158d04129bb3447b67063060c41d77380) Thanks [@minjun0219](https://github.com/minjun0219)! - `rocky statusline --full` 을 Antigravity(`agy`)의 statusLine 에도 걸 수 있다. stdin 의 `product: "antigravity"` 로 알아보고, 한도는 agy 가 주는 `quota`(Gemini 모델이면 `gemini-*`, 그 밖이면 `3p-*` 버킷)로 그린다 — Claude 쪽 토큰·API·캐시·계정 배지는 보지 않고, 경보는 배지로 고정된다. `--source auto|stdin|api|none` 플래그와 환경 변수 `ROCKY_STATUSLINE_SOURCE` 로 설정의 `source` 를 그 실행에서만 바꿀 수 있다(플래그가 이긴다, 갱신 프로세스에도 같은 값이 간다).

- [#391](https://github.com/minjun0219/rocky/pull/391) [`afbd996`](https://github.com/minjun0219/rocky/commit/afbd996c30d7974bc73283c48292495b986ae484) Thanks [@minjun0219](https://github.com/minjun0219)! - statusline 진단 명령 둘을 더한다. `rocky statusline doctor [--session ID]` 는 읽는 설정·`source`·설정 폴더와 로그인된 계정·keychain 항목(macOS 면 후보 전부)·credentials 파일·캐시 폴더·guard·크레딧 상태·토큰을 찍고, `extraCommands` 를 statusline 과 같은 경로로 돌려 항목마다 결과(ok · 출력 없음 · 건너뜀 · 미설치 · 타임아웃 · 비정상 종료)를 보인다. `rocky statusline probe` 는 usage API 원본 응답을 보인다(필드 확인용).

- [#390](https://github.com/minjun0219/rocky/pull/390) [`e46832e`](https://github.com/minjun0219/rocky/commit/e46832ecde991908466bd617cda0ae99f1029428) Thanks [@minjun0219](https://github.com/minjun0219)! - 크레딧 guard 를 더한다. `rocky.json` 의 `statusline.guard: true` 와 `UserPromptSubmit` 훅 `rocky statusline guard` 를 걸면, 한도가 소진돼 크레딧이 차감되기 시작할 때(또는 최근 크레딧이 늘었을 때) prompt 를 막는다(exit 2). 설정 오류·데이터 없음·크레딧이 꺼진 계정·`source: none` 에서는 막지 않는다. `rocky statusline allow [30m|2h|off]` 로 잠시 풀거나 다시 켠다 — 계정과 상관없이 하나라 어느 터미널에서 불러도 된다.

### Patch Changes

- [#393](https://github.com/minjun0219/rocky/pull/393) [`71cbd8f`](https://github.com/minjun0219/rocky/commit/71cbd8feb9281f2a149417d11f38c4f88b4baf53) Thanks [@minjun0219](https://github.com/minjun0219)! - 보드의 "새 세션 띄우기"를 두 곳(탭 둘 · 웹과 CLI)에서 거의 동시에 누르면 같은 워크트리에 세션이 둘 뜰 수 있던 것을 고쳤다 — 진 쪽은 409 를 받는다.

- [#387](https://github.com/minjun0219/rocky/pull/387) [`098b51a`](https://github.com/minjun0219/rocky/commit/098b51a5475e5ed37d6568924f59f96edd971d8c) Thanks [@minjun0219](https://github.com/minjun0219)! - Cloudflare Access 로 들어온 웹 화면의 ⋯ 메뉴 맨 아래에 로그인한 이메일과 **로그아웃**을 보인다(`/cdn-cgi/access/logout` — Access 세션 전체가 끝난다). `/api/health` 가 그 이메일을 `accessUser` 로 알려 준다. 로컬·테일넷 화면에는 없다.

## 0.43.0

### Minor Changes

- [#379](https://github.com/minjun0219/rocky/pull/379) [`8d7ede7`](https://github.com/minjun0219/rocky/commit/8d7ede70eaf0d764dcb8ad917c43142f4f82ff54) Thanks [@minjun0219](https://github.com/minjun0219)! - `rocky statusline --full` 이 한도 경보를 깜빡이고 계정 배지를 붙인다. 5h/7d 가 임박(`alertPercent`)·소진으로 오르면 그 창이 6초 동안 0.5초마다 배지와 굵은 빨강을 오간 뒤 배지로 남는다(창이 리셋되면 다시 무장). `rocky.json` 의 `statusline.badges` 에 이메일을 키로 `emoji` 또는 `glyph`+`color` 를 적으면 로그인된 계정이 상태 줄 앞에 표시된다. cc-usage 와 같은 출력이다.

- [#381](https://github.com/minjun0219/rocky/pull/381) [`5fb55d2`](https://github.com/minjun0219/rocky/commit/5fb55d239d56fb26a113ea8cc8ec17fe29005404) Thanks [@minjun0219](https://github.com/minjun0219)! - `rocky statusline --full` 의 크레딧 금액이 쓰는 중인지 색으로 알려 준다. 안 쓸 때는 원래 색(사용률 그라데이션)을 회색 쪽으로 반 섞은 옅은 색이고, 쓰기 시작하면(이번 window 에서 늘었거나 15분 안에 늘어남) 3초에 걸쳐 원래 색으로 페이드한다. 다시 안 쓰면 바로 옅어진다. `rocky.json` 의 `statusline.creditFade: false` 로 끄면 cc-usage 와 같은 색이다.

## 0.42.0

### Minor Changes

- [#354](https://github.com/minjun0219/rocky/pull/354) [`1512d07`](https://github.com/minjun0219/rocky/commit/1512d07e637ff8e230769fd77632b16629721393) Thanks [@minjun0219](https://github.com/minjun0219)! - 웹 원격 제어 탭의 Antigravity 줄에 켜기·끄기 버튼이 생겼다(로컬 화면에서만 — 폰·테일넷 화면에는 없다). rc 블록이 없는 기기라도 `agy` 가 있으면 원격 제어 탭이 보이고 Antigravity 줄만 나온다. 실패하면 사유를 줄 아래에 남긴다.

- [#353](https://github.com/minjun0219/rocky/pull/353) [`b639c8b`](https://github.com/minjun0219/rocky/commit/b639c8ba417486cb61282300a958b2a44199f8a8) Thanks [@minjun0219](https://github.com/minjun0219)! - Antigravity 원격 제어(`agy remote-control`)를 rocky 에서 켜고 끈다 — `rocky rc agy [start|stop]` 과 `POST /api/rc/antigravity/{start,stop}`(로컬 전용). `GET /api/rc/servers` 의 `antigravity` 는 이제 `rc` 블록과 상관없이 `agy` 가 설치돼 있으면 실린다.

- [#342](https://github.com/minjun0219/rocky/pull/342) [`735691b`](https://github.com/minjun0219/rocky/commit/735691b6e5d31d4651ce7e1a724b7f2862709b16) Thanks [@minjun0219](https://github.com/minjun0219)! - Antigravity(`agy`)에서 rocky 보드를 쓸 수 있다. 레포의 `antigravity/` 번들을 `agy plugin install`로 깔면 데몬 MCP(`todo_*` · `note_*`)와 `board` 스킬, 참조(`rocky-12`)를 보드 항목으로 읽는 규칙이 붙는다. `board` 스킬에는 작업을 Antigravity로 넘기는 절차가 생겼다 — 브리프를 할 일의 `description`에 쓰고, agy가 actor `antigravity`로 착수·댓글·완료를 남기면 Claude Code 세션에 보드 변경으로 주입된다. 워크로그는 agy가 MCP 서버를 플러그인 폴더에서 띄워 프로젝트를 못 가리므로 아직 빠져 있다.

- [#370](https://github.com/minjun0219/rocky/pull/370) [`160ae53`](https://github.com/minjun0219/rocky/commit/160ae53684994a7bafe811925a1325d899add163) Thanks [@minjun0219](https://github.com/minjun0219)! - 웹 피드의 "내 차례"에 사람 답을 기다리는 background 세션(`claude agents` 의 `blocked`)을 올린다. 제목은 그 세션이 기다리는 것이고, 누르면 에이전트 탭으로 간다. 그 세션이 든 할 일이 있으면 진행 행(멈춤)만 남긴다. 에이전트 탭을 끄면 이 행도 없다.

- [#374](https://github.com/minjun0219/rocky/pull/374) [`65ef477`](https://github.com/minjun0219/rocky/commit/65ef4772bc1f4b26c486a14deb80c08ba767d353) Thanks [@minjun0219](https://github.com/minjun0219)! - `rocky rc nightly --dry-run`(`GET /api/rc/nightly/preview`)으로 야간 재시작 리허설을 본다 — 지금 설치된 claude 버전으로 떠 있는 rc 서버마다 다시 띄울지 · 기다릴지 · 건너뛸지를 보인다. 아무것도 내리거나 띄우지 않는다.

- [#376](https://github.com/minjun0219/rocky/pull/376) [`0834baa`](https://github.com/minjun0219/rocky/commit/0834baaecb080564ea6b383827457c89ef3bf2c5) Thanks [@minjun0219](https://github.com/minjun0219)! - 야간 재시작이 바쁜 rc 서버를 `rc.nightly.busyUntil`(기본 07:00)까지 5분마다 다시 보고 쉬면 다시 띄운다. 내렸는데 못 띄운 서버는 그때까지 1 · 2 · 4 · 8 · 16분 간격으로 다시 띄워 보고, 그래도 안 되면 되살림 표식을 남겨 감시에 넘긴다.

- [#375](https://github.com/minjun0219/rocky/pull/375) [`dbf0e91`](https://github.com/minjun0219/rocky/commit/dbf0e91a6e839478180f0c2c97880997b9ec0912) Thanks [@minjun0219](https://github.com/minjun0219)! - `rc.nightly` 블록을 두면 데몬이 매일 새벽(기본 04:30)에 `claude update` 를 돌리고, 기동 버전이 설치 버전과 다르면서 쉬는 rc 서버만 다시 띄운다. 고정 하나를 먼저 띄워 보고 나머지를 내리며, 내리기 직전마다 네트워크를 확인한다. 못 띄운 서버는 되살림 표식을 남겨 감시(`rc.supervise`)가 비고정이어도 서버 모드로 띄운다. 맥이 그 시각에 자고 있었으면 깬 뒤 한 번 돈다. 현황 API 에 `nightly`(시각 · 도는 중 · 마지막 결과)가 실린다.

- [#373](https://github.com/minjun0219/rocky/pull/373) [`a30e6b4`](https://github.com/minjun0219/rocky/commit/a30e6b43056503c574253ca40f960bf05818fba4) Thanks [@minjun0219](https://github.com/minjun0219)! - 데몬이 rc 서버를 띄울 때마다 그때 설치된 claude 버전을 `rc/<라벨>.version` 에 남기고, 감시(`rc.supervise`)는 야간 재시작이 내리고 못 띄워 되살림 표식(`rc/<라벨>.revive`)이 남은 서버를 비고정이어도 서버만 띄운다.

- [#345](https://github.com/minjun0219/rocky/pull/345) [`2484080`](https://github.com/minjun0219/rocky/commit/24840807704eada28691c707a35134cc7dc8b126) Thanks [@minjun0219](https://github.com/minjun0219)! - 데몬이 `claude rc` 서버를 띄우고 다시 띄운다. `rocky rc start|restart <라벨>`(`--fresh` · `--wait`)과 로컬 전용 `POST /api/rc/servers/:label/{start,restart}` 가 생겼다. 재시작은 열린 세션이 있으면 이어받고(`-c`), `already served` 면 45·90초 뒤 다시 띄운다. 서버는 데몬과 다른 프로세스 그룹으로 떠서 데몬을 재시작·업데이트해도 살아 있다.

- [#360](https://github.com/minjun0219/rocky/pull/360) [`308d0b9`](https://github.com/minjun0219/rocky/commit/308d0b9a2686eedc2fa46eb0385e8d165862593b) Thanks [@minjun0219](https://github.com/minjun0219)! - `rc.supervise` 를 켜면 데몬이 2분마다 고정 rc 서버를 보고 꺼져 있으면 스스로 띄운다(연달아 못 뜨면 30분까지 쉬었다 다시). 데몬 맥락의 claude 자격이 끊기거나 돌아오면 macOS 배너로 한 번씩 알리고, 회복 뒤에는 끊기기 전에 뜬 서버를 "자격 의심"으로 표시한다. `rocky rc` 와 현황 API 에 감시 상태 · `authSuspect` 가 실린다.

- [#346](https://github.com/minjun0219/rocky/pull/346) [`91aeb1f`](https://github.com/minjun0219/rocky/commit/91aeb1fa9ae623e16f5b7dc2350f08bb84b0dfe4) Thanks [@minjun0219](https://github.com/minjun0219)! - 웹 원격 제어 탭에서 rc 서버를 띄우고 다시 띄운다. 꺼진 대상은 **띄우기**, 떠 있는 대상은 **재시작**(붙은 원격 세션이 끊긴다고 한 번 더 묻는다)이고, 진행 중에는 상태와 실패 이유를 행에 보여 준다. 로컬 주소로 연 화면에서만 보인다.

- [#358](https://github.com/minjun0219/rocky/pull/358) [`9278e3f`](https://github.com/minjun0219/rocky/commit/9278e3fcb38c86d2e1b8bde169c0dfd81b0e478c) Thanks [@minjun0219](https://github.com/minjun0219)! - `rocky statusline --full` 이 다른 도구의 statusline 줄을 덧붙인다. `rocky.json` 의 `statusline.extraCommands[]`(`command` argv · `timeoutMs` 기본 300)에 적은 명령을 나란히 돌려, rocky 줄과 보드 줄 사이에 설정 순서대로 붙인다. `{{cwd}}` · `{{session_id}}` 를 치환하고(쓰인 값이 비면 건너뜀), 늦거나 실패한 명령은 그 줄만 빠진다. cc-usage 의 `extra_commands` 와 같은 출력이다.

- [#340](https://github.com/minjun0219/rocky/pull/340) [`59d77d4`](https://github.com/minjun0219/rocky/commit/59d77d4e57a69a27fe328a9c9636bce50bd2a889) Thanks [@minjun0219](https://github.com/minjun0219)! - `rocky statusline --full` 이 cc-usage 와 같은 statusline 을 그린다 — 경로·git 줄(브랜치·변경 수·ahead/behind), 모델·effort·ctx·5h/7d 줄(남은 비율·리셋 시각·경보 배지), 한도가 소진되면 크레딧 줄, 그 아래 보드 줄. 앞의 줄들은 CLI 가 직접 그려 데몬이 없어도 남는다. 한도 설정은 `rocky.json` 최상위 `statusline` 블록(`source` · `alertPercent`). `extra_commands`·usage API 조회는 다음 조각에서 더한다.

- [#365](https://github.com/minjun0219/rocky/pull/365) [`71db87d`](https://github.com/minjun0219/rocky/commit/71db87d51e50cf8c7f4285689fa7646017ae987e) Thanks [@minjun0219](https://github.com/minjun0219)! - `rocky statusline --full` 이 usage API 로 한도·크레딧을 읽는다. `source: api`·`auto`(stdin 에 한도가 없을 때)나 한도가 소진됐을 때 statusline 이 갱신 프로세스를 detached 로 띄우고 기다리지 않는다 — 결과(5h/7d, 남은 크레딧, 실패 이유·stale)는 다음 렌더에 나온다. 계정은 세션의 `CLAUDE_CONFIG_DIR` 로 판단하고, 토큰은 `tokenEnv` → keychain(기본 설정 폴더만) → `.credentials.json` 순으로 읽기만 한다. 캐시(`~/.cache/rocky/statusline/`)는 설정 폴더와 로그인된 계정별로 갈라 계정을 바꿔도 남의 숫자를 그리지 않는다. 설정: `statusline` 블록의 `configDir` · `pollSeconds` · `creditPollSeconds` · `creditDivisor` · `currency` · `alwaysShowCredits` · `keychainService` · `credentialsFile` · `tokenEnv`. cc-usage 의 refresh 와 같은 동작이다.

- [#349](https://github.com/minjun0219/rocky/pull/349) [`ac6fbba`](https://github.com/minjun0219/rocky/commit/ac6fbba4b4a4ddc976c0ced01c453906cf347e95) Thanks [@minjun0219](https://github.com/minjun0219)! - 토큰 추천에 `fresh-session` 규칙을 더한다 — 맥락이 무거운데(요청당 캐시 읽기, 기본 20만↑) 출력이 작으면(기본 1만↓) 기계적인 후속은 새 세션이나 가벼운 서브에이전트로 넘기라고 권한다. 근거에 턴 평균 캐시 읽기(`avgCacheReadTokens`)와 요청당 캐시 읽기(`avgContextTokens`)를 싣고, 기준은 `tokens.recommend` 의 `freshSession` · `heavyContextTokens` · `freshSessionOutputTokens` 로 바꾼다.

- [#332](https://github.com/minjun0219/rocky/pull/332) [`0d27621`](https://github.com/minjun0219/rocky/commit/0d276213ecc23e185b39082b97e6cbf8bc89fd68) Thanks [@minjun0219](https://github.com/minjun0219)! - 기본 브랜치 검증: 실패하면 그 자리에서 한 번 더 돌고 두 번 연속 실패일 때만 알린다. `rocky verify --rerun [보드]`(`POST /api/verify/rerun`, 로컬 전용)로 같은 커밋을 다시 돌려 환경 탓 거짓 실패를 푼다. 대상 디렉터리의 `runs.jsonl` 에 실행 이력(통과·실패·끊김)과 검증을 못 한 이유를 남긴다.

- [#369](https://github.com/minjun0219/rocky/pull/369) [`c92be1e`](https://github.com/minjun0219/rocky/commit/c92be1eb510724ea5cb776d1c180a99538df6cf0) Thanks [@minjun0219](https://github.com/minjun0219)! - 웹 UI에 **에이전트** 탭을 더한다. `claude agents` 가 보는 세션을 내 차례(사람 답을 기다리는 background 세션) → 실행 중 → 쉬는 중으로 묶어 보이고, background 세션에는 Claude 가 남긴 요약(기다리는 것 · 지금 하는 일)을 한 줄 싣는다. ⋯ 메뉴에서 탭을 끌 수 있다.

- [#348](https://github.com/minjun0219/rocky/pull/348) [`18e79f6`](https://github.com/minjun0219/rocky/commit/18e79f687edd6d5006ef6532b09fec2fa08cabd7) Thanks [@minjun0219](https://github.com/minjun0219)! - `rocky mcp worklog --roots` 가 생겼다. 서버를 작업 폴더 밖에서 띄우는 호스트(Antigravity)를 위해, 도구를 부를 때마다 클라이언트에 MCP `roots/list` 를 물어 그 폴더를 워크로그의 프로젝트로 쓴다. 답을 못 받으면 서버 cwd 로 물러서지 않고 에러를 낸다. Antigravity 번들에 워크로그와 `worklog` 스킬을 다시 넣었다. 플래그 없이 띄우는 Claude Code·Codex·opencode 는 그대로다.

### Patch Changes

- [#351](https://github.com/minjun0219/rocky/pull/351) [`5d2b413`](https://github.com/minjun0219/rocky/commit/5d2b413a7aef675bf9dd71c09cbb433fe5c66973) Thanks [@minjun0219](https://github.com/minjun0219)! - 오래 조용했던 세션에 보드 변경이 100건 넘게 밀려 있으면, 훅이 그 너머를 건너뛰지 않고 다음 프롬프트에 이어서 싣는다.

- [#377](https://github.com/minjun0219/rocky/pull/377) [`a5b5264`](https://github.com/minjun0219/rocky/commit/a5b5264224b20057a6ab20cd78f11260203a6abd) Thanks [@minjun0219](https://github.com/minjun0219)! - 원격 제어 화면과 `rocky rc` 에 기동 버전이 설치 버전과 다른 서버를 `구버전` 으로 보이고, 웹 원격 제어 탭의 환경 카드에 야간 재시작 시각과 마지막 결과(재시작 · 건너뜀 · 못 띄움)를 한 줄로 보인다.

- [#361](https://github.com/minjun0219/rocky/pull/361) [`cd8c0cd`](https://github.com/minjun0219/rocky/commit/cd8c0cd8e3a534b258f3b7dd8133669eba113cbd) Thanks [@minjun0219](https://github.com/minjun0219)! - 웹 원격 제어 요약 줄에 감시 상태(`감시 중` · 데몬 자격이 끊기면 `자격 끊김`)를 보이고, 자격이 끊기기 전에 뜬 서버 행에 다시 띄우기를 권하는 줄을 더한다.

- [#367](https://github.com/minjun0219/rocky/pull/367) [`9582e15`](https://github.com/minjun0219/rocky/commit/9582e1589ecc0f183ee5fbc682c694095a802d61) Thanks [@minjun0219](https://github.com/minjun0219)! - `claude agents --json` 의 pid 없는 background 세션(사람 답을 기다리며 잠든 `blocked`)을 세션 목록에서 버리지 않는다. 버리면 그 세션이 든 할 일이 "세션 없음" 으로 판정되어 24시간 뒤 자동으로 풀렸다.

- [#368](https://github.com/minjun0219/rocky/pull/368) [`f8af240`](https://github.com/minjun0219/rocky/commit/f8af2401307d8cae87bb9b6e137b49e685628244) Thanks [@minjun0219](https://github.com/minjun0219)! - `GET /api/sessions` 의 background 행에 Claude Code 가 `~/.claude/jobs/<id>/state.json` 에 남긴 작업 요약을 `job` 으로 싣는다 — `detail`(지금 하는 일) · `needs`(사람에게 기다리는 것) · `updatedAt`. 내부 파일이라 못 읽거나 형식이 다르면 그 행만 `job` 이 없다.

- [#331](https://github.com/minjun0219/rocky/pull/331) [`7a74183`](https://github.com/minjun0219/rocky/commit/7a7418381ec8c484343ab341ea4952d59ceadd74) Thanks [@minjun0219](https://github.com/minjun0219)! - `/clear` 된 세션은 더 이상 옛 세션의 PR·수집함 알림으로 깨우지 않는다. 남은 구독은 감시만 이어 가고, 새 세션으로 넘길지·지켜보기만 할지·해지할지는 `POST /api/sessions/cleared` 로 정한다(웹 화면은 다음 변경).

- [#337](https://github.com/minjun0219/rocky/pull/337) [`abf108f`](https://github.com/minjun0219/rocky/commit/abf108f4aa60d8e27332451159973e9b81c6bb0b) Thanks [@minjun0219](https://github.com/minjun0219)! - 웹 GitHub 탭 "세션 전달" 칸에 `/clear` 된 세션이 뜨고, 남은 구독을 새 세션으로 넘길지·지켜보기만 할지·해지할지 고를 수 있다.

## 0.41.0

### Minor Changes

- [#306](https://github.com/minjun0219/rocky/pull/306) [`357ce9d`](https://github.com/minjun0219/rocky/commit/357ce9dc59a2ba0f553fdce7131a5b89c0899ab6) Thanks [@minjun0219](https://github.com/minjun0219)! - "에이전트에게 보내기"(핸드오프)가 쉬는 세션을 바로 깨운다 — 대상 세션이 받은편지함을 등록했으면 데몬이 그 소켓에 요청을 꽂아 턴을 연다(PR 알림과 같은 길). 응답에 `woke` 가 실리고 웹 상세는 "세션을 깨웠다" / "세션이 다음 턴에 집는다" 를 보여 준다.

- [#315](https://github.com/minjun0219/rocky/pull/315) [`1853ea9`](https://github.com/minjun0219/rocky/commit/1853ea9686d08afef93e029a81bc574e08ad6b5c) Thanks [@minjun0219](https://github.com/minjun0219)! - `/rocky:review-fix` 의 기계적인 단계를 가벼운 모델의 서브에이전트가 맡는다 — 충돌 해소·판단이 필요 없는 리뷰 수정·CI 실패는 `quick-fix`(Sonnet, 자기 워크트리), 머지 뒤 정리는 `merge-cleanup`(Haiku). 대화가 길어진 세션이 몇 줄짜리 손질을 위해 긴 맥락을 다시 읽지 않는다.

- [#319](https://github.com/minjun0219/rocky/pull/319) [`89cb190`](https://github.com/minjun0219/rocky/commit/89cb1903e5979c30c7fe2abc63ec95b5b4e0bbd2) Thanks [@minjun0219](https://github.com/minjun0219)! - 데몬이 기본 브랜치(보통 `main`)의 새 커밋을 받아 게이트를 다시 돈다 — `rocky.json` 의 `verify.targets[]` 로 켜고, 전용 detached 워크트리에서 단계를 차례로 돌며 실패·복구만 배너로 알린다. `rocky verify` · `GET /api/verify` 로 마지막 결과를 본다.

- [#321](https://github.com/minjun0219/rocky/pull/321) [`c746503`](https://github.com/minjun0219/rocky/commit/c74650311f66691041ab97e757e613129906d846) Thanks [@minjun0219](https://github.com/minjun0219)! - `claude rc` 서버 현황을 데몬이 보여 준다. `rocky.json` 의 `rc` 블록(`root` · `pinned` · `targets`)에 폴더를 적으면 `GET /api/rc/servers` 와 `rocky rc` 가 대상별 실행 여부 · 열린 세션 수 · 떠 있은 시간, 목록 밖 폴더에서 도는 서버, 로그인 상태, Antigravity 상태를 낸다. 보기만 하고 서버를 띄우거나 내리지 않는다.

- [#330](https://github.com/minjun0219/rocky/pull/330) [`d0ba25f`](https://github.com/minjun0219/rocky/commit/d0ba25f7d5c415d422b4cb21cd5b146b4287a3ab) Thanks [@minjun0219](https://github.com/minjun0219)! - 웹 UI 에 원격 제어 탭과 피드 요약 줄을 더한다. 피드 머리 아래 `원격 제어 7/13 · 세션 4` 한 줄이 고정 서버가 꺼졌을 때 경고를 붙이고, 누르면 고정 서버를 펼친다. 원격 제어 탭은 고정 · 부를 수 있음 · 대상 밖 서버와 로그인 · Antigravity 상태를 보여 준다. rc 가 꺼진 기기에서는 둘 다 나타나지 않고, ⋯ 메뉴에서 탭을 끌 수 있다.

- [#307](https://github.com/minjun0219/rocky/pull/307) [`eb5095a`](https://github.com/minjun0219/rocky/commit/eb5095ae06c138b5f40b5d8416c98679f4ee62f0) Thanks [@minjun0219](https://github.com/minjun0219)! - PR 을 링크한 할 일을 머지로 닫는다. 데몬이 구독한 PR 의 머지를 보면 그 PR 주소를 링크한 할 일을 완료하고 댓글을 남긴다(링크한 다른 PR 이 아직 열려 있으면 기다리고, 머지 없이 닫히면 댓글만). `/rocky:review-request` 는 세션이 든 할 일에 PR 링크를 붙이고, Stop 훅은 PR 을 기다리는 할 일을 더 묻지 않는다.

- [#313](https://github.com/minjun0219/rocky/pull/313) [`a972418`](https://github.com/minjun0219/rocky/commit/a972418913e10daeff91bd6c3d7e2f65d6274323) Thanks [@minjun0219](https://github.com/minjun0219)! - Claude Code 세션의 모델·effort·토큰을 데몬이 트랜스크립트에서 색인한다. `rocky tokens`(모델 × effort 합계), `rocky tokens here`(지금 세션과 추천), `/api/tokens/*`, MCP `token_summary` · `token_current_session`으로 보고, effort를 낮추거나 Sonnet으로 바꿀지 규칙 기반으로 권한다. 훅 설정은 필요 없다.

### Patch Changes

- [#335](https://github.com/minjun0219/rocky/pull/335) [`fccddfa`](https://github.com/minjun0219/rocky/commit/fccddfadae82e37ea40d658b82858cbbb039f35c) Thanks [@minjun0219](https://github.com/minjun0219)! - rocky 를 쓰는 에이전트용 스킬 다섯 개를 더한다 — `pull-request`(PR 구독과 받은편지함 메시지별 할 일), `token-usage`(모델·effort 고르기), `branch-verify`(기본 브랜치 검증 읽기), `handoff`(넘겨받은 일의 start→done), `worklog`(무엇을 언제 기록하나). 상황이 맞을 때만 로드된다.

- [#292](https://github.com/minjun0219/rocky/pull/292) [`0b063d7`](https://github.com/minjun0219/rocky/commit/0b063d7481ce25fb8e2d14a568bd89357cdda13c) Thanks [@minjun0219](https://github.com/minjun0219)! - 피드의 수집함 행이 출처를 말하고("todoist 수집함에 보드로 안 옮긴 항목 2건"), 누르면 항목이 원래 앱 링크로 펼쳐진다.

- [#289](https://github.com/minjun0219/rocky/pull/289) [`dc4b70f`](https://github.com/minjun0219/rocky/commit/dc4b70f17810de3460a28600e720ce9211587c9d) Thanks [@minjun0219](https://github.com/minjun0219)! - 피드 "내 차례" 의 `읽지 않은 댓글 N건 더` 를 누르면 나머지가 그 자리에 펼쳐진다(목록에서 💬 를 찾지 않아도 된다).

- [#334](https://github.com/minjun0219/rocky/pull/334) [`34a5277`](https://github.com/minjun0219/rocky/commit/34a527717a63e9742300a1f029b08f081d9ff6ae) Thanks [@minjun0219](https://github.com/minjun0219)! - 쉬던 세션을 받은편지함 메시지가 깨운 턴에 훅이 같은 PR 전이를 또 주입하지 않는다.

- [#310](https://github.com/minjun0219/rocky/pull/310) [`f23182d`](https://github.com/minjun0219/rocky/commit/f23182dac7d94641f0fafe3b83675648d16daa6f) Thanks [@minjun0219](https://github.com/minjun0219)! - 세션 받은편지함 등록을 DB 에 남겨 데몬이 다시 떠도 PR 알림(머지 등)이 버려지지 않게 한다. 못 보낸 전달은 이유(`reason`)가 `/api/deliveries` 와 웹의 ✗ 툴팁에 남는다.

- [#329](https://github.com/minjun0219/rocky/pull/329) [`6267e72`](https://github.com/minjun0219/rocky/commit/6267e722519024698b18208bf12f32923c7e7235) Thanks [@minjun0219](https://github.com/minjun0219)! - PR 상태 훅 주입과 rocky 채널이 그 PR 을 구독한 세션에만 간다. 같은 보드의 다른 세션은 남의 PR 머지 후보·충돌을 더 이상 받지 않고, 받은편지함으로 이미 받아들인 전이는 다음 턴 훅이 다시 넣지 않는다(승인 창에서 보류·거절된 것은 넣는다). 채널은 `/clear` 뒤에도 지금 세션을 따라간다.

- [#309](https://github.com/minjun0219/rocky/pull/309) [`9e5f29b`](https://github.com/minjun0219/rocky/commit/9e5f29b6cce45eaa0cb0804ef60a39a89f89126f) Thanks [@minjun0219](https://github.com/minjun0219)! - 처음 본 PR 이 이미 머지·닫힘이면 그 전이도 첫 tick 에 낸다 — 구독한 뒤 첫 tick 전에 머지됐거나 머지된 PR 을 구독해도 링크한 할 일이 닫히고 구독이 걷힌다.

- [#305](https://github.com/minjun0219/rocky/pull/305) [`cc5c89e`](https://github.com/minjun0219/rocky/commit/cc5c89ea8d6be0484c698e78d782526806533009) Thanks [@minjun0219](https://github.com/minjun0219)! - 잠깐 쉬었다 온 첫 요청(요약·목록)이 세션 조회(`claude agents --json`)를 3초 기다리던 것을 없앤다 — 지난 세션 목록을 30분까지 바로 쓰고 뒤에서 새로 받으며, 데몬이 뜰 때 미리 받아 둔다. 세션 없는 진행중을 자동으로 멈추는 스윕은 캐시 없는 목록으로 판단한다.

- [#308](https://github.com/minjun0219/rocky/pull/308) [`10936d8`](https://github.com/minjun0219/rocky/commit/10936d861061275a97e5b3fb4271811d1b3426e5) Thanks [@minjun0219](https://github.com/minjun0219)! - 할 일 상세의 PR 링크 칩에 데몬이 감시 중인 PR 의 상태(충돌·확인·머지·CI·결정 필요·초안)를 붙인다.

- [#317](https://github.com/minjun0219/rocky/pull/317) [`05ec33a`](https://github.com/minjun0219/rocky/commit/05ec33a6c1a874e1fd627c3781dcd8a2ddf7fc1d) Thanks [@minjun0219](https://github.com/minjun0219)! - 토큰 추천의 `switch-to-sonnet` 이 바꾸는 시점을 함께 말한다 — 다음 작업 경계(커밋 직후)나 새 세션. 세션 중간에 모델을 바꾸면 캐시를 새로 쌓아 긴 세션에선 오히려 비싸다. effort 변경은 캐시를 지키므로 `lower-effort` 문구는 그대로다.

- [#290](https://github.com/minjun0219/rocky/pull/290) [`f1e34d1`](https://github.com/minjun0219/rocky/commit/f1e34d124bd5c5f3d5eab6862975f3c94cd33c4a) Thanks [@minjun0219](https://github.com/minjun0219)! - 폰(터치 기기)에서 할 일 행의 칩 줄이 오른쪽 끝 열로 밀려 깨지던 것과, p1·p2 우선순위 칩이 글자 없는 단색 상자로 보이던 것을 고친다.

- [#294](https://github.com/minjun0219/rocky/pull/294) [`d2ff23a`](https://github.com/minjun0219/rocky/commit/d2ff23a8b2c4d96d588f277d989a39a0ac30a33c) Thanks [@minjun0219](https://github.com/minjun0219)! - 웹 UI 점검에서 나온 것들을 고친다 — 폰 상세의 닫기가 시트 바닥 "닫기" 줄로 바뀌어 댓글 입력칸을 가리지 않고, 폰에서 글자 버튼(편집·새로고침·설정·열린 PR·레포 링크)이 손가락 크기가 되고, 작업로그가 마크다운 원문 대신 글자로 보이고 MCP 도구 이름이 짧아지고, 상세에서 내부 id·p4 칩과 완료한 일의 "시작" 버튼을 뺀다.

- [#293](https://github.com/minjun0219/rocky/pull/293) [`1a6ad93`](https://github.com/minjun0219/rocky/commit/1a6ad93e40cb3d07040e680c1b1cd7632c84d75b) Thanks [@minjun0219](https://github.com/minjun0219)! - 보던 탭이 주소에 실린다(`?view=todos`) — 새로고침·앱 전환(폰 Safari 가 페이지를 다시 부른다)·뒤로가기·탭 복제가 모두 같은 탭으로 돌아온다. 맨 주소는 여전히 피드(첫 화면)다.

- [#328](https://github.com/minjun0219/rocky/pull/328) [`3e0d596`](https://github.com/minjun0219/rocky/commit/3e0d59628bfacd93d9643ca2631d82a6d5c395e3) Thanks [@minjun0219](https://github.com/minjun0219)! - 웹 UI 문구를 다듬는다. 줄표로 이은 안내문은 문장으로 끊고 영문 뒤 조사는 붙여 쓰며 용어를 하나로 맞춘다(할 일·노트·실행 중·진행 중·머지 후보). 화면 문장은 해요체로 바꾼다. 새 노트의 기본 제목이 "새 메모"에서 "새 노트"로 바뀐다.

- [#295](https://github.com/minjun0219/rocky/pull/295) [`d4c0854`](https://github.com/minjun0219/rocky/commit/d4c08541d0798938d1fbc4bdf74cc8728fdc9915) Thanks [@minjun0219](https://github.com/minjun0219)! - 한글 라벨이 "섹 션 · 읽지 않은 댓글" 처럼 띄엄띄엄 보이던 것을 고친다 — 고정폭 글꼴 뒤에 본문 한글 글꼴을 두고, 고정폭 라벨의 낱말 사이를 줄이고, 한글 라벨에 준 자간을 걷는다.

- [#299](https://github.com/minjun0219/rocky/pull/299) [`40b4b83`](https://github.com/minjun0219/rocky/commit/40b4b8359c53b6fa32b4ae6531e11997481a0401) Thanks [@minjun0219](https://github.com/minjun0219)! - 폰에서 두 글자 버튼(보드 "편집", 수집함 "설정" 등)도 손가락 폭이 된다 — 높이만 44px 이고 폭은 21px 였다.

## 0.40.0

### Minor Changes

- [#273](https://github.com/minjun0219/rocky/pull/273) [`0c4caf9`](https://github.com/minjun0219/rocky/commit/0c4caf971739799572f116bb9a8768adc6a69e9b) Thanks [@minjun0219](https://github.com/minjun0219)! - 데몬이 작업로그·사용 로그(JSONL)를 로그 색인(`logs.db`)으로 옮겨 `GET /api/logs/worklog` 로 보드·할 일·검색어별로 읽게 한다 — 쓰기는 그대로 JSONL 이고, 색인은 전용 스레드가 1분마다 새 줄만 옮기며 지워도 다시 만든다. 보드 작업로그 탭·할 일 상세·통계의 바탕이다.

- [#281](https://github.com/minjun0219/rocky/pull/281) [`6431606`](https://github.com/minjun0219/rocky/commit/64316065d493206231410239af9ba79c447fceb6) Thanks [@minjun0219](https://github.com/minjun0219)! - 작업로그 탭에 통계(최근 30일)를 더한다 — 회고(레포별·요일별 턴, 턴이 몰린 할 일)와 rocky 개선(느린 표면·실패하는 표면·안 쓴 표면). `GET /api/logs/stats` 가 로그 색인에서 낸다.

- [#282](https://github.com/minjun0219/rocky/pull/282) [`1129c27`](https://github.com/minjun0219/rocky/commit/1129c27583f435b7b34251ba54eb321fa93733bd) Thanks [@minjun0219](https://github.com/minjun0219)! - 웹 노트 동시 편집을 웹소켓 하나(`GET /api/ws`)로 옮긴다 — 열린 노트가 연결 하나로 문서·편집·프레즌스를 주고받아, 노트마다 SSE 를 열고 편집마다 POST 하던 것이 없어진다(호스트당 연결 한도·요청 왕복). 다른 사이트에서 연 소켓은 핸드셰이크에서 거부하고, 소켓을 못 열면 예전 HTTP 길로 간다.

- [#288](https://github.com/minjun0219/rocky/pull/288) [`ce4c59e`](https://github.com/minjun0219/rocky/commit/ce4c59e64c18de0e825df3a95b3ffe7b1f932867) Thanks [@minjun0219](https://github.com/minjun0219)! - 웹 보드 UI 전면 리디자인 (A안: 미니멀 관제판) — Pretendard Variable 한글 가변 폰트 번들링 및 적용, 12px 미만(11px) 텍스트 상향으로 시인성 개선, 상단 42px 초슬림 단일 바 헤더(피드·할 일·노트·작업로그·GitHub 5개 탭 및 실시간 에이전트 인디케이터 통합), 4px 마이크로 뱃지 기반 작업 목록 및 완료 작업 하단 접기(Collapsible), 상세 모달 및 반응형 최적화.

- [#278](https://github.com/minjun0219/rocky/pull/278) [`2f52626`](https://github.com/minjun0219/rocky/commit/2f52626f2f75a6c4dfcb83c8102ef4680cf9091e) Thanks [@minjun0219](https://github.com/minjun0219)! - TUI(`rocky tui`, `rocky-tui` 바이너리)를 걷어낸다 — 90일 동안 한 번도 쓰이지 않았고, 보드 화면은 웹 UI 하나로 간다. 릴리스 tarball 에서도 빠진다.

- [#280](https://github.com/minjun0219/rocky/pull/280) [`d4656e7`](https://github.com/minjun0219/rocky/commit/d4656e71b8a8a1d52c261ba8b5f7e4147e5d8f1b) Thanks [@minjun0219](https://github.com/minjun0219)! - 웹 보드에 **작업로그** 탭을 더한다 — 고른 보드(레포)의 작업 기록을 최신순으로 보고 찾기·종류로 거른다. 할 일 상세의 타임라인에는 그 할 일을 든 세션의 턴이 "작업" 줄로 끼어든다. 백그라운드 알림·셸 출력은 원문 대신 짧은 이름으로 보인다.

### Patch Changes

- [#287](https://github.com/minjun0219/rocky/pull/287) [`c6f7d8b`](https://github.com/minjun0219/rocky/commit/c6f7d8bdfe448a60d74a784376740f8c2b77d7a7) Thanks [@minjun0219](https://github.com/minjun0219)! - `/rocky:review-fix` 가 Codex 가 사용 한도에 걸렸다고 알린 PR(`verdict: limited`)을 봇 리뷰를 기다리느라 머지 후보 알림에서 막지 않는다 — 다른 조건이 맞으면 "봇 리뷰 없음(한도)" 을 적고 알린다.

- [#275](https://github.com/minjun0219/rocky/pull/275) [`c894abb`](https://github.com/minjun0219/rocky/commit/c894abbbbaceab8a85effe655793a82b05361eb6) Thanks [@minjun0219](https://github.com/minjun0219)! - Stop 훅(`log-turn`)이 턴마다 세션 트랜스크립트 전체(수십 MB)를 읽던 것을 파일 끝에서부터 읽게 한다 — 41MB 트랜스크립트에서 0.12초(릴리스) → 0.04초(디버그), 세션이 길어져도 늘지 않는다.

- [#279](https://github.com/minjun0219/rocky/pull/279) [`3fbad02`](https://github.com/minjun0219/rocky/commit/3fbad02693dce97dd0a65b4c740cf94f04d0c4f1) Thanks [@minjun0219](https://github.com/minjun0219)! - 턴 기록(`log-turn`)에 그 세션이 들고 있는 할 일을 `todo:<ref>` 태그로 붙인다 — 보드 할 일 상세가 작업 흐름을 모으는 바탕. 백그라운드 작업 알림·셸 출력처럼 하네스가 넣은 메시지는 요청 칸에 원문 대신 짧은 이름만 남긴다.

- [#276](https://github.com/minjun0219/rocky/pull/276) [`f2a0b20`](https://github.com/minjun0219/rocky/commit/f2a0b20e6135b3bd7e411d6064b357c466945ab9) Thanks [@minjun0219](https://github.com/minjun0219)! - 할 일 목록이 진행 중 항목의 세션 상태를 붙이려고 세션 목록(`claude agents`, 수백 ms)을 묻던 것을 기다리지 않게 한다 — 3초가 지난 값도 60초까지는 바로 주고 뒤에서 한 번 새로 받는다(진행 중 표시가 한 번 늦을 수 있다).

- [#272](https://github.com/minjun0219/rocky/pull/272) [`e9fba79`](https://github.com/minjun0219/rocky/commit/e9fba796e2babce662f82518c81c189fcc7bf5f2) Thanks [@minjun0219](https://github.com/minjun0219)! - 보드에서 넘겨받은 할 일을 끝낸 세션이 `done` 을 부르지 않아 "멈춤" 으로 남던 것을 막는다 — 핸드오프 주입문이 닫는 법(`done`/`stop`)까지 말하고, Stop 훅이 그 세션이 들고 있는 진행 중 할 일이 있으면 턴을 한 번 막고 닫았는지 묻는다(그 확인으로 이어진 턴은 다시 막지 않는다).

- [#274](https://github.com/minjun0219/rocky/pull/274) [`e306ad1`](https://github.com/minjun0219/rocky/commit/e306ad10b0075cd5b1b515441c2b021f1079264b) Thanks [@minjun0219](https://github.com/minjun0219)! - 웹 보드가 데몬 이벤트마다 보드 전체(요청 8개)를 다시 받던 것을 그 이벤트가 건드린 것만 받게 한다 — 할 일 수정은 3개, 노트 수정은 1개. 전체 보기에서 겹치던 할 일·핸드오프 요청도 한 번으로 줄였다.

## 0.39.0

### Minor Changes

- [#271](https://github.com/minjun0219/rocky/pull/271) [`7746df8`](https://github.com/minjun0219/rocky/commit/7746df87263a2cb9610eed24c2a3d43500b2cceb) Thanks [@minjun0219](https://github.com/minjun0219)! - 웹 보드의 알림 탭을 **피드**로 바꿔 맨 앞 탭이자 첫 화면으로 둔다. 피드는 PR 알림(결정 필요·머지 후보·오래된 충돌·CI 실패) 아래에 **내 차례**(넘김·멈춘 진행·읽지 않은 댓글·수집함)를 함께 싣는다 — "돌고 있음" 은 탭 위로 올라가 어느 탭에서든 늘 보인다. 지난번에 본 탭을 기억해 열지 않는다.

### Patch Changes

- [#270](https://github.com/minjun0219/rocky/pull/270) [`f571f2b`](https://github.com/minjun0219/rocky/commit/f571f2b40d7f11fa56d903dd0c7d7809268d618d) Thanks [@minjun0219](https://github.com/minjun0219)! - 웹 보드의 "할 일" 에서 "전체" 를 고르면 보드별 묶음을 머리(이름 · 항목 수)를 눌러 접을 수 있다 — 접힘은 브라우저에 남는다.

- [#268](https://github.com/minjun0219/rocky/pull/268) [`2c1f29e`](https://github.com/minjun0219/rocky/commit/2c1f29e54223af0af475159de0326200eac94075) Thanks [@minjun0219](https://github.com/minjun0219)! - 웹 보드의 할 일 설명을 노트와 같은 마크다운 편집기(서식 툴바·⌘B/⌘K)로 편집한다 — 고정 높이 스크롤 상자 대신 글 길이만큼 자라고, 저장은 ⌘Enter, 취소는 Esc. 상세 드로어가 버전 줄에 덮이거나 좁은 화면의 닫기 버튼이 마지막 줄을 가리던 것도 고쳤다.

## 0.38.1

### Patch Changes

- [#266](https://github.com/minjun0219/rocky/pull/266) [`4dd633e`](https://github.com/minjun0219/rocky/commit/4dd633e2f230e0f79774c33cc1c30977814dcc07) Thanks [@minjun0219](https://github.com/minjun0219)! - `rocky update`·`rocky daemon restart` 가 launchd 밖에서 뜬 옛 데몬(고아)이 포트를 쥔 경우에도 교체한다 — 고아를 pid 로 내리고 launchd 의 새 데몬이 목표 버전으로 응답하는지 확인하며, 버전이 목표와 다르면 ✓ 대신 실패로 끝난다(바이너리를 못 받은 경우 포함). `rocky daemon status` 는 포트를 쥔 데몬이 launchd 의 것이 아니면 ⚠ 로 알린다. launchd job 이 있으면 CLI 가 데몬을 따로 띄우지 않고 launchd 에게 맡기며, launchd 가 띄운 데몬은 포트가 차 있으면 끝나는 대신 기다렸다 이어받는다.

## 0.38.0

### Minor Changes

- [#265](https://github.com/minjun0219/rocky/pull/265) [`c4297ee`](https://github.com/minjun0219/rocky/commit/c4297eef934752299cb3de323c709c1b7678139d) Thanks [@minjun0219](https://github.com/minjun0219)! - 웹 보드에 **알림** 탭을 더한다 — 오너가 손댈 것만 모은다: 결정 필요·머지 후보 PR, 30분 넘게 그대로인 충돌·CI 실패, 세션이 사라진 진행 중 할 일. 탭 옆 숫자는 GitHub 탭에서 이쪽으로 옮겼다.

- [#264](https://github.com/minjun0219/rocky/pull/264) [`2f40f3c`](https://github.com/minjun0219/rocky/commit/2f40f3c9e534d33bf40d34089753cd347819d529) Thanks [@minjun0219](https://github.com/minjun0219)! - 웹 GitHub 탭이 구독한 PR 을 레포별로 보이고, 레포의 "열린 PR" 을 펼치면 그때만 그 레포의 열린 PR 을 불러와 "지켜보기"(세션 없는 구독)로 구독한다 — `GET /api/prs/open?repo=`(1포인트, 60초 캐시, 보드·구독에 있는 레포만).

- [#260](https://github.com/minjun0219/rocky/pull/260) [`10c1f45`](https://github.com/minjun0219/rocky/commit/10c1f452626a6a10cf6e5100585e8dbe92f25907) Thanks [@minjun0219](https://github.com/minjun0219)! - PR 을 GitHub 검색 조건으로도 구독한다 — `rocky pr subscribe --filter "repo:o/r author:@me"`(라벨·`project:org/5`·`review-requested:@me` 등 검색 조건 그대로). 데몬이 3분마다 검색해 걸린 열린 PR 을 그 세션이 받는다. `rocky pr unsubscribe --filter ID` 로 해지하면 그 필터로 들어온 구독도 걷힌다.

- [#258](https://github.com/minjun0219/rocky/pull/258) [`492733f`](https://github.com/minjun0219/rocky/commit/492733f94c3731ef49f8e7139ddaf31926bdb8ed) Thanks [@minjun0219](https://github.com/minjun0219)! - PR 감시를 구독한 PR 만으로 좁힌다 — `rocky pr subscribe|unsubscribe N`·`rocky pr subscriptions`, `/rocky:review-request`·`/rocky:review-fix` 가 그 세션으로 구독한다. 알림은 구독한 세션에만 가고(같은 레포의 다른 PR 이 한 세션에 쏟아지지 않는다), 레포 목록 조회를 없애 GitHub API 비용이 구독한 PR 수에만 비례한다. 구독은 머지·닫힘에서 풀린다.

- [#263](https://github.com/minjun0219/rocky/pull/263) [`f3d3369`](https://github.com/minjun0219/rocky/commit/f3d3369aeca8a7d8d31b84594d746a68516eacb5) Thanks [@minjun0219](https://github.com/minjun0219)! - 보드 옵션 `autoResolve` 를 `reviewFix`(CLI `rocky board review-fix on|off`)로 바꾼다 — 옛 이름은 한 릴리스 동안 입력으로 받는다.

### Patch Changes

- [#261](https://github.com/minjun0219/rocky/pull/261) [`bfd2add`](https://github.com/minjun0219/rocky/commit/bfd2addcfcdcbe57b865aa4bd726b43f0d397475) Thanks [@minjun0219](https://github.com/minjun0219)! - 업그레이드 때 새 데몬이 포트를 먼저 잡고 옛 데몬이 완전히 끝난 뒤에야 DB 를 연다(마이그레이션 포함) — 종료 중인 옛 데몬과 겹쳐 DB 가 손상된 일을 막는다. 기동 때 DB 무결성을 확인해 `/api/health` 의 `dbIntegrity` 로 보인다.

- [#262](https://github.com/minjun0219/rocky/pull/262) [`508e03f`](https://github.com/minjun0219/rocky/commit/508e03f3c0591377d7d293f1824e17c149ab2b6a) Thanks [@minjun0219](https://github.com/minjun0219)! - 워크로그가 레포를 찾을 때 물려받은 `GIT_DIR` 등을 무시하고 그 폴더의 레포를 본다. pre-push 훅도 테스트 전에 git 환경을 지운다.

## 0.37.0

### Minor Changes

- [#255](https://github.com/minjun0219/rocky/pull/255) [`138f637`](https://github.com/minjun0219/rocky/commit/138f637b5744e2fd465a267e58022101cfaaf307) Thanks [@minjun0219](https://github.com/minjun0219)! - `rocky update [--check]` 가 플러그인·데몬을 최신 릴리스로 올린다(0.36.0 의 `rocky upgrade` 는 한 릴리스 동안 숨은 별칭으로 남는다 — 0.36.0 에서 올릴 때는 그 바이너리의 `rocky upgrade` 를 부른다). 할 일 수정은 `rocky edit REF [플래그]` — 예전처럼 `rocky update REF …` 를 부르면 업데이트를 돌리지 않고 `edit` 으로 안내한다.

## 0.36.0

### Minor Changes

- [#241](https://github.com/minjun0219/rocky/pull/241) [`3fe0478`](https://github.com/minjun0219/rocky/commit/3fe0478e3d84ce6df26a5438222518161fd4d5f6) Thanks [@minjun0219](https://github.com/minjun0219)! - `rocky statusline` — statusline 에 끼우는 rocky 한 줄(cc-usage `extra_commands` 나 `statusLine.command` 에 그대로). `rocky upgrade [--check]` — 마켓플레이스 갱신·플러그인 올리기·데몬 교체를 한 번에.

- [#249](https://github.com/minjun0219/rocky/pull/249) [`503a969`](https://github.com/minjun0219/rocky/commit/503a9693e1ef027bb9302ae50fbac37e2fb36f96) Thanks [@minjun0219](https://github.com/minjun0219)! - 웹 노트 탭을 게시판처럼 바꾼다 — 목록 한 줄 → 상세(전체 높이 편집기, `/{board}/notes/{n}`), 고정한 노트는 위에 접을 수 있는 카드로. 편집기는 CodeMirror 하나로 통일하고 마크다운 꾸밈·서식 툴바(⌘B·⌘I·⌘K)를 붙인다.

- [#248](https://github.com/minjun0219/rocky/pull/248) [`c0876f1`](https://github.com/minjun0219/rocky/commit/c0876f167542bdb52d2ec10b1693fd89b5f39bb9) Thanks [@minjun0219](https://github.com/minjun0219)! - 노트를 고정할 수 있다 — `POST /api/notes/:ref/(pin|unpin)`, `rocky note pin|unpin REF`. 고정 시각(`pinnedAt`)이 노트에 실린다.

- [#246](https://github.com/minjun0219/rocky/pull/246) [`bc5df77`](https://github.com/minjun0219/rocky/commit/bc5df779a7672eedff46040fb5b521af8be50021) Thanks [@minjun0219](https://github.com/minjun0219)! - 보드마다 알릴 PR 작성자를 정한다(`rocky board pr-authors @me`, 웹 보드 편집의 "PR 알림") — 그 작성자의 PR 전이만 세션·배너·브릿지로 가고, 기록과 `rocky pr` 에는 전부 남는다. 보드에 레포를 처음 붙인 첫 조회는 기준선만 잡고 기존 PR 을 한꺼번에 알리지 않는다.

- [#242](https://github.com/minjun0219/rocky/pull/242) [`6073c33`](https://github.com/minjun0219/rocky/commit/6073c33375519ef6e03c7f86743eee934b293d9b) Thanks [@minjun0219](https://github.com/minjun0219)! - 웹 보드에 GitHub 탭을 둔다 — PR 상태판과 수집함(이슈)을 할 일 화면에서 옮겼다. 움직일 PR 수는 탭 옆 숫자로, 행마다 숨기기(PR 은 상태가 바뀌면 다시 보인다), ⋯ 메뉴에서 탭 끄기.

- [#243](https://github.com/minjun0219/rocky/pull/243) [`ce872c9`](https://github.com/minjun0219/rocky/commit/ce872c945c9324582835d2050d56da7fd13b794e) Thanks [@minjun0219](https://github.com/minjun0219)! - 웹 GitHub 탭에 "세션 전달" 칸 — PR·수집함 알림을 어느 세션이 받는지와 최근 보낸 알림을 보고, 세션별로 "보내지 않기"(그 보드의 다음 세션이 받는다)와 수집함 구독 해지를 한다. 로컬에서 연 화면에서만.

### Patch Changes

- [#252](https://github.com/minjun0219/rocky/pull/252) [`39e440d`](https://github.com/minjun0219/rocky/commit/39e440d23bf2a8813fb2b17b2844a1e009641503) Thanks [@minjun0219](https://github.com/minjun0219)! - 웹 보드의 마크다운을 GFM 파서로 그린다 — 할 일 설명·댓글·노트가 한 렌더러를 쓰고, 굵게 안의 코드·`[글자](주소)` 링크·제목·목록·표·코드 블록이 깨지지 않는다.

- [#254](https://github.com/minjun0219/rocky/pull/254) [`5f98668`](https://github.com/minjun0219/rocky/commit/5f98668c5ac0e98ced7552a61ecda628724534c7) Thanks [@minjun0219](https://github.com/minjun0219)! - `/rocky:review-fix` 13단계 — 다른 작업 도중에 리뷰·충돌·CI 실패 메시지가 오면 워크트리 서브에이전트에 맡기고 하던 일을 계속한다(판단이 필요한 건만 메인이 묻는다). 데몬의 세션 메시지도 그렇게 안내한다.

- [#251](https://github.com/minjun0219/rocky/pull/251) [`e621739`](https://github.com/minjun0219/rocky/commit/e621739f6f1d1842b527e05f6f62cbd371dae20b) Thanks [@minjun0219](https://github.com/minjun0219)! - 웹 보드의 머리(보드 전환·보기 탭)와 맨 아래 버전 줄을 스크롤해도 늘 보이게 고정한다. 노트 편집기 툴바도 머리 밑에 붙는다.

- [#253](https://github.com/minjun0219/rocky/pull/253) [`cb45137`](https://github.com/minjun0219/rocky/commit/cb451371abf44573a8150db85c5124acddacc86d) Thanks [@minjun0219](https://github.com/minjun0219)! - 텔레그램 알림 브릿지가 굵은 머리 + PR 링크로 보낸다 — mdwire `telegram-html` 로 PR 제목의 코드·`<`·`&` 가 깨지지 않는다.

## 0.35.0

### Minor Changes

- [#239](https://github.com/minjun0219/rocky/pull/239) [`41156f3`](https://github.com/minjun0219/rocky/commit/41156f3a144295e53c7878e26871445d95a3f855) Thanks [@minjun0219](https://github.com/minjun0219)! - 수집함 구독 — 세션이 `rocky inbox subscribe <소스>` 로 구독하면 그 뒤 새로 생긴 항목을 데몬이 5분마다 확인해 그 세션에 메시지로 알린다(PR 알림과 같은 받은편지함). 알리기만 하고 착수는 사람이 정한다. 웹 보드 수집함 머리에 구독 중인 목록이 보인다.

### Patch Changes

- [#240](https://github.com/minjun0219/rocky/pull/240) [`8c12808`](https://github.com/minjun0219/rocky/commit/8c128089be5e8c9fd5e980a6bd62ec17fd302334) Thanks [@minjun0219](https://github.com/minjun0219)! - 세션에 보내는 "머지 후보" 메시지에서 끝줄의 출처 표시와 겹치던 첫머리("데몬이 기계적으로 본 결과다")를 뺀다.

- [#237](https://github.com/minjun0219/rocky/pull/237) [`910dd53`](https://github.com/minjun0219/rocky/commit/910dd53c2357acac214e71693924fbe6d296aba7) Thanks [@minjun0219](https://github.com/minjun0219)! - statusline 기본 템플릿에서 이모지와 숫자 사이를 한 칸 띄운다(`⏰ 2  📥 3`).

## 0.34.0

### Minor Changes

- [#233](https://github.com/minjun0219/rocky/pull/233) [`e29aa5c`](https://github.com/minjun0219/rocky/commit/e29aa5c5fd863d3cf4595312d21ad6bc5cf9d8f6) Thanks [@minjun0219](https://github.com/minjun0219)! - 보드마다 수집함을 등록한다 — `rocky.json` 의 `todo.inboxAdapters[]` 에 어댑터를 두면, 보드 화면에서 어댑터가 `--describe` 로 알려 준 칸(예: 프로젝트·필터)을 채워 그 보드 전용 수집함을 만든다. 등록·삭제는 로컬 요청만, 재기동 없이 반영된다.

- [#234](https://github.com/minjun0219/rocky/pull/234) [`c2f45db`](https://github.com/minjun0219/rocky/commit/c2f45dbaa1d729b78051b30493b2ccdc88565830) Thanks [@minjun0219](https://github.com/minjun0219)! - 웹 보드의 보드 머리 아래에 그 보드의 수집함(아직 안 올린 항목)을 보이고, "설정" 에서 어댑터를 골라 칸을 채워 보드 수집함을 등록·삭제한다.

- [#227](https://github.com/minjun0219/rocky/pull/227) [`c95c6c1`](https://github.com/minjun0219/rocky/commit/c95c6c19968218196a16702935bbd6cd5fae67dd) Thanks [@minjun0219](https://github.com/minjun0219)! - `rocky daemon restart` 를 더한다 — 버전과 상관없이 지금 설치본으로 데몬을 교체한다(launchd 상주면 job 재등록). 웹 보드 맨 아래 줄에 도는 데몬 버전을 보이고, 화면을 연 뒤 데몬이 바뀌면 새로고침을 권한다.

- [#230](https://github.com/minjun0219/rocky/pull/230) [`97567b5`](https://github.com/minjun0219/rocky/commit/97567b51fedc91d62cd28afa2734e8af2294c688) Thanks [@minjun0219](https://github.com/minjun0219)! - `rocky today`·세션 시작 요약에 미올림 수집함 항목을 제목으로 싣고(최대 3개, 넘치면 `… 외 N건`), `rocky inbox [--json]` 을 더한다. "이미 올라감" 판정을 현재 보드가 아니라 전 보드(보관 포함)의 링크로 넓혔고, 세션 시작 요약은 수집함 어댑터를 기다리지 않는다.

- [#235](https://github.com/minjun0219/rocky/pull/235) [`943af87`](https://github.com/minjun0219/rocky/commit/943af87b7a8cab580beea2481fdfeda80377dd61) Thanks [@minjun0219](https://github.com/minjun0219)! - PR 감시가 CI 실패를 그 레포의 세션에 알린다(`pr-ci-failed`) — 세션이 로그를 보고 인프라 문제면 한 번 재실행, 코드 문제면 고쳐 푸시한다(`/rocky:review-fix` 12단계). 사람 배너는 없다.

### Patch Changes

- [#231](https://github.com/minjun0219/rocky/pull/231) [`cde4082`](https://github.com/minjun0219/rocky/commit/cde408250d9433f7a2299baf7091f6ea73930b23) Thanks [@minjun0219](https://github.com/minjun0219)! - github-project 수집함 어댑터가 `--filter "assignee:@me type:Bug component/s:Web"` 로 보드 필터 문자열을 그대로 받고, `--describe` 로 입력 칸 목록을 낸다.

- [#228](https://github.com/minjun0219/rocky/pull/228) [`321fc23`](https://github.com/minjun0219/rocky/commit/321fc23d83064e009dbd8b953c8ed406c5d25ce4) Thanks [@minjun0219](https://github.com/minjun0219)! - github-project 수집함 어댑터가 보드를 앞 100개만 훑던 것을 이슈 검색으로 바꾼다 — 큰 보드에서도 조건에 맞는 이슈를 놓치지 않고, 결과가 잘리면 실패로 알린다.

- [#232](https://github.com/minjun0219/rocky/pull/232) [`1a49676`](https://github.com/minjun0219/rocky/commit/1a4967629a751af8a5cdc67b1b9a02c82719754d) Thanks [@minjun0219](https://github.com/minjun0219)! - `/rocky:review-request` 가 PR 을 만든 뒤 `/rocky:review-fix` 로 이어갈지 정한다 — 보드 자동 처리가 켜졌으면 데몬에 맡기고, 아니면 레포별로 한 번 물어 메모리에 남긴 답대로 진행한다.

## 0.33.1

### Patch Changes

- [#225](https://github.com/minjun0219/rocky/pull/225) [`0de952b`](https://github.com/minjun0219/rocky/commit/0de952b0b2fdcaffdef2bfa2802929788e609ffd) Thanks [@minjun0219](https://github.com/minjun0219)! - 데몬이 종료 신호 뒤 열린 SSE 연결을 끝없이 기다리지 않는다 — 3초 유예 뒤 나간다. 전에는 교체된 옛 데몬이 옛 세션의 SSE 를 물고 포트만 놓은 채 남아, 그 안의 PR 감시가 새 데몬과 겹칠 수 있었다.

## 0.33.0

### Minor Changes

- [#223](https://github.com/minjun0219/rocky/pull/223) [`2373eec`](https://github.com/minjun0219/rocky/commit/2373eec38cd987c255ef41b01f70109419c5b7f9) Thanks [@minjun0219](https://github.com/minjun0219)! - `/rocky:review` 를 없앤다 — 버그 찾기는 기본 `/code-review` 가 같은 diff 를 더 잘 본다. 이 커맨드만의 몫이던 "요구사항 대비 점검" 은 `/rocky:finish` 의 한 단계(2.5)로 옮겼다: 위험한 변경이면 커밋 전에 `/code-review` 와 `reviewer` 서브에이전트를 돌린다.

- [#223](https://github.com/minjun0219/rocky/pull/223) [`2373eec`](https://github.com/minjun0219/rocky/commit/2373eec38cd987c255ef41b01f70109419c5b7f9) Thanks [@minjun0219](https://github.com/minjun0219)! - 커맨드 이름을 바꾼다 — `/rocky:finish` → `/rocky:review-request`(게이트 → 프리리뷰 → 커밋 → PR), `/rocky:resolve-reviews` → `/rocky:review-fix`(리뷰 반영 → 머지 후보 판단 → 머지 뒤 정리). 옛 이름은 남기지 않는다. 데몬이 세션에 보내는 메시지도 새 이름을 가리킨다.

- [#222](https://github.com/minjun0219/rocky/pull/222) [`1eb9ac9`](https://github.com/minjun0219/rocky/commit/1eb9ac9c03d26f62ab2b7ee368a0041eb1b2c4fa) Thanks [@minjun0219](https://github.com/minjun0219)! - 리뷰 스레드 리액션의 뜻을 뒤집는다 — 🚀 = 고쳐서 내보냈다(resolve 해도 된다), 👀 = 오너가 봐야 한다(결정 필요). 데몬의 머지 후보 판정과 `pr-threads.ts` 가 새 뜻으로 센다(스냅숏·`/api/prs` 필드 `rocket` → `decision`, 옛 스냅숏은 그대로 읽힌다). `/rocky:resolve-reviews` 는 Codex 코멘트에 판정대로 👍/👎 피드백을 달고, 채팅 보고의 줄마다 그 코멘트 링크와 리뷰어 제목을 붙인다.

### Patch Changes

- [#219](https://github.com/minjun0219/rocky/pull/219) [`25f5649`](https://github.com/minjun0219/rocky/commit/25f564997d8d61966125195fc631c0dba7ddb311) Thanks [@minjun0219](https://github.com/minjun0219)! - `todo_write` 도구 설명과 board 스킬에서 특정 외부 할 일 앱 이름을 걷어냈다(외부 앱은 링크로만 참조한다는 규칙에 맞춤). `/rocky:finish`·`/rocky:resolve-reviews` 의 테스트 게이트를 레포의 test 스크립트(`bun run test`)로 바꿨다.

- [#221](https://github.com/minjun0219/rocky/pull/221) [`a87cb31`](https://github.com/minjun0219/rocky/commit/a87cb3101c6d93dbc8d19a4c1d4ab3045143b312) Thanks [@minjun0219](https://github.com/minjun0219)! - PR 이 머지되면 데몬이 그 레포 세션의 받은편지함에 "머지됨" 을 보낸다 — 세션은 `/rocky:resolve-reviews` 11단계대로 머지 뒤 정리(main 최신화·브랜치 정리·릴리스 PR·스택 다음 PR·머지 뒤 리뷰)를 한 번 한다. 사람에게 가는 macOS 배너와 알림 브릿지에는 여전히 보내지 않는다.

## 0.32.3

### Patch Changes

- [#217](https://github.com/minjun0219/rocky/pull/217) [`92b7c52`](https://github.com/minjun0219/rocky/commit/92b7c520f68b0963c18e40172382cc51cd544bcc) Thanks [@minjun0219](https://github.com/minjun0219)! - "머지 가능" 을 기계 판정(머지 후보)과 세션 판단으로 나눈다 — 데몬 알림 문구가 "머지 후보" 가 되고, 세션은 `/rocky:resolve-reviews` 8단계(응답 안 한 리뷰 요청·봇 리뷰 필수 레포의 봇 신호·방금 한 푸시·작업 중 표시)를 본 뒤에 알린다. `pr-threads.ts ready` 가 응답 안 한 리뷰 요청을 이유로 낸다. 머지 뒤에 붙은 리뷰는 `pr-threads.ts after-merge` 로 찾아 다음 PR 에 고치고 링크를 건다(`/rocky:finish` 도 PR 을 만들기 전에 본다).

## 0.32.2

### Patch Changes

- [#216](https://github.com/minjun0219/rocky/pull/216) [`ada947b`](https://github.com/minjun0219/rocky/commit/ada947b2f1777e7a3ec7d5572cc2c78b90efa975) Thanks [@minjun0219](https://github.com/minjun0219)! - `/rocky:resolve-reviews` 가 봇 리뷰를 기본으로 기다리지 않는다 — 봇 흔적이 보이면 "다음부터 봇 리뷰를 기다릴까" 를 한 번 묻고, 그렇다고 하면 세션 메모리에 남겨 그 레포에서만 `watch --wait-bot` 으로 기다린다. 레포 흔적으로 자동 판단하던 방식(`verdict: "none"`)은 걷어냈다.

- [#214](https://github.com/minjun0219/rocky/pull/214) [`c815983`](https://github.com/minjun0219/rocky/commit/c8159830e4143dd294faf2b15f4254451871dc73) Thanks [@minjun0219](https://github.com/minjun0219)! - `/rocky:resolve-reviews` 의 봇 리뷰 대기가 리뷰 봇이 없는 레포에서 timeout 을 꽉 채우지 않는다 — 최근 PR 20개에 봇 흔적이 없으면 CI 만 기다리고 `verdict: "none"` 으로 바로 돌아온다.

## 0.32.1

### Patch Changes

- [#212](https://github.com/minjun0219/rocky/pull/212) [`4d624a3`](https://github.com/minjun0219/rocky/commit/4d624a3e0f7cfa6799de5bada93d518e9a47b2ed) Thanks [@minjun0219](https://github.com/minjun0219)! - GitHub 프로젝트 보드 수집함 어댑터(`bridges/github-project/inbox.ts`) — 보드 필터(`assignee:@me type:Bug component/s:Web`)를 인자로 옮겨, 조건에 맞는 열린 이슈를 rocky 수집함에 띄운다. 로그인된 `gh`(`read:project`)를 쓰고, 이슈 타입이 없는 개인 계정 레포는 같은 이름의 라벨로 본다.

## 0.32.0

### Minor Changes

- [#210](https://github.com/minjun0219/rocky/pull/210) [`f35dc69`](https://github.com/minjun0219/rocky/commit/f35dc69dcf254723bdc12fd3e6eda27bf75a30ee) Thanks [@minjun0219](https://github.com/minjun0219)! - 리뷰가 붙은 PR 을 세션이 알아서 처리하게 한다 — 보드마다 그 레포의 세션이 `rocky board auto-resolve on|off` 로 켠다(기본 끔, `PATCH /api/boards/:key {"autoResolve": true}` 는 로컬 요청 전용). 처음 보는 처리 안 된 리뷰 스레드가 생기면 데몬이 "리뷰 도착" 전이(`pr-review`)를 남기고, 켠 보드의 레포면 그 레포에서 일하는 Claude Code 세션에 `/rocky:resolve-reviews N` 절차대로 처리하라는 메시지를 받은편지함으로 보낸다. 배너·알림 브릿지는 이 전이를 쓰지 않는다.

- [#207](https://github.com/minjun0219/rocky/pull/207) [`30a2a48`](https://github.com/minjun0219/rocky/commit/30a2a481691c2fb85119b2c5f9920dc81734bdba) Thanks [@minjun0219](https://github.com/minjun0219)! - PR 이 "확인·머지해도 된다" 나 충돌로 바뀌면 데몬이 그 레포에서 일하는 Claude Code 세션을 **받은편지함 소켓**으로 깨운다(`pr.sessionNotify`, 기본 켬). 훅이 턴마다 세션의 소켓(`CLAUDE_CODE_MESSAGING_SOCKET`)을 데몬에 등록하고, 데몬은 그 레포 보드에서 가장 최근에 쓰인 세션 하나에 메시지 한 줄을 쓴다 — 쉬던 세션도 그 자리에서 턴이 열리고, 채널과 달리 개발 플래그가 필요 없다.

### Patch Changes

- [#209](https://github.com/minjun0219/rocky/pull/209) [`c454605`](https://github.com/minjun0219/rocky/commit/c454605f1b9d205d9b373c5a8d6145cc95678986) Thanks [@minjun0219](https://github.com/minjun0219)! - 세션 훅이 PR 감시 전이("확인·머지해도 된다"·충돌)를 **그 세션이 일하는 보드의 것만** 주입한다. 전에는 모든 세션에 모든 레포의 PR 전이가 들어가, 예를 들어 tally 세션이 rocky PR 의 머지 가능 알림을 받았다. 세션 cwd 가 어느 보드로도 안 풀리면 PR 전이는 싣지 않는다.

- [#204](https://github.com/minjun0219/rocky/pull/204) [`7cb8bd4`](https://github.com/minjun0219/rocky/commit/7cb8bd4f7891289d142c241b24f9d001f4b7396d) Thanks [@minjun0219](https://github.com/minjun0219)! - 웹 보드의 머리줄을 한 줄로 줄이고 가로 보드 탭 줄을 보드 스위처로 바꾼다. 머리줄에는 보드 스위처(지금 보드 이름 → 누르면 전 보드 목록과 진행중 개수, 새 보드 추가)와 `⋯` 메뉴(테마·보관된 항목 보기·편집자 이름·전체 보기·새로고침)만 남고, 연결 표시는 끊겼을 때만 나온다. 설명 없던 `LINK ♪`·활동 띠·보관됨 체크박스·이름 버튼은 메뉴로 들어가거나 없어졌다.

- [#208](https://github.com/minjun0219/rocky/pull/208) [`f5eacec`](https://github.com/minjun0219/rocky/commit/f5eacec898fda1f1bf4311331543c6676ba13d15) Thanks [@minjun0219](https://github.com/minjun0219)! - 웹 보드의 "지금" 에 **PR** 묶음을 더한다 — 보고 있는 보드 레포의 열린 PR 전부(전체 보기면 전 보드)를 손댈 순서(충돌 → 머지 가능 → CI 실패 → 결정 필요 → 대기 → 초안)로, 둘째 줄에 CI·스레드·갱신 시각. 행 앞 상태 표시는 글꼴 문자(●◆◌) 대신 lucide 아이콘으로 바꾼다.

## 0.31.1

### Patch Changes

- [#203](https://github.com/minjun0219/rocky/pull/203) [`8e9e7b5`](https://github.com/minjun0219/rocky/commit/8e9e7b5c6adae74db88e750a310f9d2792b47c5c) Thanks [@minjun0219](https://github.com/minjun0219)! - 웹 보드의 노트를 목록 아래 접힌 레일에서 꺼낸다 — 화면 맨 위 "할 일 | 노트" 전환으로 노트가 화면 전체를 쓴다. 할 일 보기에 있을 때 누가(에이전트 포함) 노트를 고치면 "노트" 옆에 점이 찍힌다. 보던 쪽은 새로고침 뒤에도 유지된다.

- [#202](https://github.com/minjun0219/rocky/pull/202) [`8b245d6`](https://github.com/minjun0219/rocky/commit/8b245d6b3ddbd3d37030ac876be04768f6cbf4e4) Thanks [@minjun0219](https://github.com/minjun0219)! - 웹 보드의 "지금" 을 좁은 패널에서 읽히게 바꾼다 — 4열 표 대신 "내 차례"(우선순위순 최대 5행: PR 충돌 → 머지 가능 → 세션 없음·멈춤 → 넘김 → 최근 3일의 읽지 않은 댓글 → 수집함)와 "돌고 있음" 두 묶음의 행 목록. 상태는 행마다 배지를 반복하지 않고 글리프(●◆◌○)와 묶음 머리의 개수로 말한다. 경과는 초가 흐르는 표기를 1시간 미만의 진행중에만 쓰고, 나머지는 "12분"·"3시간"·"5일", 30일부터는 "8월 4일부터".

- [#201](https://github.com/minjun0219/rocky/pull/201) [`fcc6baa`](https://github.com/minjun0219/rocky/commit/fcc6baa3622dc28735203d9f95b8eeabd254656c) Thanks [@minjun0219](https://github.com/minjun0219)! - 웹 보드가 데이터 갱신(에이전트의 노트 편집·보드 변경·1분 틱)마다 페이지를 보드 탭 줄까지 끌어올리던 버그를 고친다. 활성 탭은 탭 줄 안에서만 가운데로 당기고, 선택이 바뀔 때만 당긴다.

## 0.31.0

### Minor Changes

- [#196](https://github.com/minjun0219/rocky/pull/196) [`13df326`](https://github.com/minjun0219/rocky/commit/13df326fbc198f235555da6cd6437aa10ad43e1c) Thanks [@minjun0219](https://github.com/minjun0219)! - PR 감시의 ready·충돌 알림을 **알림 브릿지**로도 보낸다 — `rocky.json` `pr.notifiers[]`(수집함 `todo.inbox[]` 와 같은 명령 규약). 데몬이 argv 그대로 실행하고 stdin 에 전이 JSON 을 준다; 어느 서비스인지는 데몬이 모른다. 참조 구현 `bridges/telegram/notify.ts`(Bot API, 토큰은 `op read`). macOS 배너(`pr.notify`)와 독립.

## 0.30.0

### Minor Changes

- [#197](https://github.com/minjun0219/rocky/pull/197) [`120ecf3`](https://github.com/minjun0219/rocky/commit/120ecf31102422f507bb493b1dd1fed5b21320d4) Thanks [@minjun0219](https://github.com/minjun0219)! - rocky 채널 — worklog stdio MCP 서버가 Claude Code channels(`claude/channel`)를 선언하고 데몬의 PR 전이(확인·머지 가능 / 충돌)를 `notifications/claude/channel` 로 세션에 밀어 넣는다. 세션이 그 자리에서 깨어나므로 훅 주입과 달리 사람이 타이핑하기를 기다리지 않는다. 리서치 프리뷰라 `claude --dangerously-load-development-channels plugin:rocky@rocky-marketplace` 로 띄운 세션만 받는다. `plugin.json` 에 `channels` 항목 추가.

## 0.29.2

### Patch Changes

- [#194](https://github.com/minjun0219/rocky/pull/194) [`2248e0f`](https://github.com/minjun0219/rocky/commit/2248e0f8e1ba85e96d9993b00ff2e3c061861743) Thanks [@minjun0219](https://github.com/minjun0219)! - PR 감시가 GitHub GraphQL 한도를 다 쓰지 않는다. 비용은 실제가 아니라 `first:` 로 요청한 노드 수라 첫 쿼리가 열린 PR 이 없어도 레포당 263 포인트였고(3분 × 레포 10개 → 두 tick 에 시간당 5,000 소진, 세션의 `gh` 까지 마비), 이제 레포당 상태 목록 한 번(1 포인트) + 실제로 열린 PR 에만 CI·스레드 상세 배치 한 번(PR 당 ~1.5 포인트)으로 묻는다. 응답마다 잔여 예산을 읽어 1,000 밑이거나 한도 에러를 받으면 그 tick 을 멈추고 리셋까지 쉰다. `/api/health` 의 `prWatch` 에 `rateLimit`(cost·remaining·resetAt)·`pausedUntil` 이 실린다.

## 0.29.1

### Patch Changes

- [#192](https://github.com/minjun0219/rocky/pull/192) [`d543177`](https://github.com/minjun0219/rocky/commit/d543177b28b443fdf0ea5a1180a2ef12d45e3343) Thanks [@minjun0219](https://github.com/minjun0219)! - `rocky … | head` 처럼 읽는 쪽이 먼저 닫혀도 CLI 가 "Broken pipe" 패닉 대신 조용히 끝난다.

## 0.29.0

### Minor Changes

- [#175](https://github.com/minjun0219/rocky/pull/175) [`18c827d`](https://github.com/minjun0219/rocky/commit/18c827df8e85226c22b8915415f2c9eecf00a2c5) Thanks [@minjun0219](https://github.com/minjun0219)! - 플러그인 업그레이드 중 데몬이 사라지는 사고를 막고, 실패하면 알린다.

  - 부트스트랩이 새 버전을 받은 직후엔 입구(훅/MCP/CLI)와 무관하게 `current` 링크를 건다 — worklog MCP 기동이 받아 놓고 링크는 옛 버전에 남던 구멍.
  - launchd 교체가 bootout 뒤 서비스가 내려가길 기다린 뒤 bootstrap 을 재시도하고 로드를 확인한다. 그래도 실패하면 데몬을 launchd 밖에서라도 띄우고 세션 컨텍스트에 `⚠ rocky 데몬: …` 로 알린다.
  - `rocky daemon status` / `rocky config show` 가 "plist 는 있으나 로드되지 않음" 을 가르고 `rocky daemon install` 을 고치는 명령으로 보여 준다. `rocky daemon start` 는 띄운 데몬이 launchd 상주인지 밖인지 적는다.
  - `rocky version` / `rocky --version` 추가.

- [#176](https://github.com/minjun0219/rocky/pull/176) [`67c9427`](https://github.com/minjun0219/rocky/commit/67c942773537a859080d8646cb0edba99f5b3449) Thanks [@minjun0219](https://github.com/minjun0219)! - 노트 본문이 CRDT(Yjs 호환) 문서가 된다 — 사람과 에이전트가 같은 메모를 동시에 고쳐도 서로 지우지 않고 글자 단위로 합쳐진다. 에이전트·CLI 의 set/append 는 데몬이 최소 편집으로 문서에 넣고, `notes.content` 는 늘 합쳐진 최신 본문이다(기존 노트는 처음 열 때 지금 본문으로 문서를 만든다). 웹·다른 클라이언트용 라우트 `GET/POST /api/notes/:ref/doc`, `GET …/doc/events`(노트별 SSE), `POST …/presence`. 웹 편집 히스토리는 60초 창으로 묶인다.

- [#179](https://github.com/minjun0219/rocky/pull/179) [`e89b70c`](https://github.com/minjun0219/rocky/commit/e89b70c1c7d82e2ab8705b78298f8a5dd8eba40f) Thanks [@minjun0219](https://github.com/minjun0219)! - 웹 UI 메모에 두 번째 편집기(CodeMirror) — 메모 헤더의 스위치로 기본 textarea 와 번갈아 쓴다(임시, 하나만 남길 예정). CodeMirror 쪽은 같이 보는 사람의 커서·선택 영역을 이름표와 함께 그린다.

- [#177](https://github.com/minjun0219/rocky/pull/177) [`4c88b70`](https://github.com/minjun0219/rocky/commit/4c88b7091dd04fac7a5cb01bd1add1c8a4ec4180) Thanks [@minjun0219](https://github.com/minjun0219)! - 웹 UI 메모가 실시간으로 합쳐진다 — 본문에 포커스가 들어오면 세션이 열리고, 에이전트·CLI·다른 브라우저의 편집이 타이핑 중인 textarea 에 글자 단위로 그 자리에 들어온다(커서 유지, 한글 조합 중엔 잠시 보류). 같이 보는 사람·에이전트 이름을 카드 아래 표시. 제목은 예전처럼 blur 저장.

- [#187](https://github.com/minjun0219/rocky/pull/187) [`580fd16`](https://github.com/minjun0219/rocky/commit/580fd16a0832073338bcd3ed90d0df866bb2337b) Thanks [@minjun0219](https://github.com/minjun0219)! - PR 감시의 바탕 — `rocky.json` 에 `pr` 블록(`enabled` / `intervalMinutes` / `notify`), PR 스냅숏을 기억하는 `pr_watch` 테이블, "확인·머지해도 된다"·충돌·머지·닫힘 전이를 보드 히스토리(actor `rocky`, action `pr-*`)로 남기는 스토어. 데몬의 주기 조회와 알림은 다음 층.

- [#190](https://github.com/minjun0219/rocky/pull/190) [`9fdab07`](https://github.com/minjun0219/rocky/commit/9fdab07d2f3dea526c467e63b00695619a02c75d) Thanks [@minjun0219](https://github.com/minjun0219)! - PR 감시가 데몬에서 돈다 — `repo` 가 설정된 보드의 PR 을 3분마다 보고, "확인·머지해도 된다"(CI 초록 + 리뷰 스레드 처리됨 + 충돌 없음)와 충돌을 macOS 알림·보드 "지금" 표·세션 훅 주입으로 알린다. `rocky pr` 로 열린 PR 상태를 읽는다. `rocky.json` `pr` 블록으로 간격·알림을 조절.

### Patch Changes

- [#184](https://github.com/minjun0219/rocky/pull/184) [`a0a0abb`](https://github.com/minjun0219/rocky/commit/a0a0abb8b8295b68e23cddc9bf70dea702ef9a36) Thanks [@minjun0219](https://github.com/minjun0219)! - `/rocky:resolve-reviews` 에 "확인·머지해도 되면 알려줘" 절차 — 첫 봇 판정까지 기다렸다가 CI 초록 + 지적 전부 처리 + 결정 필요 건 없음일 때 알린다(스택은 맨 아래 PR 만, 머지 뒤 GitHub 의 서버 리베이스를 전제). Codex 자동 리뷰는 ready 때 한 번뿐이라 수정 푸시 뒤엔 CI 만 본다.

- [#174](https://github.com/minjun0219/rocky/pull/174) [`4265769`](https://github.com/minjun0219/rocky/commit/4265769fed8ef0679895943c35796d703941a44e) Thanks [@minjun0219](https://github.com/minjun0219)! - 웹 UI: 전체 보기(또는 다른 보드)에서 상세를 열어도 뒤 화면이 그 todo 의 보드로 바뀌지 않는다. 주소가 보고 있는 보드와 열린 todo 를 따로 싣는다 — `/?todo=rocky-12`, `/tally?todo=rocky-12`. 같은 보드면 예전처럼 `/rocky/12`.

## 0.28.0

### Minor Changes

- [#164](https://github.com/minjun0219/rocky/pull/164) [`6f90d07`](https://github.com/minjun0219/rocky/commit/6f90d070f025bdcd3c147cd6f6b159e871068736) Thanks [@minjun0219](https://github.com/minjun0219)! - 죽은 세션이 든 진행중(doing)을 데몬이 저절로 풀어 준다 — 에이전트가 `start` 만 하고 세션이
  사라진 채 24시간이 지나면 10분 주기 스윕이 `stop` 으로 돌리고 `rocky` 이름으로 댓글을 남긴다
  ("세션 없음 — 진행중 자동 해제 (claude-code 착수 2026-08-03, 56일)"). 사람이 든 것·세션이 살아
  있는 것·판정 불가는 그대로. 핸드오프는 여전히 자동 만료 없음.

- [#169](https://github.com/minjun0219/rocky/pull/169) [`9a22d83`](https://github.com/minjun0219/rocky/commit/9a22d833026a4e0d38f22584bc8641a704cb7014) Thanks [@minjun0219](https://github.com/minjun0219)! - 사용 로그 — rocky 의 표면(REST 라우트 · MCP 도구 · `rocky <cmd>` · 훅 · 웹 UI 이벤트)이 얼마나
  쓰이는지를 `~/.config/rocky/usage/YYYY-MM.jsonl` 에 한 줄씩 남긴다(내용 없이 이름·누가·클라이언트·
  성공·시간만; statusline·SSE·health 는 제외). `rocky usage [--since 30d] [--json]` 이 많이 쓴 표면·
  에러·**안 쓴 표면**·시간 분포를 낸다. `rocky.json` `usage` 블록 / `ROCKY_USAGE=0` 으로 끈다.
  표면을 빼거나 바꾸는 PR 은 이 수치를 인용한다.

- [#166](https://github.com/minjun0219/rocky/pull/166) [`de2f8ab`](https://github.com/minjun0219/rocky/commit/de2f8abd6801ba370c69b5e5cdaef15010517130) Thanks [@minjun0219](https://github.com/minjun0219)! - 웹 UI 팔레트를 "관제판" 으로 갈았다 — 슬레이트 그레이 바탕(라이트 기본, 다크 동반)에 상태색만:
  `running`(그린) · `mine`(번트 오렌지, 내 차례) · `dead`(세션 없음) · `link`. 갈색·앰버·아이스블루의
  "두 대기" 톤은 걷어냈다(에이전트/사람 구분은 다음 단계에서 글자로). 컴포넌트가 아직 쓰는 옛 이름
  (`warm`/`cool`)은 새 의미(강조/링크)로 매핑돼 있고, 레이아웃과 동작은 그대로다.

- [#168](https://github.com/minjun0219/rocky/pull/168) [`c2aafe9`](https://github.com/minjun0219/rocky/commit/c2aafe9e6a9cfe239322c708e9d97813c91b4532) Thanks [@minjun0219](https://github.com/minjun0219)! - 웹 UI 레이아웃을 관제판으로 — 한 열. 맨 위 "지금" 표가 전 보드의 진행중(누가 · 초 단위로 흐르는
  경과 · 상태), 세션 없음, 핸드오프 대기, 읽지 않은 댓글, 수집함 미올림을 사람이 손댈 것부터 보여주고
  (행을 누르면 상세), 그 아래 보드 탭 행 → 섹션 목록 → 메모(접힘 토글). 왼쪽 보드 사이드바와 오른쪽
  메모 열은 없어졌다. 동작은 그대로다.

### Patch Changes

- [#172](https://github.com/minjun0219/rocky/pull/172) [`91adcb6`](https://github.com/minjun0219/rocky/commit/91adcb6e596fd04e5156a40e94de26a421144d4c) Thanks [@minjun0219](https://github.com/minjun0219)! - 터미널에서 `rocky` 가 바로 불린다 — SessionStart 가 `~/.local/bin/rocky` 를 `current/rocky` 로 걸어
  두고(릴리스마다 따라온다), `rocky config show` 가 링크·PATH 상태를 `cli` 행으로 알리며, `rocky config link`
  로 지금 바로 걸 수 있다. 심볼릭 링크로 불려도 `rocky tui`·`rocky daemon start` 가 형제 바이너리를
  제대로 찾는다.

- [#162](https://github.com/minjun0219/rocky/pull/162) [`e21439f`](https://github.com/minjun0219/rocky/commit/e21439fe95245b51cdaf98354508dc5eed3d4786) Thanks [@minjun0219](https://github.com/minjun0219)! - `/reload-plugins` 로 플러그인만 갈아 끼운 세션에서도 데몬이 새 버전으로 재기동된다 — 매 턴의
  `UserPromptSubmit` 훅이 도는 데몬이 자기보다 **오래됐을 때만** 올린다(없거나 더 새 데몬은 그대로,
  옛 플러그인 세션과 새 세션이 서로 뒤집지 않게). 지금까지는 다음 세션 시작 때까지 구버전이 남았다.

- [#170](https://github.com/minjun0219/rocky/pull/170) [`98ca22f`](https://github.com/minjun0219/rocky/commit/98ca22f4d439d7ddc61eaa09227da2188ae3e4c7) Thanks [@minjun0219](https://github.com/minjun0219)! - 웹 UI 가 서버를 안 거치는 조작 — 보드 탭 · 항목 열기 · 메모 접기 · 테마 · 보관됨 표시 · 빠른
  추가 — 를 `web:*` 이름으로 사용 로그에 보낸다. `/rocky:usage` 커맨드는 `rocky usage --json` 을
  읽어 뺄 것·손볼 것·더 쓸 것을 제안한다(결정은 사람이).

- [#165](https://github.com/minjun0219/rocky/pull/165) [`26df2d0`](https://github.com/minjun0219/rocky/commit/26df2d074f37afd957a360886102df17136ab74f) Thanks [@minjun0219](https://github.com/minjun0219)! - 웹 UI 글자를 키우고 대비를 올렸다 — 본문 14→15px, 칩·뱃지·시각 10→12px, 대문자 라벨 10→11px,
  보드 이름 17→20px, 다크의 흐린 글자색을 4.5:1 위로. 임의 px 대신 여섯 단 타입 스케일 토큰
  (`text-micro/chip/meta/sm/body/title`)만 쓴다. 상단의 온도 띠(최근 활동 48건 눈금)에는
  "에이전트 · 3시간 전" 처럼 마지막 활동을 글자로 붙여 무슨 뜻인지 읽히게 했다.

## 0.27.0

### Minor Changes

- [#158](https://github.com/minjun0219/rocky/pull/158) [`1578e2e`](https://github.com/minjun0219/rocky/commit/1578e2e8d04f573576e10467a68d41bf6af2852f) Thanks [@minjun0219](https://github.com/minjun0219)! - `/rocky:config` 와 `rocky config` — 새 기기 셋업을 손으로 하지 않는다. `rocky config show` 가
  설정 파일·설치본·데몬·launchd 상주·세션 요약·노출·수집함·Claude Code statusline 연결·보드 ↔
  레포 경로를 한 번에 점검해 `다음 할 일` 을 내고(`--json` 가능), `rocky config init` 은 기본
  `rocky.json`(expose off · sessionSummary on · `$schema`)을 없을 때만 만든다. 슬래시 커맨드는 그
  결과를 보고 빠진 항목을 하나씩 물어 채우고, `expose off` 같은 값 변경도 같은 자리에서 한다.
  사용자 파일(`settings.json` 의 statusLine)은 덮어쓰지 않는다.

## 0.26.1

### Patch Changes

- [#156](https://github.com/minjun0219/rocky/pull/156) [`fdd0b7c`](https://github.com/minjun0219/rocky/commit/fdd0b7c55a59a78fdae3b7c6b3c1c84afd8c29a8) Thanks [@minjun0219](https://github.com/minjun0219)! - 보관된 todo 의 핸드오프는 더 이상 "열린" 것으로 세지 않는다 — `GET /api/handoffs?open=true` 와
  요약(`rocky today` · SessionStart · `/api/summary` 의 `handoffsOpen`)에서 빠진다. 배달만 되고
  착수 없이 보관된 옛 테스트 핸드오프가 요약에 "핸드오프 대기 1" 로 영원히 남던 문제. 보관을
  해제하면 다시 열린다.

## 0.26.0

### Minor Changes

- [#148](https://github.com/minjun0219/rocky/pull/148) [`698e076`](https://github.com/minjun0219/rocky/commit/698e0767619607c5769e4c5218920aa1d946017f) Thanks [@minjun0219](https://github.com/minjun0219)! - 수집함 어댑터 첫 실물 — `bridges/todoist/inbox.py`. Todoist API v1 의 미완료 작업을 규약
  `{ "items": [...] }` 로 낸다(`--filter` 로 Todoist 필터 문법, 커서 페이지 전부 수집, 완료·삭제
  제외, 링크는 `app.todoist.com/app/task/<id>`). 토큰은 `--op op://…` 로 1Password Agent Vault
  에서 읽고 값은 어디에도 찍지 않는다. `python3` stdlib 만, 의존 없음.

- [#150](https://github.com/minjun0219/rocky/pull/150) [`cf0e145`](https://github.com/minjun0219/rocky/commit/cf0e1459de50a7d38cd4e2d09f2168e0dd1e239f) Thanks [@minjun0219](https://github.com/minjun0219)! - 보드 요약 세 자리 — `rocky today`(마감·진행중·핸드오프·수집함 미올림 + 항목 4개, Claude Code 의
  `! rocky today` 로 LLM 없이), SessionStart 훅이 같은 요약을 세션 컨텍스트에 넣기
  (`todo.sessionSummary: false` 로 끔), statusline 템플릿 변수 `{due}`·`{collect}`. 뒤에서
  `GET /api/summary` 와 `GET /api/inbox?cached=true`(기다리지 않는 수집함 조회 — 캐시만, 없으면
  백그라운드 갱신)가 생겼다.

- [#151](https://github.com/minjun0219/rocky/pull/151) [`b12051e`](https://github.com/minjun0219/rocky/commit/b12051e076e3ac7bb929004a85b4570560dc0fb3) Thanks [@minjun0219](https://github.com/minjun0219)! - 웹 UI 복귀 — rocky-todo 시절의 React 보드 UI 를 `web/` 로 되살려 릴리스 tarball 에 `dist/` 로 동봉한다.
  데몬이 실행 파일 옆 `dist/` 를 `http://127.0.0.1:8636/` 에 서빙한다(`ROCKY_TODO_UI_DIST` 로 override).
  보드·섹션·상세 드로어(마크다운·댓글·타임라인)·핸드오프·이슈 생성·새 세션·메모 그대로. 이름만 rocky 로
  (`/rocky:board` 복사, localStorage 키). 테일넷 없이 밖에서 닿는 길(Cloudflare Tunnel + Access)은 다음 조각.

### Patch Changes

- [#154](https://github.com/minjun0219/rocky/pull/154) [`505e1fb`](https://github.com/minjun0219/rocky/commit/505e1fb0784945c513022739a435f05b424d991e) Thanks [@minjun0219](https://github.com/minjun0219)! - TUI 의 GitHub 이슈·PR 상태를 `gh pr view` 프로세스 대신 GraphQL 한 요청으로 읽는다 — 토큰은
  `gh auth token` 으로 한 번 받아 메모리에만(헤더로만 나감), 링크가 N 개여도 요청 하나(보드 rocky-21).

- [#152](https://github.com/minjun0219/rocky/pull/152) [`59d7931`](https://github.com/minjun0219/rocky/commit/59d793191decc99fa704f1e05fcc604cce1f1e17) Thanks [@minjun0219](https://github.com/minjun0219)! - Cloudflare Tunnel·Access 가 붙이는 헤더(`cf-connecting-ip` / `cf-ray` / `cf-access-*`)를 중계 헤더로
  본다 — 터널 경유 요청이 원격으로 분류되어 이슈 생성·spawn·claim 이 막힌다(의도). 테일넷 없이 웹 UI 에
  닿는 설정 절차를 `docs/board.md` 에 적었다.

## 0.25.0

### Minor Changes

- [#143](https://github.com/minjun0219/rocky/pull/143) [`17548b1`](https://github.com/minjun0219/rocky/commit/17548b1a9ac43b9cedb9e13b6247d145daee396e) Thanks [@minjun0219](https://github.com/minjun0219)! - 수집함 — 외부 투두 앱을 읽기 전용으로 보드 옆에 띄운다. `rocky.json` 의 `todo.inbox[]` 에
  어댑터 명령을 등록하면 데몬이 실행해 stdout JSON(`{ "items": [...] }`)을 읽고
  `GET /api/inbox?refresh=true` 로 소스별로 합쳐 준다(동시 실행, 소스별 60초 캐시, 실패는 그 소스만
  `available:false`). 동기화가 아니다 — 보드로 올리는 건 클라이언트가 링크를 달아 한다. 참조 구현
  `bridges/file/inbox.sh`. MCP 도구는 그대로 5개.

- [#146](https://github.com/minjun0219/rocky/pull/146) [`986e27e`](https://github.com/minjun0219/rocky/commit/986e27e033b8448275def46bd5970c779b19ce5d) Thanks [@minjun0219](https://github.com/minjun0219)! - TUI — `rocky tui [--board K]` 가 보드를 터미널 화면으로 띄운다(새 바이너리 `rocky-tui`, 릴리스
  tarball 동봉). 섹션별 목록 + 선택 항목 상세(설명·링크·댓글), SSE 로 자동 갱신(끊기면 백오프
  재연결 + 전체 refetch), `s`/`x`/`d`/`o`/`a` 로 상태 변경, `Tab` 으로 보드 전환. 보드는 CLI 와
  같은 규약으로 cwd 에서 유추한다. 수집함 탭·핸드오프·GitHub 상태는 다음 조각.

- [#147](https://github.com/minjun0219/rocky/pull/147) [`4122c06`](https://github.com/minjun0219/rocky/commit/4122c064191c361be16c6772c871e34801363a40) Thanks [@minjun0219](https://github.com/minjun0219)! - TUI 두 번째 조각 — 수집함 탭(`Tab`, 외부 투두 앱 항목을 `p` 로 보드 백로그에 링크 달아 올리기,
  이미 올라간 항목은 ✓), 핸드오프(`h`, 후보 1개면 바로·여럿이면 세션 피커) · 새 세션(`n`) ·
  이슈 생성(`i`), 상세의 GitHub 이슈·PR 상태 한 줄(`gh` 백그라운드 조회, 5분 캐시), 열린 핸드오프
  표시(`⇢N`). 보드 전환 키는 `Tab` 에서 `[`/`]` 로 옮겼다.

## 0.24.1

### Patch Changes

- [#139](https://github.com/minjun0219/rocky/pull/139) [`a87e907`](https://github.com/minjun0219/rocky/commit/a87e907b2305c4ff189112263aeb51b3a26fecb5) Thanks [@minjun0219](https://github.com/minjun0219)! - 버전 없는 고정 진입점 `~/.local/share/rocky/current/rocky` — SessionStart 훅이 부트스트랩한
  버전으로 `current` 링크를 걸어 둔다. 플러그인 밖에서 worklog MCP 를 붙이는 설정(`claude -p
--strict-mcp-config` 배치 잡, Codex, opencode)이 릴리스마다 경로를 고치지 않아도 된다.

## 0.24.0

### Minor Changes

- [#135](https://github.com/minjun0219/rocky/pull/135) [`7800d00`](https://github.com/minjun0219/rocky/commit/7800d000166b32984508664cfe7f7dfc4d1beb76) Thanks [@minjun0219](https://github.com/minjun0219)! - 이름을 `rocky` 로 통일한다 — CLI `rocky-todo` → `rocky`, 데몬 `rocky-todod` → `rockyd`, 크레이트
  `rocky-core` / `rockyd` / `rocky-cli`, 릴리스 자산 `rocky-v<버전>-<target>.tar.gz`, 설치 디렉터리
  `~/.local/share/rocky/v<버전>/`, 부트스트랩 `plugin/bin/rocky` 와 env `ROCKY_BIN` /
  `ROCKY_RELEASE_BASE`, launchd 라벨 `com.rocky.daemon`, 데몬 health 의 `name: "rocky"`,
  훅 주입 블록 제목 `# rocky: …`. 문서는 `docs/board.md` 로 옮기고 웹 UI·데스크톱 앱 절을 걷어냈다.

  그대로 두는 것: 보드 key(데이터), `~/.config/rocky/todo`, 설정 env `ROCKY_TODO_*`(`todo` 블록의
  키), MCP 도구명 `todo_*` / `note_*`, 역사 문서.

  **업그레이드 주의**: 옛 이름(`name: "rocky-todo"`)으로 응답하는 0.23.0 이하 데몬은 새 CLI 가
  자기 데몬으로 보지 않는다 — 업데이트 전에 옛 데몬을 내려야 새 데몬이 포트를 잡는다.
  `rocky-todo daemon install` 로 launchd 에 올려 뒀다면 `pkill` 은 KeepAlive 가 바로 되살리므로
  job 부터 내린다:

  ```bash
  launchctl bootout "gui/$(id -u)/com.rocky.todo" 2>/dev/null
  rm -f ~/Library/LaunchAgents/com.rocky.todo.plist
  pkill -f rocky-todod
  ```

  상주가 필요하면 업데이트 뒤 `rocky daemon install` 로 새 라벨(`com.rocky.daemon`)을 등록한다.
  `~/.local/share/rocky-todo/` 의 옛 설치본은 지워도 된다.

## 0.23.0

### Minor Changes

- rocky-todo 를 흡수한다 (hail-mary D-046). Rust 데몬 `rocky-todod` · CLI `rocky-todo` · 코어가
  `crates/` 로 들어오고(히스토리 보존, `--allow-unrelated-histories`), 플러그인은 하나 `rocky` 가
  된다 — MCP 서버는 데몬 http(`rocky`, 보드 5 도구) + 임시 stdio(`worklog`, 4 도구), 훅은
  SessionStart(데몬 기동) · UserPromptSubmit(보드 주입) · Stop(핸드오프 → 워크로그 기록),
  커맨드에 `/rocky:next`, 스킬에 `board` 가 추가된다. 보드 도구 id 는
  `mcp__plugin_rocky_rocky__todo_*`, worklog 는 당분간 `mcp__plugin_rocky_worklog__worklog_*`.

  안 가져온 것: React 웹 UI · Tauri 앱 · `rocky-todo app` 서브커맨드 · TS 참조 구현 —
  rocky-todo 히스토리에 남는다. GUI 는 Swift 또는 TUI 로 별도 결정. 이름(`rocky-todo` /
  `rocky-todod` / 크레이트)은 아직 옛것이며 개명은 별도 PR.

  릴리스 tarball 은 바이너리 둘만 담고, `bin/rocky-todo` 부트스트랩은 이 레포의 Release 에서
  받는다. `package.json` · `plugin.json` · `Cargo.toml` · `Cargo.lock` 버전이 lockstep 이어야
  한다(`ensure-daemon` 이 정확 일치로 구버전을 판정).

  worklog 도 Rust 로 간다 — `worklog_*` 4 도구는 CLI 의 stdio MCP 서버(`rocky-todo mcp worklog`),
  Stop 훅의 턴 기록은 `rocky-todo hook log-turn`. 저장 형식(JSONL)·경로·프로젝트 키
  (`<basename>-<sha1[:8]>`)는 TS 판과 바이트 동일해 기존 앵커가 그대로 이어진다. 데몬이 아니라
  CLI 인 이유는 워크로그가 프로젝트별인데 데몬은 호출자의 cwd 를 모르기 때문. 이로써 런타임 TS
  가 사라지고 `package.json` 은 개발 도구(biome·changesets·릴리스 스크립트)만 남는다.

  플러그인 표면은 `plugin/` 로 모이고 마켓플레이스 소스가 `./plugin` 이 된다 — 설치본에
  `crates/`·`target/`·`node_modules` 가 더 이상 복사되지 않는다.

- [#131](https://github.com/minjun0219/rocky/pull/131) [`daf2641`](https://github.com/minjun0219/rocky/commit/daf2641c63b43ba6be46f5e822462c283264a267) Thanks [@minjun0219](https://github.com/minjun0219)! - `openapi_*` 7종 · `seo_validate` · `notion_*` 4종과 단독 CLI `openapi-mcp` 를 제거한다.
  39개 레포 5,216 턴의 워크로그를 세어 보니 호출이 0회였다 (같은 기간 `worklog_read` 는 78회).
  4,145 LOC 와 런타임 의존 6개(`@apidevtools/swagger-parser` · `swagger2openapi` · `js-yaml` ·
  `openapi-types` · `pino` · `ogpeek`)가 같이 빠지고, MCP 표면은 `worklog_*` 4개만 남는다.
  `rocky.json` 의 `openapi` / `seo` 블록은 이제 알 수 없는 키로 거부되니 옛 설정 파일에
  남아 있으면 지운다. 전부 git 히스토리에서 꺼낼 수 있다.

## 0.22.2

### Patch Changes

- [#129](https://github.com/minjun0219/rocky/pull/129) [`e9e286c`](https://github.com/minjun0219/rocky/commit/e9e286c933c80ce3f5b683107a2aac02b0372a80) Thanks [@minjun0219](https://github.com/minjun0219)! - `/rocky:finish` 의 PR 본문을 "무엇을·왜 / 변경 사항 / 검증" 세 부분 열 줄 안팎으로 줄이고
  (변경 사항은 파일별 나열이 아니라 읽어야 할 1~3곳만, 항목마다 제목 → 설명 → (스니펫) →
  `[경로:줄](PR 의 Files changed 위치)` 순. 링크는 새 `scripts/permalink.ts` 가 만든다.
  레포에 PR 템플릿이 있으면 그쪽이 우선),
  `/rocky:resolve-reviews` 가 스레드를 닫지 않도록 바꾼다 — 어떤 판정이든 코멘트도 resolve 도
  하지 않고 첫 코멘트에 👀 리액션만 남긴다. 에이전트가 닫은 스레드는 사용자가 다시 열어 보지
  않으므로, 닫는 행위를 사용자의 검토 기록으로 되돌린다. 머지 가능 판정에서도 "미해결 스레드 0"
  조건을 뺐다.

## 0.22.1

### Patch Changes

- [#127](https://github.com/minjun0219/rocky/pull/127) [`8fa01b8`](https://github.com/minjun0219/rocky/commit/8fa01b857980c882fb931cf10a7d5d5bec3459b8) Thanks [@minjun0219](https://github.com/minjun0219)! - `/rocky:finish` 의 PR 본문을 "무엇을·왜 / 변경 사항 / 검증" 세 부분 열 줄 안팎으로 줄이고
  (변경 사항은 파일별 나열이 아니라 읽어야 할 1~3곳만, 항목마다 제목 → 설명 → (스니펫) →
  `[경로:줄](PR 의 Files changed 위치)` 순. 링크는 새 `scripts/permalink.ts` 가 만든다.
  레포에 PR 템플릿이 있으면 그쪽이 우선),
  `/rocky:resolve-reviews` 가 스레드를 닫지 않도록 바꾼다 — 어떤 판정이든 코멘트도 resolve 도
  하지 않고 첫 코멘트에 👀 리액션만 남긴다. 에이전트가 닫은 스레드는 사용자가 다시 열어 보지
  않으므로, 닫는 행위를 사용자의 검토 기록으로 되돌린다. 머지 가능 판정에서도 "미해결 스레드 0"
  조건을 뺐다.

## 0.22.0

### Minor Changes

- [#126](https://github.com/minjun0219/rocky/pull/126) [`242ba12`](https://github.com/minjun0219/rocky/commit/242ba1220c613d1047aec547014747ba282d7322) Thanks [@minjun0219](https://github.com/minjun0219)! - 번들 스킬 `todoist` 를 제거한다

  rocky 가 다루는 작업 목록은 rocky-todo 보드 하나이고 기록은 `worklog_*` 다. 외부 태스크
  서비스 연동은 이 플러그인의 표면에 두지 않는다 — 자격증명을 싣지 않고 세션에 연결된 MCP 만
  빌려 쓰는 스킬이어도 마찬가지다.

  - `skills/todoist/` 삭제. 스킬 자체는 오너의 `harness` 레포로 이관했다.
  - `README.md` · `docs/hosts.md` · `.claude-plugin/plugin.json`(description + keywords) 동기화.
  - `AGENTS.md` 의 _Scope → Out_ 에 "외부 태스크 서비스 연동 금지" 항목 추가.

  되살릴 일이 있으면 git 히스토리에서 꺼낼 수 있다.

- [#124](https://github.com/minjun0219/rocky/pull/124) [`c4ef827`](https://github.com/minjun0219/rocky/commit/c4ef827b69f8209a15e57838d35a3af2937b3e23) Thanks [@minjun0219](https://github.com/minjun0219)! - reviewer 서브에이전트를 추가한다

  `/rocky:review` 안에 인라인으로 있던 리뷰어 역할(읽기 전용 규율·심각도 기준·출력 형식)을
  `agents/reviewer.md` 로 추출했다. 커맨드는 이 작업에만 해당하는 정보(요약·요구사항·범위)만
  넘기는 얇은 dispatcher 가 되고, 같은 역할을 다른 진입점에서도 쓸 수 있다 — "리뷰해줘" 로
  직접 호출하는 경로 포함.

  추출하면서 규율 두 가지를 명문화했다: **검증 후 단언**(돌려본 것만 통과라고 쓰고 안 돌렸으면
  미검증이라고 적는다)과 **false pass 함정 체크리스트**(출력을 잘라 읽어 에러 요약을 놓치는 것,
  비교 명령이 조용히 빈 결과를 내는 것, 회귀 테스트의 negative control 부재 등).

## 0.21.0

### Minor Changes

- [#122](https://github.com/minjun0219/rocky/pull/122) [`98a0a09`](https://github.com/minjun0219/rocky/commit/98a0a090cc4581fdbcf61c3cf0acebbe3982f783) Thanks [@minjun0219](https://github.com/minjun0219)! - `/rocky:review-pr` 을 `/rocky:resolve-reviews` 로 바꾼다.

  옛 이름은 동사+목적어로 읽혀서("PR 을 리뷰해라" — 실제로 빌트인 `/review` 가 하는 일)
  완료 전 자기 diff 를 검토받는 `/rocky:review` 와 계속 헷갈렸다. 새 이름은 하는 일과 대상을
  말한다 — 받은 리뷰 스레드를 해소한다. 동작은 그대로다.

  **옛 이름은 남기지 않았다** — `/rocky:review-pr` 는 더 이상 없다.

  지침도 같이 조정했다. 판정의 축이 "호출자 판단이 필요한가" 하나로 정리된다:

  - 판단이 필요 없는 **명백한 오류는 즉시 고치고 코멘트 없이 resolve**. 명백하다고 부르는
    조건을 셋(지적이 사실임을 코드로 확인 · 고치는 방법이 하나 · 다른 결정을 건드리지 않음)으로
    못박았고, 하나라도 아니면 확인 필요로 보낸다.
  - 호출자가 확인해야 하는 건은 **resolve 하지 않고** 채팅으로 보고한다. 열려 있는 스레드가
    "아직 결정되지 않았다" 는 표시라서, 닫으면 그 사실이 사라진다.
  - **GitHub 코멘트는 가급적 달지 않는다.** 수정한 건도 무효인 건도 코멘트 없이 resolve 만
    한다 — 무엇을 고쳤는지는 커밋과 diff 가 말한다. 코멘트를 만드는 경우는 호출자가 승인한
    반론 하나뿐이다.

## 0.20.1

### Patch Changes

- [#120](https://github.com/minjun0219/rocky/pull/120) [`d75aee4`](https://github.com/minjun0219/rocky/commit/d75aee485a7fa36a34294348c67c57136fb4f97d) Thanks [@minjun0219](https://github.com/minjun0219)! - `/rocky:recall` 이 읽은 프로젝트와 다른 워크로그에 쓰지 못하게 막는다

  worklog 의 프로젝트 키는 MCP 서버 프로세스의 `process.cwd()` 에서 나오는데, rocky MCP 서버는
  프로젝트마다 따로 뜬다. 세션이 붙은 인스턴스가 실행 도중 갈아끼워지면 **한 번의 recall 안에서도
  읽는 프로젝트와 쓰는 프로젝트가 갈린다** — 실제로 A 를 읽고 B 에 digest 를 써서 B 의 watermark 를
  오염시키는 사고가 났다. 그 프로젝트의 digest 가 그것 하나뿐이면 이전 항목 전부가 다음 증분에서
  영구히 건너뛰어진다.

  커맨드 절차에 방어를 넣었다 — 1단계에서 `projectKey` 를 적어두고, append 직전 `worklog_status` 를
  다시 불러 대조한 뒤 불일치면 중단한다. 이미 오염된 경우의 복구 절차(정정 note + 전체 범위 재-digest)도
  예외 처리에 명시했다.

## 0.20.0

### Minor Changes

- [#118](https://github.com/minjun0219/rocky/pull/118) [`9f3e68d`](https://github.com/minjun0219/rocky/commit/9f3e68df2d61f87c9ff0ce9f53c76ebca9b29582) Thanks [@minjun0219](https://github.com/minjun0219)! - `/rocky:review-pr` 에서 재리뷰 폴링 루프를 걷어냈다. PR 에 지금 붙어 있는 리뷰를 **한 번에
  처리하고 끝난다** — 60초 간격 `Monitor` 폴링, 8분 수렴 판정, 라운드 상한, 진전 없음 감지가 모두
  사라졌다 (249줄 → 205줄, `Monitor` 도구 의존도 제거).

  원래 이 루프는 **Copilot 이 푸시마다 자동 재리뷰한다**는 전제 위에 있었다. 그 설정을 끄면서
  전제가 사라졌고, 그대로 두면 오지 않을 재리뷰를 매번 8분씩 기다리게 된다. 무엇보다 라운드가
  계속 쌓이는 방식 자체가 피로했다 — 한 PR 에서 라운드 6까지 간 적도 있다.

  리뷰를 한 번 더 받고 싶으면 `@copilot review` / `@codex review` 를 직접 달고 커맨드를 다시 부른다.
  미해결 스레드가 0 이면 머지 가능 판정으로 바로 넘어가고, 봇 리뷰 대기로 `BLOCKED` 이면 그 사실만
  보고하고 끝낸다(기다리지 않는다).

### Patch Changes

- [#117](https://github.com/minjun0219/rocky/pull/117) [`459f5b8`](https://github.com/minjun0219/rocky/commit/459f5b89aa566d9ddb4a07a2f4a8abea718ae630) Thanks [@minjun0219](https://github.com/minjun0219)! - `/rocky:recall` 의 서브에이전트 모델 선택을 특정 모델 고정에서 **등급 선택**으로 일반화한다.
  Haiku/Sonnet 은 예시(기본값)로 남고, 배치 크기 기준(작은 배치 → 더 저렴한 쪽, 큰 배치 → 더 큰
  컨텍스트/품질)은 그대로다. 세션 환경이 다른 저비용 백엔드(예: 로컬 모델 위임)를 제공하면 스킬
  지시와 충돌 없이 그쪽을 고를 수 있다. 다이제스트 출력 형식과 `kind:"digest"` 기록 방식은 변경 없다.

## 0.19.0

### Minor Changes

- [#115](https://github.com/minjun0219/rocky/pull/115) [`9fda5c4`](https://github.com/minjun0219/rocky/commit/9fda5c44b08df5ab5de6457a5e493bc1c6a96abe) Thanks [@minjun0219](https://github.com/minjun0219)! - worklog 프로젝트 키를 cwd 가 아니라 **레포 루트** 기준으로 잡는다. git worktree 에서 작업해도
  원본 워크스페이스와 같은 워크로그에 쌓인다.

  `git rev-parse --git-common-dir` 는 linked worktree 안에서도 주 워크트리의 `.git` 을 가리킨다 —
  그 경로를 해시하면 worktree 와 원본이 한 키로 접힌다. git 레포가 아니면 예전처럼 cwd 기준이고,
  git 호출은 실패해도 throw 하지 않는다 (워크로그 기록이 git 유무로 깨지면 안 된다).

  경로는 `realpathSync` 로 정규화한다 — worktree 의 common dir 은 realpath 로 나오는데 cwd 는
  아닐 수 있어(macOS 의 `/tmp` → `/private/tmp`), 정규화하지 않으면 같은 레포가 여전히 두 해시로
  갈린다.

  **왜 고쳤나**: 실측 결과 `~/.config/rocky/worklog` 에 디렉터리가 58 개 쌓여 있었는데 실제
  프로젝트는 15 개였다. 나머지는 worktree 마다 갈라진 조각과, 그 worktree 가 삭제된 뒤 남은
  고아였다. 이 상태에서는 worktree 에서 `/rocky:recall` 을 돌려도 본체 히스토리를 못 읽어,
  "프로젝트를 넘나드는 기억"이라는 워크로그의 존재 이유가 깨진다.

  기존 디렉터리는 이름에서 원본 cwd 를 역산할 수 없어(sha1) 자동 마이그레이션이 제공되지 않는다.
  본체에서 쌓은 워크로그는 키가 그대로라 영향이 없고, worktree 조각만 새 키로 다시 시작된다.

- [#113](https://github.com/minjun0219/rocky/pull/113) [`5b08c06`](https://github.com/minjun0219/rocky/commit/5b08c062e0a2e1095fb7d98e189b37aaf9a40963) Thanks [@minjun0219](https://github.com/minjun0219)! - 소울(페르소나)과 statusline, 그리고 `/rocky:codex` · `/rocky:issue` 커맨드를 걷어냈다.

  - **소울** — `souls/*.md` 3종, `soul.ts`, `inject-soul` 훅, `/rocky:soul`, `rocky.json` 의
    `soul` / `callsign` 키. 재미로 넣은 기능이었고, 동시에 rocky 가 세션 컨텍스트에 넣던
    **유일한** 것이었다 (주입 1,310자 → 605자로 압축했다가 기능째 제거).
  - **statusline** — 템플릿 3종, `statusline.ts`, `sync-statusline` 훅, `/rocky:statusline`,
    `docs/statusline.md`. 컨텍스트 비용은 0 이었지만 함께 정리했다.
  - **커맨드** — `/rocky:codex` (Codex 위임은 공식 `openai/codex-plugin-cc` 가 덮는다),
    `/rocky:issue`.

  훅은 `Stop`(턴 자동 기록) 하나만 남는다 — **SessionStart 가 사라져 이제 플러그인이 세션
  컨텍스트에 넣는 것이 아무것도 없다.** MCP 도구 16 종과 `/rocky:brainstorm` · `/rocky:review` ·
  `/rocky:finish` · `/rocky:review-pr` · `/rocky:recall` 커맨드, 스킬 2종은 그대로다.

  기존 `rocky.json` 에 `soul` / `callsign` 이 남아 있으면 unknown key 로 거부되니 지워야 한다.
  `~/.claude/settings.json` 의 `statusLine` 설정과 `~/.config/rocky/statusline.sh` 도 직접 정리해야
  한다 (rocky 는 사용자 설정을 건드리지 않는다).

- [#113](https://github.com/minjun0219/rocky/pull/113) [`5b08c06`](https://github.com/minjun0219/rocky/commit/5b08c062e0a2e1095fb7d98e189b37aaf9a40963) Thanks [@minjun0219](https://github.com/minjun0219)! - opencode 위임 런타임을 걷어냈다. `/rocky:opencode` · `/rocky:opencode-jobs` 커맨드, companion CLI,
  잡 저장소, `SessionStart`/`SessionEnd` 잡 배선 훅, `rocky.json` 의 `opencode` 블록과
  `ROCKY_OPENCODE_*` 환경 변수가 사라진다 (코드 1,737 LOC + 테스트 9 파일).

  도입(v0.17) 이후 실제로 돈 위임 잡이 1 건뿐이었고, 커맨드 실행 흔적도 없었다. Codex 위임
  (`/rocky:codex`)은 그대로 남는다. MCP 도구 16 종(openapi*\* 7 / seo_validate / notion*\_ 4 /
  worklog\_\_ 4)과 소울·statusline·`Stop` 훅도 전부 유지된다.

  기존 `rocky.json` 에 `opencode` 블록이 남아 있으면 이제 unknown key 로 거부되니 지워야 한다.
  `~/.config/rocky/jobs/` 의 기존 잡 기록 파일은 삭제하지 않았다.

## 0.18.0

### Minor Changes

- [#110](https://github.com/minjun0219/rocky/pull/110) [`a16bfa8`](https://github.com/minjun0219/rocky/commit/a16bfa8b764a9f06f9377085f1fbe4f9c48a0d54) Thanks [@minjun0219](https://github.com/minjun0219)! - `/rocky:brainstorm` · `/rocky:review` 슬래시 커맨드 추가 — superpowers 플러그인을 걷어내면서 실제로 쓰던 두 발상만 rocky 자체 커맨드로 재작성했다. `/rocky:brainstorm` 은 아이디어를 설계로 다듬는다(맥락 파악 → 한 번에 하나씩 질문 → 접근안 2~3개 → 설계 → 규모가 클 때만 스펙 문서). `/rocky:review` 는 완료 선언 전 신선한 컨텍스트의 서브에이전트로 현재 작업 diff 를 검토시킨다(이미 열린 PR 의 스레드 대응인 `/rocky:review-pr` 과 별개). 원본과 달리 **강제 게이트가 아니다** — 사용자가 부를 때만 돌고, 작은 수정에는 요구하지 않는다. 설계·계획 산출물 디렉터리는 `docs/superpowers/` 에서 `docs/design/` 으로 개명(기존 문서는 경로만 갱신해 보존).

## 0.17.0

### Minor Changes

- [#107](https://github.com/minjun0219/rocky/pull/107) [`efab755`](https://github.com/minjun0219/rocky/commit/efab75579b221dc87780c3265589c3da14c27ecf) Thanks [@minjun0219](https://github.com/minjun0219)! - opencode 위임 런타임 추가 — `/rocky:opencode` 백그라운드 실행 + `/rocky:opencode-jobs`

  `/rocky:opencode` 의 dispatch 를 companion 런타임(`src/opencode-companion.ts`)이 맡는다. 프롬프트를
  `--prompt-file` 로 넘겨 셸 인용 문제를 없애고, `--format json` NDJSON 을 파싱해 최종 텍스트와
  opencode 세션 id 를 뽑는다. `--background` 를 붙이면 자기 자신을 detached `job-worker` 로 재실행해
  즉시 잡 id 를 돌려주고, 잡 조회·회수·취소는 새 커맨드 `/rocky:opencode-jobs` 가 담당한다.

  - 잡 상태는 `~/.config/rocky/jobs/<project-key>` 에 인덱스 + payload + 진행 로그로 저장
    (`ROCKY_OPENCODE_JOBS_DIR` / `rocky.json` 의 `opencode.dir`, `opencode.maxJobs` 기본 50)
  - `SessionStart`/`SessionEnd` 훅이 세션 id 를 주입해 잡을 세션별로 격리하고, 세션 종료 시
    진행 중이던 워커의 프로세스 그룹을 정리한다 (잡 기록은 보존)
  - 취소는 `kill(-pid)` 로 프로세스 그룹 전체를 끊어 opencode 자식까지 함께 종료
  - `rocky.json` 에 `opencode` 블록 추가 (`dir` / `maxJobs` / `model` / `agent`)
  - MCP 도구 표면은 변경 없음 — Codex / opencode 호스트에 영향 없다

- [#108](https://github.com/minjun0219/rocky/pull/108) [`83c9777`](https://github.com/minjun0219/rocky/commit/83c9777a55de5aad5f3eca10aff10d0134e3276f) Thanks [@minjun0219](https://github.com/minjun0219)! - `/rocky:review-pr` 슬래시 커맨드 추가 — PR 에 붙은 리뷰(Copilot / Codex / 사람)를 미해결 0 까지 처리한다. 수집 → 분류 → 수정 + 게이트 → 라운드당 커밋 1개 푸시 → resolve → 재리뷰 대기를 반복하고, 판단이 갈리는 지적은 보류 큐에 모아 수렴 후 사용자와 상의해 승인된 반론만 코멘트 + resolve 한다. 미해결 0 + checks 통과 시 머지 가능 알림을 보내며, 머지 자체는 하지 않는다. `/rocky:finish` 의 후속 안내도 이 커맨드로 교체.

## 0.16.0

### Minor Changes

- [#105](https://github.com/minjun0219/rocky/pull/105) [`2fa89e1`](https://github.com/minjun0219/rocky/commit/2fa89e1b0b74220cde056d71371eb3570222ddd0) Thanks [@minjun0219](https://github.com/minjun0219)! - `delegating-to-codex` 번들 스킬을 제거하고 `/rocky:codex` 를 자기완결화

  공식 [`openai/codex-plugin-cc`](https://github.com/openai/codex-plugin-cc) 플러그인이 Codex 위임
  영역을 공유 app-server 런타임 기반으로 더 넓게 덮으면서, rocky 가 같은 메커니즘을 스킬로 중복
  배포할 근거가 사라졌다. 공식 쪽 커버 범위는 스킬 3종(`codex-cli-runtime`, `codex-result-handling`,
  `gpt-5-4-prompting`)과 커맨드(`/codex:rescue`, `/codex:review`, `/codex:transfer`)다.

  `/rocky:codex` 에는 공식 플러그인에 없는 고유 가치가 남아 있어 유지한다 — **격리 git worktree** 와
  **rocky 플러그인 표면 무결 검증**(MCP 도구 개수/이름 + `.claude-plugin/plugin.json` 의 `mcpServers`).
  스킬에 있던 자기완결 프롬프트 원칙, 감독자 규칙, 샌드박스·모델 선택 가드레일은 커맨드 본문으로
  흡수했고, 공식 플러그인을 써야 할 상황을 커맨드 상단에 명시했다.

## 0.15.0

### Minor Changes

- [#102](https://github.com/minjun0219/rocky/pull/102) [`c67167c`](https://github.com/minjun0219/rocky/commit/c67167c648808f56b540eb48cff85f1217fde3b5) Thanks [@minjun0219](https://github.com/minjun0219)! - rocky-todo(공유 보드 데몬)를 별도 레포/플러그인 `minjun0219/rocky-todo` 로 분리했다. rocky 본체에서 todo 코드·데몬·웹 UI·CLI·`notify-todo` 훅·`todo` 스킬·`docs/rocky-todo.md` 를 제거하고 react/react-dom/zustand 의존을 걷어냈다. `rocky.json` 의 `todo` 키는 관용한다(rocky 는 무시, rocky-todo 데몬이 소비 — 공유 파일이라 거부하지 않음). rocky 마켓플레이스가 rocky-todo 를 github source 2번째 entry 로 서빙하므로 `claude plugin install rocky-todo@rocky-marketplace` 로 설치할 수 있다.

## 0.14.0

### Minor Changes

- [#100](https://github.com/minjun0219/rocky/pull/100) [`65aa2ea`](https://github.com/minjun0219/rocky/commit/65aa2ea25b6376056c163ec897215bf5d11ec1e8) Thanks [@minjun0219](https://github.com/minjun0219)! - rocky-todo 공유 todo/스크래치패드 데몬 추가 — 시스템 유일 상주 데몬(127.0.0.1:8636, bun:sqlite)이 계층/섹션/보드 todo + 스티커 메모 + 전 변경 히스토리(아카이브만, 삭제 없음)를 들고, 에이전트는 `/mcp`(streamable HTTP, `todo_list`/`todo_write`/`todo_status`/`note_list`/`note_write`) 또는 `rocky-todo` CLI(온디맨드 자동 기동, `daemon install` launchd 등록)로, 호출자는 React 웹 UI(SSE 실시간, 처리중 actor 뱃지)로 같은 보드를 본다. 역방향(사람→에이전트)은 `UserPromptSubmit` 훅이 데몬의 `/api/changes` 피드를 세션별 커서로 읽어 호출자의 웹 편집분만 자동 주입한다 (`todo.watch`/`ROCKY_TODO_WATCH` 토글, fail-open). 노출은 `todo.expose` 채널(`lan` 내부망 0.0.0.0 / `tailscale-serve` 테일넷 serve, 배열 조합 또는 단일 문자열, 기본 없음 = 이 머신만 — tailscale 채널이 없으면 tailscale 을 일절 안 건드림; 수동 `rocky-todo tailscale on|off|status`). 전체 기능은 마스터 스위치 `todo.enabled`(기본 off — 상주 데몬 opt-in, env `ROCKY_TODO_ENABLED` 우선)로 게이트된다. `rocky.json` 에 `todo.enabled`/`todo.port`/`todo.dir`/`todo.expose`/`todo.watch` 키, env `ROCKY_TODO_PORT`/`ROCKY_TODO_DIR`/`ROCKY_TODO_ACTOR`/`ROCKY_TODO_WATCH`/`ROCKY_TODO_EXPOSE`, 번들 스킬 `todo`, `docs/rocky-todo.md` 추가. 기존 full-surface MCP 표면(`src/index.ts`)은 불변.

## 0.13.0

### Minor Changes

- [#96](https://github.com/minjun0219/rocky/pull/96) [`e30f9d6`](https://github.com/minjun0219/rocky/commit/e30f9d6688273d29bb59f76b13c2cdda0b567efc) Thanks [@minjun0219](https://github.com/minjun0219)! - feat(statusline): 번들 statusline 추가 — statusLine 템플릿 3종(`statusline/<name>.sh`: `duo` 2줄 기본 / `mini` 1줄 / `full` 3줄+세션 비용·변경량·경과)을 플러그인이 소유하고, `/rocky:statusline` 커맨드가 고른 템플릿을 안정 경로 `~/.config/rocky/statusline.sh` 로 설치(user `settings.json` 의 `statusLine` 1회 지정, 초안 확인 + 타임스탬프 백업). 새 `SessionStart` 훅(`src/hooks/sync-statusline.ts`)이 설치본 헤더의 템플릿 마커를 읽어 플러그인 업데이트를 같은 템플릿에서 자동 전파한다 (미설치 시 no-op, fail-open). MCP tool 표면 변화 없음.

- [#99](https://github.com/minjun0219/rocky/pull/99) [`d16592a`](https://github.com/minjun0219/rocky/commit/d16592a9fa30e6b0e0d1512dae0c0b1a25777514) Thanks [@minjun0219](https://github.com/minjun0219)! - statusline full 템플릿 고도화 — git dirty(`*`)·ahead/behind(`↑↓`) 세그먼트, ctx/left 임계값 경고색(안전 dim / 70·30 경고 / 90·10 위험), 경과 5분 이상일 때 시간당 비용(`($N.N/h)`) 표시. 템플릿 3종 표시 내용 문서 `docs/statusline.md` 신설.

- [#98](https://github.com/minjun0219/rocky/pull/98) [`f488c79`](https://github.com/minjun0219/rocky/commit/f488c79fde2665c65c586ef94a18d56006f4a121) Thanks [@minjun0219](https://github.com/minjun0219)! - todoist 번들 스킬 추가 — 세션에 연결된 Todoist MCP 로 현재 레포의 작업 목록을 파악(다음 작업 제안: Todoist + git + worklog 교차)·등록(컨벤션 + 차등 확인 게이트)·마감하는 Claude Code 전용 스킬. rocky 는 Todoist 접근을 배포하지 않으며 도구 부재 시 중단·안내한다.

## 0.12.0

### Minor Changes

- [#91](https://github.com/minjun0219/rocky/pull/91) [`246243c`](https://github.com/minjun0219/rocky/commit/246243c3104ce96c4bd023aacb6d7f0e255bfcca) Thanks [@minjun0219](https://github.com/minjun0219)! - 소울이 사용자를 부르는 호칭(`callsign`) 설정 지원 — `rocky.json` 최상위 `callsign` 키(한 줄, 1~40자, project > user)를 `SessionStart` 훅이 소울 컨텍스트에 함께 주입하고, `/rocky:soul <name>` 세팅 플로우가 호칭을 물어보며, 새 `call` 서브커맨드로 호칭만 조회/변경/제거할 수 있다.

### Patch Changes

- [#95](https://github.com/minjun0219/rocky/pull/95) [`cf7dc50`](https://github.com/minjun0219/rocky/commit/cf7dc500868e21bd3b476c0e448d1bea47c89a47) Thanks [@minjun0219](https://github.com/minjun0219)! - docs(finish): PR·커밋 제목 장황화 금지 규칙 추가 — 제목에 핵심 하나를 넘는 나열·부연을 넣지 않는다(요약부 대략 50자 초과 금지), 밀려난 세부는 본문으로. `/finish` 커맨드와 AGENTS.md / FEATURES.md 의 출력 규칙에 금지형으로 반영 (본문 상세함은 기존 유지).

- [#94](https://github.com/minjun0219/rocky/pull/94) [`d4c127c`](https://github.com/minjun0219/rocky/commit/d4c127ca6a1f75660f897e00512d8be0f9ea79d1) Thanks [@minjun0219](https://github.com/minjun0219)! - rocky 소울 시그니처 다듬기 — 이해 선언을 "이해해." → "Understand!" / "이해 못 해." → "이해 못 함." 으로 바꾸고, "Amaze!" 는 항상 느낌표 종결임을 명시하고, 질문은 항상 "질문." 으로 종결하도록("커밋할까? 질문.") 규칙을 뒤집음.

## 0.11.0

### Minor Changes

- a881fb8: changesets 기반 버전 자동화 도입 — main 병합 시 `changesets/action` 이 "Version Packages" PR 을 자동으로 열어 `package.json` + `.claude-plugin/plugin.json` 버전 범프와 `CHANGELOG.md` 를 관리한다. 두 버전 파일은 `scripts/sync-plugin-version.ts` 로 lockstep 유지. (npm publish 는 자동화 대상 아님)
