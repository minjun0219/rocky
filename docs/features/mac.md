# 맥 앱 — 메뉴 막대, 할 일 창, 에이전트 아바타

> rocky 를 **고치는** 에이전트용 개발 문서. 쓰는 법은 `README.md` "맥 앱". 2026-10-27 오너 요청으로 만들었다 — 웹 UI 는
> 그대로 두고, 네이티브여야 나은 자리(메뉴 막대에 늘 보이는 상태, 창 하나로 여는 보드)부터 작게 시작한다.

## 규칙

- **데몬의 클라이언트다.** 상주 서버가 아니고 저장하지 않는다. REST 로 읽고 쓰고 `/api/events` SSE 로 갱신을 받는다. 모든 요청에
  `x-rocky-actor` 와 `x-rocky-client: mac` 을 붙인다(사용 로그의 클라이언트가 `mac` 이 된다 — 서버는 헤더를 먼저 본다).
  데몬·`rocky_core` 는 고치지 않는다 — 앱이 필요로 하는 응답이 없으면 그건 데몬 쪽 별도 변경이다.
- **30일 동안 `mac` 클라이언트 사용이 0이면 걷는다**(`rocky usage --since 30d` 의 클라이언트 줄). ratatui TUI 가 90일 0회로 걷힌 전례와 같은 기준이다.
- **언어 경계의 가장자리다.** Swift 는 `mac/` 안에만 있다. 판정 로직을 Swift 로 새로 만들지 않는다 — 데몬이 낸 값(`ready`·`doingState`)을
  그대로 쓰고, 화면용 규칙은 웹·TUI 의 사본이다(`slices(for:)` = `web/lib.ts` 의 `slicesFor`, `boardGroups` = `tui/rows.ts` 의 보드 트리).
  한쪽 규칙을 바꾸면 사본도 같이 고친다.
- **쓰기는 보내고 SSE 를 기다린다.** 상태 변경(`POST /api/todos/:ref/status` 의 `{ action }`)·추가(`POST /api/todos`)는 웹·TUI 와 같은 요청이고
  낙관적 갱신이 없다. SSE 프레임은 150ms 로 모아 건드린 묶음만 다시 받는다.
- **SSE 는 바이트로 줄을 자른다.** `URLSession.AsyncBytes.lines` 는 빈 줄(프레임 끝)을 넘겨주지 않는다. 끊기면 1s→2s→…30s 로 다시 붙고,
  붙을 때마다(첫 연결도) 전부 다시 받는다. 간격은 10초 넘게 붙어 있다 끊긴 것만 처음으로 되돌린다(TUI 와 같다). 요청의 무응답 제한은
  하루로 둔다 — 데몬은 바뀐 것이 있을 때만 보내서 기본 60초면 조용한 동안 계속 끊긴다.
- **PR 알림은 띄우지 않는다.** 머지 후보·충돌 배너는 데몬이 이미 띄운다(`rockyd::prwatch::osascript_notifier`). 앱은 메뉴에
  열린 PR 중 `ready` 이고 초안이 아닌 것을 목록으로만 보인다. 머지는 사람 몫이다.
- **에이전트 상태는 `GET /api/sessions` 를 묻는다** — 세션 목록은 SSE 로 오지 않는다. 아바타가 보이면 15초, 끄면 60초(웹 에이전트 탭의
  `POLL_MS`·`IDLE_POLL_MS` 와 같다), 메뉴를 열 때·아바타를 켤 때 한 번 더. 데몬이 `claude agents --json` 을 3초 캐시하므로 더 자주 묻지 않는다.
  묶음(`agentPhase`)과 둘째 줄(`needs`·`detail`)은 웹 `web/agents.ts` 의 사본이다. 메시지·멈추기는 웹 에이전트 탭 몫이다(누르면 `/?view=agents`).
- **아바타는 보이기만 한다.** 화면 구석의 떠 있는 패널(포커스를 가져가지 않고 모든 Space 에 뜬다)이 가장 급한 상태 하나를 보인다 —
  내 차례(background `blocked`)면 깡충거리며 말풍선에 `needs`, 도는 세션이 있으면 팔로 두드리고, 다 쉬면 웅크려 숨만 쉰다. 배너·소리는 내지 않는다.
  그림은 Gemini 가 그린 돌 골렘 조각(`Resources/rocky/*.png`, 원본의 절반 크기)을 관절로 이은 컷아웃 리그다 — 조각을 바꾸면
  `RigPart` 의 pivot·at 좌표(PNG 픽셀)도 같이 바꾼다. .app 은 `bundle.sh` 가 복사한 `Contents/Resources/rocky/` 에서, `swift run` 은
  SwiftPM 리소스 번들에서 읽는다(`Bundle.module` 은 .app 에서 번들을 못 찾으면 멈추므로 쓰지 않는다). 그림을 못 읽으면 회색 돌 하나를 그린다.
  메뉴의 토글이나 아바타의 오른쪽 클릭으로 끈다(`defaults` 의 `avatar`). 자리는 패널이 기억한다(`RockyAvatar`).
