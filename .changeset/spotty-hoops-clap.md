---
"@minjun0219/rocky": minor
---

소울(페르소나)과 statusline, 그리고 `/rocky:codex` · `/rocky:issue` 커맨드를 걷어냈다.

- **소울** — `souls/*.md` 3종, `soul.ts`, `inject-soul` 훅, `/rocky:soul`, `rocky.json` 의
  `soul` / `callsign` 키. 재미로 넣은 기능이었고, 동시에 rocky 가 세션 컨텍스트에 넣던
  **유일한** 것이었다 (주입 1,310자 → 605자로 압축했다가 기능째 제거).
- **statusline** — 템플릿 3종, `statusline.ts`, `sync-statusline` 훅, `/rocky:statusline`,
  `docs/statusline.md`. 컨텍스트 비용은 0 이었지만 함께 정리했다.
- **커맨드** — `/rocky:codex` (Codex 위임은 공식 `openai/codex-plugin-cc` 가 덮는다),
  `/rocky:issue`.

훅은 `Stop`(턴 자동 기록) 하나만 남는다 — **SessionStart 가 사라져 이제 플러그인이 세션
컨텍스트에 넣는 것이 아무것도 없다.** MCP 도구 16 종과 `/rocky:brainstorm` · `/rocky:review` ·
`/rocky:finish` · `/rocky:review-pr` · `/rocky:recall` 커맨드, 스킬 2종은 그대로다.

기존 `rocky.json` 에 `soul` / `callsign` 이 남아 있으면 unknown key 로 거부되니 지워야 한다.
`~/.claude/settings.json` 의 `statusLine` 설정과 `~/.config/rocky/statusline.sh` 도 직접 정리해야
한다 (rocky 는 사용자 설정을 건드리지 않는다).
