# 보드 패널 — 세션 옆 패널에 보드를 띄우는 mod

> rocky 를 **고치는** 에이전트용 개발 문서. 쓰는 법은 `README.md` "Claude Code 전용 표면". Claude Code function hooks 는
> early access 라 API 가 릴리스마다 바뀔 수 있다 — 모듈을 고치기 전에 엔진이 깐 타입(`plugin/.claude-plugin/types/`, gitignore)이나
> 번들 `plugin-authoring` 스킬의 타입 파일을 다시 읽는다.

## 규칙

- **읽기만 한다.** 보드를 바꾸는 버튼을 두지 않는다 — 착수·완료는 TUI·웹 UI·보드 도구 몫이다.
- **명령은 그릴 데가 있을 때만 등록한다.** 터미널(REPL, `session.start` 의 `isInteractive`)이거나, 원격 클라이언트(데스크톱·모바일
  앱)가 붙었을 때다. 원격 제어처럼 SDK 로 도는 세션은 `isInteractive: false`·표면 없음으로 시작해 앱이 붙을 때(`session.attach`)
  등록하고, 시작할 때 이미 붙어 있으면(`$.session.surfaces()`) 바로 등록한다. 끝까지 아무도 안 붙는 `-p` 실행은 등록하지 않는다.
  여러 번 붙어도 한 로드에 한 번만.
- **패널은 저절로 열지 않는다** — `/rocky-board`(`immediate` — 턴 중에도 바로 열린다)로만 연다(오너 결정 2026-10-14).
- **데몬은 패널이 열려 있는 동안만 묻는다.** 5초 타이머는 패널을 처음 열 때(다시 로드된 모듈이 열린 패널을 이어받을 때도) 켜고
  끄지 않는다 — 열지 않은 세션에는 타이머가 없다(시작할 때 기록을 한 번 볼 뿐). 바퀴마다 먼저 엔진의 기록(`$.ui.panes()`)을 보고
  패널이 없으면 데몬을 묻지 않고 돌아선다(사람이 닫든 원격 앱에서 닫든 같다. 다른 패널 뒤 탭은 열린 것으로 본다). 기록을 못 읽으면
  데몬 탓으로 돌리지 않고 `패널 상태를 읽지 못했다` 를 남긴다. `/rocky-board` 는 바로 한 바퀴 돌고 — 바퀴가 도는 중이면 그 바퀴가
  끝난 뒤 한 번 더 — 다시 열면 마지막 id 부터 이어 간다. 타이머를 끄지 않는 까닭: 닫혔다고 본 바퀴가 타이머를 끄기 전에 다시 열면
  열린 패널이 멈춘 채 남는다. `ui.close` 훅을 쓰지 않는 까닭: 엔진 테스트 키트의 `$.ui` 에 `close` 가 없어 닫힘을 테스트로 일으킬
  수 없다.
- **폴링은 이 보드가 바뀌었을 때만 할 일을 다시 읽는다.** 5초마다 `GET /api/changes?sinceId=<마지막 id>&limit=50` 으로 새 히스토리
  행을 받고, 그 행의 `boardKey` 가 이 보드일 때만 `/api/todos` 를 다시 부른다(행이 꽉 차면 더 있을 수 있어 다시 읽는다). `lastId` 는
  전 보드의 것이라 그것만 보면 다른 보드의 변화에도 모든 세션의 패널이 다시 읽는다. `/api/todos` 는 진행 중 항목이 있으면 세션
  목록(SWR 3초)을 건드려서, 세션마다 다시 읽으면 뒤에서 `claude agents` 조회가 끊이지 않는다. 첫 바퀴는 `limit=0` 으로 id 만 받는다.
- **요약은 폴더가 바뀌었을 때와 60초마다만**(`/api/summary` — 보드 key·수집함 수). 수집함은 히스토리에 남지 않아 변화로는 알 수 없다.
- **요약은 `cached=true` 로 부른다.** 그냥 부르면 데몬이 캐시가 만료된 수집함 어댑터(외부 명령)를 실행하고 기다린다. 플래그는
  `"true"` 만 켜진다(`server.rs` 의 `flag`) — `cached=1` 은 꺼진 것으로 읽혀 조용히 느려진다. 그래서 수집함 수는 캐시가 없으면 `null`.
- **못 읽으면 사유만 남기고 지난 목록은 둔다.** 사유에는 어느 요청이 왜인지(`/api/changes 응답 500`, JSON 이 아님, 데몬에 닿지
  않음)를 담는다. 히스토리를 못 읽어도 할 일을 몇 초마다 통째로 다시 읽지 않는다. 바뀐 게 없으면 상태를 쓰지 않는다(다시 그리지 않는다).
