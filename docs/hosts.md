# 호스트 지원 매트릭스

rocky 표면이 Claude Code / Codex CLI / opencode / Antigravity에서 각각 어디까지 커버되는지, 그리고 각 호스트가
확장 메커니즘을 네이티브로 어디까지 지원하는지 정리한 실측 자료. 2026-07 기준(Antigravity 열은 2026-10 `agy 1.2.14`).
Antigravity는 넘겨받은 작업을 처리하는 보조 호스트라 보드만 붙였다 — [`docs/antigravity.md`](./antigravity.md).

> rocky는 세 full-surface 호스트(Claude Code plugin / Codex CLI / opencode)에서 **오늘 기준 MCP 도구만** 공유한다. 슬래시 커맨드·`Stop` 훅·스킬은 Claude Code plugin 에만 배포돼 있다 (소울·statusline은 v0.19에서 제거). **단, 이는 "다른 호스트가 그 확장을 못 한다"는 뜻이 아니다** — Codex와 opencode도 2026 기준 커맨드 / 훅 / 스킬 / 서브에이전트 / 번들 플러그인을 네이티브로 지원한다. rocky가 아직 그 호스트용 버전을 만들지 않았을 뿐이라 대부분 이식 가능하다. 아래 두 표가 (A) 호스트가 네이티브로 뭘 지원하는지 와 (B) rocky 표면이 각 호스트에서 어디까지 커버되는지 를 나눠 보여준다.

### A. 호스트 확장 메커니즘 (네이티브 지원)

| 메커니즘 | Claude Code | Codex CLI | opencode | Antigravity |
| --- | --- | --- | --- | --- |
| MCP stdio 서버 | ✅ | ✅ `[mcp_servers.*]` | ✅ `mcp{type:"local"}` | ◐ `mcp_config.json` — 플러그인 서버의 cwd 가 플러그인 폴더(작업 폴더는 MCP `roots` 로만) |
| 슬래시 커맨드 | ✅ `commands/` (project+user) | ◐ `~/.codex/prompts/*.md` (user-only, **deprecated → skills**) | ✅ `.opencode/command/*.md` (`$ARGUMENTS`/`$1`/`` !`sh` ``/`@file`) | ◐ 플러그인 `commands/` 를 검증기가 인식(미실측) |
| SessionStart 훅 | ✅ | ◐ `SessionStart` hook (**실험적 · 기본 off · no Windows**) 또는 AGENTS.md 정적 병합 | ✅ plugin `session.created` / `instructions` | ✗ 문서에 없음 — `PreInvocation` 으로 대신 |
| Stop · 턴 훅 | ✅ | ◐ `Stop` hook(실험) 또는 `notify`(agent-turn-complete) | ✅ plugin `session.idle` / `message.updated` | ✅ `hooks.json` `Stop` / `PostInvocation`(stdin 에 `transcriptPath`) |
| Skills (SKILL.md) | ✅ | ✅ 동일 스펙 | ✅ `.claude/skills/` 직접 읽음 | ✅ `skills/<name>/SKILL.md` |
| Subagents | ✅ | ✅ `.codex/agents/*.toml` | ✅ `.opencode/agent/*.md` | ◐ 플러그인 `agents/` 를 검증기가 인식(미실측) |
| AGENTS.md 세션 주입 | ✅ | ✅ (3-scope) | ✅ (3-scope + `CLAUDE.md` fallback) | ✅ `AGENTS.md`/`GEMINI.md` + 플러그인 `rules/` |
| 단일 번들 플러그인 + 마켓플레이스 | ✅ `.claude-plugin/` + `marketplace.json` | ✅ `.codex-plugin/plugin.json` + 마켓플레이스 (**2026-03 신규**) | ✗ 우산 매니페스트 없음 (surface별 개별 / npm plugin) | ✅ `plugin.json` 번들 · `agy plugin install <dir>`(마켓플레이스는 미실측) |

범례: ✅ 1급 지원 · ◐ 되지만 제약/실험적 · ✗ 등가물 없음.

### B. rocky 표면별 커버 현황

