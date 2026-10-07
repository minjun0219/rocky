# Antigravity에서 rocky 쓰기

Google Antigravity(CLI `agy`, 앱, IDE)에서 rocky 보드를 쓰는 방법. 메인 호스트는 Claude Code이고, agy에는
작업(주로 디자인)을 넘겨받아 처리하는 데 필요한 만큼만 붙인다 — 보드·워크로그 MCP + `board`·`worklog` 스킬 + 짧은 규칙 하나 +
사람의 보드 변경을 넣는 훅 하나. 실측 기준은 `agy 1.2.14`(2026-10, 훅은 `1.2.17`).

## 설치

레포의 `antigravity/`가 agy 플러그인 번들이다(`plugin.json` · `mcp_config.json` · `hooks.json` · `rules/` · `skills/`).

```bash
agy plugin install <rocky 레포>/antigravity
agy plugin list            # rocky — skills, mcpServers, hooks
```

설치는 번들을 `~/.gemini/config/plugins/rocky/`로 **복사**한다(`skills/*`의 심볼릭 링크는 실제 파일로
풀린다). 그래서 rocky를 갱신하면 같은 명령을 다시 돌려야 새 스킬·훅이 들어간다. 끄려면 `agy plugin disable rocky`.
워크로그 서버와 훅은 `~/.local/share/rocky/current/rocky`(부트스트랩이 걸어 두는 링크)를 부르므로, 그 바이너리가
`--roots`와 `hook notify-todo agy`를 아는 버전이어야 한다. 모르는 옛 바이너리는 훅에서 아무것도 내지 않는다.

## 붙는 것

| 번들 파일 | 내용 |
| --- | --- |
| `mcp_config.json` | 데몬 MCP `http://127.0.0.1:8636/mcp`(`serverUrl`) — `todo_*` · `note_*` · `token_*`, 그리고 `rocky mcp worklog --roots` — `worklog_*` |
| `skills/board` · `skills/worklog` | Claude Code 플러그인의 `plugin/skills/*`와 같은 파일(링크) |
| `hooks.json` | `PreInvocation` → `rocky hook notify-todo agy` — 아래 *보드 변경 주입* |
| `rules/AGENTS.md` | 항상 켜지는 규칙 — `rocky-12` 같은 참조는 보드 항목, actor는 `antigravity`, 결정은 워크로그에, 데몬이 꺼졌으면 멈춤 |

agy의 HTTP MCP 클라이언트는 streamable HTTP를 말하고, 데몬의 stateless `/mcp`에 그대로 붙는다.

**워크로그의 프로젝트는 `roots`로 정한다.** agy는 플러그인 MCP 서버를 **플러그인 폴더에서** 띄우고, 작업 폴더를 환경
변수로 넘기지 않는다(`PLUGIN_ROOT`만 있다). 그래서 cwd로 프로젝트를 정하는 기본 동작으로는 모든 기록이 한 칸에 쌓인다.
`--roots`는 도구를 부를 때마다 클라이언트에 MCP `roots/list`를 물어 첫 `file://` 폴더를 프로젝트로 쓴다(agy는 작업
폴더를 돌려준다). 답을 못 받으면 cwd로 물러서지 않고 에러를 낸다. roots는 MCP에서 폐기 예정(SEP-2577 — 폐기 뒤 1년은
동작)이지만 아직 대체 수단이 없다.

## 보드 변경 주입

Claude Code의 `UserPromptSubmit` 훅처럼, 마지막 확인 이후 **사람이** 보드에서 바꾼 것(웹 UI의 댓글·수정 등)을
`# rocky: 마지막 확인 이후 호출자의 보드 변경` 블록으로 넣는다. 다른 점만 적는다.

- **모델 호출마다 돈다.** `PreInvocation`은 턴 시작이 아니라 모델을 부를 때마다(한 턴에 도구 호출 수 + 1번) 불린다.
  그래서 턴 중간에 단 댓글도 다음 모델 호출에 들어간다. 호출당 20ms 남짓이다(디버그 빌드, 데몬이 꺼져 있어도 같다).
- **커서는 대화별**(`conversationId` — 턴이 바뀌어도 같다), 파일은 `<todo dir>/hook-cursors-agy.json`. Claude Code 세션의
  `hook-cursors.json`과 나눠, 호출이 잦은 이 훅이 그쪽 최근 100칸을 밀어내지 않게 했다. 첫 호출은 위치만 적는다.
- **agy 자신의 변경은 뺀다.** `antigravity`는 에이전트 목록 밖이라(아래 *넘기기 흐름* 3) 사람 필터만으로는 agy가 방금 한
  `start`·댓글이 자기에게 돌아온다. Claude Code 등 다른 에이전트의 변경도 Claude Code 쪽과 같이 빠진다.
- **`ephemeralMessage`로 넣는다 — 다음 턴에는 남지 않는다.** 트랜스크립트에는 `EPHEMERAL_MESSAGE` 단계로 기록되지만,
  다음 턴에 도구를 못 쓰게 하고 물으면 모델은 그 내용을 모른다(실측). 그래서 사람의 변경은 그 모델 호출에서 한 번 보이고
  끝난다. `userMessage`는 다음 턴까지 남지만 `<USER_REQUEST>`로 감싸여 사용자가 한 요청으로 들어가므로 쓰지 않는다 —
  웹에서 나중에 하려고 만든 할 일이 지금 하라는 요청으로 읽힌다. (도구를 열어 두면 모델이 트랜스크립트 파일을 직접 읽어
  찾아내므로, 남는지 잴 때는 도구를 막아야 한다.)
- 핸드오프·PR 전이·받은편지함·데몬 업그레이드는 하지 않는다 — Claude Code 세션의 것이다. 끄기는 Claude Code와 같다
  (`todo.watch: false` 또는 `ROCKY_TODO_WATCH=0`).

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
- **턴 자동 기록.** agy에도 `Stop` 훅이 있지만(stdin JSON에 `transcriptPath`), 트랜스크립트 모양이 달라 파서를 새로
  짜야 한다. 아직 하지 않았다.
- **`/rocky:*` 커맨드와 서브에이전트.** `gh`·Claude Code 서브에이전트에 기대는 흐름이라 옮기지 않는다.
  `agy plugin import`가 Claude Code 플러그인을 가져올 수 있지만 rocky 매니페스트는 `${CLAUDE_PLUGIN_ROOT}`와
  CC 전용 커맨드를 쓰므로 쓰지 않는다.

## statusline

agy의 statusLine도 Claude Code와 같은 꼴의 JSON을 stdin으로 주므로 `rocky statusline --full`을 그대로 건다
(`/statusline rocky statusline --full`). 한도는 agy가 주는 `quota`로 그리고, Claude 쪽 토큰·캐시는 보지 않는다 —
[`docs/board.md`](./board.md) "statusline에 얹기".

## 원격 제어(`agy remote-control`)

rocky는 이 기기의 agy 원격 제어 데몬을 보고 켜고 끈다 — `rocky rc agy [start|stop]`, 웹 원격 제어 탭의 Antigravity 줄
(로컬 화면에서만 버튼). `rc` 블록과 상관없이 `agy`가 설치돼 있으면 보인다. 세부는 README의 *rc 서버 현황*.

호스트별 비교는 [`docs/hosts.md`](./hosts.md).
