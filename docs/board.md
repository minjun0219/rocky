# rocky 보드 — 공유 todo / 스크래치패드 데몬

로키(에이전트)와 호출자가 하나의 작업 보드를 공유하는 로컬 데몬. 시스템에 **단 하나**만
떠서 Claude Code / opencode / Codex 의 모든 세션·모든 프로젝트가 같은 데이터를 본다.
rocky 플러그인의 본체다 — 옛 별도 레포 `rocky-todo` 를 2026-09 에 흡수했다.

```
에이전트/CLI ───────►┌─ /api/*       REST                       ← CLI
(MCP or CLI)         │─ /api/events  SSE 변경 브로드캐스트
                     └─ /mcp         MCP streamable HTTP        ← Claude Code / opencode / Codex
        데몬 (rockyd, 127.0.0.1:8636) → SQLite ~/.config/rocky/todo/todo.db
```

- 계층(todo/subtask) + 섹션 + 보드(프로젝트) 단위, 우선순위(p1–p4)/라벨/마감일/링크 첨부.
- 처리중 표시: `start` 하면 `show`·statusline 에 actor + 경과가 붙는다. 그 작업을 든 세션이
  사라졌으면 "세션 없음", 살아 있는데 턴이 끝났고 완료 처리가 없으면 "멈춤" 으로 바뀐다
  (세션을 대조할 수 없을 때만 경과 30분 기준 "오래됨").
- 죽은 세션이 든 진행중은 저절로 풀린다: 에이전트가 `start` 만 하고 세션이 사라진 채 24시간이
  지나면 데몬이 10분 주기 스윕에서 `stop` 으로 돌리고 `rocky` 이름으로 댓글을 남긴다. 사람이
  든 것, 세션이 살아 있는 것(idle), 판정 불가(unknown)는 그대로 둔다.
- **삭제 없음** — 모든 엔티티는 아카이브만 된다. 모든 변경은 히스토리(누가/무엇을/언제)로 남는다.
- 스크래치패드 메모: 보드 소속 or 글로벌, 에이전트/호출자 모두 편집.
- **웹 UI** — rocky-todo 시절의 React UI 를 `web/` 로 되살렸다(2026-09-28). 데몬이 `http://127.0.0.1:8636/` 에 서빙한다. 아래 "웹 UI" 절.

## 설치 = 활성화

보드 데몬은 **별도 스위치가 없다** — rocky 플러그인 설치 자체가 활성화 경계다:

```bash
claude plugin marketplace add minjun0219/rocky
claude plugin install rocky@rocky-marketplace
```

런타임에 끄려면 `claude plugin disable rocky`.

설치되면 플러그인이 두 가지를 배선한다:
- **`mcpServers` (http)** — 데몬의 `/mcp` (streamable HTTP, 도구 5개)를 세션에 등록. 수동
  `claude mcp add` 불필요.
