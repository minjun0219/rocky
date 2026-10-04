# Rocky

[![CI](https://github.com/minjun0219/rocky/actions/workflows/ci.yml/badge.svg)](https://github.com/minjun0219/rocky/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](./LICENSE)
[![Built with Rust](https://img.shields.io/badge/Built%20with-Rust-black)](https://www.rust-lang.org)

개인용 에이전트 도구다. 본체는 **Rust 상주 데몬(공유 todo 보드와 MCP)과 CLI**이고, 그 위에 얇은 Claude Code 플러그인이 워크로그(기록과 정리)와 PR 워크플로 커맨드를 얹는다. 이름은 *Project Hail Mary*의 Rocky에서 따왔다. 2026-09에 별도 레포였던 rocky-todo를 흡수했다. 화면은 브라우저용 웹 UI 하나이고, 데몬이 `http://127.0.0.1:8636/`에서 서빙한다.

> **v0.23에서 걷어낸 것**: `openapi_*` 7종, `seo_validate`, `notion_*` 4종과 단독 CLI `openapi-mcp`. 39개 레포 5,216턴의 워크로그를 세어 보니 호출이 0회였다. 전부 git 히스토리에 있으니 필요해지면 거기서 꺼낸다.

> **공개에 관하여**: 이 저장소는 소유자가 혼자 쓰려고 만든 개인 플러그인이다. 누구나 참고·포크·설치할 수 있게 MIT로 공개하지만 범용 제품은 아니다. 표면과 규칙은 소유자의 워크플로에 맞춰 바뀐다. 구조와 패턴(단일 패키지 MCP 서버, CLI 위임, 기록과 정리의 분리 등)을 참고 자료로 보기를 권한다.

## 한눈에

MCP 서버는 둘이다. 데몬의 streamable HTTP(`127.0.0.1:8636/mcp`, 보드 도구 5개)와 CLI의 stdio 서버(`rocky mcp worklog`, 워크로그 도구 4개)다. 워크로그는 프로젝트별이라 세션의 cwd를 아는 CLI가 연다.

stdio 서버는 **rocky 채널**도 겸해, 데몬이 본 PR 전이(머지 후보·충돌)를 세션에 알린다. 채널 알림은 `claude --dangerously-load-development-channels plugin:rocky@rocky-marketplace`로 띄운 세션만 받는다([`docs/board.md`](./docs/board.md) "PR 감시").

Claude Code 플러그인은 `.claude-plugin/plugin.json`의 `mcpServers`로 두 서버를 붙이고, Codex와 opencode는 직접 등록해서 쓴다. 보드 데몬의 설치·CLI·설정·핸드오프는 [`docs/board.md`](./docs/board.md)에 있다.

### MCP 도구 표면

| 도구군 | 개수 | 하는 일 | 등록 조건 |
| --- | --- | --- | --- |
| `todo_*` / `note_*` | 5 | 공유 todo와 스크래치패드 보드: `todo_list` / `todo_write` / `todo_status` / `note_list` / `note_write`. Rust 데몬(`crates/rockyd`)의 `/mcp`. 삭제는 없고(보관만), 모든 변경을 히스토리에 남긴다. | 데몬 기동 시 |
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
- **업데이트**: `rocky update`가 마켓플레이스 갱신 → 플러그인 → 데몬 교체를 한 번에 한다(`--check`는 버전 비교만). statusline에는 `rocky statusline` 한 줄로 끼운다([`docs/board.md`](./docs/board.md) "statusline 에 얹기").
- **수집함 CLI**: `rocky inbox [--json]`이 소스별 항목을 보여 준다(✓는 이미 어느 보드든 올라간 것). `rocky today`와 세션 시작 요약은 아직 안 올린 항목을 최대 3개 싣는다. 세션에 "gh-bugs 구독해"라고 하면(`rocky inbox subscribe`) 그 뒤 새 항목을 데몬이 그 세션에 알린다. 착수는 사람이 정한다([`docs/board.md`](./docs/board.md) "요약").
- **훅** (`hooks/hooks.json`): `SessionStart`가 데몬을 띄우고(버전이 다르면 재기동), `UserPromptSubmit`이 사람이 보드에서 바꾼 것을 세션에 알리고, `Stop`이 핸드오프를 집은 뒤 턴을 워크로그에 자동으로 남긴다(`kind:"turn"`, LLM을 쓰지 않는다, `worklog.autoCapture`로 끈다). 모든 훅은 실패해도 세션을 막지 않는다.
- **스킬** (`skills/`): `board`는 보드 에티켓(start→done, 링크 첨부, 보관만)과 MCP·CLI 폴백을, `writing-cc-plugin`은 Claude Code 플러그인 작성 가이드와 매니페스트·컴포넌트·배포 레퍼런스를 담는다.
- **서브에이전트** (`agents/`): `reviewer`는 새 컨텍스트에서 **diff와 요구사항만** 받아 검토하는 읽기 전용 리뷰어다. `/rocky:review-request`가 위험한 변경일 때 요구사항 대비 점검으로 띄우고(버그 찾기는 기본 `/code-review`), "리뷰해줘"처럼 직접 부를 수도 있다. 돌려 본 것만 통과라고 쓰고, 통과처럼 보이는 실패(false pass)를 따로 챙기며, 파일을 고치거나 머지하지 않는다.

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

허용하는 top-level 키는 아래 넷뿐이다. 그 밖의 키는 바로 거부한다(오타 가드). 제거된 `openapi` / `seo`도 거부하니 옛 설정 파일에 남아 있으면 지운다. 정확한 모양은 [`rocky.schema.json`](./rocky.schema.json)과 `crates/rocky-core/src/config.rs`가 함께 정한다.

| 키 | 내용 |
| --- | --- |
| `worklog` | `dir`(env `ROCKY_WORKLOG_DIR` 우선) / `autoCapture`(기본 true) / `captureMaxChars`(기본 800) / `digestThreshold`(기본 40) |
| `usage` | 사용 로그. `dir`(기본 `~/.config/rocky/usage`) / `enabled`(기본 true). 표면별 호출을 월별 JSONL로 남기고 `rocky usage`로 읽는다. 내용은 싣지 않는다 |
| `pr` | PR 감시. `enabled`(기본 true) / `intervalMinutes`(기본 3) / `notify`(기본 true) / `sessionNotify`(기본 true) / `notifiers[]`(알림 브릿지, 예: `bridges/telegram/`). 데몬은 **구독한 PR만** 본다. 동작은 [`docs/board.md`](./docs/board.md) "PR 감시" |
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

## 문서 맵

이 README가 사람용 진입점이고, 더 깊은 문서는 아래가 전부다. 도구 하나하나의 입출력은 문서가 아니라 **도구 정의 자체**(`crates/*/src/*mcp*.rs`의 `#[tool]`)에 있다. 에이전트는 그걸 직접 읽는다.

| 문서 | 대상 | 내용 |
| --- | --- | --- |
| [`AGENTS.md`](./AGENTS.md) | 에이전트 | **단일 기준 문서**: 레이아웃·범위·코딩 규칙·변경 체크리스트·리뷰 기준 |
| [`docs/architecture.md`](./docs/architecture.md) | 에이전트 (영문) | 코드만 봐서는 알 수 없는 설계 근거. 필요할 때만 읽는 심화 레퍼런스 |
| [`docs/hosts.md`](./docs/hosts.md) | 사람 | 호스트 지원 매트릭스: 세 호스트의 확장 방식과 rocky 표면 커버 현황(실측) |
| [`docs/backlog.md`](./docs/backlog.md) | 사람 | 백로그: 보류 항목과 다시 넣을 후보 |
| [`docs/board.md`](./docs/board.md) | 사람 | 보드 데몬: 설치·기동·CLI·설정·핸드오프·세션 띄우기 |
| [`docs/rewrite/`](./docs/rewrite/) | 에이전트 | TS → Rust 포팅 기록: `contract.md`(외부 표면 계약, 정본) · `decisions.md` · `rust-notes.md` |
| [`docs/codex.md`](./docs/codex.md) / [`docs/opencode.md`](./docs/opencode.md) | 사람 | 다른 호스트에서 MCP 서버를 쓰고 싶을 때 |

## 역사 / 아카이브

v0.2까지의 journal / mysql / spec-pact / pr-watch 도메인과 에이전트·스킬은 [`archive/pre-openapi-only-slim`](https://github.com/minjun0219/rocky/tree/archive/pre-openapi-only-slim) 브랜치에 남겨 뒀다. 쓰임새가 잡히는 대로 [`docs/backlog.md`](./docs/backlog.md)의 후보 단위로 다시 넣는다. journal은 v0.6에 되살렸고, v0.9에서 이름을 `worklog`로 바꿨다. notion(v0.5에 되살림)과 openapi · seo는 실사용이 0회라 v0.23에서 다시 걷어냈다. 예전 네이티브 opencode 플러그인은 `.archive/`에 두었다가 지웠으니 필요하면 git 히스토리에서 꺼낸다. 지금의 opencode 지원은 그 플러그인을 되살린 것이 아니라 stdio MCP를 등록하는 방식이다.

> 세 호스트에서 rocky 표면이 어디까지 커버되는지는 [`docs/hosts.md`](./docs/hosts.md)에 있다.

## 개발

필요한 것: [Bun](https://bun.sh)(개발 도구·웹 UI 빌드), [Rust](https://www.rust-lang.org)(`rust-toolchain.toml`의 stable).

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
