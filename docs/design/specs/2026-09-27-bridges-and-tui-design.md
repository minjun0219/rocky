# 브리지 규약 + TUI — 설계 (데스크톱 중심 기기 조각)

- 날짜: 2026-09-27
- 상태: 구현 완료 — PR 1 #143(데몬 inbox) · PR 2 #146(TUI 기본) · PR 3 #147(TUI 나머지) · 어댑터 #148(Todoist).
  2026-09-28 추가: 웹 UI 를 `web/` 로 되살림(오너 결정 — 테일넷 없이 밖에서 보드에 닿는 길: 웹 UI + Cloudflare Tunnel + Access). 결정 7 의 "데스크톱 앱" 조각은 이걸로 대체

## 한 줄

**데몬(`rockyd`)이 보드의 정본이고, 나머지는 전부 그 위에 REST/SSE 로 붙는 별도 프로세스다.**
외부 투두 앱(구글 투두·Todoist·…)은 rocky 와 동기화하지 않는다 — 별개의 수집함으로 두고,
rocky 는 **읽어서 보여주고, 고른 것을 보드로 올리며, 링크로 참조**만 한다.

## 결정 표

| # | 질문 | 결정 | 버린 것과 이유 |
|---|---|---|---|
| 1 | 어느 기기부터 | **데스크톱 중심 기기**. 기기마다 데몬·보드·브리지가 독립이고, 모바일 중심 기기는 다음 조각 | 기기 간 동기화는 하지 않는다 — 따로 관리 |
| 2 | 외부 투두 ↔ 보드 방향 | **동기화 없음. 참조만** | 수집/양방향 동기화 — ID 매핑·충돌·데이터 형태 차이를 전부 떠안는다 |
| 3 | "참조" 의 범위 | **TUI 에 수집함 탭** — 외부 미완료 항목을 읽기 전용으로 보고, 고른 걸 보드로 올린다(링크 자동) | 링크만(에이전트가 대신 올림) — 무인·화면 흐름이 없다 / 보드 옵션으로 데몬이 후보에 섞기 — 데몬이 외부 인증을 들게 된다 |
| 4 | 어댑터가 사는 곳 | **별도 프로세스 → 데몬에 등록**. TUI·`/rocky:next`·MCP 가 같은 수집함을 본다 | TUI 안 직접 — 수집함을 TUI 만 본다 / 데몬 내장 — 토큰·연동 코드가 데몬에 |
| 5 | 데몬 ↔ 어댑터 연결 | **명령 실행, stdout JSON** (git credential helper 모양). 상주 없음, TTL 캐시 | 루프백 HTTP 서버 — 포트·기동 관리 / 어댑터가 밀어넣기 — 데몬이 외부 상태를 저장 |
| 6 | 첫 조각의 어댑터 | **없음 — 규약과 구조만**. 테스트용 파일 어댑터 하나 | 구글 투두·네이버웍스·Todoist 는 각각 후속 조각 |
| 7 | TUI 범위 | 보드 읽기 + SSE 갱신 · 수집함 탭 + 올리기 · 상태 변경 · 핸드오프/새 세션 · **항목에 물린** 이슈·PR 상태 + 이슈 생성 | 레포 이슈·PR 목록 / PR 에 행동(리뷰·머지) — gh 의 승인 모델과 겹치고 범위가 커진다 |
| 8 | TUI 코드 위치 | **rocky 레포, 새 크레이트 `rocky-tui`** (별도 바이너리) | CLI 서브커맨드 — 훅 바이너리에 TUI 라이브러리가 링크된다 / 별도 레포 — lockstep·릴리스가 하나 더 |
| 9 | 어댑터 코드 위치 | **rocky 레포 `bridges/<name>/`**. Scope Out 항목을 이에 맞게 고친다 | 비공개 레포 — 혼자 쓰는 도구에서 공개 표면 위생의 실익이 없다 |

Cloudflare Worker 중계는 **선택지로만** 남긴다(필수 아님). 들어온다면 "폰 → Worker 큐 → 맥의
어댑터가 바깥으로 가져오는" 수집함 어댑터 하나일 뿐이라 이 설계를 바꾸지 않는다.

## 시나리오 (데스크톱 중심 기기)

- **S1 데스크톱 작업 중** — cmux 에서 Claude Code 옆에 TUI 를 띄워 두고 보드를 본다. 에이전트가
  바꾸면 바로 반영된다. 항목을 세션에 넘기거나 새 세션을 띄운다.
- **S2 폰에서 수집** — 폰의 투두 앱에 한 줄 적는다. 데스크톱으로 돌아와 TUI 수집함 탭을 열면
  그 항목이 보이고, 보드로 올릴 걸 고른다. 폰 쪽 항목은 그대로 둔다(정리는 사람이).

