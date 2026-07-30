# 호스트 지원 매트릭스

rocky 표면이 Claude Code / Codex CLI / opencode 에서 각각 어디까지 커버되는지, 그리고 각 호스트가
확장 메커니즘을 네이티브로 어디까지 지원하는지 정리한 실측 자료. 2026-07 기준.

> rocky 는 세 full-surface 호스트(Claude Code plugin / Codex CLI / opencode)에서 **오늘 기준 MCP 도구만** 공유한다. 슬래시 커맨드·`Stop` 훅·스킬은 Claude Code plugin 에만 배포돼 있다 (소울·statusline 은 v0.19 에서 제거). **단, 이는 "다른 호스트가 그 확장을 못 한다"는 뜻이 아니다** — Codex 와 opencode 도 2026 기준 커맨드 / 훅 / 스킬 / 서브에이전트 / 번들 플러그인을 네이티브로 지원한다. rocky 가 아직 그 호스트용 버전을 만들지 않았을 뿐이라 대부분 이식 가능하다. 아래 두 표가 (A) 호스트가 네이티브로 뭘 지원하는지 와 (B) rocky 표면이 각 호스트에서 어디까지 커버되는지 를 나눠 보여준다.

### A. 호스트 확장 메커니즘 (네이티브 지원)

| 메커니즘 | Claude Code | Codex CLI | opencode |
| --- | --- | --- | --- |
| MCP stdio 서버 | ✅ | ✅ `[mcp_servers.*]` | ✅ `mcp{type:"local"}` |
| 슬래시 커맨드 | ✅ `commands/` (project+user) | ◐ `~/.codex/prompts/*.md` (user-only, **deprecated → skills**) | ✅ `.opencode/command/*.md` (`$ARGUMENTS`/`$1`/`` !`sh` ``/`@file`) |
| SessionStart 훅 | ✅ | ◐ `SessionStart` hook (**실험적 · 기본 off · no Windows**) 또는 AGENTS.md 정적 병합 | ✅ plugin `session.created` / `instructions` |
| Stop · 턴 훅 | ✅ | ◐ `Stop` hook(실험) 또는 `notify`(agent-turn-complete) | ✅ plugin `session.idle` / `message.updated` |
| Skills (SKILL.md) | ✅ | ✅ 동일 스펙 | ✅ `.claude/skills/` 직접 읽음 |
| Subagents | ✅ | ✅ `.codex/agents/*.toml` | ✅ `.opencode/agent/*.md` |
| AGENTS.md 세션 주입 | ✅ | ✅ (3-scope) | ✅ (3-scope + `CLAUDE.md` fallback) |
| 단일 번들 플러그인 + 마켓플레이스 | ✅ `.claude-plugin/` + `marketplace.json` | ✅ `.codex-plugin/plugin.json` + 마켓플레이스 (**2026-03 신규**) | ✗ 우산 매니페스트 없음 (surface별 개별 / npm plugin) |

범례: ✅ 1급 지원 · ◐ 되지만 제약/실험적 · ✗ 등가물 없음.

### B. rocky 표면별 커버 현황

| rocky 표면 | Claude Code | Codex | opencode | 메모 |
| --- | --- | --- | --- | --- |
| MCP 도구 (openapi 7 + `seo_validate` + worklog 4 + notion 4 = 16) | ✅ 배포됨 | ✅ 배포됨 | ✅ 배포됨 | 공유 코어 — 이미 3-호스트 완결 |
| `/rocky:finish` | ✅ | ◐ 커버 가능 (skill) | ◐ 커버 가능 (command) | `gh` CLI 의존, 로직은 호스트 중립 |
| `/rocky:resolve-reviews` | ✅ | ◐ 커버 가능 (skill) | ◐ 커버 가능 (command) | `gh` CLI 의존, 로직은 호스트 중립 |
| `/rocky:recall` | ✅ | ◐ 커버 가능 | ◐ 커버 가능 | 정리는 host-LLM 몫 → 호스트별 모델(Haiku↔Sonnet 상당) 매핑 필요 |
| 턴 자동 기록 (Stop hook → worklog) | ✅ | ◐ Stop hook / notify — **트랜스크립트 포맷 상이** | ◐ plugin `session.idle` — **SDK client 접근, 포맷 상이** | `src/hooks/transcript.ts` 를 호스트별 재작성해야 (실제 비용) |
| skill `writing-cc-plugin` | ✅ | ◐ 스펙 호환하나 내용이 CC 전용 | ✅ `.claude/skills/` 자동 발견 | 메커니즘은 커버, 내용 가치는 CC 한정 |
| skill `todoist` | ✅ | ◐ 커버 가능 | ✅ `.claude/skills/` 자동 발견 | 세션에 연결된 Todoist MCP 에만 의존 — 로직은 호스트 중립 |
| 단일 설치 유닛 | ✅ `.claude-plugin/` + `rocky-marketplace` | ◐ `.codex-plugin/plugin.json` 로 번들화 가능 (`codex plugin` 서브커맨드 실재) | ✗ 우산 없음 → config 트리 / npm plugin | Codex 가 새로 연 길 |
| 동반 플러그인 `rocky-todo` (별도 레포) | ✅ 같은 마켓 2번째 entry | ◐ 데몬 MCP 가 HTTP 라 등록만 하면 됨 | ◐ 동일 | 데몬·웹UI 는 호스트 무관, 플러그인 배선과 훅만 CC 전용 |

범례: ✅ rocky 가 이미 배포 · ◐ 호스트는 지원, rocky 미구현(커버 가능) · ✗ 등가물 없음 · — 무의미.

### 요약

- **이미 완결**: MCP 코어 — 세 호스트 동등.
- **정적으로 쉬운 커버**: 소울 / 규칙을 AGENTS.md 정적 병합으로. 스킬은 opencode 가 `.claude/skills/` 를 이미 자동 발견한다.
- **훅 필요(품이 듦)**: 턴 자동 기록 — 호스트별 트랜스크립트 파서 재작성이 실제 비용.
- **새로 열린 길**: Codex 를 `.codex-plugin/plugin.json` 번들 플러그인으로 (MCP + skills + hooks 한 번에). opencode 는 우산 매니페스트가 없어 config 트리 / npm plugin 로 나눠 배포.

> **신뢰도 캐비앗**: Codex 확장 스택(hooks · plugins · 마켓플레이스)은 2026 초 신규 + 일부 실험적이다 — hooks 기본 off · no Windows, custom prompts deprecated(→ skills), skills 경로 `.agents/skills` vs `.codex/skills` 유동. 설치본 `codex-cli 0.144.5` 기준으로 `codex plugin` 서브커맨드와 `~/.codex/{skills,plugins}` 존재는 실측 확인했으나, 세부 스펙은 이식 직전에 그때의 `codex --version` 으로 재확인할 것. opencode(실측 `1.18.4`)의 `.opencode/command|agent|plugin` 은 단수 디렉터리명이 정식이다(복수형도 허용).
