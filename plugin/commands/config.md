---
description: rocky 설치·설정을 점검하고 빠진 것을 채운다 — 새 기기 셋업(rocky.json · launchd 상주 · Claude Code statusline 연결 · 보드 ↔ 레포 경로 · 수집함 어댑터)과 값 바꾸기(expose · port · sessionSummary 등)를 한 커맨드로. 상태는 `rocky config show` 가 재고, 사람이 정할 것만 한 번에 하나씩 묻는다.
argument-hint: "[항목 또는 바꿀 값 — 예: statusline · expose off · inbox · 비우면 전체 점검]"
---

대상: `$ARGUMENTS` (비어 있으면 전체 점검 → 빠진 것을 순서대로)

`rocky.json` 은 사람이 손으로 만들지 않아도 된다. 이 커맨드가 **지금 상태를 재고, 빠진 항목을
하나씩 물어 채운다.** 값이 이미 정해진 건(기본값) 묻지 않는다. 출력은 한국어, 식별자·경로·명령은
영어 그대로.

## 원칙

1. **판정은 `rocky config show` 가 한다.** 여기서 설정 파일을 직접 파싱해 다시 판단하지 않는다 —
   체크 목록(`config` / `install` / `daemon` / `launchd` / `session-summary` / `expose` / `inbox`
   / `statusline` / `board`)과 `다음 할 일` 이 그대로 작업 목록이다.
2. **사용자 파일은 덮어쓰지 않는다.** `rocky.json` 은 있으면 키 단위로 고치고(없는 키만 추가·
   지시받은 키만 변경), `~/.claude/settings.json` 의 `statusLine` 은 절대 갈아 끼우지 않는다 —
   기존 스크립트 끝에 조각을 **붙이는** 것까지만, 그것도 사용자가 고르면.
3. **한 번에 하나씩 묻는다.** 항목마다 `AskUserQuestion` 하나. 선택지에 "건너뛰기" 를 늘 둔다.
   기본값으로 충분한 것(예: 요약 on)은 묻지 않고 넘어간다.
4. **비밀값은 다루지 않는다.** 수집함 어댑터가 토큰을 쓰면 `op://` 참조만 적는다(agent-vault
   규율). 값을 채팅·argv·파일에 찍지 않는다.
5. **데몬 설정(`port` / `dir` / `expose` / `inbox`)을 바꿨으면 재기동이 필요하다** — 마지막에
   `rocky daemon stop && rocky daemon start`(launchd 상주면 `stop` 만으로 되살아난다) 를 돌리고
   `rocky config show` 로 다시 확인한다.

## 절차

### 1. 점검

```bash
rocky config show
```

- `rocky` 가 없으면 `~/.local/share/rocky/current/rocky` 로 부른다. 그것도 없으면 플러그인이
  아직 첫 세션을 안 연 것이다 — `claude plugin install rocky@rocky-marketplace` 뒤 새 세션에서
  다시 부르라고 안내하고 멈춘다.
- 출력을 **그대로** 보여준다(재작문 금지). `✓` 는 됨, `✗` 는 꼭 고쳐야 함, `·` 는 선택.
- `$ARGUMENTS` 가 특정 항목이나 값을 가리키면(예: `statusline`, `expose off`, `inbox`) 그 항목만
  3단계로 간다. 비어 있으면 `다음 할 일` 순서대로 2단계.

### 2. 빠진 항목을 하나씩

각 항목은 **물어보고 → 실행하고 → 결과 한 줄**. 순서는 `다음 할 일` 그대로(Required 먼저).

