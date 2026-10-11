# TUI 를 Ink 로 다시 — 설계 (cmux 의 웹 패널 대체, 첫 조각: 할 일)

- 날짜: 2026-10-11
- 상태: 구현 — 아래 *PR 순서* 의 스택. cmux 실기기 확인은 남음. 규칙의 정본은 [`docs/features/tui.md`](../../features/tui.md)

## 한 줄

**cmux 안에서 웹 UI 패널을 대신할 터미널 화면을 Bun + React + Ink 로 만든다.** 웹 UI 는 폰·원격(터널 + Access)
용으로 그대로 남는다. TUI 는 데몬의 REST/SSE 를 쓰는 클라이언트라 언어 경계의 가장자리(`web/`·`bridges/` 와 같은 자리)다.

## 왜 다시인가

- **동기** — cmux 에서 브라우저 패널에 갔다가 터미널로 돌아올 때 잠깐 멈칫한다. 웹 쪽 원인은 아니다: 페이지는 숨겨질
  때 아무 일도 하지 않고(`web/main.tsx` 의 `visibilitychange` 는 다시 보일 때만 재조회) 멈칫은 cmux 가 웹뷰와
  터미널 사이를 오가는 비용이다. 터미널 패널끼리는 그 비용이 없다.
- **지난 TUI(`rocky-tui`, ratatui) 를 걷은 이유는 기술이 아니었다** — "90일 동안 0회, 웹 UI 하나로"(#278). 그때는
  웹 패널로 충분했고, 지금은 그 웹 패널을 오가는 게 거슬린다는 구체적인 장면이 생겼다.
- **성공 기준** — 2주 뒤 cmux 에서 웹 패널을 열지 않게 됐다(오너 체감). **철거 기준** — 30일 동안 `tui`
  클라이언트 사용이 0이면 다시 걷는다(`rocky usage --since 30d`).

## 결정 표

| # | 질문 | 결정 | 버린 것과 이유 |
|---|---|---|---|
| 1 | 웹 UI 와의 관계 | **cmux 안의 웹 패널만 대체**. 웹은 폰·원격용으로 유지 | 웹 철거 — 터널·원격 제어 탭이 웹에 있다 |
| 2 | 첫 조각 범위 | **할 일 화면 하나**(지금 표 · 섹션 트리 · 상세 읽기 · 상태 변경 · 빠른 추가) | 피드·노트·워크로그·rc·github·에이전트 — 30일 진입 수가 할 일(1,018) 다음은 노트(513, 연 것 8번)·피드(507)이고 나머지는 9~29. 다음 조각 후보는 피드 |
| 3 | 띄우는 방식 | **상주 + 폭 반응**. 100열 미만은 한 열(목록 → 상세로 밀어 넣기), 100열 이상은 두 열(목록 + 커서를 따라가는 상세) | 상주 한 열만 / 필요할 때 전체 화면만 |
| 4 | 실행·배포 | **번들 JS 한 파일 + 시스템 `bun`**. `rocky tui`(Rust)가 포트·actor 를 정하고 `bun --no-install tui.js` 를 exec | `bun build --compile` 단일 바이너리 — 플랫폼마다 59MB 이상(hello world 실측) / 레포에서 `bun run` — 설치본에 없다 |
| 5 | 웹 코드 공유 | **`web/types.ts`·`web/lib.ts`(순수 함수) 를 import**, 상태는 TUI 전용의 작은 zustand 스토어 | `web/store.ts` 를 쪼개 공유 — 1,192줄이 `window.location`·`history`·`localStorage` 에 묶여 있고 이번 목표와 무관한 리팩터다 |
| 6 | 새 의존성 | **`ink`(8.x) · `ink-testing-library`, 둘 다 devDependencies** — 번들에 들어가니 런타임 설치가 없다 | — |

## 스파이크 결과 (2026-10-11)

레포 밖 임시 폴더에서 Ink 8.0.0 + React 19.3.0 으로 보드 목록 + SSE 카운터 + j/k 이동 앱을 만들어 확인했다.

| 항목 | 결과 |
| --- | --- |
| React 버전 | Ink 8 은 `react >=19.3.0` 을 요구한다. 레포 lockfile 은 이미 19.3.0 이다 |
| 번들 | `Bun.build({ target: 'bun' })` 로 **529KB 한 파일**. yoga-layout 의 WASM 도 안에 들어가 `node_modules` 없이 돈다 |
| 함정 | 번들 맨 위에 `import … from "react-devtools-core"` 가 **정적으로** 남는다(Ink 는 `DEV=true` 일 때만 쓴다). `node_modules` 가 없는 폴더에서는 Bun 이 그 패키지를 네트워크로 **자동 설치**해서 돌아가 버리고, 있는 폴더에서는 시작하자마자 죽는다. → 빌드 플러그인으로 빈 모듈을 끼우고, 실행은 늘 `bun --no-install` |
| SSE | Bun 에는 전역 `EventSource` 가 없다 → `fetch` 스트림을 `\n\n` 프레임으로 자르는 파서(스파이크에서 20줄)로 `/api/events` 를 받았다 |
| 키 입력 | 가상 터미널(`script`)에서 j · ↓(이스케이프 시퀀스) · q 가 들어오고, q 로 언마운트 → 프로세스가 스스로 끝난다(SSE 를 연 채로도) |
| 대체 화면 | `alternateScreen: true` 로 진입·복귀(`?1049h`/`?1049l`) 각 1회 |
| 남은 것 | **cmux 실기기**에서 키 입력·패널 폭 변경·멈칫 없음 확인(오너) |

## 구조

```
tui/
├── main.tsx        진입 — 인자(--port --actor) → render(<App/>, { alternateScreen, incrementalRendering })
├── api.ts          REST(fetch) + SSE(fetch 스트림 파서). fetch 를 주입받아 테스트가 가짜로 바꾼다
├── store.ts        zustand vanilla — boards · todos · sections · 선택 보드 · 커서 · 열린 상세
├── App.tsx         폭으로 한 열/두 열을 고른다(useWindowSize)
├── components/     머리 · 지금 표 · 트리 · 상세 · 상태 줄 · 빠른 추가 입력
├── dist/           빌드 산출물 tui.js (기존 `.gitignore` 의 `dist/` 패턴이 덮는다)
└── *.test.ts(x)    순수 로직은 bun test, 화면은 ink-testing-library
scripts/build-tui.ts  Bun.build + react-devtools-core 빈 모듈 플러그인 → tui/dist/tui.js
```

`web/` 에서 가져오는 것은 타입(`types.ts`)과 순수 함수(`lib.ts` — 트리·정렬·지금 표 행·`slicesFor`)뿐이다.
`lib.ts` 의 브라우저 접근(`copyRef` 의 document)은 이미 주입식이라 TUI 에서 부르지 않으면 닿지 않는다.

## 화면

- **한 열(100열 미만)** — 머리(보드 이름 · 연결 상태 · 진행중 수) → 지금 표 → 섹션별 트리. Enter 로 상세에
  들어가고 Esc 로 나온다.
- **두 열(100열 이상)** — 왼쪽 목록, 오른쪽 상세. 상세는 커서를 따라간다.
- **상세** — 제목 · 상태 · 메타(마감·우선순위·라벨·링크) · 본문(마크다운 원문) · 타임라인(댓글·히스토리). 읽기만.
- **키** — `j`/`k`·↑↓ 이동, Enter 상세, Esc 뒤로, `s` 착수 · `x` 멈춤 · `d` 완료(`POST /api/todos/:ref/status` 의
  `{ action }` — 웹과 같은 요청), `a` 빠른 추가(제목 한 줄), `[` `]` 보드 전환, `r` 새로고침, `q` 종료. 마우스는 받지 않는다.
- 시간 표시·색·상태 어휘는 `web/DESIGN.md` 를 따른다(색은 상태에만, 에이전트/사람은 글자로).

## 데이터 흐름

1. 시작 — `GET /api/boards` · `/api/todos` · `/api/sections` 를 받는다.
2. 갱신 — `/api/events` 를 fetch 스트림으로 연다. 이벤트를 150ms 창으로 묶어 `slicesFor` 가 고른 묶음 중
   이 화면이 쓰는 것(`boards`·`todos`·`sections`)만 다시 받는다. 웹 `main.tsx` 와 같은 규칙.
3. 쓰기 — 상태 변경·추가는 REST 로 보내고 화면은 SSE 갱신을 기다린다(낙관적 갱신 없음 — 첫 조각에서는 단순하게).
4. 모든 요청에 `x-rocky-actor`(CLI 와 같은 actor)와 `x-rocky-client: tui` 를 붙인다.

## 실패 처리

- 요청 실패 — 아래 상태 줄에 메서드 · 경로 · 상태 코드를 띄우고 앱은 죽지 않는다.
- SSE 끊김 — 머리에 "끊김" 을 띄우고 1s → 2s → … 최대 30s 간격으로 다시 연결, 붙으면 전체를 다시 받는다.
- 데몬이 꺼져 있음 — `rocky tui`(Rust)가 exec 전에 기존 CLI 경로로 데몬을 확인·기동한다. TUI 안에서는 끊김으로 보인다.
- 종료 — q · Ctrl+C 모두 언마운트로 끝나고 대체 화면에서 빠져나온다.

## 실행·배포

- **`rocky tui`** (Rust CLI 서브커맨드) — 설정에서 포트, 플래그·설정에서 actor 를 정하고(`build_cli_context`),
  사용 로그에 `rocky tui` 를 남긴 다음(exec 하면 프로세스가 바뀌어 뒤에서는 못 남긴다) 실행 파일 옆 `tui.js` 를
  `bun --no-install tui.js --port N --actor A` 로 exec 한다. `bun` 이 PATH 에 없거나 `tui.js` 가 없으면 무엇이 없는지 한 줄로 알리고 1 로 끝난다.
- **릴리스** — `release.yml` 이 `bun run build:tui` 로 만든 `tui.js` 를 tarball 의 바이너리 옆에 넣는다. 부트스트랩의
  필수 검사는 `rocky`·`rockyd` 그대로(없으면 `rocky tui` 만 실패).
- **개발** — `bun run tui` 가 `tui/main.tsx` 를 바로 띄운다(번들 없이, 개발 데몬 포트를 인자로).
- **`dist/` 를 쓰지 않는다** — 데몬이 `/` 로 서빙하는 폴더라 터널로 밖에 보인다.

## 사용 집계

- REST 는 `x-rocky-client: tui` 로 클라이언트 `tui` 가 된다(`rocky_core::usage::client_of` 가 헤더를 먼저 본다 — 서버 수정 없음).
- `KNOWN_SURFACES` 에 `(Cli, "rocky tui")` 를 되살려 안 쓰면 안 쓴 표면으로 보이게 한다.

## 검증

- 순수 로직(SSE 파서 · 폭 판정 · 키 → 동작 매핑)은 `bun test`, 화면은 `ink-testing-library` 로 프레임 문자열을 본다.
  `package.json` 에 `test:tui` 를 더하고 `test` 에 잇는다(웹의 happy-dom preload 를 타지 않게 따로).
- `tsconfig.json` 의 `include` 에 `tui/**` 를 더해 `typecheck` 가 본다.
- 수동 — cmux 패널에서 좁은/넓은 폭 전환, 다른 세션이 보드를 바꿀 때 반영, 착수/완료 키.

## PR 순서 (스택)

처음엔 셋으로 잡았는데 첫 층이 1,100줄을 넘어 데이터·줄 목록·화면으로 나눴다.

1. 이 스펙
2. 데이터 — REST·SSE(`tui/api.ts`)와 상태(`tui/store.ts`), 의존성·`test:tui`
3. 줄 목록 — "지금" + 섹션 트리, 커서·글리프(`tui/rows.ts`)
4. 화면·진입 — 한 열 목록, 대체 화면, `tui/main.tsx`, 번들(`scripts/build-tui.ts`)·`build:tui`
5. 상세 + 상태 변경 키 + 두 열 + 빠른 추가
6. `rocky tui` 진입(Rust) + 릴리스 tarball·`rocky update --main` + 사용 집계 + 문서(`AGENTS.md`·`README.md`·
   `docs/features/tui.md`) + changeset

## 이 조각 밖

피드 · 노트(CRDT 편집은 계속 웹) · 워크로그 · rc · github · 에이전트 화면, 핸드오프/새 세션 띄우기, 댓글 쓰기,
마우스, 테마 전환.