- **사용 로그는 노출과 사용을 따로, 세션마다 한 번씩.** 패널의 요청은 모두 `x-rocky-client: claude-code-panel` 을 달고, 데몬은 그
  클라이언트의 **읽기(GET)**를 라우트로 세지 않는다(`usage::rest_surface` — 쓰기는 패널이 하지 않으니 그 헤더를 달아도 라우트로 센다).
  `x-rocky-usage: open` 은 패널을 연 뒤 첫 보드 읽기(`/api/todos`)에 붙어 `hook board-panel`(보드를 보였다),
  `use` 는 `/rocky-board` 실행이나 패널 포커스(`isFocused`)에 가장 가벼운 읽기로 붙어 `hook board-panel use`(사람이 손을 댔다)가 된다
  (오너 결정 2026-10-13). 패널이 `/rocky-board` 로만 열리게 된 뒤로 둘은 거의 같이 움직인다 — 보드가 없는 폴더에서 연 패널은 `use`
  만, 다시 로드된 모듈이 이어받은 패널은 `open` 만 남긴다.
- **리로드하면 다시 시작한다.** 모듈이 다시 로드되면 엔진이 타이머를 걷고 `session.start` 를 다시 부른다(엔진 문서: "once per fresh
  load of one") — 명령 등록이 거기서 다시 걸리고, 패널이 열려 있으면(`$.ui.panes()` — 엔진의 기록이라 모듈이 바뀌어도 남는다)
  폴링도 이어 간다.
- **상태 계약은 혼자 선다.** `plugin/types/index.d.ts` 는 import 할 수 없어(`claude plugin validate`) `BoardView` 모양을 옮겨 적었다.
  `data.ts` 와 어긋나면 플러그인 타입 검사(`register.tsx` 의 `atom`·`update`)가 잡는다.

## 코드

| 무엇 | 어디 |
| --- | --- |
| 순수 판정(요약·할 일 → 패널 줄, `lastId` 읽기) | `plugin/hooks/board-panel/data.ts` |
| 모듈(세션 시작·폴링·`/rocky-board`·`Pane` 그리기) | `plugin/hooks/board-panel/register.tsx` |
| 상태 계약 | `plugin/types/index.d.ts`(`plugin.json` 의 `types`) |
| 연결 | `plugin/hooks/hooks.json` 의 `modules` |
| 사용 로그(패널 클라이언트 → `hook board-panel`) | `crates/rocky-core/src/usage.rs`(`rest_surface`·`PANEL_CLIENT`), `crates/rockyd/src/server.rs` |

테스트: 순수 판정 `plugin/hooks/board-panel/data.test.ts`(bun, CI — `bun run test`), 모듈 `register.test.tsx`(엔진 키트 —
`bun run test:panel`, claude CLI 가 있어야 해서 CI 밖), 사용 로그 `crates/rocky-core/tests/it/usage_test.rs`
(`panel_requests_count_once_as_the_panel_surface`)·`crates/rockyd/tests/it/usage_test.rs`(`panel_polls_are_not_logged_and_its_first_board_read_counts_once`).

## 함정

- **`claude plugin test plugin/` 을 그대로 돌리지 않는다.** 그 폴더의 `*.test.ts` 까지 엔진 키트로 돌려 bun 테스트(`plugin/scripts`,
  `data.test.ts`)가 `bun:test` 를 못 불러 실패한다. `bun run test:panel`(`scripts/panel-check.ts`)이 검증은 배포되는 `plugin/` 그대로,
  테스트는 모듈 파일만 옮긴 임시 폴더에서 돌린다 — 엔진 테스트는 `*.test.tsx` 로 둔다.
- 계약(`plugin/types/index.d.ts`)과 `data.ts` 의 모양이 어긋나는 것을 정확히 잡는 플러그인 타입 검사는 CI 밖이다(엔진 타입이 claude
  CLI 에서 온다). CI 에서는 `data.test.ts` 가 두 타입의 필드 이름만 대조한다 — 필드의 타입을 바꿀 땐 `bun run test:panel` 도 돌린다.
- 엔진 테스트의 `$` 에는 `state` 가 없다 — 상태를 심지 말고 `session.start` 를 일으키고 아래 세계(`http.fetch`·`session.cwd`·
  `command.register`·`ui.open`·`ui.panes`)를 `{ value }` 로 답한 뒤 `mock.clock(on).settle()` 로 돌린다.
- 테스트 키트의 `$.ui` 에는 `open`·`close`·`panes` 가 없다(2.1.299 실측 — `focus`·`input`·`mount`·`press`·`render`·`scroll`·`select`
  뿐). 사람이 닫는 것은 세계의 패널 기록을 바꿔 흉내 낸다(`closePane`).
- `bun run test:panel` 은 테스트 파일의 타입을 보지 않는다. 엔진이 이 폴더를 로드하며(`claude --plugin-dir plugin`)
  `plugin/.claude-plugin/types/`(gitignore)에 타입을 깐 뒤 `bunx tsc --noEmit -p plugin/tsconfig.json` 으로 본다.
- 패널 본문은 엔진이 스크롤을 맡는다(`scroll.bodyRows`) — 줄 수를 세어 자르지 않는다. 폭은 `props.bodyColumns`.
