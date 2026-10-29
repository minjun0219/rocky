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
  묶음(`agentPhase`)과 둘째 줄(`needs`·`detail`)은 웹 `web/agents.ts` 의 사본이다. 메시지·멈추기는 웹 에이전트 탭 몫이다(아바타를 두 번 누르거나 오른쪽 클릭 메뉴로 `/?view=agents`). 한 번 누르는 건 아무것도 열지 않는다 — 끌어 옮기다 놓는 것과 헷갈려 매번 웹 UI 가 떴다.
- **아바타는 보이기만 한다.** 화면 구석의 떠 있는 패널(포커스를 가져가지 않고 모든 Space 에 뜬다)이 가장 급한 상태 하나를 보인다 —
  내 차례(background `blocked`)면 껑충 뛰고 주먹 인사를 하고(말풍선에 `needs`, 누르면 닫히고 글이 바뀌면 다시 뜬다), 도는 세션이 있으면 주먹을 들어 두드리고, 다 쉬면 엎드려 숨만 쉰다. 배너·소리는 내지 않는다.
  같은 동작만 되풀이하지 않게 몸짓을 세 층으로 쌓는다 — 바탕 루프(숨 · 기분 동작) 위에 작은 동작을 가중 랜덤으로 얹고(직전 것은 빼고), 드문 사건(튜브)은 `Roamer` 가 일으킨다.
  도는 중이면 망치를 3-7번(박자 0.7-0.95초를 매번 새로) 내리치고 두리번 · 땀 닦기 · 팔 돌리기 · 발 구르기 · 잠깐 앉기 중 하나를 하고, 창 끝에 서 있으면 내려다보기가 후보에 든다.
  내 차례는 처음 4.8초만 크게 뛰며 주먹 인사를 하고, 그 뒤엔 마우스 쪽을 보고 서 있다가 10-20초마다 손만 흔든다(기다리는 세션이 바뀌면 다시 크게).
  착지하면 0.2초 쭈그리고, 점프 전에는 0.3초 쪼그린다. 걷기 박자는 천천히 흔들리고 팔은 몸보다 한 박자 늦다.
  고르는 판정은 `RockyKit` 의 `Choreographer`(후보표 · 가중치 · 길이 범위) 순수 함수이고, 자세(`RockyPose`)는 고른 토막을 그리기만 한다. 몸짓은 돌아다니기를 꺼도 고른다.
  그림은 오너가 고른 시안(프로젝트 파일 `mac-avatar/concept-face.jpg`의 네 자세)을 윤곽대로 잘라 낸 조각 PNG 20개(`Resources/rocky/`)를 짜 맞춘 것이다(`RockyArt`) —
  몸통 · 다리 둘 · 팔(내린 손 · 편 손 · 머리에 댄 손) · 눈 · 눈썹 · 입 조각을 서 있는 자세의 좌표에 놓아 움직이고, 자기 · 갸웃 · 뛰는 중은 시안의 그 자세를 통째로 그린다.
  시안 픽셀을 그대로 쓰고 엄지만 지웠다(오너 결정 2026-10-29 — 도형으로 다시 그리니 질감과 자세가 시안과 달라졌다. 세 손가락이 컨셉이라 엄지척 대신 주먹 인사를 쓴다).
  다리를 셋 이상으로 늘리거나 몸 아래로 곧게 뻗는 것을 붙이지 않는다(오너 결정 2026-10-29 — 세로로 길쭉한 실루엣은 민망하게 읽힌다).
  조각을 자르는 트레이서(Python)와 앱과 같은 셈으로 그려 보는 하네스, 시안 비교는 레포 밖 프로젝트 파일 `mac-avatar/trace/`에 있다 — 조각을 다시 자르면
  `RockyArt` 의 좌표 표(`Sprite.origin` · 팔의 `pivot` · `theta` · 얼굴 조각의 `from`)를 그 출력(`rig.json`)에 맞춰 같이 고친다.
  .app 은 `bundle.sh` 가 복사한 `Contents/Resources/rocky/` 에서, `swift run` 은 SwiftPM 리소스 번들에서 읽는다(`Bundle.module` 은 .app 에서 번들을 못 찾으면 멈추므로 쓰지 않는다). 그림을 못 읽으면 회색 돌 하나를 그린다.
  메뉴의 토글이나 아바타의 오른쪽 클릭으로 끈다(`defaults` 의 `avatar`). 자리는 돌아다니기를 끈 동안만 패널이 기억한다(`RockyAvatar`).
- **돌아다니기**(`defaults` 의 `roam`, 기본 켜짐) — 일하는 중이면 창 윗면을 걷고, 발판 끝에서 돌아서거나 뛰어내리고, 앞을 막은 창 옆면이
  `jumpReach`(260pt) 안이면 쪼그렸다가 윗면으로 뛰어오르고 더 높으면 돌아선다. 화면 가장자리에서는 돌아선다(기어오르기는 2026-10-29 오너 결정으로 뺐다). 밟던 창이 사라지거나 옮겨 가면 떨어지고, 누르고 있는 동안은 걷지 않고 포인터에 매달려 있다가(누른 채 멈춰 있어도) 놓으면 그 자리에서 떨어진다. 내 차례·연결 없음이면 서 있고, 다 쉬면 앉는다.
  걷다가 가끔(초당 4%) 헤일메리의 터널처럼 반투명 튜브를 놓아 멀리 있는 다른 발판으로 건너간다 — 관은 로키 패널 바로 뒤의 클릭을 흘려보내는 패널에 그리고,
  로키는 관 안에서 0.45배로 줄어든다. 튜브는 한 번에 하나, 건너는 중엔 기분이 바뀌어도 마저 건너고, 끌어다 놓으면 관이 사라진다.
  판정(발판·벽·한 걸음)은 `RockyKit` 의 `Roamer` 순수 함수이고, 앱은 초당 30번 걸음을 재고 패널을 옮기기만 한다. 창 목록은
  `CGWindowListCopyWindowInfo` 로 0.5초마다 위치만 읽는다 — 창 이름을 읽지 않으니 화면 기록 권한을 묻지 않는다. 패널은 로키 칸(말풍선이 떠 있는 동안은 패널 전체) 위에 마우스가 있을 때만 클릭을 받고, 나머지는 아래 창으로 흘려보낸다(`ignoresMouseEvents`).
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
| 돌아다니기(발판·벽·걸음·튜브) | `mac/Sources/RockyKit/Roam.swift` |
| 몸짓 고르기(후보표·가중치·서 있는 자리) | `mac/Sources/RockyKit/Moves.swift` |
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
  창을 열 때 `NSApp.activate` 를 먼저 불러야 창이 앞으로 온다. 앱 아이콘은 `mac/AppIcon.icns`(아바타를 그린 것)를 `bundle.sh` 가 넣는다.
- `rocky update --main` 은 맥 앱을 빌드하지 않는다 — 맥 앱은 `mac/scripts/bundle.sh` 로 따로 만든다.