| rocky 표면 | Claude Code | Codex | opencode | Antigravity | 메모 |
| --- | --- | --- | --- | --- | --- |
| MCP 도구 (worklog 4) | ✅ 배포됨 | ✅ 배포됨 | ✅ 배포됨 | ✅ `--roots` | 공유 코어. agy 는 서버를 플러그인 폴더에서 띄우므로 프로젝트를 MCP `roots` 로 정한다 |
| `/rocky:review-request` | ✅ | ◐ 커버 가능 (skill) | ◐ 커버 가능 (command) | — | `gh` CLI 의존, 로직은 호스트 중립 |
| `/rocky:review-fix` | ✅ | ◐ 커버 가능 (skill) | ◐ 커버 가능 (command) | — | `gh` CLI 의존, 로직은 호스트 중립 |
| `/rocky:recall` | ✅ | ◐ 커버 가능 | ◐ 커버 가능 | — | 정리는 host-LLM 몫 → 호스트별 모델(Haiku↔Sonnet 상당) 매핑 필요 |
| 턴 자동 기록 (Stop hook → worklog) | ✅ | ◐ Stop hook / notify — **트랜스크립트 포맷 상이** | ◐ plugin `session.idle` — **SDK client 접근, 포맷 상이** | ✅ `Stop` 훅 — 파서 `transcript::agy` | `crates/rocky-core/src/transcript.rs`를 호스트별 재작성해야 (실제 비용) — agy 는 했다 |
| 보드 변경 주입 (사람의 편집 → 세션) | ✅ `UserPromptSubmit` | 미구현 — 작업 전에 `todo_list` | 미구현 — 작업 전에 `todo_list` | ✅ `PreInvocation` 훅 | agy 는 모델 호출마다 돌고 커서가 대화별이다 |
| skill `board` | ✅ | ◐ 스펙 호환 | ◐ 스펙 호환 | ✅ 번들 `skills/board` | 넘기기 흐름(*Antigravity 로 넘기기*)이 여기 있다 |
| skill `writing-cc-plugin` | ✅ | ◐ 스펙 호환하나 내용이 CC 전용 | ✅ `.claude/skills/` 자동 발견 | — | 메커니즘은 커버, 내용 가치는 CC 한정 |
| 단일 설치 유닛 | ✅ `.claude-plugin/` + `rocky-marketplace` | ◐ `.codex-plugin/plugin.json`로 번들화 가능 (`codex plugin` 서브커맨드 실재) | ✗ 우산 없음 → config 트리 / npm plugin | ✅ `antigravity/` 번들 | Codex가 새로 연 길 |
| 보드 데몬 MCP (`rockyd`, `/mcp`) | ✅ plugin.json의 http 서버 | ◐ HTTP 라 등록만 하면 됨 | ◐ 동일 | ✅ 번들의 `serverUrl` | 데몬은 호스트 무관, 플러그인 배선과 훅만 CC 전용 |

범례: ✅ rocky가 이미 배포 · ◐ 호스트는 지원, rocky 미구현(커버 가능) · ✗ 등가물 없음 · — 무의미.

### 요약

- **이미 완결**: MCP 코어 — 세 호스트 동등.
- **정적으로 쉬운 커버**: 소울 / 규칙을 AGENTS.md 정적 병합으로. 스킬은 opencode가 `.claude/skills/`를 이미 자동 발견한다.
- **훅 필요(품이 듦)**: 턴 자동 기록 — 호스트별 트랜스크립트 파서 재작성이 실제 비용(Antigravity 는 `transcript::agy` 로 했다).
- **새로 열린 길**: Codex를 `.codex-plugin/plugin.json` 번들 플러그인으로 (MCP + skills + hooks 한 번에). opencode는 우산 매니페스트가 없어 config 트리 / npm plugin로 나눠 배포.

> **신뢰도 캐비앗**: Codex 확장 스택(hooks · plugins · 마켓플레이스)은 2026 초 신규 + 일부 실험적이다 — hooks 기본 off · no Windows, custom prompts deprecated(→ skills), skills 경로 `.agents/skills` vs `.codex/skills` 유동. 설치본 `codex-cli 0.144.5` 기준으로 `codex plugin` 서브커맨드와 `~/.codex/{skills,plugins}` 존재는 실측 확인했으나, 세부 스펙은 이식 직전에 그때의 `codex --version`으로 재확인할 것. opencode(실측 `1.18.4`)의 `.opencode/command|agent|plugin`은 단수 디렉터리명이 정식이다(복수형도 허용).