모바일 중심 기기(외부 투두 앱이 주 화면), 데스크톱 앱, CF Worker 는 이 조각 밖이다.

## 구조

```mermaid
flowchart LR
  subgraph clients[클라이언트 — 별도 프로세스]
    tui[rocky-tui]
    next[/rocky:next · MCP/]
  end
  subgraph daemon[rockyd — 정본]
    api[REST · SSE]
    inbox[GET /api/inbox<br/>TTL 60s 캐시]
    db[(SQLite)]
    api --- db
    inbox --- api
  end
  subgraph bridges[bridges/&lt;name&gt; — 어댑터 = 명령]
    gt[gtasks]
    fl[file]
  end
  cfg[/rocky.json<br/>todo.inbox[]/]
  tui -- "REST / SSE" --> api
  next -- REST --> api
  cfg -. 등록 .-> inbox
  inbox -- "argv 실행 → stdout JSON" --> gt
  inbox -- "argv 실행 → stdout JSON" --> fl
  gt -. "op read (토큰)" .-> vault[(1Password<br/>Agent Vault)]
```

경계 판정 — 내부를 읽지 않고도 무엇을 하는지 알 수 있는가:

| 단위 | 하는 일 | 모르는 것 |
|---|---|---|
| 어댑터 (`bridges/<name>`) | 외부 앱의 미완료 항목을 규약 JSON 으로 낸다 | 보드·데몬·TUI 의 존재 |
| `rockyd` `/api/inbox` | 등록된 어댑터를 돌려 합쳐 준다 | 외부 앱이 무엇인지, 인증 |
| `rocky-tui` | 보드와 수집함을 보여주고, 사용자의 키 입력을 REST 호출로 바꾼다 | 어댑터, DB |

## 1. 수집함 어댑터 규약

**설정** — user `rocky.json` 의 `todo` 블록(데몬 설정) 안:

```json
{
  "todo": {
    "inbox": [
      { "name": "gtasks", "command": ["/Users/me/.local/bin/rocky-inbox-gtasks"] },
      { "name": "file",   "command": ["sh", "bridges/file/inbox.sh", "~/inbox.json"], "timeoutMs": 5000 }
    ]
  }
}
```

- `name`: 소스 식별자. `[a-z0-9-]+`, 보드 key 와 같은 규칙. 응답과 링크 제목에 쓰인다.
- `command`: argv 배열. **셸을 거치지 않는다**(기존 `Runner` 와 같은 원칙 — 주입 없음).
- `timeoutMs`: 기본 10 000. 넘기면 죽이고 그 소스만 `available: false`.
- `rocky.schema.json` 과 `crates/rocky-core/src/config.rs` 를 lockstep 으로 고친다(체크리스트 5).

**실행** — stdin 없음, 인자는 `command` 그대로. env 는 데몬의 것을 물려준다. **토큰은 어댑터가
1Password Agent Vault 에서 `op read` 로 읽는다**(홈 디렉토리에 평문 토큰 파일을 두지 않는다).
데몬은 인증을 모른다. `bridges/` 는 공개 레포에 있으므로 계정 식별자·메일 주소를 코드에 박지
않는다 — 어느 계정인지는 `op` 항목 이름을 env 나 인자로 받는다.

**출력** — stdout 에 JSON 하나, exit 0:

```json
{
  "items": [
    {
      "id": "MTIz",
      "title": "보드 TUI 수집함 탭 설계",
      "url": "https://tasks.google.com/task/MTIz",
      "note": "본문(옵션, markdown 아님, 평문)",
      "due": "2026-10-01",
      "createdAt": "2026-09-27T01:02:03Z"
    }
  ]
}
```

- `id`·`title` 필수. `url`·`note`·`due`(YYYY-MM-DD)·`createdAt`(RFC 3339) 옵션.
- 완료된 항목은 내지 않는다 — "미완료 목록" 이 규약이다. 정렬은 어댑터 몫(보통 최신순).
- exit ≠ 0 이거나 JSON 이 아니면 그 소스는 `available: false, reason: <stderr 첫 줄 또는 파싱 에러>`.
  다른 소스는 정상.
- 규약 버전은 두지 않는다 — 필드 추가는 호환이고, 깨는 변경이 필요해지면 그때 `version` 을
  더한다(YAGNI).

**테스트용 어댑터** — `bridges/file/inbox.sh`: 인자로 받은 JSON 파일을 그대로 `cat` 한다.
데몬 라우트 테스트와 TUI 수동 확인이 이걸 쓴다. 실제 앱 어댑터는 후속 조각.

## 2. 데몬 — `GET /api/inbox`

```
GET /api/inbox?refresh=true
→ 200 {
  "sources": [
    { "name": "gtasks", "available": true,  "fetchedAt": "…", "items": [ …규약 item… ] },
    { "name": "file",   "available": false, "reason": "exit 1: no such file", "items": [] }
  ]
}
```