- **hooks** — `SessionStart` 훅이 데몬을 기동하고, `UserPromptSubmit` 훅이 보드의 사람 변경을
  주입하며, `Stop` 훅이 그 세션 앞으로 온 핸드오프 요청을 자동 착수시킨다(아래 "보드→세션
  핸드오프" 참고). 셋 다 플러그인 업데이트 후 **첫 세션부터** 적용된다.

훅과 CLI 는 전부 **네이티브 바이너리**(Rust)다 — bun 이 필요 없다. 플러그인 설치본에는
바이너리가 들어 있지 않고, 진입점 `bin/rocky`(셸 부트스트랩)가 플러그인 버전에 맞는
[GitHub Release](https://github.com/minjun0219/rocky/releases) tarball(`rocky` + `rockyd`)을
`~/.local/share/rocky/v<version>/` 에 **한 번** 받아
풀고(`SHA256SUMS` 검증) 이후로는 그대로 실행한다. 첫 SessionStart 가 그 다운로드를 맡는다 —
실패하면(오프라인 등) 그 세션은 보드 없이 지나가고 다음 세션이 다시 시도한다. 지원 플랫폼은
지금 Apple Silicon macOS 뿐이다.

| 환경변수 (부트스트랩) | 의미 |
|---|---|
| `ROCKY_BIN` | 설정되면 다운로드 없이 이 바이너리를 실행한다 — 레포에서 개발할 때 `target/debug/rocky` |
| `ROCKY_RELEASE_BASE` | tarball 을 받을 기준 URL (기본 `https://github.com/minjun0219/rocky/releases/download`) |
| `XDG_DATA_HOME` | 설치 위치의 상위 (기본 `~/.local/share`) |

> **첫 세션 순서 주의**: SessionStart 훅의 데몬 기동과 http MCP 초기화 순서는 보장되지 않는다.
> 첫 세션에서 MCP 가 `failed` 로 뜨면 `/mcp` 패널에서 retry 하거나 다음 세션이면 붙는다.
> 상시 상주(`daemon install`)면 이 창이 사라진다.

## 데몬 기동

설치 후엔 SessionStart 훅이 세션 시작 때 데몬을 자동 기동한다 (없으면 detached spawn, fail-open).
플러그인을 올린 뒤에는 **다음 세션 시작** 또는 **`/reload-plugins` 한 세션의 다음 프롬프트**에서
데몬이 새 버전으로 재기동된다 — 매 턴 훅이 도는 데몬이 자기보다 오래됐을 때만 올린다(더 새
데몬이나 없는 데몬은 건드리지 않는다). CLI 도 필요 시 온디맨드로 자동 기동한다. 로그인 시
상시 상주를 원하면:

```bash
rocky daemon install     # launchd 등록 (KeepAlive) — macOS
rocky daemon status      # 기동 여부 + launchd 상태 (plist 만 있고 로드가 안 됐으면 고치는 명령까지)
rocky daemon uninstall
rocky --version          # 설치된 CLI 버전
```

`rocky daemon start` 는 데몬을 띄운 뒤 그것이 launchd 상주인지, launchd 밖의 온디맨드
프로세스인지를 같이 적는다 — "✓ daemon on" 만 보고 상주가 복구된 줄 알지 않게.

> **플러그인 업그레이드 뒤 데몬이 사라졌다면**: 옛 job 을 내린 뒤 새 job 등록이 실패한
> 경우다(launchd 의 bootout 이 비동기라 바로 이어지는 bootstrap 이 튈 수 있다). 지금은
> 훅이 재시도하고, 그래도 안 되면 launchd 밖에서라도 데몬을 띄운 뒤 세션 컨텍스트에
> `⚠ rocky 데몬: …` 로 알린다. `rocky daemon status` 가 "plist 는 있으나 로드되지 않음"
> 이면 `rocky daemon install` 로 다시 등록한다.

> **이미 `daemon install` 을 해둔 환경**은 plist 가 자동 갱신되지 않는다 — GitHub 이슈
> 기능(`gh` PATH 인식)을 쓰려면 `rocky daemon uninstall && rocky daemon install`
> 로 한 번 다시 깐다.

**터미널에서 `rocky`**: SessionStart 가 `~/.local/bin/rocky` 를 `~/.local/share/rocky/current/rocky`
로 걸어 둔다(버전 링크를 거치므로 릴리스마다 따라온다). `~/.local/bin` 이 PATH 에 있어야 하는데
그건 셸 몫이다 — `rocky config show` 의 `cli` 행이 없으면 없다고 말하고, 세션을 안 열고 지금 걸려면
`~/.local/share/rocky/current/rocky config link`.

레포에서 직접 실행: `cargo run -p rockyd`. 설치본 포그라운드는 `rocky daemon run`.

> **TS 판에서 올라온 환경**: 이전 `daemon install` 의 plist 는 bun 으로 TS 데몬을 띄운다.
> 첫 SessionStart 훅이 버전 차이를 보고 plist 를 네이티브 바이너리로 다시 써서 재기동하므로
> 보통은 손댈 게 없다 — 안 되면 `rocky daemon install` 을 한 번 다시 실행한다.

> **PATH 회귀 수정 (재설치 필요)**: 이전 버전으로 `daemon install` 을 이미 해뒀다면
> `rocky daemon install` 을 다시 실행하라 — plist 에 설치 시점 PATH 를 굽는 수정이라,
> 재설치해야 launchd 데몬이 `claude` CLI(핸드오프 기능이 쓴다)를 PATH 에서 찾는다.

## 네이티브 바이너리를 직접 쓰기

CLI 는 따로 받을 필요가 없다 — 플러그인이 받아둔 바이너리를 쓴다. 경로는
**`~/.local/share/rocky/current/rocky`** 를 쓴다: `current` 는 SessionStart 훅(또는 새 버전을 처음 받은 호출)이 그
설치본의 버전(`v<version>`)으로 걸어 두는 링크라, 릴리스가 바뀌어도 이 경로는 그대로다.
PATH 에 두려면 `ln -s ~/.local/share/rocky/current/rocky ~/.local/bin/rocky`.
플러그인 밖에서 worklog MCP 를 붙일 때도 이 경로다 — 예컨대 `claude -p --strict-mcp-config`
배치 잡은 플러그인 MCP 가 끊기므로 `--mcp-config` 에
`{"mcpServers":{"worklog":{"command":"/Users/you/.local/share/rocky/current/rocky","args":["mcp","worklog"]}}}`
를 준다(Codex·opencode 는 [`codex.md`](./codex.md) · [`opencode.md`](./opencode.md)).
`rocky` 와 `rockyd` 는 **한 디렉터리에** 있어야 한다: CLI 는 옆의 `rockyd` 를 먼저 찾는다. 레포에서 개발할 땐 `cargo build --workspace` 뒤
`ROCKY_BIN=target/debug/rocky` 로 부트스트랩을 우회한다.

## 웹 UI — 브라우저에서 보드 (`http://127.0.0.1:8636/`)

`web/` 의 React 앱(zustand · Tailwind v4 · Radix Dialog)을 릴리스 때 `bun run build:ui` 로 `dist/` 에
번들해 tarball 에 넣고, 데몬이 **실행 파일 옆 `dist/index.html`** 을 찾아 `/` 에 서빙한다
(`ROCKY_TODO_UI_DIST` 로 다른 디렉터리를 줄 수 있다 — 레포에서 개발할 때 `$PWD/dist`). 퍼머링크
(`/rocky/12`) 새로고침은 SPA fallback 으로 돌아온다.

- **화면** — 왼쪽 보드 목록(+ 새 보드), 가운데 섹션별 항목(체크·순서 이동 핸들·번호 버튼은
  `/rocky:board rocky-12` 슬래시 커맨드 복사), 오른쪽 메모 레일. 항목을 누르면 상세 드로어 —
  마크다운 설명, 섹션/보드 이동, 시작·완료·보관, **에이전트에게 보내기**(핸드오프), GitHub 이슈
  만들기, 새 세션 띄우기, 댓글, 타임라인. 상단은 SSE 연결 표시와 최근 활동 띠(앰버=에이전트,
  블루=사람), 테마(시스템/밝게/어둡게), 보관됨 표시, actor 이름.
- **갱신** — SSE(`/api/events`)로 변경이 오면 refetch. 낙관적 갱신 없음(서버가 정본).
- **로컬 전용 기능**(이슈 생성·새 세션)은 `/api/health` 의 `issueCreateAllowed`/`spawnAllowed` 를
  보고 버튼 대신 이유를 보여준다 — 강제는 서버가 한다. 테일넷·터널 경유에서는 그 둘이 막힌다.
- **개발** — `bun run build:ui` 뒤 `ROCKY_TODO_UI_DIST=$PWD/dist cargo run -p rockyd`. 테스트는
  `bun run test:dom`(happy-dom + testing-library). `web/types.ts` 는 Rust 응답 타입의 사본이라 계약이
  바뀌면 같이 고친다.
- 밖에서 닿는 길(테일넷 없이)은 아래 "밖에서 닿기 — Cloudflare Tunnel + Access".
- **화면은 한 열이다(관제판)**: 맨 위 "지금" 표 — 보고 있는 보드와 무관하게 전 보드의
  진행중(누가·경과가 초 단위로 흐른다)·세션 없음·핸드오프·읽지 않은 댓글·수집함 미올림을
  사람이 손댈 것부터 — 그 아래 보드 탭 행, 그 보드의 섹션 목록, 맨 아래 메모(접힘 토글).
  "지금" 의 행을 누르면 그 항목의 상세가 열린다.
- **상단 가운데의 온도 띠**는 최근 활동 48건을 시간순(왼쪽=과거)으로 늘어놓은 눈금이다 —
  앰버는 에이전트, 블루는 사람, 과거로 갈수록 흐려진다. 옆의 "에이전트 · 3시간 전" 이 마지막
  활동이고, 눈금 위에 올리면 그 한 건의 actor·시각이 뜬다. 보드가 지금 얼마나 뜨겁고 누가
  데우고 있는지를 보는 자리다.
- 글자 크기는 여섯 단(`text-micro` 11 · `chip` 12 · `meta` 13 · `sm` 14 · `body` 15 · `title` 20,
  `web/styles/tokens.css`)뿐이다 — 임의 px 유틸리티를 새로 만들지 않는다.

## 요약 — `rocky today` · 세션 시작 요약 · statusline

"지금 뭐 봐야 하나" 를 몇 줄로. 셋이 같은 판정(`rocky_core::summary`)을 쓴다.

- **`rocky today [--json]`** — 첫 줄에 개수(마감 지남 · 오늘 마감 · 진행중 · 핸드오프 대기 · 수집함
  미올림), 그 아래 항목 최대 4개(지난 마감 → 오늘 마감 → 진행중 순). Claude Code 프롬프트에서
  **`! rocky today`** 로 치면 LLM 턴 없이 그대로 뜬다(`!` 는 셸 실행 모드). 수집함은 어댑터를
  실행해(캐시 없으면 기다림) 캐시를 데운다.
- **SessionStart 요약** — `ensure-daemon` 훅이 데몬을 확인한 뒤 같은 문자열을 stdout 으로 내
  세션 컨텍스트에 넣는다(`todo.sessionSummary: false` 로 끔). 데몬이 없거나 실패하면 조용히
  건너뛴다. 컨텍스트에 들어가는 글이라 5줄을 넘지 않는다.
- **statusline** — 템플릿 변수 `{due}`(오늘·지난 마감 미완료 수)와 `{collect}`(수집함 미올림 수)
  추가. 기본 템플릿에 `[  ⏰{due}][  📥{collect}]` 로 들어 있고, 사용자 템플릿에는 직접 넣는다.
  수집함은 **기다리지 않는 조회**(`GET /api/inbox?cached=true`) — 캐시된 것만 쓰고, 없거나 만료됐으면
  뒤에서 갱신을 시작한다. 그래서 첫 줄에는 비어 있다가 다음 틱부터 채워지고, 보고 있는 동안 60초
  주기로 새로워진다. 1초마다 도는 자리라 어댑터를 기다리면 안 된다.
- `GET /api/summary?cwd=&cached=true` — 위 셋이 쓰는 JSON. `cached=true` 면 수집함을 기다리지 않고,
  캐시가 없으면 `collect` 를 생략한다(모름 ≠ 0).

## TUI — 터미널에 띄워 두는 보드 (`rocky tui`)

브라우저 없이 Claude Code 옆 터미널 분할에 보드를 띄워 둔다. `rocky tui [--board K]` 가 옆에 있는
`rocky-tui` 바이너리를 실행한다(릴리스 tarball 에 함께 들어 있다 — `rocky`·`rockyd` 와 **한
디렉터리**). 레포에서는 `cargo build -p rocky-tui` 뒤 `target/debug/rocky-tui`.

- **보드 고르기** — `--board` > `boards.path` 하위 > key 가 cwd 경로 세그먼트 > git remote 유추
  (CLI·statusline 과 같은 규약). `[`/`]` 로 보드 전환, `Tab` 으로 보드 ↔ 수집함 탭 전환.
- **보드 탭** — 왼쪽 섹션별 목록(진행중은 세션 판정까지 글리프로: ● live · ◐ idle · ◌ gone · ◍ unknown,
  열린 핸드오프는 `⇢N`), 오른쪽 선택 항목 상세(설명·링크·댓글 최근 5개). 링크가 GitHub 이슈·PR 이면
  옆에 상태 한 줄(`PR #143 · open · CI ✓ · 리뷰 대기`) — TUI 가 `gh auth token` 으로 받은 토큰을 메모리에만
  두고 GitHub GraphQL 에 **링크 전부를 한 요청**으로 묻는다(백그라운드 스레드, 5분 캐시, 토큰 없으면 그
  줄만 비움). 종류는 URL 이 아니라 API 가 정한다(`/issues/1` 이 PR 이면 PR 로 표시).
- **수집함 탭** — `GET /api/inbox` 를 소스별로 보여준다(실패 소스는 사유와 함께). 이미 보드에 올라간
  항목(현재 보드 todos 의 `links[].url` 에 같은 url)은 ✓ 올라감. `p` 가 선택 항목을 **백로그 섹션**에
  `links: [{ url, title: "<소스>: <제목>" }]` 를 달아 올린다(`POST /api/todos`). 외부 앱 쪽은 건드리지 않는다.
- **키** — `j`/`k` 이동 · `s` start · `x` stop · `d` done · `o` reopen · `a` archive · `h` 핸드오프 ·
  `n` 새 세션(spawn) · `i` 이슈 생성 · `p` 보드로 올리기(수집함) · `r` 새로고침(수집함에서는 어댑터 다시
  실행) · `q` 종료.
- **핸드오프** — `h` 는 `GET /api/sessions?board=` 의 `matched` 후보가 **정확히 1개**면 바로 넘기고,
  아니면 세션 피커(매칭된 것 먼저, `*` 표시)를 띄운다 — 데몬 라우트의 자동 매칭 기준과 같다. 넘긴 뒤
  상세에 "핸드오프 대기 N — 세션이 다음 턴에 집어간다". `poke` 는 보내지 않는다(그건 에이전트 표면).
- **갱신** — `GET /api/events`(SSE)를 별도 스레드가 읽고, 이벤트가 오면 보드를 **refetch** 한다
  (payload 는 보지 않는다 — 계약). 끊기면 1·2·4·8초 백오프로 재연결하고 붙을 때마다 전체 refetch
  (놓친 변경을 그렇게 따라잡는다). 데몬이 없으면 상단에 "데몬 없음" 을 띄우고 3초마다 다시
  두드린다 — **TUI 는 데몬을 띄우지 않는다**(그건 훅·CLI 몫).

## MCP 도구 5개 (에이전트)

| 도구 | 하는 일 |
| --- | --- |
| `todo_list` | 보드/항목 조회 (`{ board }` 현황, `{ id }` 상세+히스토리+댓글, `{ boards: true }` 보드 목록). `includeArchived` 는 `{ id }` 단건 조회에서 댓글까지 함께 통제한다 |
| `todo_write` | todo 생성/수정 (board, title, section, parentId, priority, due, labels, links, comment, createIssue, actor) |
| `todo_status` | 상태 전환 — `start` / `stop` / `done` / `reopen` / `archive` / `unarchive` |
| `note_list` | 스크래치패드 메모 조회 (보드 소속 or 글로벌) |
| `note_write` | 메모 생성/수정/append/archive (`mode`) |

각 도구의 `id` 인자는 아래 "CLI 표면" 의 REF 문법을 그대로 받는다 — 맨숫자(`12`)처럼 보드
접두사가 없는 번호는 같이 넘기는 `board` 인자가 그 컨텍스트가 된다. `createIssue: true` 는
그 todo 를 GitHub 이슈로 만들고 URL 을 `links` 에 붙인다 (아래 "GitHub 이슈로 만들기" 참고).

## 호스트별 MCP 등록

Claude Code 에서는 플러그인 설치로 자동 등록되므로 수동 작업이 필요 없다. **opencode / Codex** 는
플러그인 훅을 돌리지 않으므로 수동 등록한다. 데몬의 MCP 엔드포인트는 `http://127.0.0.1:8636/mcp`
(streamable HTTP, 도구 5개: `todo_list` / `todo_write` / `todo_status` / `note_list` / `note_write`).
`rocky mcp setup` 이 스니펫을 출력한다.

**opencode** (`~/.config/opencode/opencode.json`):

```json
{ "mcp": { "rocky": { "type": "remote", "url": "http://127.0.0.1:8636/mcp" } } }
```

**Codex** (`~/.codex/config.toml`, streamable HTTP 지원 버전):

```toml
[mcp_servers.rocky]
url = "http://127.0.0.1:8636/mcp"
```

Codex 버전이 HTTP MCP 를 지원하지 않으면 CLI(`rocky`)를 Bash 로 쓰면 된다 — 표면은 동일하다.
어느 호스트든 세션 시작 시 데몬이 떠 있어야 도구가 붙는다 — 상시 사용이면 `daemon install` 권장.

## 다음 작업 고르기 (`/rocky:next`)

브라우저를 열지 않고 세션에서 바로 고르는 경로. `/rocky:next` 를 치면 착수 후보를
랭킹해 보여주고, 고른 항목을 `start` 표시한 뒤 그 자리에서 시작한다. 참조를 알고 있으면
`/rocky:next rocky-12` 로 픽커를 건너뛴다.

커맨드는 후보를 **텍스트 목록으로 그대로 보여주고** 번호나 참조로 고르게 한다 — 클릭형 선택
UI 는 쓰지 않는다(블록이 다 만들어져야 렌더돼서 목록이 늦게 나타난다).

랭킹은 CLI(`rocky next`)와 같은 판정을 쓴다 — **주인 없는 진행중**(세션이 사라졌거나
멈춘 doing) → 마감(지남 > 오늘 > 7일 내) → 판정할 수 없는 진행중(사람이 잡은 것 등) →
우선순위 → 최근 댓글. 이 순서는 **뒤집히지 않는다**: 아래쪽 기준이 아무리 쌓여도 위쪽
기준을 넘지 못하므로, 마감 지난 p1 이 이어받을 p4 를 밀어내는 일은 없다. 살아 있는 세션이
붙들고 있는 항목과 **열린 자식을 가진 우산 항목**은 후보에서 빠진다. 근거는 목록에 그대로
찍힌다:

```
$ rocky next
1. rocky-todo-22  데몬 라우트에 Origin 검사  — 이어받기(멈춤) · p2
2. rocky-todo-21  웹 UI 라이트 모드 마이그레이션  — p2 · 최근 댓글
```

고른 항목의 보드가 지금 레포와 다르면 어디서 할지(여기서 / 새 세션 spawn / 다른 세션
handoff)를 한 번 더 묻는다. 같은 레포면 묻지 않는다.

`--json` 은 `ls --json` 과 달리 **컴팩트 형태**다 — 고를 때 필요한 필드(`ref`·`number`·`board`
·`title`·`reason`·`priority`·`status`·`due`·`labels`·`commentCount` + 160자로 자른 `summary`)만
낸다. `description` 전문은 싣지 않는다 — 전문이 필요하면 `show REF`. 스크립트나 CLI 를 직접
부르는 호스트용이고, 커맨드는 텍스트 쪽을 쓴다.

## 보드 메타 — 이름·key·설명·GitHub

보드는 자기 정체를 들고 있다. 보드를 열면 목록 위에 헤더가 뜨고(이름 · key · 한 줄 설명 ·
GitHub 링크 · 레포 경로), 오른쪽 `편집` 으로 그 자리에서 고친다 — 웹 폼은 네 필드를 다루고
`path` 는 표시만 한다(그건 spawn 이 자기 입력창에서 받는다). CLI 는 다섯 다 고친다:

| 항목 | 무엇 | CLI |
| --- | --- | --- |
| `title` | 사람이 읽는 이름 (사이드바에 뜨는 값) | `board title "Tally"` |
| `key` | 참조 접두사(`tally-12`)이자 레포 이름으로 유추되는 식별자 | `board rename tally` |
| `description` | "이 보드가 무엇인가" 한 줄 | `board desc "가계부 앱"` (인자 없으면 지움) |
| `repo` | GitHub `owner/name` — 이슈 생성 대상 | `board repo OWNER/NAME` |
| `path` | 메인 레포 절대경로 — spawn 이 워크트리를 만드는 자리 | `board path [절대경로]` |

`rocky board show [KEY]` 로 한 보드의 전부를 본다. `board ls` 는 설명까지 한 줄로 붙인다.

**key 를 바꿔도 옛 참조는 죽지 않는다.** 옛 key 는 별칭으로 남아 입력으로 계속 받는다 —
히스토리·댓글·GitHub 이슈 본문에 박힌 `gotgan-12` 도, 훅/CLI 가 cwd 에서 유추해 보내는 옛
`board` 인자도 그대로 그 보드로 풀린다. 반대로 **내보내는 문자열은 언제나 새 key** 다
(`refOf`). 그 대가로 한 번 쓴 key 는 은퇴한다 — 다른 보드가 그 이름을 다시 가질 수 없고,
시도하면 `board key already in use` 로 거절된다.

- key 는 만들 때와 같은 검증을 받는다 — 공백과 `#` 는 참조로 되읽을 수 없어 거절된다.
- 이름을 바꾸는 목적은 보통 `key` 를 레포 디렉터리 이름에 **맞추는** 것이다. 반대로
  어긋나게 두면 잃는 게 하나 있다: 세션 ↔ 보드 자동 매칭은 세션 cwd 의 경로 세그먼트에
  key 가 나타나는지로 판정하므로(핸드오프 대상 고르기, 방치된 doing 판정) 디렉터리 이름과
  다른 key 는 후보를 못 찾는다 — 기능이 죽지는 않고 사람이 대상을 직접 고르게 된다.
  statusline 의 보드 판정은 다르다 — `boards.path` 를 먼저 보므로 경로만 설정돼 있으면
  이름이 어긋나도 정확하다.
- `rocky board show` 가 옛 이름(`previousKeys`)을 함께 보여준다 — 그 참조가 아직
  살아 있다는 걸 아는 자리다.

REST 로는 `PATCH /api/boards/:key` 하나가 다섯 필드를 **함께** 받는다(한 트랜잭션이라
부분 적용이 없다). `null` 은 "지운다"이고 빈 문자열은 400 — 폼이 실수로 비워 보낸 값이
설정을 날리지 않게 하려는 구분이다.

## GitHub 이슈로 만들기

todo 하나를 GitHub 이슈로 올릴 수 있다 — CLI `rocky issue REF [--repo OWNER/NAME]`,
MCP `todo_write { id, createIssue: true }`
셋 다 같은 경로를 탄다(새 MCP 도구가 아니라 기존 `todo_write` 의 필드다 — 도구는 여전히
5개). 만들어진 이슈 URL 은 그 todo 의 링크에 자동으로 붙고(제목은 `#<이슈번호>`), 기존
`updateTodo` 를 거치므로 히스토리·SSE·훅 주입에 그대로 실린다.

- **인증**: `gh` CLI 를 빌린다 — 토큰을 저장하지 않는다. `gh` 가 없거나 로그인 전이면 그
  사유를 그대로 보여준다.
- **보드마다 GitHub 레포(`owner/name`)를 알아야 한다** — 보드는 원래 key(=git remote
  basename)만 알아서 owner 를 모른다. 채우는 경로 셋:
  - `rocky board repo [OWNER/NAME]` — 인자 없으면 cwd 의 git remote 에서 유추
  - `rocky issue REF` 는 보드에 repo 가 없으면 cwd 에서 유추해 진행한다 — 저장은
    서버가 `gh` 성공 후 todo 의 실제 보드에 한다(CLI 는 더 이상 미리 PATCH 하지 않는다)
- 이미 이슈 링크가 있는 todo 는 다시 만들지 않는다. **역방향 동기화는 없다** — 이슈를
  닫아도 todo 는 자동으로 완료되지 않고, 이슈 본문/제목이 사후에 바뀌어도 todo 에는
  반영되지 않는다.
- **로컬(루프백) 요청만 이슈를 만들 수 있다.** `gh` 인증을 빌리기 때문이다 — 보드를 노출하는
  것(`todo.expose`)과 GitHub 계정 권한을 노출하는 것은 다른 얘기라, 노출 설정과 무관하게
  이 표면만 잠긴다. 노출된 주소로 접속한 브라우저는 버튼 대신 그 이유를 보고(이미 만들어진
  이슈로 가는 링크는 그대로 열린다), REST 는 403, MCP `todo_write` 는 도구 에러가 된다.
  `tailscale serve` 를 거친 접속도 마찬가지다 — 프록시가 루프백으로 중계하지만 중계 흔적
  (`X-Forwarded-*` / `Tailscale-User-*`)으로 구분한다. 폰에서 보드를 보다 이슈를 만들려면
  그 머신에서 CLI(`rocky issue REF`)를 쓰거나 에이전트에게 시킨다.

## 사람→에이전트 자동 전달 (UserPromptSubmit 훅, Claude Code 전용)

에이전트→웹 방향은 SSE 로 실시간이고, 반대 방향은 **훅**이 닫는다: 사용자가 프롬프트를
보낼 때마다 플러그인의 `UserPromptSubmit` 훅이 데몬의 `/api/changes` 를 세션별 커서
이후로 읽어 **호출자(사람)의 변경만** 요약해 컨텍스트로 주입한다. 사람이 CLI/REST 로 todo 를
추가하고 아무 말이나 걸면 에이전트가 그 변경을 이미 알고 있는 구조다. 사람이 단 댓글도
같은 경로로 주입된다(본문 200자 절단, 개행은 공백으로 정리).

- 결정론적 (LLM 미사용), fail-open — 데몬이 꺼져 있으면 조용히 no-op (훅이 데몬을 기동하진 않는다)
- 에이전트 자신의 변경(claude-code/codex/opencode)은 걸러서 자기 반향 없음
- 끄기: `rocky.json` `todo.watch: false` 또는 env `ROCKY_TODO_WATCH=0`

## 보드 → 세션 핸드오프 (턴 경계 배달, Claude Code 전용)

보드의 todo 를 실행 중인 Claude Code 세션에 넘길 수 있다 — `rocky handoff REF [--session NAME]
[--message "본문"]`(REST `POST /api/todos/:ref/handoffs` 도 같은 일). 데몬은
세션에 아무것도 밀 수 없으므로 요청은 큐에 쌓이고, 대상 세션이 **턴 경계**에 이를 때
훅이 집어간다 — `UserPromptSubmit`(턴 시작) 또는 `Stop`(턴 끝, `decision: block` 으로 그
자리에서 착수). 한 번에 한 건씩 순서대로 소화한다.

> **큐잉은 배달이 아니다.** 턴 경계가 와야 배달되므로 **idle 세션은 아무 일도 일어나지
> 않는다** — 누군가 그 세션의 턴을 열어줘야 한다. 그래서 `handoff` 응답(`--json`)에는
> `poke: { to, message }` 가 함께 온다. 에이전트라면 그대로 `SendMessage` 로 보내면 되고
> (그 메시지가 여는 바로 그 턴에 훅이 상세 지시를 주입한다), 사람이라면 그 세션에 아무
> 입력이나 한 줄 넣으면 된다. CLI 출력도 이 두 갈래를 그대로 안내한다.

운영자가 알아둘 것:
- **`claude` CLI 가 PATH 에 있어야 동작한다** — 세션 목록(`rocky sessions`)이
  `claude agents --json` 을 실행해서 얻기 때문이다. 없으면 이 기능(CLI +
  `sessions`/`handoff` CLI)만 비활성되고, 보드의 나머지 기능은 정상 동작한다.
- **`Stop` 훅은 신규다** — 플러그인을 이 버전으로 업데이트하면 다음 세션이 아니라 **그 세션의
  다음 Stop 이벤트부터** 곧바로 적용된다(훅 등록 자체는 SessionStart 때가 아니라 플러그인
  설치 시점에 이미 반영되어 있다).
- 대상 세션은 보드 key 와 세션 cwd 의 **경로 세그먼트** 매칭으로 고른다 — 후보가 정확히 1개면
  자동으로 그 세션에 보내고, 여러 개면 `--session` 으로 직접 골라야 한다.
- 대기 중인 요청에 TTL 은 없다 — 대상 세션이 종료돼도 큐에는 남고 "세션 없음"(stale)으로만
  표시된다. 취소하려면 `rocky handoff REF --cancel`.
- **배달 이후도 추적한다.** 세션이 요청을 집어간 뒤 그 항목에 `start`(또는 start 를 건너뛴
  `done`)를 부르면 "착수함"으로 기록되고, `done` 이면 "완료"까지 남는다. 집어가 놓고
  아무것도 하지 않으면 드로어에 "받았지만 착수하지 않았다" 와 **다시 보내기** 버튼이 뜬다.
  판정에 시간 제한은 없다 — 대상 세션이 사라졌거나 일을 멈춘 상태일 때만 뜨고, 아직 작업
  중이면 조용하다. 자동 재배달은 하지 않는다(다시 보내면 새 요청이 생기고 원래 기록은
  남는다) — "보냈는데 조용히 사라졌다" 를 만들지 않기 위해서다.
- MCP 도구는 늘지 않았다 — 여전히 5개(`todo_list` / `todo_write` / `todo_status` /
  `note_list` / `note_write`). 핸드오프는 사람이 세션에 넘기는 경로이지, 에이전트가 호출하는
  도구가 아니다.

## 보드 → 새 워크트리 세션 (spawn, 로컬 전용)

실행 중인 세션이 없어도 보드에서 바로 새 작업을 시작시킬 수 있다 — `rocky spawn REF
[--message "본문"]`(REST `POST /api/todos/:ref/spawn`). 데몬은 git 을
전혀 만지지 않는다 — `claude --bg --worktree todo-<번호>` 를 실행해 **Claude Code 에게
워크트리 생성을 맡긴다.**

- **경로 설정**: 보드마다 메인 레포의 절대경로(`boards.path`)를 알아야 spawn 이 동작한다.
  `rocky board path [절대경로]`(인자 없으면 지금 있는 cwd)로 설정한다 — GitHub 이슈의
  `board repo` 와 같은 모양으로, spawn 이
  성공한 뒤에만 보드에 저장된다(오타난 경로가 실패와 무관하게 눌어붙지 않는다).
  **상대경로는 거부한다**(400) — 데몬은 launchd/훅이 임의의 자리에서 띄우므로 상대경로가
  어느 레포로 풀릴지 알 수 없다. 심볼릭 링크와 `..` 은 실경로로 정규화해서 쓰고 저장한다
  — 동시 실행 가드가 `claude agents --json` 의 cwd 와 문자열로 비교하기 때문이다.
- **워크트리가 쌓이는 자리**: `<메인 레포>/.claude/worktrees/todo-<번호>`, 브랜치는
  `worktree-todo-<번호>`. 같은 todo 번호로 다시 누르면 Claude Code 가 기존 워크트리를
  재사용한다 — 워크트리 이름 자체가 "이 todo 의 워크트리" 라는 기억이라 데몬은 따로
  저장하지 않는다.
- **정리**: `claude rm <짧은 id>` 가 워크트리와 job state 를 함께 지운다. git 명령으로
  직접 지우려면 Claude Code 가 걸어둔 lock 때문에 `git worktree remove -f -f` 가 필요하다.
  **자동 삭제는 없다** — 커밋되지 않은 작업물이 조용히 사라지는 것이 이 기능에서 가장
  나쁜 실패라, 워크트리는 명시적으로 지울 때까지 남는다.
- **동시 실행 가드**: 그 워크트리에서 이미 도는 세션(백그라운드든 사람이 연 interactive
  세션이든)이 있으면 새로 띄우지 않고 기존 핸드오프 큐로 넘긴다(`reused: true`) — 두
  에이전트가 한 워크트리를 같이 고치는 사고를 막는다. 이 판정만은 **캐시 없는** 세션
  목록으로 한다(다른 조회는 TTL 3초 캐시를 쓴다). 새 세션이 `agents --json` 에 등록되기
  전의 틈은 데몬이 "방금 띄운 워크트리" 를 60초 기억해 메운다 — 그 창 안의 재요청은
  409 다(버튼 두 번 누르기/두 탭). 잠시 후 다시 누르면 된다. 이 기억은 세션을 **띄우기
  전에** 잡고 실패하면 되돌린다 — 그래야 두 탭에서 동시에 눌러도 하나만 통과하고,
  실패한 시도가 60초 동안 재시도를 막지 않는다.
- **로컬(루프백) 요청만** — GitHub 이슈 생성과 같은 등급의 게이트다. 보드 쓰기 권한이
  "이 기계에서 파일을 고치는 프로세스를 띄우는 권한" 으로 확대되는 지점이라, `todo.expose`
  로 `lan`/`tailscale-serve` 를 열어도 원격 클라이언트는 403 을 받는다(`/api/health` 의
  `spawnAllowed` 가 힌트, 강제는 서버가 한다). 원격에서 띄우려면 그 머신에서 CLI(`rocky spawn REF`)를 쓰거나
  에이전트에게 시킨다.
- **승인 프롬프트에서 멈춘 세션은 보드가 모른다** — `state` 가 그때도 `working` 으로
  보인다. 드로어와 `rocky sessions` 가 보여주는 짧은 id 로 `claude attach <id>` 하면
  붙어서 승인을 처리할 수 있다.
- **`--permission-mode` 는 넘기지 않는다** — 사용자 settings 의 `permissions.defaultMode`
  를 그대로 따른다.
- MCP 도구는 늘지 않았다 — spawn 은 사람이 보드에서 누르는 버튼으로만 남는다.

## 수집함 — 외부 투두 앱 읽기 (`todo.inbox` → `GET /api/inbox`)

외부 투두 앱(구글 투두·Todoist·…)은 rocky 와 **동기화하지 않는다.** 별개의 수집함으로 두고,
rocky 는 읽어서 보여주고 사용자가 고른 것을 보드로 올리며(링크 자동) 참조만 한다. 설계 근거는
[`design/specs/2026-09-27-bridges-and-tui-design.md`](./design/specs/2026-09-27-bridges-and-tui-design.md).

읽는 쪽은 **어댑터 = 명령**이다. `rocky.json` 의 `todo.inbox[]` 에 등록하면 데몬이 argv 그대로
실행해(셸 없음) stdout 의 JSON 을 읽는다. 어댑터 코드는 `bridges/<name>/` 에 두고, 데몬·CLI 는
이 규약으로만 안다 — 특정 서비스 이름이 `crates/` 에 들어가면 위반이다(`AGENTS.md` Scope).

```json
{ "todo": { "inbox": [
  { "name": "todoist", "command": ["python3", "/path/to/rocky/bridges/todoist/inbox.py",
                                   "--op", "op://Agent Vault/<item-uuid>/credential", "--filter", "#Inbox"],
    "timeoutMs": 30000 },
  { "name": "file",    "command": ["sh", "/path/to/rocky/bridges/file/inbox.sh", "~/inbox.json"], "timeoutMs": 5000 }
] } }
```

| 필드 | 의미 |
| --- | --- |
| `name` | `[a-z0-9-]+`. 응답의 소스 키이자, 올린 항목의 링크 제목 접두사(`gtasks: …`) |
| `command` | argv 배열. env 는 데몬 것을 물려받는다. **토큰은 어댑터가 스스로 읽는다** (`op read` — 홈의 평문 파일 금지) |
| `timeoutMs` | 기본 10000. 넘기면 죽이고 그 소스만 `available:false`(실패도 60초 캐시). 자격 조회(`op read`) + 외부 API 를 순차로 하는 어댑터는 그 합보다 크게 준다 — todoist 예시가 30000 인 이유 |

**어댑터 규약** — stdin 없음, stdout 에 JSON 하나, exit 0:

```json
{ "items": [
  { "id": "MTIz", "title": "보드 TUI 수집함 탭", "url": "https://tasks.google.com/task/MTIz",
    "note": "평문 본문(옵션)", "due": "2026-10-01", "createdAt": "2026-09-27T01:02:03Z" }
] }
```

- `id`·`title` 필수(`id` 는 숫자여도 문자열로 받는다). `url`·`note`·`due`(`YYYY-MM-DD`)·
  `createdAt`(RFC 3339) 옵션. 형식이 틀리면 **그 소스 전체가 실패**다 — 반쯤 통과시키지 않는다.
- 완료된 항목은 내지 않는다. 정렬은 어댑터 몫.
- exit ≠ 0 이면 stderr 첫 줄이 사유가 된다. 참조 구현: [`bridges/file/inbox.sh`](../bridges/file/inbox.sh)
  (JSON 파일을 그대로 낸다 — 테스트·수동 확인용). 실제 앱: [`bridges/todoist/inbox.py`](../bridges/todoist/inbox.py)
  — Todoist API v1 활성 작업(`--filter` 로 Todoist 필터 문법), 토큰은 1Password Agent Vault 에서 `op read`
  (참조는 항목 **UUID** 로 — 제목에 한글·`:` 이 있으면 `op://` 문법이 깨진다). `--from FILE` 이면 토큰 없이
  저장된 응답을 변환한다(테스트).

**`GET /api/inbox?refresh=true`** — 소스를 **동시에** 실행하고 소스별 60초 캐시(실패도 캐시된다 —
죽은 어댑터를 매 요청마다 때리지 않는다). `refresh=true` 는 캐시를 우회한다. 설정된 소스가 없으면
`{ "sources": [] }`.

```json
{ "sources": [
  { "name": "gtasks", "available": true,  "fetchedAt": "…", "items": [ … ] },
  { "name": "file",   "available": false, "reason": "exit 1: no such file: …", "fetchedAt": "…", "items": [] }
] }
```

로컬 전용이 **아니다** — 수집함 내용은 보드 내용과 같은 급이라 `todo.expose` 를 그대로 따른다.
실행되는 명령은 요청이 아니라 설정에서 오므로 원격 요청으로 임의 명령을 돌릴 길은 없다. 다만
**실패 사유의 상세(stderr 첫 줄·출력 조각)는 로컬 요청에만** 낸다 — 어댑터가 찍은 토큰·인증 URL 이
섞일 수 있어서다. 원격(`isLocalRequest` 아님 — tailscale serve 경유 포함)에는 `exit N` 만 간다.
MCP 도구는 늘리지 않았다(5개 유지) — 에이전트가 볼 필요가 생기면 `/rocky:next` 가 REST 로 읽는다.
보드로 올리는 건 클라이언트(TUI, 후속)가 `POST /api/todos` 에 `links: [{ url, title: "<name>: <title>" }]`
를 붙여 한다 — 중복 판정도 클라이언트가 현재 보드 todos 의 `links[].url` 로 한다.

## 밖에서 닿기 — Cloudflare Tunnel + Access (테일넷 없이)

테일넷을 못 쓰는 곳(회사망·다른 사람 기기)에서 웹 UI 에 닿는 길. 데몬은 그대로 127.0.0.1:8636 에
두고, `cloudflared` 가 **아웃바운드**로 Cloudflare 엣지에 붙어 `board.<도메인>` 으로 노출한다.
포트를 열지 않고, 앞단의 **Cloudflare Access** 가 본인 확인을 한다 — 데몬은 무인증이라 Access 가
유일한 문이다. 워커 코드는 없다(정본은 여전히 로컬 데몬 하나).

**순서가 곧 안전이다 — Access 정책을 먼저, 터널 실행은 마지막에.** 호스트명이 DNS 에 붙은 채 터널을
먼저 돌리면 Access 가 걸리기 전까지 무인증 보드가 인터넷에 그대로 열린다(중계 헤더 게이트는 이슈
생성·spawn·claim 만 막는다 — 보드 읽기·쓰기는 그대로 된다).

1. **Access 정책부터** — Cloudflare 대시보드 → Zero Trust → Access → Applications 에 `board.<도메인>` 을
   Self-hosted 로 등록하고, 정책을 **본인 이메일(OTP) 또는 GitHub 로그인 한 계정**으로 건다. 아직 DNS 도
   터널도 없으니 이 시점엔 아무것도 노출되지 않는다.
2. 터널 만들기(아직 실행하지 않는다):

   ```bash
   brew install cloudflared
   cloudflared tunnel login                       # 브라우저에서 계정·존 선택 → ~/.cloudflared/cert.pem
   cloudflared tunnel create rocky-board          # 터널 UUID + 자격 JSON (~/.cloudflared/<uuid>.json)
   ```

   `~/.cloudflared/config.yml`:

   ```yaml
   tunnel: <uuid>
   credentials-file: /Users/<me>/.cloudflared/<uuid>.json
   ingress:
     - hostname: board.<도메인>
       service: http://127.0.0.1:8636
     - service: http_status:404
   ```

3. DNS 를 붙인다 — `cloudflared tunnel route dns rocky-board board.<도메인>`. Access 앱이 1 에서 이미
   이 호스트명을 덮고 있어야 한다.
4. **정책 확인 뒤 실행** — `cloudflared tunnel run rocky-board` 로 띄우고, 로그아웃한 브라우저(또는 시크릿
   창)에서 `https://board.<도메인>` 이 **Access 로그인 화면**으로 떨어지는지 먼저 본다. 보드가 바로 보이면
   즉시 터널을 내리고(Ctrl-C) 1 로 돌아간다. 확인됐으면 `sudo cloudflared service install` 로 launchd 상주.

- **데몬 쪽 판정** — cloudflared 는 `cf-connecting-ip`·`cf-ray`·`x-forwarded-for` 를, Access 는
  `cf-access-jwt-assertion`·`cf-access-authenticated-user-email` 을 붙인다. 전부 중계 헤더 목록에 있어
  터널 경유 요청은 **원격**으로 분류된다 — 이슈 생성·새 세션(spawn)·claim 은 막히고(의도), 나머지
  보드 기능은 된다. 웹 UI 는 `/api/health` 의 `issueCreateAllowed`/`spawnAllowed` 로 그 버튼을 이유와
  함께 비활성으로 그린다.
- **cross-site 가드** — 브라우저는 `https://board.<도메인>` 을 같은 출처로 보므로(`Sec-Fetch-Site:
  same-origin`) 변경 요청이 막히지 않는다. 데몬이 `Host` 를 보지 않는 이유가 이것이다.
- **한계** — 맥이 자면 안 보인다(테일넷과 같음). 맥이 자도 보여야 하면 워커 미러(스펙의 "CF Worker
  중계") 가 다음 단계다. Access 의 JWT 를 데몬이 검증하지는 않는다 — 원본(127.0.0.1)은 터널 말고는
  닿을 길이 없으니 엣지 검증으로 충분하다고 본다. 터널 자격 파일은 홈에 남는 평문이라 "홈에 평문 토큰을 두지 않는다" 원칙과
  같은 취급(600, 백업 제외).
- `todo.expose` 채널은 건드리지 않는다 — 터널은 데몬 밖 프로세스라 데몬 설정이 필요 없다.

## 노출 범위 (`todo.expose` — 기본 이 머신만)

보드에 **인증이 없으므로** 노출은 전부 opt-in 채널이다. user `rocky.json` 의
`todo.expose` 에 채널을 넣는다 — 배열로 조합하거나, 하나면 문자열로:

```jsonc
{ "todo": { "expose": ["lan", "tailscale-serve"] } }   // 내부망 + 테일넷 동시
{ "todo": { "expose": "lan" } }                  // 내부망만
{ "todo": { "expose": "off" } }                  // 미설정과 동일 (기본)
```

| 채널 | 열리는 범위 | 바인딩 | 비고 |
| --- | --- | --- | --- |
| (없음) | 이 머신만 | 127.0.0.1 | 기본값 |
| `"lan"` | 같은 내부망의 모든 기기 (`http://<이 머신 IP>:8636`) | 0.0.0.0 | 무인증 — 집 등 신뢰망 전용. `rocky open` 이 내부망 주소를 함께 출력 |
| `"tailscale-serve"` | 테일넷에 연결된 내 기기들 (HTTPS) | 127.0.0.1 유지 | tailscaled 프록시가 중계, 기동 시 `tailscale serve` 자동 보장. 테일넷 Serve 기능 첫 사용 시 관리 콘솔 1회 승인 필요 |

- 핸드오프 "보내기"(`POST /api/todos/:ref/handoff`)와 세션 목록(`GET /api/sessions`)은
  노출 채널을 그대로 타 원격에서도 된다 — 의도된 동작(폰에서 보드 보다 보내기). **새 세션
  띄우기(`POST /api/todos/:ref/spawn`)는 다르다** — 이슈 생성과 같이 노출 설정과 무관하게
  로컬 요청만 받는다(위 "보드 → 새 워크트리 세션" 참고). `claim`(`POST /api/handoffs/claim`)
  은 훅 전용이라 루프백(127.0.0.1/::1) 요청만 받는다 —
  훅은 항상 로컬에서 붙으니 기능 손실은 없다. 판정은 이슈 생성과 같은 `isLocalRequest`
  를 쓴다 — **소스 주소가 루프백이고 동시에 중계 헤더가 없어야** 로컬로 본다. 주소만
  보면 부족하기 때문이다:
  - `lan` 은 데몬이 `0.0.0.0` 에 직접 바인딩하므로 원격 요청의 소스 주소가 실제 LAN IP 로
    보인다 — 주소만으로 걸러진다.
  - `tailscale-serve` 는 데몬이 계속 127.0.0.1 에만 바인딩하고 tailscaled 의 로컬 프록시가
    테일넷 요청을 다시 `127.0.0.1:<port>` 로 다이얼해 전달한다(위 표의 "바인딩" 참고).
    그래서 소스 주소는 항상 127.0.0.1 이지만, tailscale serve 가 붙이는
    `Tailscale-User-*` 헤더가 남으므로 **이 요청도 404 로 막힌다.**

  헤더는 위조로 "있게" 만들 수는 있어도 "없게" 만들 수는 없다 — 위조는 요청을 덜
  신뢰하는 방향으로만 작용하므로 이 판정을 우회하는 데 쓸 수 없다.
- env `ROCKY_TODO_EXPOSE`(콤마 구분)가 설정되면 config 를 통째로 덮어쓴다 — `off` 로 강제 차단.
- `tailscale-serve` 채널이 없으면 rocky 는 tailscale 을 일절 건드리지 않는다 (tailscale 이 금지된 환경).
  수동 제어: `rocky tailscale on|off|status`.
- **기동 시 자동 보장은 남의 노출을 빼앗지 않는다.** `tailscale serve` 의 노출 지점은 443 의
  `/` 하나뿐인 머신 공유 자원인데, 데몬의 단일 인스턴스 보장은 *같은 포트* 기준이라 다른
  포트로 뜬 개발/데모 인스턴스가 설치본과 나란히 존재할 수 있다. 그래서 기동 시에는 현재
  serve 대상 포트를 먼저 확인해서, 거기에 **살아 있는 다른 rocky 데몬**이 있으면
  양보하고(그 인스턴스는 테일넷에 노출되지 않는다) 아무도 안 듣는 죽은 포트면 되찾는다.
  일부러 넘기고 싶을 땐 명시적으로 `rocky tailscale on` — 수동 경로는 그대로 인수한다.
- `tailscale funnel`(공인 인터넷 공개)은 지원하지 않는다 — 무인증 보드라 위험하다.
- 노출되는 것은 **보드**다. GitHub 이슈 생성은 어느 채널로도 열리지 않는다 — 로컬 요청
  전용이다 ([GitHub 이슈로 만들기](#github-이슈로-만들기) 참고).
- **다른 사이트가 시킨 변경은 거부한다(403).** 데몬은 무인증이라, 사용자가 방문한 아무
  페이지나 루프백으로 폼을 POST 하면 소스 주소 기반 로컬 게이트를 그대로 통과한다. 그래서
  변경 메서드(POST/PATCH/PUT/DELETE)는 브라우저가 붙이는 `Sec-Fetch-Site` 를 먼저 보고
  `cross-site` 면 라우트에 닿기 전에 끊는다(그 헤더가 없는 구형 브라우저는 `Origin` 으로
  판정). CLI·훅·MCP 클라이언트는 두 헤더를 아예 안 보내므로 영향이 없고, 브라우저 클라이언트가
  있다면 `same-origin` 이어야 한다. `tailscale serve` 를 거친 것도 같다 —
  `Sec-Fetch-Site` 는 브라우저가 계산한 값이라 프록시가 `Host` 를 바꿔도 흔들리지 않는다.
- 데몬 설정 변경 후에는 재시작해야 반영된다: `rocky daemon stop && rocky daemon start`.
- 플러그인 업데이트는 다음 세션 시작 때 자동 반영된다 — SessionStart 훅이 실행 중인 데몬의
  버전을 확인해 구버전이면 내리고 새 버전으로 재기동한다 (보드 데이터는 `~/.config/rocky/todo`
  에 있어 그대로 보존). 즉시 반영하고 싶으면 `rocky daemon stop` 후 아무 명령이나 실행.

## CLI 표면 (사람/스크립트/폴백)

```
rocky ls [--board K|--all] [--archived] [--json]
rocky next [--board K|--all] [--limit N] [--json]   # 착수 후보 랭킹 (다음에 뭘 할까)
rocky tui [--board K]                              # 보드를 터미널 화면으로 (위 "TUI")
rocky today [--json]                               # 보드 요약 몇 줄 — 마감·진행중·핸드오프·수집함 (아래 "요약")
rocky add "제목" [--section S] [--parent REF] [--desc MD] [--due YYYY-MM-DD]
                     [--priority p1..p4] [--label a,b] [--link URL]
rocky show|start|stop|done|reopen|archive|unarchive|update REF
rocky comment REF "본문"
rocky issue REF [--repo OWNER/NAME]           # GitHub 이슈로 (gh CLI 필요)
rocky note add|ls|show|edit|append|archive
rocky history REF [--global|--note] · section ls · open
rocky board ls|show [KEY]|add KEY [제목]      # 보드 메타 — 아래 "보드 메타" 참고
rocky board rename NEWKEY|title "제목"|desc ["설명"]|repo [OWNER/NAME]|path [절대경로]
rocky handoff REF [--session NAME] [--message "본문"] · handoff REF --cancel
rocky spawn REF [--message "본문"]            # todo 전용 워크트리에 새 세션 띄우기 (로컬 전용)
rocky sessions
rocky daemon run|start|stop|status|install|uninstall · mcp setup
rocky tailscale on|off|status
```

REF 는 id 대신 사람이 읽을 수 있는 참조를 받는다: `rocky-12`(보드 지정, 가장 오른쪽 `-` 에서
갈린다) → 맨숫자 `12`(현재 보드 안의 번호) → id 전체 → id 앞부분(유일하면) 순으로 해석한다.
옛 표기(`rocky#12` / `#12`)도 입력으로는 계속 받는다. `todo ls` 는 항목마다 맨숫자만
보여준다 — 같은 저장소(cwd)에서는 그 번호를 그대로 다음 명령의 REF 로 쓰면 되고, 다른
보드를 가리키려면 `show` 로 얻은 전체 참조(`rocky-12`)나 `--board` 플래그를 쓴다. `note ls`
는 다르다 — 메모는 보드 컨텍스트가 없는 전역 번호 공간이라 맨숫자가 아니라 전체 참조
(`note-3`)를 그대로 보여준다. 랜덤 id 는
여전히 기본 키이고 `show` 상세 출력의 `id:` 줄에서 볼 수 있다. 보드 미소속 글로벌
메모는 번호가 `note-3` 으로 표시되고,
그 번호를 보드 번호와 구분해 조회하려면 `note show|edit|append|archive`/`history` 에
`--global` 을 붙인다. `note` 는 전역 메모 참조의 예약 접두사지만 보드 이름으로 쓰는 것
자체는 막지 않는다 — 다만 그 보드의 항목은 `note-3` 이 늘 전역 메모를 가리키도록
`note-N` 대신 raw id 로만 참조된다. todo 와 메모는
같은 보드 안에서도 번호 공간이 따로라 번호 `2` 가 둘 다일 수 있는데, `history` 는 todo 를
먼저 찾으므로 메모의 히스토리를 보려면 `--note`(보드 메모) 또는 `--global`(전역 메모)로
대상을 확정한다.

보드 키는 생략 시 cwd 의 git repo 이름으로 유추. actor 는 `--actor` >
`ROCKY_TODO_ACTOR` > 호스트 자동 감지 (claude-code / opencode / codex).

`show REF` 출력에는 링크·히스토리와 함께 `댓글:` 섹션(작성 시각 + actor + 본문)이 붙는다 —
히스토리 목록에서는 댓글 계열 항목을 걸러 중복을 없앤다. **댓글 편집·보관 CLI 명령은
없다** — REST(`/api/todos/:ref/comments/...`)로만 한다.

`rocky-12` 나 맨숫자 `12` 는 셸에서 그대로 쓸 수 있다: `rocky show rocky-12`. 옛 표기
(`#12` 등)처럼 `#` 로 시작하는 REF 는 bash/zsh 에서 주석 시작 문자로 해석되므로 따옴표로
감싼다: `rocky show '#12'`.

## 사용 로그 — 무엇이 쓰이나 (`rocky usage`)

rocky 의 표면이 실제로 얼마나 쓰이는지를 **상시** 남긴다. v0.23 에 도구 12개를 걷어낼 때는
39개 레포의 워크로그를 손으로 뒤져 "0건" 을 셌는데, 그 셈을 명령 하나로 만든 것이다.

- **무엇을**: 데몬 REST 라우트(웹·TUI·CLI 가 다 지나간다) · MCP 도구(보드 5 + worklog 4) ·
  `rocky <cmd>` · 훅 4개 · 웹 UI 의 이름 붙인 이벤트(`web:now-row` 등). 한 줄 = 이름 · 누가
  (`x-rocky-actor`) · 클라이언트(`x-rocky-client`: web/tui/cli) · 성공 여부 · 걸린 시간.
  **내용은 싣지 않는다** — 제목·본문·id 없이 `GET /api/todos/:ref` 처럼 모양만.
- **어디에**: `~/.config/rocky/usage/YYYY-MM.jsonl`(월별 append-only). 데몬은 전용 스레드로,
  CLI·훅·worklog MCP 는 자기가 직접 쓴다(데몬을 안 거치므로). 1초마다 도는 statusline 과
  SSE·health 는 기록하지 않는다.
- **읽기**: `rocky usage [--since 30d] [--json]` — 많이 쓴 표면(에러 수·p50/p95), **알려진
  표면 중 한 번도 안 쓰인 것**, 날짜·시각 분포, 클라이언트 비율. 데몬 없이 파일만 읽는다.
- **끄기**: `rocky.json` `usage.enabled: false` 또는 env `ROCKY_USAGE=0`. 위치는 `usage.dir` /
  `ROCKY_USAGE_DIR`.
- **개선 루프**: 표면을 빼거나 바꾸는 PR 은 이 수치를 인용한다(AGENTS.md). 판단은 사람이 —
  로그가 자동으로 무엇을 끄지는 않는다.

## 설정

`rocky.json` (user 레벨 권장 — 데몬은 project rocky.json 을 보지 않는다). **`enabled` 필드는
없다** (설치=활성화):

```json
{ "todo": { "port": 8636, "dir": "~/.config/rocky/todo", "inbox": [], "sessionSummary": true } }
```

`inbox` 는 위 "수집함" 절, `sessionSummary` 는 "요약" 절.

**손으로 만들 필요는 없다.** `rocky config show` 가 설정 파일·설치본·데몬·launchd·세션 요약·
노출·수집함·statusline 연결·보드 ↔ 레포 경로를 한 번에 점검해 `다음 할 일` 을 내고(`--json`),
`rocky config init` 이 기본 파일(expose off · sessionSummary on)을 없을 때만 만든다. Claude Code
에서는 `/rocky:config` 가 그 결과를 보고 빠진 항목을 하나씩 물어 채운다(`/rocky:config expose off`
처럼 값 변경도). `settings.json` 의 statusLine 은 덮어쓰지 않는다 — 조각을 붙일지 묻는다.

| env | 의미 |
| --- | --- |
| `ROCKY_TODO_PORT` | 데몬 포트 (기본 8636 — 키패드 "todo") |
| `ROCKY_TODO_DIR` | 데이터 디렉터리 (todo.db / daemon.pid / daemon.log / hook-cursors.json) |
| `ROCKY_TODO_ACTOR` | CLI actor 이름 강제 |
| `ROCKY_TODO_WATCH` | 보드 변경 주입 훅 on/off (기본 on) |
| `ROCKY_TODO_EXPOSE` | 노출 채널 강제 (`lan,tailscale-serve` / `off`) — 설정 시 config 무시 |
| `ROCKY_TODO_STATUSLINE` | statusline 템플릿 강제 (아래 "statusline 에 얹기") |
| `ROCKY_CONFIG` | user rocky.json 경로 override (기본 `~/.config/rocky/rocky.json`) |
| `ROCKY_TODO_UI_DIST` | 데몬이 `/` 에 서빙할 정적 디렉터리(선택 — 기본 없음. 웹 UI 를 다시 붙일 때 쓴다) |

## statusline 에 얹기

보드를 보려고 브라우저 창이나 터미널 pane 을 따로 띄우는 대신, 이미 떠 있는 Claude Code
statusline 에 세그먼트 하나로 붙인다. **보여줄 게 없으면 아무것도 출력하지 않는다.**

`GET /api/statusline?cwd=<경로>&session=<세션 id>` 가 완성된 한 줄을 `text/plain` 으로
돌려준다 — 렌더까지 데몬이 하므로 소비자 쪽은 `curl` 한 줄이면 된다. 이 자리는 1초마다 ×
열어둔 세션 수만큼 도는 곳이라, 여기서 프로세스를 하나 더 띄우지 않는 것이 설계 목적이다.

`~/.claude/statusline-command.sh` 끝에 (또는 `settings.json` 의 `statusLine.command` 에)
이어 붙인다 — 입력 JSON 에서 두 값을 꺼내 쓴다:

```sh
cwd=$(echo "$input" | jq -r '.workspace.current_dir // empty')
sid=$(echo "$input" | jq -r '.session_id // empty')
rt=$(curl -sf --max-time 0.3 "http://127.0.0.1:8636/api/statusline?cwd=$cwd&session=$sid")
[ -n "$rt" ] && printf '%s\n' "$rt"
```

(앞선 줄이 개행으로 끝난다는 전제다 — 보통 `printf '...\n'` 로 끝나므로 여기서 `\n` 을
앞에 또 붙이면 빈 줄이 하나 생긴다.)

`-f` 를 빼지 마라. 데몬이 안 떠 있으면 `curl` 이 빈 값을 내지만, **이 라우트가 없는 구버전
데몬**은 404 와 함께 JSON 에러 본문을 낸다 — `-f` 가 없으면 그 JSON 이 그대로 statusline 에
찍힌다. `-f` 는 비 2xx 응답을 무출력으로 만들어 두 경우를 같게 만든다 (fail-open —
statusline 이 보드 때문에 깨지지 않는다).

**환경 전제 셋** — 새 머신에 붙일 때 걸리는 것들이다:

- **Claude Code 전용.** `statusLine` 자체가 Claude Code 기능이고, 기본 템플릿이 쓰는
  `{mine.*}`/`{inbox}`/`{stale}` 은 세션 판정(`claude agents --json`)에 의존한다 —
  opencode/Codex 에서는 전부 비어 `{doing}` 만 남는다.
- **포트를 바꿔 썼으면** (`todo.port`) URL 의 포트도 같이 바꾼다. 안 그러면 조용히 무출력이다.
- **`jq` 가 필요하다.** statusline 입력은 stdin JSON 이라 파서 없이는 값을 못 꺼낸다.

배선은 머신마다 수동이다 — 플러그인은 사용자의 statusline 스크립트를 건드리지 않는다.

### 템플릿

`rocky.json` 의 `todo.statusline.template` 로 바꾼다. 기본값:

```
[⏺ {mine.ref} {mine.title}][ 💬{mine.comments}][  ✉{inbox}][  ⚠{stale}]
```

문법은 둘뿐이다.

- `{name}` — 값으로 치환. 모르는 이름은 그대로 남는다(오타가 눈에 보이라고).
- `[...]` — 옵셔널 그룹. 안의 placeholder 가 **전부** 비면 그룹이 통째로 사라진다.
  숫자 `0` 은 "빈 값"이다. placeholder 가 없는 그룹은 순수 장식이라 늘 남는다.

| placeholder | 뜻 |
| --- | --- |
| `{mine.ref}` / `{mine.title}` | **이 세션이** `doing` 으로 잡은 항목 (제목은 30자에서 절단) |
| `{mine.comments}` | 그 항목의 댓글 수 — 사람이 댓글을 달면 다음 갱신에 숫자가 올라간다 |
| `{inbox}` | 이 세션 앞으로 대기 중인 핸드오프 수 |
| `{stale}` | 이 보드에서 방치된 `doing` 수 (세션이 사라졌거나 턴이 끝났는데 완료가 없다) |
| `{doing}` | 이 보드의 전체 `doing` 수 |

`{mine.*}` 은 핸드오프로 시작된 작업에만 붙는다 — 세션 귀속(`doing_session_id`)이 생기는
유일한 경로라서다. 보드 판정은 `cwd` 로 하며 워크트리도 원본 보드로 모인다.

색을 넣으려면 템플릿에 ANSI 이스케이프를 직접 쓴다(JSON 문자열이라 `\u001b` 가 그대로
들어간다). 이스케이프 안의 `[` 는 그룹 문법으로 읽지 않는다:

```json
{ "todo": { "statusline": { "template": "[\u001b[33m⏺ {mine.ref}\u001b[0m {mine.title}][  ⚠{stale}]" } } }
```
