# TUI — 보드를 터미널에

> rocky 를 **고치는** 에이전트용 개발 문서. 왜 다시 만들었는지와 결정 표는
> [`2026-10-11-tui-ink-design.md`](../design/specs/2026-10-11-tui-ink-design.md).

## 규칙

- **cmux 안의 웹 패널 자리만 대신한다.** 웹 UI 는 폰·원격(터널 + Access)용으로 그대로다. 첫 조각은 할 일 화면 하나 —
  피드·노트·워크로그·rc·github·에이전트는 웹에 있다. **30일 동안 `tui` 클라이언트 사용이 0이면 걷는다**(`rocky usage --since 30d`) —
  ratatui 판(`rocky-tui`)이 90일 0회로 걷힌 전례가 있다.
- **데몬의 클라이언트다.** 상주하지 않고 저장하지 않는다. REST 로 읽고 쓰며, 모든 요청에 `x-rocky-actor` 와 `x-rocky-client: tui` 를
  붙인다(사용 로그의 클라이언트가 `tui` 가 된다 — 서버는 헤더를 먼저 본다).
- **쓰기는 보내고 SSE 를 기다린다.** 상태 변경(`POST /api/todos/:ref/status` 의 `{ action }`)·추가(`POST /api/todos`)는 웹과 같은
  요청이고 낙관적 갱신이 없다. SSE 이벤트는 150ms 로 모아 `web/lib.ts` 의 `slicesFor` 가 고른 묶음만 다시 받는다(웹 `main.tsx` 와 같은 규칙).
- **SSE 는 fetch 스트림으로.** Bun 에는 전역 `EventSource` 가 없다. 끊기면 1s→2s→…30s 로 다시 붙고, 붙을 때마다(첫 연결도) 전부 다시
  받는다 — 끊긴 동안의 변경은 오지 않고, 시작 때 읽기가 실패했거나(데몬이 늦게 뜸) 그 읽기와 구독 사이에 바뀐 것이 있을 수 있다. 간격은
  10초 넘게 붙어 있다 끊긴 것만 처음으로 되돌린다 — 200 을 주고 바로 닫는 서버에 1초마다 붙으며 전부 다시 읽지 않게.
- **웹 코드는 순수한 것만 가져다 쓴다** — `web/types.ts`·`web/lib.ts`(`nowRows`·`slicesFor`·`formatAge`·`mergeTimeline` …). `web/store.ts` 는
  `window.location`·`history`·`localStorage` 에 묶여 있어 쓰지 않고, 상태는 `tui/store.ts`(zustand vanilla)가 따로 든다.
- **키 처리는 스토어의 지금 값을 읽는다**(`store.getState()`). 렌더 시점의 값을 쓰면 키가 다시 그리기보다 빨리 올 때(빠른 타이핑·붙여넣기·j 연타)
  앞 키의 결과가 안 보여 글자가 빠지고 커서가 덜 움직인다.
- **커서는 줄 키로 든다**(`Line.key`) — 위치로 들면 SSE 갱신으로 줄이 늘고 줄 때 다른 항목으로 미끄러진다. 그 줄이 사라지면(`d` 로
  완료해 트리에서 빠짐) 맨 위가 아니라 마지막 위치의 다음 줄로 간다(`rows.ts` 의 `cursorAt`) — 맨 위로 튀면 다음 `d` 가 보지도 않은 할 일에 닿는다.
- **좁은 상세에서 `s`·`x`·`d` 는 띄운 할 일에 닿는다**(`detailId`) — 커서가 아니라. 갱신으로 커서가 옮겨도 화면에 보이는 것이 대상이다.
- **보드를 바꾼 직후엔 트리 대신 "읽는 중"** — 섹션이 어느 보드 것인지(`sectionsBoard`)를 들고, 새 섹션이 오기 전엔 가린다(섹션 없이 그리면
  섹션에 든 할 일이 다 빠져 "열린 할 일이 없어요" 로 보인다). 읽는 사이 보드를 바꾸면 그 선택을 두고 옛 섹션은 버린다.
