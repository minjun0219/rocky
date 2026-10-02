---
version: alpha
name: rocky-board
description: >-
  사람 한 명과 코딩 에이전트 여럿이 같이 쓰는 보드의 웹 UI. cmux 옆에 좁은 세로 패널로 띄워 두고
  곁눈으로 "지금 내가 손댈 게 있나" 를 읽는다. 색은 상태에만 쓴다.
colors:
  primary: "#c2410c"
  bg: "#eef1ef"
  surface: "#ffffff"
  surface-2: "#e6ebe8"
  line: "#d5dcd7"
  ink: "#15201a"
  muted: "#4e5d55"
  faint: "#586860"
  run: "#167a56"
  mine: "#c2410c"
  mine-soft: "#fff1ea"
  dead: "#7a2e12"
  link: "#1d5fb0"
  bg-dark: "#14181a"
  surface-dark: "#1c2124"
  surface-2-dark: "#242a2e"
  line-dark: "#2f373b"
  ink-dark: "#e6ebe8"
  muted-dark: "#9fada6"
  faint-dark: "#8a978f"
  run-dark: "#3fbf8a"
  mine-dark: "#f0874f"
  mine-soft-dark: "#3a2418"
  dead-dark: "#d97b5c"
  link-dark: "#7fb0f0"
typography:
  title:
    fontFamily: '"Pretendard Variable", Pretendard, -apple-system, BlinkMacSystemFont, "Apple SD Gothic Neo", sans-serif'
    fontSize: 18px
    fontWeight: 600
    lineHeight: 1.3
  body:
    fontFamily: '"Pretendard Variable", Pretendard, -apple-system, BlinkMacSystemFont, "Apple SD Gothic Neo", sans-serif'
    fontSize: 15px
    fontWeight: 400
    lineHeight: 1.5
  row:
    fontFamily: '"Pretendard Variable", Pretendard, -apple-system, BlinkMacSystemFont, "Apple SD Gothic Neo", sans-serif'
    fontSize: 14px
    fontWeight: 400
    lineHeight: 1.45
  meta:
    fontFamily: '"Pretendard Variable", Pretendard, -apple-system, BlinkMacSystemFont, "Apple SD Gothic Neo", sans-serif'
    fontSize: 13px
    fontWeight: 400
    lineHeight: 1.45
  label:
    fontFamily: ui-monospace, "SF Mono", SFMono-Regular, Menlo, monospace
    fontSize: 12px
    fontWeight: 500
    lineHeight: 1.35
    fontFeature: '"tnum"'
rounded:
  sm: 4px
  md: 8px
  lg: 10px
spacing:
  xs: 4px
  sm: 8px
  md: 12px
  lg: 16px
  gutter: 16px
  row-y: 8px
components:
  page:
    backgroundColor: "{colors.bg}"
    textColor: "{colors.ink}"
    typography: "{typography.body}"
  page-dark:
    backgroundColor: "{colors.bg-dark}"
    textColor: "{colors.ink-dark}"
    typography: "{typography.body}"
  row:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.ink}"
    typography: "{typography.row}"
    padding: 8px 12px
  row-dark:
    backgroundColor: "{colors.surface-dark}"
    textColor: "{colors.ink-dark}"
    typography: "{typography.row}"
    padding: 8px 12px
  row-meta:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.muted}"
    typography: "{typography.meta}"
  row-meta-dark:
    backgroundColor: "{colors.surface-dark}"
    textColor: "{colors.muted-dark}"
    typography: "{typography.meta}"
  section-head:
    backgroundColor: "{colors.bg}"
    textColor: "{colors.faint}"
    typography: "{typography.label}"
  section-head-dark:
    backgroundColor: "{colors.bg-dark}"
    textColor: "{colors.faint-dark}"
    typography: "{typography.label}"
  rail:
    backgroundColor: "{colors.surface-2}"
    textColor: "{colors.ink}"
    typography: "{typography.meta}"
  rail-dark:
    backgroundColor: "{colors.surface-2-dark}"
    textColor: "{colors.ink-dark}"
    typography: "{typography.meta}"
  hairline:
    backgroundColor: "{colors.line}"
    textColor: "{colors.ink}"
  hairline-dark:
    backgroundColor: "{colors.line-dark}"
    textColor: "{colors.ink-dark}"
  state-run:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.run}"
    typography: "{typography.label}"
    rounded: "{rounded.sm}"
    padding: 1px 6px
  state-run-dark:
    backgroundColor: "{colors.surface-dark}"
    textColor: "{colors.run-dark}"
    typography: "{typography.label}"
    rounded: "{rounded.sm}"
    padding: 1px 6px
  state-mine:
    backgroundColor: "{colors.mine-soft}"
    textColor: "{colors.mine}"
    typography: "{typography.label}"
    rounded: "{rounded.sm}"
    padding: 1px 6px
  state-mine-dark:
    backgroundColor: "{colors.mine-soft-dark}"
    textColor: "{colors.mine-dark}"
    typography: "{typography.label}"
    rounded: "{rounded.sm}"
    padding: 1px 6px
  state-dead:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.dead}"
    typography: "{typography.label}"
    rounded: "{rounded.sm}"
    padding: 1px 6px
  state-dead-dark:
    backgroundColor: "{colors.surface-dark}"
    textColor: "{colors.dead-dark}"
    typography: "{typography.label}"
    rounded: "{rounded.sm}"
    padding: 1px 6px
  button-primary:
    backgroundColor: "{colors.primary}"
    textColor: "{colors.surface}"
    typography: "{typography.meta}"
    rounded: "{rounded.md}"
    padding: 6px 12px
  link:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.link}"
    typography: "{typography.row}"
  link-dark:
    backgroundColor: "{colors.surface-dark}"
    textColor: "{colors.link-dark}"
    typography: "{typography.row}"
