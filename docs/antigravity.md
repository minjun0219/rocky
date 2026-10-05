# Antigravity에서 rocky 쓰기

Google Antigravity(CLI `agy`, 앱, IDE)에서 rocky 보드를 쓰는 방법. 메인 호스트는 Claude Code이고, agy에는
작업(주로 디자인)을 넘겨받아 처리하는 데 필요한 만큼만 붙인다 — 보드 MCP + `board` 스킬 + 짧은 규칙 하나.
실측 기준은 `agy 1.2.14`(2026-10).

## 설치

레포의 `antigravity/`가 agy 플러그인 번들이다(`plugin.json` · `mcp_config.json` · `rules/` · `skills/`).

```bash
agy plugin install <rocky 레포>/antigravity
agy plugin list            # rocky — skills, mcpServers
```

설치는 번들을 `~/.gemini/config/plugins/rocky/`로 **복사**한다(`skills/board`의 심볼릭 링크는 실제 파일로
풀린다). 그래서 rocky를 갱신하면 같은 명령을 다시 돌려야 새 스킬이 들어간다. 끄려면 `agy plugin disable rocky`.

## 붙는 것

| 번들 파일 | 내용 |
| --- | --- |
| `mcp_config.json` | 데몬 MCP `http://127.0.0.1:8636/mcp`(`serverUrl`) — `todo_*` · `note_*` · `token_*` |
| `skills/board` | Claude Code 플러그인의 `plugin/skills/board`와 같은 파일(링크) |
| `rules/AGENTS.md` | 항상 켜지는 규칙 세 줄 — `rocky-12` 같은 참조는 보드 항목, actor는 `antigravity`, 데몬이 꺼졌으면 멈춤 |

agy의 HTTP MCP 클라이언트는 streamable HTTP를 말하고, 데몬의 stateless `/mcp`에 그대로 붙는다.

## 넘기기 흐름

1. Claude Code 세션에서 "이건 agy로 넘길게" → 세션이 보드 할 일을 만들고 브리프를 `description`에 쓴다
   (`board` 스킬의 *Antigravity 로 넘기기*). 예전처럼 `HANDOFF-*.md` 파일을 레포에 두지 않는다.
2. agy에서 `rocky-42 해줘` → agy가 `todo_list { id }`로 읽고 `start`(actor `antigravity`) → 작업 → 결과를
   댓글로 남기고 `done`.
3. `antigravity`는 `rocky_core::actors::AGENT_ACTORS`에 **일부러 넣지 않았다**. 그래서 agy의 댓글·완료가
   Claude Code 세션의 다음 프롬프트에 "호출자의 보드 변경"으로 주입되고, 넘긴 세션이 거기서 이어받는다.
   대신 웹 UI는 agy의 착수를 사람 쪽 색으로 보이고, agy가 쥔 doing은 24시간 자동 해제 대상이 아니다.

## 없는 것

- **데몬 기동.** agy에는 SessionStart가 없다. 데몬이 떠 있어야 한다 — launchd 상주(`rocky daemon install`)를
  하거나 Claude Code 세션을 한 번 연 뒤에 쓴다. 꺼져 있으면 보드 도구가 연결 거부로 실패한다.
- **워크로그(`worklog_*`).** agy는 플러그인 MCP 서버를 **플러그인 폴더에서** 띄우고, 작업 폴더를 환경 변수로
  넘기지 않는다(`PLUGIN_ROOT`만 있다). `rocky mcp worklog`는 프로세스 cwd로 프로젝트를 정하므로 모든 기록이
  한 칸에 쌓인다 — 그래서 번들에서 뺐다. agy는 MCP `roots`로 작업 폴더를 알려 주므로(`roots/list` →
  `file:///…/<workspace>`), 워크로그 서버가 `roots`를 읽게 하면 되살릴 수 있다.
- **턴 자동 기록, 보드 변경 주입.** agy에도 `Stop` · `PreInvocation` 훅이 있지만(stdin JSON에
  `transcriptPath`), 트랜스크립트 모양이 달라 파서를 새로 짜야 한다. 아직 하지 않았다.
- **`/rocky:*` 커맨드와 서브에이전트.** `gh`·Claude Code 서브에이전트에 기대는 흐름이라 옮기지 않는다.
  `agy plugin import`가 Claude Code 플러그인을 가져올 수 있지만 rocky 매니페스트는 `${CLAUDE_PLUGIN_ROOT}`와
  CC 전용 커맨드를 쓰므로 쓰지 않는다.

## 원격 제어(`agy remote-control`)

rocky는 이 기기의 agy 원격 제어 데몬을 보고 켜고 끈다 — `rocky rc agy [start|stop]`, 웹 원격 제어 탭의 Antigravity 줄
(로컬 화면에서만 버튼). `rc` 블록과 상관없이 `agy`가 설치돼 있으면 보인다. 세부는 README의 *rc 서버 현황*.

호스트별 비교는 [`docs/hosts.md`](./hosts.md).
