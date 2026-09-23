# Rocky

[![CI](https://github.com/minjun0219/rocky/actions/workflows/ci.yml/badge.svg)](https://github.com/minjun0219/rocky/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](./LICENSE)
[![Runtime: Bun](https://img.shields.io/badge/Runtime-Bun%20%E2%89%A5%201.0-black)](https://bun.sh)

개인용 에이전트 도구 — **Rust 상주 데몬(공유 todo 보드 + MCP) + CLI** 가 본체이고, 그 위의 얇은 Claude Code 플러그인이 워크로그(기록↔정리)와 PR 워크플로 커맨드를 얹는다. 이름은 *Project Hail Mary* 의 Rocky 에서. 2026-09 에 별도 레포였던 rocky-todo 를 흡수했다(hail-mary D-046) — 웹 UI 는 그쪽 히스토리에 두고 오지 않았고, GUI 는 Swift 또는 TUI 로 뒤에 정한다.

> **v0.23 에서 걷어낸 것** — `openapi_*` 7종, `seo_validate`, `notion_*` 4종과 단독 CLI `openapi-mcp`. 39개 레포 5,216 턴의 워크로그를 세어 보니 호출이 0회였다. 전부 git 히스토리에 있으니 필요해지면 거기서 꺼낸다.

> **공개에 관하여** — 이 저장소는 소유자가 혼자 쓰려고 만든 개인 플러그인이다. 누구나 참고·포크·설치할 수 있도록 MIT로 공개하지만, 범용 제품이 아니라서 표면과 규칙이 소유자의 워크플로우에 맞춰 바뀐다. 구조와 패턴(단일 패키지 MCP 서버, CLI 위임, 기록↔정리 분리 등)을 참고 자료로 보는 쪽을 권한다.

## 한눈에

MCP 서버 둘 — 데몬의 streamable HTTP(`127.0.0.1:8636/mcp`, 보드 5 도구)와 CLI 의 stdio 서버(`rocky mcp worklog`, worklog 4 도구 — 프로젝트별이라 세션 cwd 를 아는 쪽이 연다). Claude Code plugin 은 `.claude-plugin/plugin.json` 의 `mcpServers` 로 둘 다 붙이고, Codex / opencode 는 직접 등록해서 쓴다. 보드 데몬의 설치·CLI·설정·핸드오프는 [`docs/board.md`](./docs/board.md).

### MCP 도구 표면

| 도구군 | 개수 | 하는 일 | 등록 조건 |
| --- | --- | --- | --- |
| `todo_*` / `note_*` | 5 | 공유 todo / 스크래치패드 보드 — `todo_list` / `todo_write` / `todo_status` / `note_list` / `note_write`. Rust 데몬(`crates/rockyd`)의 `/mcp`. 삭제 없음(아카이브만), 전 mutation 히스토리 기록. | 데몬 기동 시 |
| `worklog_*` | 4 | append-only 로컬 JSONL **기록(記錄)** 레이어 — 결정 / blocker / 답변 / 메모를 turn 을 넘겨 남긴다 (`append` / `read` / `search` / `status`). 외부 의존 0. | 항상 |

각 도구의 입출력과 side effect 는 별도 문서가 아니라 **도구 정의 자체**가 단일 소스다 — `crates/rockyd/src/mcp.rs`(보드) / `crates/rocky-cli/src/worklog_mcp.rs`(worklog) 의 `#[tool]` 정의를 읽으면 된다.

### Claude Code 전용 표면 (MCP tool 아님)

아래는 Claude Code plugin 으로 설치했을 때만 붙는다 (MCP tool 표면과 별개):

- **슬래시 커맨드** (`commands/`) — `/rocky:next` (보드에서 다음 작업 고르기 → start 표시 → 착수), `/rocky:brainstorm` (아이디어를 설계로 — 맥락 파악 → 한 번에 하나씩 질문 → 접근안 2~3개 → 설계; **게이트가 아니라 도구**), `/rocky:review` (완료 선언 전 신선한 컨텍스트 서브에이전트로 현재 작업 diff 셀프 리뷰 — PR 스레드 대응인 `/rocky:resolve-reviews` 과 별개), `/rocky:finish` (게이트 → 커밋 → 푸시 → PR 생성), `/rocky:resolve-reviews` (PR 에 붙은 리뷰를 해소 — 판단이 필요 없는 명백한 오류는 즉시 고치고, 스레드에는 👀 리액션만 남긴 채 전부 열어 둔 뒤 채팅으로 보고. GitHub 코멘트와 resolve 는 사용자가 지시할 때만. 머지 가능 시 알림, 머지는 하지 않는다. 재리뷰를 기다리지 않는다), `/rocky:recall` (워크로그를 앵커 히스토리 다이제스트 `kind:"digest"` 로 증분 정리 — 기록의 짝인 **정리(整理)** 레이어). CI 실패 자동 수정은 Claude Code 빌트인 `/autofix-pr` 이 별도 선택지.
- **훅** (`hooks/hooks.json`) — `SessionStart` 가 데몬을 띄우고(버전이 다르면 재기동), `UserPromptSubmit` 이 사람이 보드에서 바꾼 것을 세션에 주입하고, `Stop` 이 핸드오프를 집어 온 뒤 `kind:"turn"` 워크로그를 자동 기록한다 (결정론적, LLM 미사용; `worklog.autoCapture` 로 토글). 전부 fail-open.
- **스킬** (`skills/`) — `board`: 보드 에티켓(start→done, 링크 첨부, 아카이브만) + MCP/CLI 폴백; `writing-cc-plugin`: Claude Code 플러그인 작성 가이드 + 매니페스트·컴포넌트·배포 레퍼런스.

- **서브에이전트** (`agents/`) — `reviewer`: 신선한 컨텍스트에서 **diff 와 요구사항만** 받아 검토하는 읽기 전용 리뷰어. `/rocky:review` 가 이 에이전트를 띄우고, "리뷰해줘" 처럼 직접 부를 수도 있다. 돌려본 것만 통과라고 쓰고(검증 후 단언), 통과처럼 보이는 실패(false pass) 함정을 따로 챙기며, 파일을 고치거나 머지하지 않는다.

> **작업 목록은 보드 하나다.** rocky 는 외부 태스크 서비스와 연동하지 않는다 —
> 작업 목록은 데몬의 보드(`todo_*`), 작업 기록은 `worklog_*` 다. 전에 번들로 있던 `todoist` 스킬은 제거했다.

> **v0.19 에서 걷어낸 것** — 소울(페르소나) 주입과 `SessionStart` 훅, statusline 템플릿 3종과 동기화 훅, opencode 위임 런타임, `/rocky:codex` · `/rocky:issue` · `/rocky:opencode` · `/rocky:opencode-jobs` 커맨드. 재미로 넣었거나 실사용이 없던 것들이라 정리했다 — 전부 git 히스토리에서 꺼낼 수 있다. `rocky.json` 의 `soul` / `callsign` / `opencode` 키도 함께 사라져 이제 거부되니, 예전 설정 파일에 남아 있으면 지워야 한다.

## 시작하기

요구사항: [Bun](https://bun.sh) ≥ 1.0 (빌드 단계 없음 — Bun이 TS를 직접 실행한다).

### Claude Code plugin

이 저장소 자체가 플러그인 소스이자 마켓플레이스다 (`.claude-plugin/marketplace.json`, 별도 파사드 없음). 일반 설치는 GitHub 소스로:

```bash
claude plugin marketplace add minjun0219/rocky
claude plugin install rocky@rocky-marketplace
```

원격 세션 안에서는 `/plugin` 슬래시 커맨드로 동일하게 설치한다. 설치본은 GitHub `main`에서 clone되므로 코드 변경은 push 후 `claude plugin update rocky`로 반영된다.

플러그인 소스는 `plugin/` 디렉토리다(마켓플레이스 `source: "./plugin"`) — 설치본에는 `crates/`·`target/`·`node_modules` 가 복사되지 않는다. 설치본이 쓰는 MCP 서버는 `plugin/.claude-plugin/plugin.json` 의 `mcpServers` 둘뿐이고, 저장소에 `.mcp.json` 을 두지 않는 이유는 그게 설치본 MCP 설정으로 새기 때문이다.

설치하면 그 뒤로는 손댈 게 없다 — 매 턴이 `Stop` 훅으로 워크로그에 쌓이고, 쌓인 것을 `/rocky:recall` 로 정리한다.

## 설정

`rocky.json` (project `./rocky.json` > user `~/.config/rocky/rocky.json`, [JSON Schema](./rocky.schema.json) 로 IDE 자동완성 지원)을 읽는다:

```json
{
  "$schema": "https://raw.githubusercontent.com/minjun0219/rocky/main/rocky.schema.json",
  "worklog": { "captureMaxChars": 600 }
}
```

허용 키는 아래 둘뿐이다 (그 외 top-level 키는 즉시 reject — 오타 가드. 제거된 `openapi` / `seo` 도 이제 거부되니 옛 설정 파일에 남아 있으면 지운다). 정확한 모양은 [`rocky.schema.json`](./rocky.schema.json) 과 `crates/rocky-core/src/config.rs` 가 lockstep 으로 들고 있다.

| 키 | 내용 |
| --- | --- |
| `worklog` | `dir` (env `ROCKY_WORKLOG_DIR` 우선) / `autoCapture` (기본 true) / `captureMaxChars` (기본 800) / `digestThreshold` (기본 40) |
| `todo` | 보드 데몬 설정(`port` / `dir` / `expose` / `watch` / `statusline`). Rust 데몬(`crates/`)이 읽고, TS 로더는 통과만 시킨다 — 자세한 모양은 [`docs/board.md`](./docs/board.md) |

### 환경 변수

`ROCKY_WORKLOG_AUTO_CAPTURE` 는 서버가 아니라 `Stop` 훅이 읽으므로 Claude Code 전용이다.

| 변수 | 기본값 | 영향 |
| --- | --- | --- |
| `ROCKY_CONFIG` | `~/.config/rocky/rocky.json` | user-level `rocky.json` 경로 override |
| `ROCKY_WORKLOG_DIR` | `~/.config/rocky/worklog/<project-key>` | 워크로그 JSONL 위치. `worklog.dir` 보다 우선 |
| `ROCKY_WORKLOG_AUTO_CAPTURE` | `1` | `Stop` 훅 턴 자동 기록 on/off. `0`/`false`/`off`/`no` 만 비활성 |

## 문서 맵

이 README 가 사람용 진입점이고, 그보다 깊이 들어가는 문서는 아래가 전부다. 도구 하나하나의 입출력은 문서가 아니라 **도구 정의 자체**(`crates/*/src/*mcp*.rs` 의 `#[tool]`)가 단일 소스다 — 에이전트는 그걸 직접 읽는다.

| 문서 | 대상 | 내용 |
| --- | --- | --- |
| [`AGENTS.md`](./AGENTS.md) | 에이전트 (영문) | **단일 source of truth** — Layout / Scope / coding rules / change checklist / 리뷰 규약 |
| [`docs/architecture.md`](./docs/architecture.md) | 에이전트 (영문) | 코드만 봐선 안 나오는 설계 근거 — 필요할 때만 읽는 심화 레퍼런스 |
| [`docs/hosts.md`](./docs/hosts.md) | 사람 | 호스트 지원 매트릭스 — 세 호스트의 확장 메커니즘 + rocky 표면 커버 현황 (실측) |
| [`docs/backlog.md`](./docs/backlog.md) | 사람 | 백로그 — 보류 항목 + 도메인 재추가 후보 |
| [`docs/board.md`](./docs/board.md) | 사람 | 보드 데몬 — 설치·기동·CLI·설정·핸드오프·spawn |
| [`docs/rewrite/`](./docs/rewrite/) | 에이전트 | TS → Rust 포팅 기록 — `contract.md`(외부 표면 계약, 정본) · `decisions.md` · `rust-notes.md` |
| [`docs/codex.md`](./docs/codex.md) / [`docs/opencode.md`](./docs/opencode.md) | 사람 | 다른 host 에서 MCP 서버를 쓰고 싶을 때 |

## 역사 / 아카이브

v0.2 까지의 journal / mysql / spec-pact / pr-watch 도메인 + 에이전트 + 스킬은 [`archive/pre-openapi-only-slim`](https://github.com/minjun0219/rocky/tree/archive/pre-openapi-only-slim) 브랜치에 박제되어 있고, 활용 패턴이 잡히는 대로 [`docs/backlog.md`](./docs/backlog.md)의 후보 단위로 재추가한다 — journal은 v0.6 에 재추가되어 v0.9 에서 `worklog` 로 개명됐다. notion (v0.5 재추가) 과 openapi · seo 는 v0.23 에서 다시 걷어냈다 — 실사용이 0회였다. 예전 네이티브 opencode plugin 은 in-tree `.archive/` 에 두었다가 걷어냈다 — 필요하면 git 히스토리에서 꺼낸다. 현재 opencode 지원은 이 플러그인의 부활이 아니라 stdio MCP 등록 방식이다.

> 세 호스트에서 rocky 표면이 어디까지 커버되는지 (슬래시 커맨드·훅·스킬·소울 이식 가능 범위 포함) 는 [`docs/hosts.md`](./docs/hosts.md) 참고.

## 개발

```bash
bun install        # 개발 도구 (biome · changesets · husky 훅 배선). 런타임 TS 는 없다
bun run check      # Biome 검증 (scripts/ · plugin/scripts/)
bun run typecheck  # tsc --noEmit
bun test           # 릴리스·부트스트랩·permalink 스크립트 테스트
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
cargo build --workspace   # target/debug/{rocky,rockyd}; ROCKY_BIN=target/debug/rocky 로 부트스트랩을 우회
```

같은 게이트를 `.husky/pre-commit` (lint-staged + 시크릿 스캔) / `.husky/pre-push` (typecheck + test) 와 CI ([`ci.yml`](./.github/workflows/ci.yml)) 가 반복 실행한다. 기여 규칙·레이아웃은 [`AGENTS.md`](./AGENTS.md).

개발 중 외부 라이브러리 문서용 `context7` MCP 는 유저 스코프에 둔다 (레포 `.mcp.json` 은 설치본으로 새므로 두지 않는다):

```bash
claude mcp add --scope user --transport http context7 https://mcp.context7.com/mcp
```

## 라이선스

[MIT](./LICENSE) © Minjun Kim