- 설정된 소스가 없으면 `sources: []` (에러 아님 — TUI 는 탭을 숨긴다).
- 소스별로 **동시에** 실행하고(`tokio::join`), 소스별 **TTL 60초 캐시**. `refresh=true` 는
  캐시를 무시한다. TUI 의 탭 진입은 캐시, 명시적 새로고침 키는 `refresh`.
- 실행은 기존 `rockyd::runner::Runner` 를 그대로 쓴다(`default_runner`, 주입 가능 → 테스트는
  가짜 러너). timeout 은 소스 설정값.
- **로컬 전용이 아니다.** 수집함 내용은 보드 내용과 같은 급의 개인 데이터라 `todo.expose` 의
  노출 판단을 그대로 따른다. 실행되는 명령은 요청이 아니라 설정에서 오므로 원격 요청으로
  임의 명령을 돌릴 길은 없다.
- 순수 판정(응답 JSON 파싱·검증, 캐시 만료 판정)은 `rocky_core::inbox` 에, 라우트·실행은
  `rockyd::server` 에(코딩 규칙: 순수 로직은 core, 데몬은 배선).
- MCP 도구는 **늘리지 않는다**(5개 유지). 에이전트가 수집함을 보려면 `/rocky:next` 가 REST 로
  읽는다 — 그건 후속이고, 이 조각에서 `/rocky:next` 는 손대지 않는다.

## 3. 보드로 올리기

TUI 가 기존 `POST /api/todos` 를 쓴다. 데몬 변경 없음.

```json
{ "board": "<현재 보드>", "title": "<item.title>", "description": "<item.note>",
  "section": "백로그", "due": "<item.due>",
  "links": [ { "url": "<item.url>", "title": "<source>: <item.title>" } ] }
```

- `section: "백로그"` 는 없으면 만들어진다(`ensure_section` — todo 생성 경로의 기존 동작). 섹션 이름은 TUI 설정으로 바꿀 수 있게 두되 기본은 `백로그`.
- **중복 판정은 TUI 가 한다** — 현재 보드 todos(이미 들고 있음)의 `links[].url` 에 같은 url 이
  있으면 수집함 목록에서 "올라감 ✓" 으로 표시하고 다시 올리지 않는다. `url` 이 없는 항목은
  판정 불가 — 그대로 보인다(어댑터가 url 을 주는 게 정상 경로).
- 올린 뒤 외부 앱 쪽은 **건드리지 않는다**(결정 2). 완료 반영도 없다.

## 4. `rocky-tui`

**크레이트** — `crates/rocky-tui`, 바이너리 `rocky-tui`. 의존: `ratatui` + `crossterm`(**새 런타임
의존 둘** — TUI 를 만들기로 한 결정이 곧 이 의존의 승인이다. 워크스페이스 `Cargo.toml` 에 한 번
선언), HTTP 는 이미 있는 `ureq`, JSON 은 `serde_json`. `rocky-core` 의 타입(`TodoView`,
`Board`, …)을 그대로 쓴다.

**진입** — `rocky tui` 서브커맨드가 **옆에 있는 `rocky-tui` 를 exec** 한다(CLI 가 옆의 `rockyd`
를 찾는 것과 같은 규약). 릴리스 tarball 에 `rocky-tui` 를 더한다. 부트스트랩의 필수 검사는
`rocky`·`rockyd` 그대로 — `rocky-tui` 가 없으면 `rocky tui` 만 "TUI 바이너리가 없다" 로 실패한다.

**보드 선택** — 기동 시 cwd 로 유추(CLI 와 같은 규약: `boards.path` 하위 → key 경로 세그먼트).
`--board K` 로 고정. 탭 키로 보드 전환.

**화면** — 하나의 창, 두 탭 + 상세 패널:

| 탭 | 내용 |
|---|---|
| 보드 | 섹션별 항목(진행중 항목은 `doingState` 로 live/idle/gone 표시), 오른쪽 상세 패널(설명·댓글·히스토리·링크·**GitHub 상태**) |
| 수집함 | 소스별 항목. `available: false` 소스는 이유와 함께 접힌 채. 이미 올라간 항목은 ✓ |

키(첫 판, 바꿔도 됨): `j/k` 이동 · `Enter` 상세 · `s` start · `d` done · `a` archive · `h` 핸드오프
(세션 목록 `GET /api/sessions` 에서 고름, 후보 1개면 바로) · `n` 새 세션(`POST …/spawn`) ·
`i` 이슈 생성(`POST …/issue`) · `p` 보드로 올리기(수집함) · `r` 새로고침(수집함은 `refresh=true`)
· `Tab` 탭 · `q` 종료.