---

# rocky 보드 — DESIGN.md

웹 UI(`web/`)를 고치는 사람과 에이전트가 먼저 읽는 규칙이다. 위 frontmatter 가 **정규 토큰**이고,
본문은 그 근거와 토큰으로 표현되지 않는 규칙(정보 우선순위·시간·상호작용)이다. 둘이 어긋나면
토큰이 이긴다. 형식은 Google 의 [DESIGN.md 스펙](https://github.com/google-labs-code/design.md)을
따르고, 검사는 `bunx @google/design.md lint web/DESIGN.md` 다.

> 옛 rocky-todo 의 Tauri 앱 `DESIGN.md`(AGENTS.md *Scope → Out*)와는 다른 문서다. 이것은 지금의
> 웹 UI 전용이다.

규칙 옆 `[근거]` 는 이 문서를 쓸 때 조사한 출처다(맨 아래 "근거" 절).

## Overview

**무엇을 위한 화면인가.** 보드 주인은 cmux 터미널에서 에이전트 여럿을 돌리면서, 옆에 세로로 좁게
띄운 이 화면을 곁눈으로 본다. 화면이 답할 질문은 하나다 — **"지금 내가 손대야 할 것이 있나"**.
그다음이 "무엇이 돌고 있나", 그다음이 "이 보드에 남은 일" 이다. 이 순서가 화면의 위→아래 순서다.

**어디에 뜨나.** cmux 의 브라우저 pane(WKWebView = Safari 엔진)이다. 권장 거처는 오른쪽 Dock 에
주소창 없이(`chrome: false`, cmux 0.64.23+) 붙여 두는 것이다. 폭은 **280~420px** 가 흔하고 기준은
**360px**, 하한 276px(Dock 최소), 페이지 줌 110% 면 약 250px 까지 줄어든다. 높이는 전체일 때
820~870px, 위아래로 나누면 380~420px 이다. [근거: cmux]

**성격.** 관제판(방향 A, 2026-09-28 오너 결정). 슬레이트 바탕에 잉크 그린블랙, **색은 상태에만**
쓴다. 에이전트/사람은 색이 아니라 글자로 구분한다. 조용한 화면이 기본이고, 움직임은 상태가 바뀐
순간에만 있다.

## Colors

토큰은 역할 이름이고, `-dark` 가 붙은 것이 다크 테마 값이다(스펙에 테마 개념이 없어 접미사로 가른다
— 그래야 린터가 두 테마의 대비를 모두 검사한다). CSS 에서는 `web/styles/tokens.css` 의 같은 이름
변수(`--bg`, `--run` …)가 이 값을 그대로 옮긴 것이고, Tailwind 유틸리티는 `bg-surface`,
`text-run` 처럼 **의미 이름으로만** 쓴다. 원색 팔레트(`text-green-600`)는 금지다.

| 역할 | 라이트 | 다크 | 쓰는 곳 |
| --- | --- | --- | --- |
| `bg` | `#eef1ef` | `#14181a` | 페이지 바탕. `body` 에 **반드시 명시**한다(cmux 가 빈 영역을 터미널 색으로 칠한다) |
| `surface` | `#ffffff` | `#1c2124` | 행·카드 바탕 |
| `surface-2` | `#e6ebe8` | `#242a2e` | 접힌 레일·선택된 행 — 한 단 밝기 차로 층을 만든다 |
| `line` | `#d5dcd7` | `#2f373b` | 1px hairline. 그림자 대신 |
| `ink` | `#15201a` | `#e6ebe8` | 제목·본문 |
| `muted` | `#4e5d55` | `#9fada6` | 메타(ref·시각·누가) |
| `faint` | `#586860` | `#8a978f` | 섹션 머리·비활성 |
| `run` | `#167a56` | `#3fbf8a` | **돌고 있음** — 세션이 일하는 중 |
| `mine` / `mine-soft` | `#c2410c` / `#fff1ea` | `#f0874f` / `#3a2418` | **내 차례** — 사람이 손댈 것. `primary` 와 같은 색이다 |
| `dead` | `#7a2e12` | `#d97b5c` | **세션 없음** — 점선 테두리와 함께 |
| `link` | `#1d5fb0` | `#7fb0f0` | 외부 링크(PR·이슈)만 |

색 규칙:

- **색 하나 = 상태 하나.** run·mine·dead 셋이 전부다. 우선순위·라벨·보드는 색을 쓰지 않는다
  (p1 은 글자 `p1` 로, 굵기로). [근거: glance 4·5]
- **색만으로 말하지 않는다.** 모든 상태 표시에 글자나 글리프가 같이 간다 — 색각이상(남성 최대 8%)과
  흑백 스크린샷에서도 읽혀야 한다. [근거: glance 4]
- **파랑은 "주의" 로 쓰지 않는다.** cmux 가 에이전트가 기다리는 pane 에 파란 링을 그린다. 같은 뜻의
  파랑이 보드에도 있으면 둘이 겹친다. `link` 파랑은 링크에만 쓴다. [근거: cmux]
- **같은 경고색을 반복하지 않는다.** 같은 상태가 여러 행이면 섹션 머리에 개수로 한 번("내 차례 4")
  말하고, 행에는 작은 글리프만 둔다. 주황 배지가 여섯 줄 늘어서면 아무것도 눈에 안 띈다.
  [근거: glance 3]
- **다크에서는 순검정을 쓰지 않는다.** 층은 밝기 차(`bg` < `surface` < `surface-2`)로 가른다.

## Typography

Pretendard Variable(가변 폰트, 로컬 woff2 번들)을 기본으로 쓰며, 시스템 글꼴 스택(-apple-system, Apple SD Gothic Neo 등)으로 폴백된다.
ref·시각·개수는 모노에 `tabular-nums` 로 — 갱신될 때 자릿수가 흔들리지 않게.

| 토큰 | 크기 | 굵기 | 쓰는 곳 |
| --- | --- | --- | --- |
| `title` | 18px | 600 | 보드 이름, 상세 제목 |
| `body` | 15px | 400 | 본문(설명·댓글·노트) |
| `row` | 14px | 400 | 목록 행 제목 |
| `meta` | 13px | 400 | 행의 둘째 줄(누가·시각·보드) |
| `label` | 12px mono | 500 | ref(`rocky-12`)·상태 글자·섹션 머리·개수 |

- **12px 미만은 없다.** 곁눈으로 읽는 화면이다. 지금 코드의 `micro`(11px)는 없앤다. [근거: glance 10]
- 굵기는 400·500·600 셋만. 미읽음은 배지보다 **굵기(600)** 로 말한다(Slack 방식). [근거: glance b]
- 대문자·자간을 넓힌 라벨은 영문 섹션 머리에만. 한글에는 자간을 주지 않는다.
- 제목은 **두 줄까지 줄바꿈**(`line-clamp: 2`, `word-break: keep-all`, `overflow-wrap: anywhere`)
  하고, 메타 줄은 한 줄 말줄임이다. 320px 에서 한 줄로 자르면 한국어 제목은 거의 다 잘린다.
- 대비: 본문·행은 WCAG 4.5:1 이상 + APCA Lc 75 이상, 12px 라벨은 Lc 60 이상, 상태 글리프·테두리는
  3:1 이상. 다크에서는 WCAG 비율이 대비를 부풀리므로 APCA 로 한 번 더 본다. [근거: glance 10·11]

## Layout

**한 열이 기본이다.** 폭에 따라 이렇게 바뀐다. 가로 스크롤은 어느 폭에서도 없다(표·코드 블록만 예외).

| 폭 | 이름 | 배치 |
| --- | --- | --- |
| < 360px | narrow | 한 열. 행의 메타는 둘째 줄, 보드 칩은 숨김 |
| 360–480px | **panel (기준)** | 한 열. 메타는 둘째 줄, 여러 보드가 섞인 목록에서만 보드 칩 |
| 480–720px | wide panel | 한 열. 메타가 제목 옆으로 올라온다 |
| ≥ 720px | desktop | 목록 + 상세 두 열(상세가 오른쪽 열로 열린다) |

**위에서 아래로 — 영역 순서.** (자세한 규칙은 "Information Priority")

1. **머리줄** — 한 줄, 40px 이하, 고정. 보드 스위처 · "내 차례 N" · 끊겼을 때만 연결 점 · `⋯` 메뉴
   - **머리줄과 보기 전환(피드·할 일·노트·작업로그·GitHub), 맨 아래 버전 줄은 스크롤해도 늘 보인다.** 좁은 창은 문서가 스크롤하니
     `sticky` 로 붙이고, 그 아래 붙는 것(빠른 추가·노트 툴바)은 `.below-head` 로 머리 높이(`--app-head-h`)만큼
     내려 붙는다. 넓은 창은 화면마다 안쪽이 스크롤한다(노트 포함).
2. **피드** — 첫 화면이자 맨 앞 탭(2026-10-02 오너 — 알림 탭을 피드로 바꿨다). 오너가 손댈 것만, 위가 급하다:
   - **PR** — 결정 필요·머지 후보는 바로, 30분 넘게 그대로인 충돌·CI 실패(그 전엔 세션이 푼다). 종류별 묶음. 보드를 고르면
     그 레포의 것만. × 숨김은 종류별 — 같은 PR 이 다른 상태가 되면 다시 뜬다. 판정은 `alertRows`(`web/lib.ts`)
   - **내 차례** — 전 보드. 넘김·멈춘 진행(세션 없음·멈춤)·읽지 않은 댓글·수집함(`nowRows`). 예전엔 할 일 화면 맨 위
     "지금" 표에 있었다. 비었으면 한 줄
   - 탭 옆 `mine` 색 숫자 = PR 알림 + 내 차례. 지난번에 본 탭을 기억해 열지 않는다 — 열면 늘 피드
3. **돌고 있음** — 머리 안, 보드 스위처와 탭 사이 — 탭과 상관없이 늘 보인다(2026-10-02 오너). 세션이 붙은 진행중(전 보드).
   비었으면 숨김. 머리라 스크롤해도 남는다 — 그만큼 머리가 길어진다
4. **이 보드** — 빠른 추가 + 열린 일. 완료는 맨 아래에 접힘
   - "전체" 를 고르면 보드별로 묶이고 묶음마다 머리(▾ 이름 · 항목 수)를 눌러 접는다(2026-10-02). 접힘은 이 브라우저에만
     남는다. 한 보드를 볼 때의 섹션은 접지 않는다 — 그 보드를 보는 중이라 다 보여야 한다.
5. **노트** — 별도 화면(머리줄의 "피드 | 할 일 | 노트 | 작업로그 | GitHub" 전환). 목록 아래에 두지 않는다
5½. **작업로그** — 별도 화면(2026-10-02). 고른 보드(그 `path` 의 레포)의 작업 기록 최신순, "전체" 면 전 레포(레포 이름을
   메타에). 턴 기록은 요청이 제목, 결과가 둘째 줄(세 줄까지, 누르면 펼침). 찾기(300ms 뒤 묻는다)·종류·새로고침·더 보기.
   로그 색인은 주기 작업이라 실시간이 아니다. 할 일 상세의 타임라인에는 그 할 일을 든 세션의 턴이 "작업" 줄로(접힘)
   끼어든다 — 댓글은 사람에게 짧게, 작업 흐름은 여기서
   맨 위에 접힌 "통계 · 최근 30일"(열 때만 묻는다): 회고(레포별·요일별 턴, 턴이 몰린 할 일)와 rocky 개선(느린 표면 —
   표본 5회 이상의 p95 순, 실패율 순, 안 쓴 표면). 반복해서 보는 질문만 둔다 — 더하는 건 쓰면서
6. **GitHub** — 별도 화면. 구독한 PR 의 상태판(**레포별** — 구독 없는 레포는 머리줄 한 줄, 오른쪽 "열린 PR" 을 펼치면 그때만
   그 레포의 열린 PR 을 묻고 "지켜보기"(세션 없는 구독)를 단다)과 수집함(이슈 등)을 여기로 모았다 — 할 일 화면에 섞이면 어지럽다
   (2026-09-30). 행마다
   숨기기(×, 늘 보임) — PR 은 상태가 바뀌면 다시 보이고, 숨김은 이 브라우저에만 남는다. ⋯ 메뉴에서 탭을 끌 수 있다. 맨 아래 "세션 전달" 칸(로컬에서 연
   화면만) — 어느 세션이 PR·수집함 알림을 받나, 최근 보낸 것, 세션별 보내지 않기·구독 해지

- 간격은 4px 단위다(`xs 4 · sm 8 · md 12 · lg 16`). 좌우 여백(`gutter`)은 16px — 좁은 패널에서
  26px 는 사치다.
- 행 높이는 최소 40px(두 줄이면 자연 높이), 세로 패딩 8px. 행 전체가 클릭 영역이다.
- **첫 380px 안에 1·2 영역이 들어가야 한다.** 위아래로 나눈 cmux pane 높이다.

## Elevation & Depth

그림자는 쓰지 않는다. 층은 **밝기 한 단**(`bg` → `surface` → `surface-2`)과 **1px hairline**
(`line`)으로만 만든다. 떠 있는 것은 셋뿐이다 — `⋯` 메뉴, 보드 스위처 시트, 되돌리기 토스트. 이것들도
그림자 대신 `line` 테두리 + `surface` 바탕이다. 모달 대화상자는 되돌릴 수 없는 일(이슈 생성·새 세션)
에만 쓴다.

## Shapes

| 토큰 | 값 | 쓰는 곳 |
| --- | --- | --- |
| `sm` | 4px | 상태 글자·칩·입력 |
| `md` | 8px | 버튼·행 hover 배경 |
| `lg` | 10px | 묶음 카드(내 차례·돌고 있음 영역) |

알약(pill) 모양은 쓰지 않는다 — 버튼과 배지가 전부 같은 모서리 규칙을 따라야 한 화면이 한 목소리를 낸다.

## Components

**행(row).** 한 행 = 한 일. 구조는 고정이다:

```
[상태 아이콘] 제목 (최대 두 줄)                     [주 액션 1개]
             rocky-12 · AGENT · 12분 · (보드 칩)
```

- 첫 줄 맨 앞은 **누구 차례인가**를 말하는 **lucide 아이콘** 하나다(14px, 움직이지 않음) — `CircleDot`
  돌고 있음(run) / `CircleAlert` 내 차례(mine) / `CircleOff` 세션 없음(dead) / `CircleHelp` 모름. PR 행은
  PR 모양으로 — `GitMerge` 확인·머지 / `TriangleAlert` 충돌 / `CircleX` CI 실패 / `GitPullRequest` 대기 /
  `GitPullRequestDraft` 초안. 글꼴 문자(●◆◌)를 쓰지 않는 이유: 글꼴마다 모양·두께가 달라 흔들린다.
  상태 이름 글자(`진행중`)는 둘째 줄이나 상세에.
  [근거: ux 1]
- 주 액션은 **늘 보이는 버튼 하나**(예: PR 열기·다시 보내기·댓글 보기). 나머지는 늘 보이는 `⋯` 뒤에.
  **hover 에서만 나타나는 컨트롤은 없다** — 좁은 패널·터치(Cloudflare Tunnel 로 폰에서도 연다)에는
  hover 가 없다. 드래그 핸들도 늘 보이거나, 없거나다. [근거: ux 9]
- 클릭 타깃은 24×24px 이상, 주 액션은 32px 이상. [근거: ux 10]

**상태 글자(state).** `state-run` / `state-mine` / `state-dead` — 12px 모노, 4px 모서리, 테두리 1px.
`dead` 만 점선. 한 행에 하나까지.

**섹션 머리(section-head).** `label` 12px, `faint`. 이름 + 개수("내 차례 4"). 접을 수 있는 섹션은
머리가 곧 토글이고, 접혀도 개수는 남는다.

**보드 스위처.** 머리줄의 버튼(현재 보드 이름 또는 "전체"). 누르면 목록이 **패널을 덮는 시트**로
열린다 — 맨 위 "전체", 보드마다 "내 차례 N · 진행 M", 최근 활동순. 가로 탭 줄은 쓰지 않는다(10개가
360px 에 들어가지 않는다). 마지막 보드는 localStorage 에 기억한다. [근거: ux 13, glance M3]

**상세.** 폭 < 720px 이면 **목록을 대체하는 push 화면**이다(오버레이 드로어는 좁은 폭에서 가릴 것밖에
없다). 위에 `← 뒤로 · rocky-12 · 상태`, 늘 보이는 액션 줄(시작/완료 · 넘기기 · 새 세션 · 링크),
본문 → 댓글 → 히스토리. 닫기는 `←`·Esc·브라우저 뒤로 셋 다 같은 동작이고, 돌아오면 **목록의 스크롤
위치와 선택 행이 그대로**다. URL 을 가진다(`/rocky/12`). ≥ 720px 에서는 오른쪽 열로 연다. [근거: ux 6·7]

**빠른 추가.** "이 보드" 맨 위에 늘 보이는 입력칸. Enter 로 추가하면 행이 즉시 생기고(낙관적), 입력칸은
비워진 채 포커스가 남는다(연달아 적기).

**되돌리기 토스트.** 서버에 **온전한 역연산이 있는 일**만 확인 창 없이 바로 하고, 아래에 "되돌리기"
토스트를 5초 띄운다 — 시작(↔ 멈춤), 보관(↔ 해제), 순서 이동. [근거: ux 11]

**완료는 여기 들지 않는다.** 완료하면 서버가 진행 귀속(`doing_by`·`doing_since`·`doing_session_id`)을
지우는데(`set_todo_status`), "다시 열기" 는 그것을 되살리지 못한다 — 토스트로 약속하면 되돌렸는데
세션 귀속이 사라진 반쪽짜리가 된다. 완료는 지금처럼 바로 하되 되돌리기를 약속하지 않고, 원자적인
스냅숏·복원 경로가 서버에 생기면 그때 이 목록에 넣는다.

## Do's and Don'ts

**Do**

- 첫 화면에 "내 차례" 를 맨 위에, 전 보드 기준으로 둔다.
- 상태는 위치 → 글리프 → 굵기 → 색 순으로 말한다.
- 같은 상태가 여럿이면 섹션 머리에 개수로 한 번.
- 오래된 시각은 뭉갠다("3시간", "8월 4일부터").
- 새 데이터가 와도 사용자가 보던 자리를 지킨다.
- 두 테마를 다 확인한다 — 1440 / 860 / 360px × 라이트/다크를 Playwright 로 찍는다.

**Don't**

- 초 단위로 흐르는 시계를 오래된 항목에 쓰지 않는다(`56일 16:34:25` 금지).
- 같은 경고 배지를 행마다 반복하지 않는다.
- 완료한 일을 열린 일보다 위에 두지 않는다.
- 설명 없는 컨트롤을 머리줄에 두지 않는다(`LINK ♪`, 이름 없는 `logan` 버튼).
- 가로 탭 줄·칸반 열·가로 스크롤 표를 좁은 패널에 두지 않는다.
- 데이터 갱신으로 페이지 스크롤을 움직이지 않는다(`scrollIntoView` 는 문서 전체를 끌고 간다).
- 계속 돌아가는 스피너·펄스를 쓰지 않는다.
- cmux 가 이미 하는 일(소리·OS 알림·대기 링·에이전트 로그)을 반복하지 않는다.

## Information Priority

화면에 무엇을 먼저 올리는가 — 이 문서에서 가장 중요한 절이다.

**1. 내 차례 (전 보드).** 사람이 손대야 끝나는 것만. 순서가 곧 우선순위다:

1. PR 충돌
2. PR 확인·머지 가능
3. 진행중인데 세션이 없음(`gone`) 또는 멈춤(`idle`)
4. 넘겼는데 아무도 안 집음 / 집고 착수 안 함
5. 읽지 않은 댓글 — **최근 3일 안에 달린 것만**, 최대 3행, 나머지는 "읽지 않은 댓글 N" 한 줄

- 최대 5행, 넘치면 "N 더" 로 접는다. 비었으면 "내 차례 없음" 한 줄.
- 행마다 치우기(`⋯ → 치우기`)가 있다. 치운 항목은 새 활동이 생기면 다시 올라온다. [근거: ux b②]
- 수동적인 정보(히스토리·다른 사람이 한 일)는 여기 오지 않는다. [근거: glance 2]

**2. 돌고 있음.** 세션이 붙어 일하는 진행중(`live`)만. 행 둘째 줄에 최근 히스토리 한 줄(멈춘 지점)을
싣는다. 비었으면 섹션째 숨긴다.

**2½. PR → GitHub 탭으로 옮김(2026-09-30).** 열린 PR 상태판은 이제 GitHub 탭에 있다(위 6). 순서는 그대로 —
손댈 순서(충돌 → 머지 가능 → CI 실패 → 결정 필요 → 대기 → 초안), 같은 상태 안에서는 최근 갱신 먼저, 둘째 줄은
`#번호 · CI·스레드 · 갱신`, 누르면 GitHub 새 탭. "내 차례" 에도 PR 행을 싣지 않는다.

**3. 이 보드.** 빠른 추가 → 열린 일(섹션별, 사람이 정한 순서) → **완료(맨 아래, 접힘, "완료 N")**.
완료는 하루 지나면 접힌 묶음에서도 빠지고 보관 보기에만 남는다(Things 의 Logbook 방식). [근거: glance b]

**넘칠 때 버리는 순서(한 행 안).** 보드 칩 → ref → 시각 → 누가 → 상태 글리프 → 제목. 제목과 글리프는
끝까지 남는다.

## State Vocabulary

| 상태 | 뜻 | 코드 근거 | 아이콘(lucide) | 색 | 글자 |
| --- | --- | --- | --- | --- | --- |
| 돌고 있음 | 세션이 이 일을 하는 중 | `resolve_doing_state` → `live` | `CircleDot` | `run` | 진행중 |
| 멈춤 | 세션은 살았는데 턴이 끝나고 완료가 없다 — **가장 흔한 실패** | `idle` | `CircleAlert` | `mine` | 멈춤 |
| 세션 없음 | 진행중인데 세션이 사라졌다 | `gone` | `CircleOff` | `dead` | 세션 없음 |
| 모름 | 세션 목록을 못 읽었다 | `unknown` | `CircleHelp` | 무채색 | — |
| 내 차례 | 사람이 손댈 것(위 우선순위 1·2·4·5) | — | `CircleAlert` (PR 은 `GitMerge`·`TriangleAlert`) | `mine` | 항목별 |
| 열림 / 완료 | 아직 아무도 / 끝남 | `status` | `Circle` / `CircleCheck` | 무채색 | — |

- **모름은 없음이 아니다** — `unknown` 에 경고색을 쓰지 않는다.
- 에이전트/사람은 색이 아니라 글자(`AGENT` / 호출자 이름)다.

## Time Display

| 경과 | 표기 | 예 |
| --- | --- | --- |
| < 1분 | 방금 | 방금 |
| < 1시간, **진행중일 때만** | 분:초가 흐른다 | 12:04 |
| < 1시간 | 분 | 12분 |
| < 24시간 | 시간 | 3시간 |
| < 30일 | 일 | 5일 |
| ≥ 30일 | 날짜 | 8월 4일부터 |

- 초가 흐르는 표기는 **1시간 미만의 진행중**에만. 나머지는 1분마다 갱신한다. [근거: glance 7·8]
- 단위는 한국어로 짧게(`분`·`시간`·`일`) — 영문 `5m` 은 번역·스크린리더 문제로 피한다. [근거: glance 9]
- 정확한 시각은 `<time datetime title>` 의 툴팁으로.
- 모노 + `tabular-nums`.

## Interaction

- **키보드가 1급이다.** `j`/`k` 이동 · Enter 열기 · Esc 뒤로 · `c` 빠른 추가 · `h` 넘기기 ·
  `s`/`d` 시작·완료 · `g b` 보드 전환 · ⌘K 팔레트(보드 이름·ref 로 이동). 포인터 동선도 전부 남긴다.
  [근거: ux 8]
- **낙관적 업데이트가 기본이다.** 0.1초 안에 화면에 반영하고, 실패하면 되돌리면서 그 행에 이유를
  적는다. 새 세션 띄우기처럼 몇 초 걸리는 일은 행에 "띄우는 중…" 을 둔다. [근거: ux 12]
- **확인 창은 되돌릴 수 없는 일에만**(이슈 생성·새 세션). 역연산이 온전한 일은 되돌리기 토스트,
  완료처럼 반쪽만 되돌아가는 일은 둘 다 없이 바로 한다(위 "되돌리기 토스트"). [근거: ux 11]
- **움직임은 상태가 바뀐 순간에만** — 1초 하이라이트 후 멈춘다. `prefers-reduced-motion` 이면 없음.
  [근거: ux 4]
- **보던 자리를 지킨다.** SSE refetch·1분 틱·다른 사람(에이전트 포함)의 편집은 스크롤·포커스·열린 상세·
  입력 중인 내용을 건드리지 않는다. 가로로 넘치는 컨테이너 안에서 무엇을 보이게 할 때는 그 컨테이너의
  `scrollLeft` 만 바꾼다 — `scrollIntoView` 는 문서 전체를 세로로 끌고 간다.
- **알리는 건 cmux·OS 의 몫이다.** 이 화면은 소리·배너를 내지 않고 개수·글리프만 바꾼다. [근거: ux 5]

## Notes

노트는 사람과 에이전트가 같이 쓰는 스크래치 패드다(CRDT — `web/notedoc.ts`).

- **목록 아래에 두지 않는다.** 좁은 패널에서 목록 아래는 스크롤 1,000px 너머라 눈이 가지 않는다.
  머리줄의 **"할 일 | 노트" 전환**으로 노트가 화면 전체를 쓴다.
- 노트 화면은 게시판이다: 목록 한 줄(번호 · 제목 · 첫 내용 줄 · 갱신 시각, 최근 것이 위). 줄을 누르면
  상세로 push — 편집기가 **화면 전체 높이**로 열리고 주소가 `/{board}/notes/{n}` 이 된다(뒤로 가면 목록).
  "+ 새 노트" 도 만든 노트의 상세로 바로 간다.
- **고정한 노트는 목록 위에 카드로 펼친다**(고정한 순서). 카드는 그 자리에서 편집하고, 접으면 머리줄 +
  첫 줄만 남는다(접힘은 브라우저에 기억). 카드 머리는 좁아서 번호 · 제목 · 📌 · 크게 열기만 — 히스토리·보관은
  상세에 있다.
- 본문은 쉴 때 렌더된 마크다운(제목·목록·체크박스·인용·코드·표), 누르면 그 자리에 편집기와 서식 툴바.
- **마크다운 렌더는 한 곳이다** — `web/components/Markdown.tsx`(할 일 설명 · 댓글 · 노트). 편집기와 같은 파서
  (`@lezer/markdown` GFM)로 읽고 React 요소로 그린다(`innerHTML` 없음, 링크는 http(s)·mailto 만, 이미지는
  불러오지 않고 링크로). 문단 안 줄바꿈은 줄을 나눈다.
- "할 일" 화면에서도 노트가 바뀐 걸 알 수 있게, 전환 버튼에 새 편집 표시(`노트 •`)를 둔다.
- 편집기 세션(노트 소켓의 구독)은 포커스할 때 열고 blur 20초 뒤 닫는다. 열린 노트는 연결 하나(`/api/ws`)를 같이 쓴다.
- 에이전트의 편집은 커서·스크롤을 움직이지 않는다(위 "보던 자리를 지킨다").
- 편집기는 CodeMirror 하나다(2026-09-30 오너 결정 — textarea 는 걷었다). 마크다운 기호는 흐리게 남기고
  내용에만 모양을 준다.
- 할 일 설명도 같은 편집기·같은 서식 툴바다(2026-10-02) — 고정 높이 textarea 는 긴 설명을 작은 스크롤 상자에
  가뒀다. 실시간 문서가 아니라 저장은 버튼·⌘Enter, 취소는 버튼·Esc. 편집기는 글 길이만큼 자라고(안쪽 스크롤
  없음) 드로어가 스크롤하며, 툴바는 편집기 끝까지만 따라온다.
- 드로어는 고정 머리·버전 줄·머리줄 메뉴보다 위 층이다(z 40). 좁은 화면에서 바닥에 뜬 닫기(×)만큼 드로어 끝에
  여백을 둔다 — 없으면 타임라인 마지막 줄이 버튼 밑에 깔린다.

## Empty, Stale, Offline

- 빈 섹션: "내 차례" 는 한 줄("내 차례 없음"), "돌고 있음" 은 숨김, 보드는 빠른 추가만.
- 데몬 연결이 끊기면 머리줄에 점 하나 + "연결 끊김 · 다시 붙는 중". 보이는 데이터는 흐리게 두되
  숨기지 않는다.
- 세션 목록을 못 읽으면(`available: false`) 상태는 `모름` 으로, 사유는 툴팁에.
- **다시 그려져도 바로 보인다.** cmux 는 5분 넘게 숨은 pane 의 페이지를 내렸다가 다시 로드한다. 상태는
  URL·서버·localStorage 에 두고, 첫 그림에 스켈레톤을 두지 않는다. [근거: cmux]

## Environment

- **대상 엔진은 WebKit(Safari)** 이다. Safari 에서 안 되는 CSS·JS 는 쓰지 않는다. DPR 1(외장)과 2(내장)
  모두에서 1px hairline 이 보여야 한다.
- `prefers-color-scheme` 를 따르고, 머리줄 `⋯` 에 수동 전환(자동/라이트/다크)을 둔다.
- 주소창 없는 Dock 에서는 새로고침·홈이 없다 — `⋯` 에 "새로고침" 과 "전체 보기" 를 둔다.
- cmux 연동(사이드바 상태·알림)은 웹 UI 가 아니라 CLI/데몬 쪽 일이다. cmux 소켓은 기본적으로 cmux 안에서
  뜬 프로세스만 받으므로(launchd 데몬은 거부), 켤지는 오너가 정한다 — 선택 사항이다.

## Not in the Panel

터미널·cmux·GitHub 에 원본이 있는 것은 여기 두지 않는다: 에이전트 출력·로그, 권한 승인, diff·코드 리뷰·
머지 버튼, 워크트리 정리, 설정 화면 전체, 칸반 열, 소리·OS 알림. 요약 한 줄과 그리로 가는 링크만 둔다.
[근거: ux d]

## Known Gaps (2026-09-29 기준 코드와의 차이)

이 문서가 먼저 정해졌고 코드는 따라가는 중이다. 고칠 때 이 표를 지운다.

| 지금 코드 | 규칙 | 자리 |
| --- | --- | --- |
| 상세가 오버레이 드로어 | < 720px 에서 push | `web/components/DetailDrawer.tsx` |
| 드래그 핸들이 hover 에서만 보인다 | hover 전용 컨트롤 없음 | `web/components/TodoItem.tsx` |
| 핸드오프가 링크 파랑(`--handoff`) | 파랑은 링크만 | `web/styles/tokens.css` |

## 근거

- **cmux** — manaflow-ai/cmux 소스(`Sources/Panels/BrowserPanel.swift`, `docs/dock.md`,
  `RightSidebarWidthSettings.swift`, `cmux.schema.json` `browser.*`), https://cmux.com/docs/dock ,
  https://cmux.com/docs/notifications
- **glance** — [Apple HIG Widgets](https://developer.apple.com/design/human-interface-guidelines/widgets/) ·
  [NN/g Indicators](https://www.nngroup.com/articles/indicators-validations-notifications/) ·
  [NN/g Preattentive](https://www.nngroup.com/articles/dashboards-preattentive/) ·
  [Matthews et al. 2006, Glanceable Peripheral Displays](https://www2.eecs.berkeley.edu/Pubs/TechRpts/2006/Archive/EECS-2006-113.pdf) ·
  [Primer RelativeTime](https://primer.style/product/components/relative-time/guidelines/) ·
  [APCA](https://git.apcacontrast.com/documentation/APCA_in_a_Nutshell.html) ·
  [WCAG 2.2 1.4.11](https://www.w3.org/WAI/WCAG22/Understanding/non-text-contrast.html) ·
  [M3 window size classes](https://m3.material.io/foundations/layout/applying-layout/window-size-classes) ·
  [Linear My issues](https://linear.app/docs/my-issues) ·
  [Things Logbook](https://culturedcode.com/things/support/articles/4001304/)
- **ux** — [Linear agent interaction](https://linear.app/developers/agent-interaction) ·
  [claude-code #96510](https://github.com/anthropics/claude-code/issues/96510) ·
  [Warp agent management](https://docs.warp.dev/agents/using-agents/managing-agents) ·
  [M3 list-detail](https://m3.material.io/foundations/layout/canonical-examples/list-detail) ·
  [Linear Inbox](https://linear.app/docs/inbox) ·
  [NN/g confirmation dialogs](https://www.nngroup.com/articles/confirmation-dialog/) ·
  [NN/g response times](https://www.nngroup.com/articles/response-times-3-important-limits/) ·
  [WCAG 2.2 2.5.8](https://www.w3.org/WAI/WCAG22/Understanding/target-size-minimum.html) ·
  [Calm technology](https://en.wikipedia.org/wiki/Calm_technology)
- **format** — [google-labs-code/design.md](https://github.com/google-labs-code/design.md),
  [VoltAgent/awesome-design-md](https://github.com/voltagent/awesome-design-md)(Raycast·Linear·opencode 의
  행·배지 규칙 참고)