- **실패는 출처로 지운다**(`errorFrom`) — 읽기 실패는 다음 읽기 성공이, 동작 실패(상태 변경·추가)는 다음 키가 지운다. 관계없는 SSE 갱신이
  동작 실패를 바로 지우면 왜 안 됐는지 못 본다.
- **TUI 에는 읽음 표시가 없다** — "지금" 을 만들 때 댓글은 전부 읽은 것으로 넘긴다(아니면 모든 댓글이 "새 댓글" 행이 된다). 그리고 다
  펼친다(`expanded`) — 펼칠 동작이 없어 "N개 더" 로 접으면 그 안쪽 행에 닿을 길이 없다.
- **화면 폭 100열이 경계다**(`WIDE_COLUMNS`). 넘으면 목록 옆에 커서를 따라가는 상세(80ms 멈춘 뒤 받고 늦은 응답은 버린다), 아래면 Enter 로
  상세를 목록 자리에 띄운다.
- **actor** — `--actor` > `ROCKY_TODO_ACTOR` > 웹과 같은 기본값 `logan`. CLI 의 기본 actor(`agent`)는 사람이 쓰는 화면에 맞지 않아
  `rocky tui` 는 `--actor` 를 준 때만 넘긴다.
- **실행은 늘 `bun --no-install`, 번들은 `react-devtools-core` 를 빈 모듈로**(`scripts/build-tui.ts`). Ink 는 `DEV=true` 일 때만 그
  패키지를 쓰는데 번들에는 정적 import 로 남는다 — 그대로 두면 `node_modules` 가 있는 폴더에서는 시작하자마자 죽고, 없는 폴더에서는 Bun 이
  그 패키지를 네트워크로 몰래 받는다(2026-10-11 실측).
- **`rocky tui` 는 exec 한다** — 이 프로세스가 bun 으로 바뀌므로 사용 로그(`rocky tui`)는 exec 전에 남기고, exec 자체가 실패하면 그 자리에서
  알리고 끝낸다(호출자가 실패로 한 번 더 남기지 않게). `tui.js` 는 실행 파일(링크면
  실제 파일) 옆에서 찾는다 — 릴리스 tarball 과 `rocky update --main` 이 함께 깐다. `update --main` 은 빌드 전에 지난 번들을 지운다(`reset --hard` 가
  gitignore 된 `tui/dist/` 를 남겨, TUI 가 없던 커밋을 고정해 빌드하면 다른 버전의 번들이 깔린다).

## 코드

| 무엇 | 어디 |
| --- | --- |
| REST·SSE(파서·다시 붙기) | `tui/api.ts` |
| 상태(보드·데이터·커서·상세·빠른 추가) | `tui/store.ts` |
| 줄 목록(지금 + 섹션 트리)·커서·글리프 | `tui/rows.ts` |
| 화면·키 | `tui/App.tsx`, 상세는 `tui/Detail.tsx`, 진입은 `tui/main.tsx` |
| 번들 | `scripts/build-tui.ts` → `tui/dist/tui.js` |
| `rocky tui` 진입(Rust) | `crates/rocky-cli/src/tui_cmd.rs` |
| 배포 | `.github/workflows/release.yml`(tarball 에 `tui.js`), `crates/rocky-cli/src/update_main.rs`(main 빌드) |

테스트: `tui/*.test.ts(x)`(`bun run test:tui` — 화면은 폭을 받는 가짜 터미널로, ink-testing-library 는 100열 고정이라 한 열/두 열을 다
못 본다), `crates/rocky-cli/tests/it/tui_cmd_test.rs`(bun 인자).

## 함정

- **cmux 실기기**에서의 키 입력·폭 변경·패널 전환은 테스트가 못 본다 — 화면을 바꾸면 cmux 패널에서 한 번 띄워 본다.
- 개발 데몬에 붙이려면 `bun run tui -- --port 8699 --board <key>`. 기본 포트(8636)는 실제 보드다 — 상태 변경 키(s·x·d·a)는 실제 보드를 바꾼다.