- **보드를 바꾼 직후엔 트리 대신 "읽는 중"** — `sectionsBoard` 가 지금 보드와 다르면 가린다(섹션 없이 그리면 섹션에 든 할 일이 다 빠진다).
  받는 사이 보드를 또 바꿨으면 늦게 온 섹션을 버린다(TUI 와 같다).
- **실패는 출처로 지운다** — 읽기 실패는 다음 읽기 성공이, 동작 실패는 다음 동작이 지운다.
- **설정은 `defaults` > 환경 변수 > 기본값.** 메뉴 막대 앱은 Finder·로그인 항목으로 떠서 셸 환경을 받지 않는다:
  `defaults write dev.minjun.rocky.mac daemonURL http://127.0.0.1:8636`, `defaults write dev.minjun.rocky.mac actor <이름>`. 환경 변수
  (`ROCKY_TODO_PORT`, `ROCKY_TODO_ACTOR`)는 터미널에서 `swift run` 할 때용. 기본 actor 는 웹·TUI 와 같은 `logan`.
- **서명은 오너의 인증서로.** `mac/scripts/bundle.sh` 는 `ROCKY_MAC_SIGN_IDENTITY`(키체인의 인증서 이름 — 자체 서명 인증서면 그 Common Name)로
  `codesign` 하고, 없으면 ad-hoc(`-`)으로 서명한다. 오너의 맥은 자체 서명 인증서 `rocky` 를 쓴다(2026-10-27):
  `ROCKY_MAC_SIGN_IDENTITY=rocky mac/scripts/bundle.sh`. 서명 안 된·ad-hoc 빌드는 해시가 빌드마다 바뀌어 회사 맥 EDR 이 의심할 수 있다 — 그 맥에는
  인증서로 서명한 번들만 올린다. 앱 버전은 `package.json` 을 따른다(따로 들지 않는다 — 버전 넷을 맞추는 규칙에 다섯째를 더하지 않는다).
- **배포는 아직 손으로.** 릴리스 tarball 에 넣지 않는다 — `bundle.sh` 로 만든 `mac/build/Rocky.app` 을 `/Applications` 에 옮긴다.

## 코드

| 무엇 | 어디 |
| --- | --- |
| 응답 모양(데몬 JSON 의 사본) | `mac/Sources/RockyKit/Models.swift` |
| REST(헤더·에러 문구) | `mac/Sources/RockyKit/DaemonClient.swift` |
| SSE 파서·묶음 고르기·다시 붙기 간격 | `mac/Sources/RockyKit/Events.swift` |
| 보드 트리·메뉴 목록 | `mac/Sources/RockyKit/BoardTree.swift` |
| 세션 묶음·아바타 기분 | `mac/Sources/RockyKit/Sessions.swift` |
| 세션 묻기·아바타 패널 | `mac/Sources/RockyMac/AgentStore.swift`, `mac/Sources/RockyMac/Avatar.swift` |
| 설정 풀기 | `mac/Sources/RockyKit/Settings.swift` |
| 상태·SSE 루프 | `mac/Sources/RockyMac/BoardStore.swift` |
| 화면 | `mac/Sources/RockyMac/`(`RockyMacApp` · `MenuContent` · `BoardWindow`) |
| 아바타 조각 그림 | `mac/Sources/RockyMac/Resources/rocky/` |
| 번들·서명 | `mac/scripts/bundle.sh`, `mac/Info.plist` |

테스트: `mac/Tests/RockyKitTests`(`swift test --package-path mac`) — RockyKit 만 본다. CI 잡 `mac app (swift build + test)` 가 macOS 러너에서
테스트와 `bundle.sh`(ad-hoc 서명)를 돈다.

## 함정

- **Linux 컨테이너에는 Swift 가 없다**(클라우드 세션의 네트워크 정책이 swift.org 를 막는다). 빌드 확인은 CI 의 macOS 러너나 오너의 맥에서 한다.
- **화면(SwiftUI)은 테스트가 못 본다** — 메뉴 막대·창을 바꾸면 맥에서 한 번 띄워 본다(`swift run --package-path mac RockyMac`).
  기본 포트(8636)는 실제 보드다 — 착수·완료·추가 버튼은 실제 보드를 바꾼다.
- `LSUIElement` 앱이라 평소엔 Dock 아이콘이 없다. 할 일 창이 떠 있는 동안만 `setActivationPolicy(.regular)` 로 Dock·⌘Tab 에 보이고, 닫으면 `.accessory` 로 돌아간다.
  창을 열 때 `NSApp.activate` 를 먼저 불러야 창이 앞으로 온다. 앱 아이콘은 `mac/AppIcon.icns`(아바타 리그를 그린 것)를 `bundle.sh` 가 넣는다.
- `rocky update --main` 은 맥 앱을 빌드하지 않는다 — 맥 앱은 `mac/scripts/bundle.sh` 로 따로 만든다.