**갱신** — `GET /api/events` SSE 를 별도 스레드가 읽어 채널로 넘기고, 이벤트가 오면 해당
보드를 **refetch** 한다(계약: 구독자는 payload 를 보지 않고 refetch 만). 끊기면 1·2·4·8초
백오프로 재연결하고 재연결 직후 전체 refetch. 데몬이 없으면 화면 상단에 "데몬 없음 — 재시도
중" 을 띄우고 계속 시도한다(TUI 가 데몬을 띄우지는 않는다 — 그건 훅·CLI 몫).

**GitHub 상태** — 상세 패널에서 `links[].url` 중 `github.com/<o>/<r>/(issues|pull)/<n>` 꼴만
골라 `gh issue view <url> --json state` / `gh pr view <url> --json state,isDraft,mergedAt,
reviewDecision,statusCheckRollup` 로 읽어 한 줄(`PR #139 · open · CI ✓ · 리뷰 대기`)로 보여준다.
TUI 가 직접 `gh` 를 부른다(데몬은 관여 없음 — 읽기이고 로컬 인증). 5분 캐시, `gh` 가 없거나
실패하면 그 줄만 비운다. 항목당 링크가 여럿이면 각각 한 줄. 이슈 **생성**은 기존 데몬 라우트
(로컬 전용 — TUI 는 루프백이라 통과).

**핸드오프·spawn** — 기존 라우트 그대로. spawn 의 409(60초 창)·400 은 메시지로 보여준다.
핸드오프 응답의 `poke` 는 TUI 가 보낼 수 없다(`SendMessage` 는 에이전트 표면) — 상세 패널에
"대기 중 — 세션이 다음 턴에 집어간다" 로 표시만 한다.

## 실패 처리

| 상황 | 동작 |
|---|---|
| 어댑터 exit ≠ 0 / timeout / JSON 아님 | 그 소스만 `available:false` + reason. 나머지 정상 |
| 데몬 죽음 | TUI 는 백오프 재연결, 마지막 화면 유지 + 상단 경고 |
| SSE 끊김 | 재연결 + 전체 refetch |
| `gh` 없음·실패 | GitHub 줄만 비움. 나머지 무관 |
| 올리기 실패(400/404) | 메시지 표시, 항목은 수집함에 남음 |

## 검증

- `rocky-core/tests/inbox_test.rs` — 규약 JSON 파싱(필수 필드, 잘못된 due, 빈 items), 캐시 만료.
- `rockyd/tests/inbox_test.rs` — 가짜 `Runner` 로: 소스 2개 중 하나 실패, timeout, `refresh`,
  설정 없음 → `[]`. 파일 어댑터 `bridges/file/inbox.sh` 실제 실행 1건.
- `rocky-tui` — 순수 부분(중복 판정, GitHub URL 골라내기, gh JSON → 한 줄, 키 → 액션 매핑)은
  단위 테스트. 렌더는 ratatui `TestBackend` 스냅샷 최소 2장(보드/수집함). 나머지는 수동.
- 기존 표면 테스트(`TOOLS` 목록, REST 계약)는 변화 없어야 한다 — MCP 도구 5개 유지.

## 문서·규칙 변경

- `AGENTS.md` Scope Out 의 "외부 태스크 서비스 연동 금지" → **"연동 코드는 `bridges/<name>/` 에,
  데몬·CLI 는 규약(`/api/inbox` 명령 어댑터)으로만 안다. 데몬·CLI·MCP 도구에 특정 서비스
  이름이 들어가면 위반"** 으로 고친다 — **이 PR 에서 함께 고쳤다**(규칙과 스펙이 어긋난 채로
  머지되지 않게). Layout 에 `bridges/`, `crates/rocky-tui` 추가는 디렉터리가 생기는 PR 1·2 에서.
- `README.md` 에 TUI 절과 `todo.inbox` 설정, `docs/board.md` 에 어댑터 규약(정본은 이 문서가
  아니라 그쪽 — 이 스펙은 결정 기록).
- `rocky.schema.json` ↔ `config.rs` lockstep.

## 조각 순서 (PR)

1. **데몬 inbox** — `todo.inbox` 설정 + `GET /api/inbox` + `bridges/file/` + 규약 문서 + Scope
   수정. 데몬만 바뀌고 화면은 없다.
2. **TUI 기본** — `rocky-tui` 크레이트: 보드 탭 + SSE 갱신 + 상태 변경 + `rocky tui` 진입 +
   릴리스 tarball. 여기서 새 의존 둘이 들어온다.
3. **TUI 나머지** — 수집함 탭 + 올리기, 핸드오프/spawn, GitHub 상태 + 이슈 생성.
4. (후속, 별도 설계 없이) 첫 실제 어댑터 — 구글 투두 또는 Todoist, `bridges/<name>/`.

이후 조각(별도 설계): 모바일 중심 기기(외부 투두 앱이 주 화면), 데스크톱 앱, CF Worker 수집함 어댑터.
