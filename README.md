# Rocky

[![CI](https://github.com/minjun0219/rocky/actions/workflows/ci.yml/badge.svg)](https://github.com/minjun0219/rocky/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](./LICENSE)
[![Built with Rust](https://img.shields.io/badge/Built%20with-Rust-black)](https://www.rust-lang.org)

에이전트를 부리는 데 필요한 기능을 한곳에 모아 두는 개인 툴킷이다. 작업 보드와 하네스에서 시작했고, 에이전트 운용에 쓰이는 기능이면 무엇이든 받는다. 다만 다른 곳에 있는 기능을 한꺼번에 옮겨 오지는 않고, 소유자가 하나씩 정해서 들인다. 지금 본체는 **Rust 상주 데몬(공유 todo 보드와 MCP)과 CLI**이고, 그 위에 얇은 Claude Code 플러그인이 워크로그(기록과 정리)와 PR 워크플로 커맨드를 얹는다. 이름은 *Project Hail Mary*의 Rocky에서 따왔다. 2026-09에 별도 레포였던 rocky-todo를 흡수했다. 화면은 브라우저용 웹 UI 하나이고, 데몬이 `http://127.0.0.1:8636/`에서 서빙한다.

> **v0.23에서 걷어낸 것**: `openapi_*` 7종, `seo_validate`, `notion_*` 4종과 단독 CLI `openapi-mcp`. 39개 레포 5,216턴의 워크로그를 세어 보니 호출이 0회였다. 전부 git 히스토리에 있으니 필요해지면 거기서 꺼낸다.

> **공개에 관하여**: 이 저장소는 소유자가 혼자 쓰려고 만든 개인 플러그인이다. 누구나 참고·포크·설치할 수 있게 MIT로 공개하지만 범용 제품은 아니다. 표면과 규칙은 소유자의 워크플로에 맞춰 바뀐다. 구조와 패턴(단일 패키지 MCP 서버, CLI 위임, 기록과 정리의 분리 등)을 참고 자료로 보기를 권한다.

## 한눈에

MCP 서버는 둘이다. 데몬의 streamable HTTP(`127.0.0.1:8636/mcp`, 보드 도구 5개 + 토큰 도구 2개)와 CLI의 stdio 서버(`rocky mcp worklog`, 워크로그 도구 4개)다. 워크로그는 프로젝트별이라 세션의 cwd를 아는 CLI가 연다. 작업 폴더 밖에서 서버를 띄우는 호스트(Antigravity)는 `rocky mcp worklog --roots`로 띄워 클라이언트가 알려 주는 MCP `roots`를 프로젝트로 쓴다.

stdio 서버는 **rocky 채널**도 겸해, 데몬이 본 PR 전이(머지 후보·충돌)를 그 PR을 구독한 세션에 알린다. 채널 알림은 `claude --dangerously-load-development-channels plugin:rocky@rocky-marketplace`로 띄운 세션만 받는다([`docs/board.md`](./docs/board.md) "PR 감시").

Claude Code 플러그인은 `.claude-plugin/plugin.json`의 `mcpServers`로 두 서버를 붙이고, Codex와 opencode는 직접 등록해서 쓴다. Antigravity(`agy`)는 레포의 `antigravity/` 번들을 `agy plugin install`로 깔아 쓴다. 보드 데몬의 설치·CLI·설정·핸드오프는 [`docs/board.md`](./docs/board.md)에 있다.

### MCP 도구 표면

| 도구군 | 개수 | 하는 일 | 등록 조건 |
| --- | --- | --- | --- |
| `todo_*` / `note_*` | 5 | 공유 todo와 스크래치패드 보드: `todo_list` / `todo_write` / `todo_status` / `note_list` / `note_write`. Rust 데몬(`crates/rockyd`)의 `/mcp`. 삭제는 없고(보관만), 모든 변경을 히스토리에 남긴다. | 데몬 기동 시 |
| `token_*` | 2 | Claude Code 토큰 사용 색인 읽기: `token_summary`(모델·effort·세션·브랜치별 합계) / `token_current_session`(cwd의 최근 세션 턴별 모델·effort·토큰 + 추천). 데몬의 `/mcp`. | 데몬 기동 시 |
| `worklog_*` | 4 | append-only 로컬 JSONL **기록(記錄)** 레이어. 결정·blocker·답변·메모를 턴을 넘겨 남긴다(`append` / `read` / `search` / `status`). 외부 의존 0. | 항상 |

각 도구의 입출력과 부수 효과는 별도 문서가 아니라 **도구 정의 자체**에 있다. `crates/rockyd/src/mcp.rs`(보드)와 `crates/rocky-cli/src/worklog_mcp.rs`(워크로그)의 `#[tool]` 정의를 읽으면 된다.

### Claude Code 전용 표면 (MCP 도구 아님)

아래는 Claude Code 플러그인으로 설치했을 때만 붙는다.

- **슬래시 커맨드** (`commands/`)
  - `/rocky:next`: 보드에서 다음 작업을 골라 착수한다.
  - `/rocky:brainstorm`: 아이디어를 설계로 다듬는다. 게이트가 아니라 필요할 때 부르는 도구다.
  - `/rocky:review-request`: 게이트 → 커밋 → 푸시 → PR 생성까지 한다.
  - `/rocky:review-fix`: PR에 붙은 리뷰를 반영한다. 스레드에는 리액션(🚀 수정 완료 / 👀 결정 필요)으로 상태만 남기고, 코멘트·resolve·머지는 사람이 한다.
  - `/rocky:config`: 설치·설정을 점검하고 빠진 것을 하나씩 물어 채운다.
  - `/rocky:usage`: 사용 로그를 읽고 뺄 것·손볼 것·더 쓸 것을 제안한다. 판단은 사람이 한다.
  - `/rocky:recall`: 워크로그를 다이제스트(`kind:"digest"`)로 증분 정리한다. 기록의 짝인 **정리(整理)** 레이어다.
- **웹 UI** (`http://127.0.0.1:8636/`): 피드·보드·항목 상세(마크다운·댓글·타임라인)·노트·작업로그·GitHub 탭. 보던 탭은 주소(`?view=`)에 남아 새로고침해도 유지된다. 늘 곁에 둘 노트는 `rocky note pin REF`나 웹의 📌로 고정한다. `web/`의 React 앱을 릴리스 때 `dist/`로 번들해 tarball에 넣고 데몬이 서빙한다. 자세한 내용은 [`docs/board.md`](./docs/board.md) "웹 UI".
- **업데이트**: `rocky update`가 마켓플레이스 갱신 → 플러그인 → 데몬 교체를 한 번에 한다(`--check`는 버전 비교만). statusline에는 `rocky statusline` 한 줄로 끼운다([`docs/board.md`](./docs/board.md) "statusline에 얹기"). `rocky statusline --full`은 cc-usage 와 같은 경로·git·모델·한도 줄까지 그린다.
- **수집함 CLI**: `rocky inbox [--json]`이 소스별 항목을 보여 준다(✓는 이미 어느 보드든 올라간 것). `rocky today`와 세션 시작 요약은 아직 안 올린 항목을 최대 3개 싣는다. 세션에 "gh-bugs 구독해"라고 하면(`rocky inbox subscribe`) 그 뒤 새 항목을 데몬이 그 세션에 알린다. 착수는 사람이 정한다([`docs/board.md`](./docs/board.md) "요약").
- **훅** (`hooks/hooks.json`): `SessionStart`가 데몬을 띄우고(버전이 다르면 재기동), `UserPromptSubmit`이 사람이 보드에서 바꾼 것을 세션에 알리고, `Stop`이 핸드오프를 집은 뒤 턴을 워크로그에 자동으로 남긴다(`kind:"turn"`, LLM을 쓰지 않는다, `worklog.autoCapture`로 끈다). 모든 훅은 실패해도 세션을 막지 않는다.
- **스킬** (`skills/`): rocky 를 **쓰는** 에이전트가 상황에 맞을 때 읽는 문서다(평소엔 컨텍스트를 쓰지 않는다). `pull-request`는 PR 구독과 받은편지함 메시지별 할 일, `token-usage`는 모델·effort 고르기, `branch-verify`는 기본 브랜치 검증 결과 읽기, `handoff`는 보드에서 넘겨받은 일의 start→done, `worklog`는 무엇을 언제 기록하나를 다룬다. rocky 를 **고치는** 에이전트용 기능 문서는 레포의 `docs/features/`에 따로 있다. `board`는 보드 에티켓(start→done, 링크 첨부, 보관만)과 MCP·CLI 폴백을, `writing-cc-plugin`은 Claude Code 플러그인 작성 가이드와 매니페스트·컴포넌트·배포 레퍼런스를 담는다.
- **서브에이전트** (`agents/`): `reviewer`는 새 컨텍스트에서 **diff와 요구사항만** 받아 검토하는 읽기 전용 리뷰어다. `/rocky:review-request`가 위험한 변경일 때 요구사항 대비 점검으로 띄우고(버그 찾기는 기본 `/code-review`), "리뷰해줘"처럼 직접 부를 수도 있다. 돌려 본 것만 통과라고 쓰고, 통과처럼 보이는 실패(false pass)를 따로 챙기며, 파일을 고치거나 머지하지 않는다.
  `quick-fix`(Sonnet)와 `merge-cleanup`(Haiku)은 `/rocky:review-fix`의 기계적인 단계 — 충돌 해소·판단이 필요 없는 리뷰 수정·CI 실패, 머지 뒤 정리 — 를 가벼운 모델과 **새 맥락**에서 맡는다. 긴 세션이 몇 줄짜리 손질을 위해 대화 전체를 요청마다 다시 읽지 않게 하려는 것이다. 둘 다 강제 푸시·`-D`·코멘트·resolve·머지를 하지 않고, 판단이 필요한 건은 메인 세션에 돌려준다.

### 토큰 사용 — 모델·effort 고르기

데몬이 Claude Code 세션 트랜스크립트(`~/.claude/projects/**/*.jsonl`)를 1분마다 읽어 응답마다의 모델·effort·토큰·도구 호출을 `logs.db`에 쌓는다. 첫 바퀴가 과거 트랜스크립트 가져오기를 겸하므로 따로 가져올 명령이 없다. **훅 설정은 필요 없다.** 토큰은 훅 입력에 아예 없고 모델은 `SessionStart`에만 실리지만, 트랜스크립트에는 줄마다 둘 다 있다. 그래서 세션 쪽에 붙는 것이 없고, 데몬이 꺼져 있어도 Claude Code는 늦어지지 않는다.

```bash
rocky tokens                       # 최근 30일, 모델 × effort 별 세션·턴·요청·턴당 출력·토큰·도구 호출
rocky tokens --since 7d --by branch   # model · effort · session · branch 로도 묶는다
rocky tokens here                  # 이 디렉터리의 최근 세션 — 턴별 모델·effort·토큰, effort 변경, 추천
```

| API | 내용 |
| --- | --- |
| `GET /api/tokens/summary?groupBy=&from=&to=&days=` | 합계. `groupBy`는 `model,effort`(기본) · `model` · `effort` · `session` · `branch`, 구간은 `from`/`to`(ISO) 또는 `days`(기본 30) |
| `GET /api/tokens/sessions/:id?limit=` | 세션 머리 + 최근 턴(기본 50) + effort 변경 지점 |
| `GET /api/tokens/current?cwd=` | 그 디렉터리(또는 그 아래)에서 가장 최근에 움직인 세션의 상세 + 추천 |
| `GET /api/tokens/recommendation?sessionId=\|cwd=` | 추천과 근거 수치(턴 수·턴 평균 출력·턴 평균 캐시 읽기·요청당 캐시 읽기·도구 호출·모델·effort) |
| `GET /api/tokens/events` | SSE. 낸 규칙이 바뀐 세션마다 `event: tokens.recommendation` |

추천은 규칙 네 가지다(최근 15턴, 5턴 미만이면 판단하지 않음). ① 턴 평균 출력이 3,000토큰 이하인데 effort가 xhigh/max면 medium을 권한다. ② 창 안에서 effort를 올린 뒤 턴이 길어졌으면 이미 조정한 것으로 보고 추천하지 않는다. ③ Opus인데 도구 호출이 0이고 출력이 짧으면 Sonnet medium을 권한다. ④ 맥락(요청당 캐시 읽기)이 20만 토큰 이상인데 턴 평균 출력이 1만 토큰 이하면 — 긴 맥락을 요청마다 다시 읽으며 조금씩 쓰는 중이다 — 기계적인 후속(리뷰 반영·머지 뒤 정리)은 새 세션이나 가벼운 서브에이전트로 넘기라고 권한다. 긴 세션의 비용은 대부분 출력이 아니라 이 캐시 읽기다. 모델 전환은 다음 작업 경계(커밋 직후)나 새 세션에서 하라고 권한다 — 모델을 바꾸면 쌓인 캐시를 못 쓰고 새로 쌓는다(effort 변경은 캐시를 지킨다). 수치와 규칙 on/off는 `rocky.json`의 `tokens.recommend`로 바꾼다.

### 기본 브랜치 검증

여러 워크트리에서 PR을 연달아 머지하면 "머지된 `main`이 여전히 초록인가"를 누군가 다시 봐야 한다. 데몬이 그 일을 한다 — `rocky.json`의 `verify.targets[]`에 보드와 단계를 적으면, 원격 브랜치를 `git ls-remote`로 보다가(GitHub API 예산을 쓰지 않는다) 새 커밋이 들어오면 전용 워크트리(`<todo dir>/verify/<board>/<branch>/tree`, detached — 사람·세션의 작업 트리와 브랜치를 건드리지 않는다)에서 단계를 차례로 돈다. 한 번에 하나만 돌고, 도는 사이 커밋이 몰리면 최신 하나만 본다. 실패와 복구(실패 → 통과)만 macOS 배너로 알린다.

```json
"verify": { "targets": [ { "board": "rocky", "steps": [
  { "name": "install", "command": ["bun", "install", "--frozen-lockfile"] },
  { "name": "check", "command": ["bun", "run", "check"] },
  { "name": "cargo-test", "command": ["cargo", "test", "--workspace"] }
] } ] }
```

`rocky verify`(또는 `GET /api/verify`)가 대상마다 마지막 결과를 보여 준다 — 실패면 단계·이유·로그 파일. 명령은 설정 파일에만 있다(화면·REST가 바꾸지 않는다). 데몬이 빌드·테스트를 돌리므로 머신이 그만큼 바빠지고, 전용 워크트리의 빌드 산출물(`target/`·`node_modules/`)이 `<todo dir>/verify/` 아래에 남는다(대상마다 로그는 최근 10개만). 보드 레포의 git 훅은 돌리지 않는다.

실패하면 그 자리에서 한 번 더 돌고, 두 번 연속 실패일 때만 실패로 기록하고 알린다 — 진짜 실패는 그만큼 늦게 알게 되고, 두 번째에 통과하면 `rocky verify`가 "통과(다시 돌려서)"로 보여 준다. 그 뒤로 같은 커밋의 실패는 다시 돌지 않는다. 머신이 바빠 테스트가 시간 초과로 떨어진 것처럼 커밋 탓이 아닌 실패라면 `rocky verify --rerun [보드]`(또는 로컬에서 `POST /api/verify/rerun`, 본문 `board`·`branch`)로 지금 커밋을 바로 다시 돌린다 — 보드를 안 주면 대상 전부, 도는 중인 대상은 건너뛴다. 실행 이력은 대상 디렉터리의 `runs.jsonl`에 쌓인다 — 끝난 실행(통과·실패 — 자동 재시도 전의 첫 실패도, 두 번째 시도면 `attempt: 2`, 다시 돌린 것이면 `rerun`)과 끊긴 실행, 검증을 못 한 이유(원격을 못 읽음·준비 실패, 같은 이유가 이어지면 한 번만)를 한 줄씩.

### rc 서버 현황

사용자 설정(`~/.config/rocky/rocky.json`)의 `rc` 블록에 폴더를 적으면 데몬이 폴더마다 떠 있는 `claude rc`(Remote Control) 서버를 그 목록과 맞대어 보여 주고, 사람이 부르면 띄우거나 다시 띄운다. 스스로 되살리는 감시는 다음 단계다([설계](./docs/design/specs/2026-10-05-rc-server-design.md)).

웹 UI에서는 피드 머리 아래 한 줄(`원격 제어 7/13 · 세션 4`)과 **원격 제어** 탭으로 본다(⋯ 메뉴에서 탭을 끌 수 있다). rc 블록이 없거나 `enabled: false`면 둘 다 없다.

**Antigravity 원격 제어**(`agy remote-control`)는 rc 블록과 상관없이 그 기기에 `agy`가 있으면 보이고, 켜고 끌 수 있다 — rc 블록이 없는 기기에서는 원격 제어 탭에 Antigravity 줄만 나온다. 켜기·끄기는 이 기계의 원격 접속 데몬을 바꾸므로 로컬 요청만 된다(폰·테일넷 화면에는 버튼이 없다).

```bash
rocky rc                         # 대상별 ●/○ · 고정 · 열린 세션 수 · 떠 있은 시간, 대상 밖 서버, 자격, Antigravity
rocky rc start repo-a --wait     # 꺼진 대상을 띄운다(세션까지)
rocky rc restart repo-a --wait   # 다시 띄운다 — 열린 세션이 있으면 이어받기(-c), --fresh 면 새로. 붙은 원격 세션은 끊긴다
rocky rc agy                     # Antigravity 원격 제어 상태 한 줄
rocky rc agy stop                # agy remote-control stop(정지 + 등록 해제) — start 는 등록 + 기동
```

서버는 데몬과 다른 프로세스 그룹으로 띄워 데몬을 재시작·업데이트해도 살아 있다. 재시작은 SIGTERM 뒤 20초를 기다리고, `already served`(claude.ai 쪽 등록이 남음)면 45초·90초 뒤 다시 띄운다. 기동 로그와 이벤트는 todo 폴더의 `rc/`(`<라벨>.out` · `.err` · `events.jsonl`)에 남는다.

| API | 내용 |
| --- | --- |
| `POST /api/rc/servers/:label/start` · `/restart` | 로컬 전용(프로세스를 띄운다). 바로 202, 진행은 현황 행의 `action`(`starting`·`restarting`·`retrying`)과 `lastResult` 로 본다. `restart` 본문 `{"fresh": true}` 는 이어받지 않는다 |
| `GET /api/rc/servers` | 대상 행(`servers`), 목록에 없는 폴더에서 도는 서버(`strays`), `claude auth status` 결과(`auth`), `agy remote-control status`(`antigravity`, `agy`가 없으면 `null` — rc 블록이 없어도 잰다). `ps`·`lsof`가 실패하면 `probeError`에 사유가 실리고, 그때 꺼짐은 "모름"이다. 5초 캐시 |
| `POST /api/rc/antigravity/start` · `/stop` | `agy remote-control start`·`stop`을 돌리고 새로 잰 현황을 돌려준다(캐시도 바뀐다). 로컬 전용(403), 명령이 실패하면 502와 종료 코드·stderr |

> **작업 목록은 보드 하나다.** rocky는 외부 태스크 서비스와 동기화하지 않는다. 작업 목록은 데몬의 보드(`todo_*`), 작업 기록은 `worklog_*`다. 외부 앱(Todoist 등)은 수집함 어댑터(`bridges/`)로 읽기만 한다.

> **v0.19에서 걷어낸 것**: 소울(페르소나) 주입과 `SessionStart` 훅, statusline 템플릿 3종과 동기화 훅, opencode 위임 런타임, `/rocky:codex` · `/rocky:issue` · `/rocky:opencode` · `/rocky:opencode-jobs` 커맨드. 재미로 넣었거나 실사용이 없던 것들이라 정리했다. 전부 git 히스토리에서 꺼낼 수 있다. `rocky.json`의 `soul` / `callsign` / `opencode` 키도 함께 없어져 이제 거부되니, 예전 설정 파일에 남아 있으면 지운다.

## 시작하기

플러그인을 쓰는 데는 [Claude Code](https://claude.com/claude-code)만 있으면 된다. 첫 세션이 릴리스 바이너리를 받는다. 릴리스 바이너리는 지금 macOS(Apple Silicon)용만 있고, 다른 환경은 소스에서 빌드한다(아래 "개발").

### Claude Code 플러그인

이 저장소 자체가 플러그인 소스이자 마켓플레이스다(`.claude-plugin/marketplace.json`, 별도 파사드 없음). 설치는 GitHub 소스로 한다.

```bash
claude plugin marketplace add minjun0219/rocky
claude plugin install rocky@rocky-marketplace
```

설치 후 첫 세션이 열리면 `SessionStart` 훅이 릴리스 바이너리를 데이터 홈(`$XDG_DATA_HOME`, 기본 `~/.local/share`)의 `rocky/v<버전>/`에 받고 `~/.local/bin/rocky` 링크를 건다. `~/.local/bin`이 PATH에 있으면 터미널에서 `rocky`를 바로 쓸 수 있다(없으면 셸 rc에 `export PATH="$HOME/.local/bin:$PATH"`). 상태는 `rocky config show`의 `cli` 행에서 본다. 세션을 열지 않고 지금 링크를 걸려면 `"${XDG_DATA_HOME:-$HOME/.local/share}/rocky/current/rocky" config link`를 실행한다.

원격 세션 안에서는 `/plugin` 슬래시 커맨드로 똑같이 설치한다. 설치본은 GitHub `main`에서 clone하므로, 코드 변경은 push한 뒤 `claude plugin update rocky@rocky-marketplace`로 반영한다.

플러그인 소스는 `plugin/` 디렉터리다(마켓플레이스 `source: "./plugin"`). 그래서 설치본에는 `crates/`·`target/`·`node_modules`가 복사되지 않는다. 설치본이 쓰는 MCP 서버는 `plugin/.claude-plugin/plugin.json`의 `mcpServers` 둘뿐이다. 저장소에 `.mcp.json`을 두지 않는 이유는 그 파일이 설치본의 MCP 설정으로 새기 때문이다.

설치한 뒤로는 손댈 것이 없다. 매 턴이 `Stop` 훅으로 워크로그에 쌓이고, 쌓인 것은 `/rocky:recall`로 정리한다.

## 설정

`rocky.json`(project `./rocky.json` > user `~/.config/rocky/rocky.json`)을 읽는다. [JSON Schema](./rocky.schema.json)로 IDE 자동완성을 지원한다.

```json
{
  "$schema": "https://raw.githubusercontent.com/minjun0219/rocky/main/rocky.schema.json",
  "worklog": { "captureMaxChars": 600 }
}
```

허용하는 top-level 키는 아래 여섯뿐이다. 그 밖의 키는 바로 거부한다(오타 가드). 제거된 `openapi` / `seo`도 거부하니 옛 설정 파일에 남아 있으면 지운다. 정확한 모양은 [`rocky.schema.json`](./rocky.schema.json)과 `crates/rocky-core/src/config.rs`가 함께 정한다.

| 키 | 내용 |
| --- | --- |
| `worklog` | `dir`(env `ROCKY_WORKLOG_DIR` 우선) / `autoCapture`(기본 true) / `captureMaxChars`(기본 800) / `digestThreshold`(기본 40) |
| `usage` | 사용 로그. `dir`(기본 `~/.config/rocky/usage`) / `enabled`(기본 true). 표면별 호출을 월별 JSONL로 남기고 `rocky usage`로 읽는다. 내용은 싣지 않는다 |
| `pr` | PR 감시. `enabled`(기본 true) / `intervalMinutes`(기본 3) / `notify`(기본 true) / `sessionNotify`(기본 true) / `notifiers[]`(알림 브릿지, 예: `bridges/telegram/`). 데몬은 **구독한 PR만** 본다. 동작은 [`docs/board.md`](./docs/board.md) "PR 감시" |
| `rc` | `claude rc` 서버 현황 — **사용자 설정(`~/.config/rocky/rocky.json`)에서만 읽는다**(프로젝트 `./rocky.json` 의 `rc` 는 무시 — 데몬은 전역 하나다). `enabled`(기본 true — rc 를 못 쓰는 기기에선 false) / `root`(기본 `~/dev/workspaces`, 상대 경로면 홈 기준) / `pinned`(늘 떠 있어야 하는 폴더) / `targets`(부를 수 있는 폴더). 블록이 없거나 꺼 두면 프로브·화면 모두 없다 |
| `statusline` | `rocky statusline --full`(경로·git·모델·ctx·5h/7d 줄 — cc-usage 와 같은 출력)의 한도 설정. `source`(`auto` 기본 / `stdin` / `api` / `none`) / `alertPercent`(기본 90, `0` 이면 임박 경고 끔). usage API 조회는 아직 없어 `api`·`auto` 는 stdin 에 한도가 없으면 조회 대기로 보인다. 보드 줄 템플릿(`todo.statusline`)과는 다른 자리다 |
| `tokens` | Claude Code 토큰 색인. `enabled`(기본 true) / `dir`(트랜스크립트 루트, 기본 `$CLAUDE_CONFIG_DIR/projects` → `~/.claude/projects`) / `recommend`(`window` 15 · `minTurns` 5 · `lowOutputTokens` 3000 · `lowerEffort` · `holdAfterRaise` · `switchToSonnet` · `freshSession` · `heavyContextTokens` 200000 · `freshSessionOutputTokens` 10000) |
| `verify` | 기본 브랜치 검증(opt-in). `targets[]` — `board`(그 보드 `path`가 레포) / `branch`(기본 `main`) / `steps[]`(`name` · `command` argv · `timeoutMs` 기본 30분) — 와 `intervalSeconds`(기본 60). 아래 "기본 브랜치 검증" |
| `todo` | 보드 데몬 설정. `port` / `dir` / `expose` / `watch` / `statusline` / `inbox` / `inboxAdapters` / `sessionSummary`. Rust 데몬(`crates/`)이 읽는다. 자세한 모양은 [`docs/board.md`](./docs/board.md) |

### 환경 변수

`ROCKY_WORKLOG_AUTO_CAPTURE`는 서버가 아니라 `Stop` 훅이 읽으므로 Claude Code에서만 쓰인다.

| 변수 | 기본값 | 영향 |
| --- | --- | --- |
| `ROCKY_CONFIG` | `~/.config/rocky/rocky.json` | user-level `rocky.json` 경로 override |
| `ROCKY_LAUNCHD_LABEL` | `com.rocky.daemon` | 개발용 launchd job 라벨 override. 실제 상주 job을 건드리지 않고 launchd 동작을 재현할 때 쓴다. 전용 `ROCKY_CONFIG`(다른 포트·dir)가 없으면 무시하고, 설정이 기본 포트면 `rocky daemon`이 거부한다 |
| `ROCKY_WORKLOG_DIR` | `~/.config/rocky/worklog/<project-key>` | 워크로그 JSONL 위치. `worklog.dir`보다 우선 |
| `ROCKY_WORKLOG_AUTO_CAPTURE` | `1` | `Stop` 훅 턴 자동 기록 on/off. `0`/`false`/`off`/`no`만 끈다 |
| `ROCKY_USAGE` | `1` | 사용 로그 on/off. `0`/`false`/`off`/`no`만 끈다 |
| `ROCKY_USAGE_DIR` | `~/.config/rocky/usage` | 사용 로그 JSONL 위치. `usage.dir`보다 우선 |
| `CLAUDE_CONFIG_DIR` | `~/.claude` | 데몬이 토큰 색인할 트랜스크립트 루트(`<값>/projects`). `tokens.dir`가 있으면 그쪽이 우선 |

## 문서 맵

이 README가 사람용 진입점이고, 더 깊은 문서는 아래가 전부다. 도구 하나하나의 입출력은 문서가 아니라 **도구 정의 자체**(`crates/*/src/*mcp*.rs`의 `#[tool]`)에 있다. 에이전트는 그걸 직접 읽는다.

| 문서 | 대상 | 내용 |
| --- | --- | --- |
| [`AGENTS.md`](./AGENTS.md) | 에이전트 | **단일 기준 문서**: 레이아웃·범위·코딩 규칙·변경 체크리스트·리뷰 기준 |
| [`docs/architecture.md`](./docs/architecture.md) | 에이전트 (영문) | 코드만 봐서는 알 수 없는 설계 근거. 필요할 때만 읽는 심화 레퍼런스 |
| [`docs/hosts.md`](./docs/hosts.md) | 사람 | 호스트 지원 매트릭스: 호스트별 확장 방식과 rocky 표면 커버 현황(실측) |
| [`docs/backlog.md`](./docs/backlog.md) | 사람 | 백로그: 보류 항목과 다시 넣을 후보 |
| [`docs/board.md`](./docs/board.md) | 사람 | 보드 데몬: 설치·기동·CLI·설정·핸드오프·세션 띄우기 |
| [`docs/rewrite/`](./docs/rewrite/) | 에이전트 | TS → Rust 포팅 기록: `contract.md`(외부 표면 계약, 정본) · `decisions.md` · `rust-notes.md` |
| [`docs/codex.md`](./docs/codex.md) / [`docs/opencode.md`](./docs/opencode.md) | 사람 | 다른 호스트에서 MCP 서버를 쓰고 싶을 때 |
| [`docs/antigravity.md`](./docs/antigravity.md) | 사람 | Antigravity에 작업(주로 디자인)을 넘길 때 — 번들 설치와 넘기기 흐름 |

## 역사 / 아카이브

v0.2까지의 journal / mysql / spec-pact / pr-watch 도메인과 에이전트·스킬은 [`archive/pre-openapi-only-slim`](https://github.com/minjun0219/rocky/tree/archive/pre-openapi-only-slim) 브랜치에 남겨 뒀다. 쓰임새가 잡히는 대로 [`docs/backlog.md`](./docs/backlog.md)의 후보 단위로 다시 넣는다. journal은 v0.6에 되살렸고, v0.9에서 이름을 `worklog`로 바꿨다. notion(v0.5에 되살림)과 openapi · seo는 실사용이 0회라 v0.23에서 다시 걷어냈다. 예전 네이티브 opencode 플러그인은 `.archive/`에 두었다가 지웠으니 필요하면 git 히스토리에서 꺼낸다. 지금의 opencode 지원은 그 플러그인을 되살린 것이 아니라 stdio MCP를 등록하는 방식이다.

> 호스트마다 rocky 표면이 어디까지 커버되는지는 [`docs/hosts.md`](./docs/hosts.md)에 있다.

## 개발

필요한 것: [Bun](https://bun.sh)(개발 도구·웹 UI 빌드), [Rust](https://www.rust-lang.org)(`rust-toolchain.toml`의 stable), E2E용 브라우저(설치된 Chrome, 없으면 `bunx playwright install chromium`).

```bash
bun install        # 개발 도구(biome · changesets · husky 훅 배선)와 브릿지 의존. 데몬·CLI에는 TS가 없다
bun run check      # Biome 검증
bun run typecheck  # tsc --noEmit
bun run test       # 스크립트·웹 UI 테스트. 맨 `bun test`는 DOM 테스트 준비가 빠져 실패한다
bun run e2e        # 웹 UI E2E(Playwright): 격리 데몬과 가짜 데이터로 폰·cmux·데스크톱 세 화면
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
cargo build --workspace   # target/debug/{rocky,rockyd}. ROCKY_BIN=target/debug/rocky로 부트스트랩을 우회한다
```

`.husky/pre-commit`(lint-staged와 시크릿 스캔)과 `.husky/pre-push`(typecheck · test · `cargo fmt` · `clippy`), CI([`ci.yml`](./.github/workflows/ci.yml))가 같은 게이트를 다시 실행한다. 전체 `cargo test`와 E2E는 CI에서만 실행한다. 기여 규칙과 레이아웃은 [`AGENTS.md`](./AGENTS.md)에 있다.

개발 중 외부 라이브러리 문서용 `context7` MCP는 유저 스코프에 둔다. 레포에 `.mcp.json`을 두면 설치본으로 새기 때문이다.

```bash
claude mcp add --scope user --transport http context7 https://mcp.context7.com/mcp
```

## 라이선스

[MIT](./LICENSE) © Minjun Kim