| 항목 | 묻는 것 | 실행 |
| --- | --- | --- |
| `config` 없음 | 기본 파일을 만들까 | `rocky config init` (있으면 no-op) |
| `config` JSON 깨짐 | 어디가 깨졌는지 보여주고 고칠까 | 파일을 읽어 문법만 고친다 — 값은 건드리지 않는다 |
| `daemon` 없음 | (묻지 않음) | `rocky daemon start` 후 `show` 재확인 |
| `daemon` 버전 불일치 | 지금 재기동할까 (열린 세션의 MCP 가 잠깐 끊긴다) | `rocky daemon stop && rocky daemon start` |
| `launchd` | 상주 등록할까 (로그인 때 자동 기동·죽으면 재기동) | `rocky daemon install` |
| `statusline` | 어디에 붙일까 — statusLine 이 스크립트 파일이면 "그 파일 끝에 붙인다 / 조각만 보여준다 / 건너뛴다" | 파일 끝에 `show` 가 낸 조각을 그대로 append (`jq` 있는지 먼저 `command -v jq`). 파일이 아니면(인자 붙은 명령) 조각만 보여주고 사용자 몫 |
| `cli` (터미널에서 `rocky` 안 불림) | 링크만 없으면 (묻지 않음) 걸고, PATH 가 없으면 rc 에 넣을까 | `rocky config link` / 셸 rc 에 `export PATH="$HOME/.local/bin:$PATH"` 한 줄 (파일은 사용자가 고른다 — 자동으로 rc 를 고치지 않는다) |
| `board` 없음 | (묻지 않음 — 첫 todo 때 생긴다) | 안내만 |
| `board` path 없음/어긋남 | 이 레포 경로로 잡을까 | `rocky board path` (cwd) |
| `inbox` 없음 | 외부 투두를 읽을까 — `file` 어댑터 / 다른 어댑터 / 안 읽음 | 아래 "수집함 어댑터" |

`session-summary` 는 기본 on 이라 묻지 않는다. 끄고 싶다는 요청이 오면 3단계.

### 3. 값 바꾸기 (`$ARGUMENTS` 가 값을 가리킬 때)

`rocky.json` 을 **읽어서 그 키만** 바꾸고 다시 쓴다. 파일이 없으면 먼저 `rocky config init`.
받는 키와 모양은 `rocky.schema.json` 이 정본이다 — 스키마에 없는 키는 쓰지 않는다.

```bash
CFG=$(rocky config path)
# 예: expose 끄기 — 키 하나만 갈아 끼운다. 다른 키·순서는 그대로.
python3 - "$CFG" <<'EOF'
import json, sys
p = sys.argv[1]
c = json.load(open(p))
c.setdefault("todo", {})["expose"] = "off"
json.dump(c, open(p, "w"), ensure_ascii=False, indent=2); open(p, "a").write("\n")
EOF
```

- `expose` 는 `"off"` 또는 `["lan", "tailscale-serve"]`. 켜는 쪽은 노출 범위를 먼저 한 줄로
  설명하고 묻는다(보드는 무인증이다 — `docs/board.md` "노출 범위").
- `port` / `dir` 를 바꾸면 statusline 조각의 포트·데몬 재기동까지 같이 챙긴다.
- `sessionSummary: false` 는 그 키만.

### 수집함 어댑터 (`inbox`)

`todo.inbox[]` 항목은 **명령**이다(`{ name, command[], timeoutMs? }`, stdout 규약은
`docs/board.md` "수집함"). 이 레포의 `bridges/` 에 있는 것:

- `file` — 로컬 JSON 파일을 그대로 낸다. 비밀값 없음. 시험용으로 먼저 붙이기 좋다.
- 그 밖의 어댑터는 `bridges/<name>/` 의 README 를 따른다. 토큰이 필요한 것은 `--op op://…`
  참조를 인자로 받고 값은 1Password 에서 실행 시점에 읽는다 — **참조 문자열만 적는다**.

명령 경로는 설치본 기준이 아니라 **레포 체크아웃 경로**다(`bridges/` 는 tarball 에 없다).
그 기기에 레포가 없으면 어댑터를 붙일 수 없다고 알리고 건너뛴다.

### 4. 마무리

```bash
rocky config show
```

바꾼 것과 남은 것(사용자가 건너뛴 항목)을 한 줄씩. 데몬 설정을 바꿨으면 재기동했는지까지.
"다 됐다" 는 `show` 가 `✗` 없이 끝날 때만.

## 실패 / 예외

| 상황 | 처리 |
| --- | --- |
| `rocky` 바이너리 없음 | 플러그인 설치·첫 세션 안내 후 멈춤 |
| 데몬이 안 뜸 | `rocky daemon status` 출력과 `~/.config/rocky/todo/daemon.log` 꼬리 20줄을 보여주고 멈춤 |
| `jq` 없음 | statusline 조각은 붙이지 않고 `brew install jq` 안내 |
| `settings.json` 이 JSON 으로 안 읽힘 | 손대지 않고 사실만 보고 |
| 사용자가 항목을 건너뜀 | 그대로 두고 다음 항목 |
