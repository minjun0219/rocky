# `claude rc` 서버 관리를 데몬으로 — 설계 초안

> **상태**: 승인(2026-10-05, (가)안) · **날짜**: 2026-10-05 · **보드**: rocky-28
> 코드 변경 없이 조사만 한 결과다. 「결정할 것」의 답이 나오면 조각별 계획으로 넘어간다.

## 1. 무엇을 옮기나

폴더마다 `claude rc`(Remote Control) 서버를 하나씩 띄워 두고, 죽으면 살리고, Claude Code가 업데이트되면
쉬는 서버만 골라 재시작하는 일이다. 지금은 별도 레포의 Go CLI(본체 약 4.4k줄, 테스트 약 2.8k줄,
테스트 함수 124개)가 launchd 잡 둘로 돈다.

| 잡 | 주기 | 하는 일 |
|---|---|---|
| 주기 실행 (`-q`) | 5분 + 로그인 시 | 고정 대상 중 꺼진 것과 되살림 표식이 붙은 것을 띄운다 |
| 야간 실행 (`--nightly`) | 04:30 | `claude update` → 구버전이면서 쉬는 서버만 재시작 |

사람은 터미널에서 현황 보기(`-s`), 재시작(`-r`), 띄우기(`<이름>`)를 쓴다.

**옮긴 뒤의 모양**: 감시·기동·재시작·야간 재시작은 `rockyd` 안의 루프가 하고, 현황 보기와 띄우기는 웹 UI와
`rocky rc` CLI가 한다. 어느 폴더를 상시로 둘지는 **설정 값**이다. rocky 코드는 그 값을 정하지 않는다.

## 2. 조사로 확인한 사실

### 2.1 프로세스 모델 — 이관 조건 ② (재시작·업데이트해도 서버가 산다)

- 지금 떠 있는 rc 서버 10개는 모두 `PPID 1`, `PGID == PID`다. 옛 CLI는 `nohup` + `Setpgid`로 띄우고 바로 놓는다.
  옛 CLI의 launchd 잡에도 `AbandonProcessGroup`은 없다. 그런데도 잡이 5분마다 끝날 때 서버는 산다.
  **launchd가 잡을 정리할 때 보는 것은 잡의 프로세스 그룹이고, 새 그룹으로 나간 프로세스는 정리 대상이 아니다.**
- 같은 방식을 rockyd에 쓰면 갈래 (가)가 된다. 새 프로세스 그룹으로 띄우고 `kill_on_drop`을 쓰지 않는 것이다.
  보드에는 (나)(대상마다 LaunchAgent)가 더 튼튼하다는 추측이 적혀 있었다. 하지만 위 실측을 보면 (가)로 충분하다.
  plist에 `AbandonProcessGroup`을 켜는 것은 보조 안전장치로 둔다.
- 지금 rockyd의 자식 실행 경로 두 개(`runner::default_runner`, `spawnctl::run_in_dir`)는 모두 `kill_on_drop(true)`이고
  데몬과 같은 그룹이다. 그래서 장수 프로세스용 경로를 **따로** 둔다. 기존 경로는 건드리지 않는다.
- 자식은 `XPC_SERVICE_NAME`을 물려받는다. `launched_by_launchd`의 오판을 피하려고 직접 띄울 때 이미 떼는 값이다.
  rc 서버를 띄울 때도 똑같이 뗀다.

### 2.2 push와 자격 — 이관 조건 ①

- 지금 rc 서버의 env에도 `SSH_AUTH_SOCK`이 없다. ssh 키는 `UseKeychain`으로 키체인에서 읽는다.
  그래서 ssh push는 **"키체인을 읽을 수 있는 launchd 도메인에서 떴는가"** 문제로 좁혀진다.
- 옛 CLI가 이미 밟은 함정이 있다. `user/<uid>`(Background) 도메인에서는 키체인을 못 읽어서 "Not logged in"
  서버가 떴다. 그래서 잡을 `gui/<uid>`(Aqua)로 되돌렸다. rocky의 plist도 `gui/<uid>`에 올라가니 같은 조건이다.
  다만 `LimitLoadToSessionType`은 없다.
- **지금 이 맥의 rockyd는 launchd 잡이 아니다.** 세션 훅이 detached로 띄운 것이다. 그러면 rc 서버의 자격 맥락이
  "어느 세션이 데몬을 처음 띄웠나"에 따라 달라진다. 상주 감시를 켜려면 **`rocky daemon install`이 전제**다.

### 2.3 rocky 쪽에서 그대로 쓸 수 있는 것

| 필요 | 기존 것 |
|---|---|
| 주기 루프 | `spawn_sweeper` / `spawn_inbox_watcher` 모양 (`tokio::spawn` + sleep 루프, `daemon.rs`에서 설정으로 게이트) |
| 띄우기 권한 | `is_local_request` (spawn 라우트와 같은 등급), cross-site 차단은 라우팅 전에 이미 있음 |
| 하위 명령 hang 방지 | `spawnctl`의 drain grace + kill 패턴. 옛 CLI는 같은 문제를 임시 파일 출력으로 풀었다 |
| 설정 파싱 | `config.rs` 블록별 로더 + `rocky.schema.json` (fail-open) |
| 사용 로그 | `KNOWN_SURFACES` (`usage.rs:235`) |

## 3. 접근안

**추천 — 데몬이 직접 띄우고 놓는다(갈래 가)**
- 새 프로세스 그룹으로 띄우고 핸들은 버린다. 감시는 매 주기 `ps` + `lsof`로 다시 찾는다.
  pid 파일을 두지 않는 것은 옛 CLI와 같다. 데몬이 재시작해도 상태를 다시 구성할 수 있다.
- 다만 이 방식에는 전제가 붙는다. **데몬이 상주해야 하고, `gui` 도메인이어야 한다.**

**기각 — 대상마다 LaunchAgent(갈래 나)**
- 서버 생존은 launchd가 보장한다. 하지만 plist를 rocky가 쓰고 지워야 한다.
- `-c`/`--session-id`처럼 기동마다 바뀌는 인자를 plist로 넘기기 어렵다.
- 판정 로직(already served 재시도, 단일 세션 모드)이 결국 데몬에 있어야 해서 이득이 적다.

**기각 — 옛 Go 바이너리를 데몬이 부른다**
- 데몬 코어는 Rust이고, 이관하는 취지와 맞지 않는다.

## 4. 구조

```
rocky_core::rc        순수 판정 — ps 출력 파싱(IsServerCommand), 대상 해석, 기동 모드 결정,
                      쉬는 서버 판정, 등록 결과 판정(.out/.err), 야간 순서, 자격 관찰
rockyd::rc            배선 — 프로브(ps·lsof·pgrep·claude auth status·claude --version),
                      기동/정지(새 그룹·SIGTERM 20s 유예), 감시 루프, 야간 루프, REST
rocky_cli rc          `rocky rc` (status / start / restart / log) — 데몬 REST의 얇은 클라이언트
web/                  rc 서버 현황 — 한 줄씩, 띄우기·재시작 버튼(로컬 요청일 때만 보임)
```

- **동시성**: 옛 CLI의 `mkdir` 잠금과 워치독은 데몬 안의 대상별 `Mutex`와 작업 타임아웃으로 바꾼다.
  단일 인스턴스는 이미 포트가 보장한다.
- **상태**: 버전 기록·되살림 표식·자격 관찰은 `todo.db`의 표 하나에 둔다(`rc_servers`).
  이벤트 로그는 JSONL로 남기고, 로그 색인이 이를 `logs.db`로 옮긴다. JSONL이 진실이라는 원칙과 같다.
- **REST**: `GET /api/rc/servers`(열림), `POST /api/rc/servers/:name/{start,restart}`(로컬 전용).
- **MCP 도구는 만들지 않는다**. 에이전트가 서버를 바꾸는 손잡이를 늘리지 않는다(6절의 확인 관문과 같은 이유).

## 5. 설정 모양 (안)

```jsonc
// ~/.config/rocky/rocky.json — 사용자 설정에만(전역 데몬이라 todo 블록과 같은 규칙)
"rc": {
  "enabled": true,                  // 기능 전체 스위치(기본 true). false 면 블록이 없을 때와 같다 — rc 를 못 쓰는 기기에서 끈다
  "supervise": false,               // 감시 루프. 기본 꺼짐 — 옛 CLI와 동시에 띄우지 않게
  "root": "~/dev/workspaces",       // 상대 경로의 기준
  "pinned": ["repo-a", "repo-b"],   // 늘 떠 있어야 하는 것(죽으면 살린다, 세션 모드)
  "targets": ["repo-c"],            // 화면에서 부를 수 있는 것(자동으로 띄우지 않는다, 서버만 모드)
  "nightly": { "at": "04:30", "busyUntil": "07:00", "quietMinutes": 60 }
}
```

- **명령은 설정 파일, 값은 화면**(수집함 어댑터와 같은 원칙): 화면은 `pinned`·`targets`에 있는 이름만 띄운다.
  임의 경로를 화면에서 받지 않는다.
- 레포 안에는 이 맥의 실제 목록을 쓰지 않는다. 예시 이름만 쓴다.

## 6. 옛 CLI와의 동등성 체크리스트

옛 CLI에서 밟고 고친 함정이다. 각 항목은 `rocky_core::rc`의 단위 테스트로 고정한다.

| # | 함정 | 가져갈 대응 |
|---|---|---|
| 1 | 자가-살해 — 세션 안에서 자기 서버를 내리면 호출자도 죽는다 | 데몬은 어느 세션의 조상도 아니라 **구조로 풀린다**. 다만 CLI가 부르는 경로에서 ppid 사슬 검사는 남긴다(데몬이 요청자 pid를 받아 판정) |
| 2 | `zsh -c "…claude rc --name…"` 셸을 서버로 오인 | argv 구조로 판정: basename `claude` + `rc`/`remote-control` + `--name` |
| 3 | 일회성 `claude rc -c`·`--help`를 서버로 오인 | `--name`이 있어야 서버 |
| 4 | macOS `pgrep`가 조상을 뺀다 | 서버 목록은 `ps -axww`, 자식은 `pgrep -a -P` |
| 5 | `409 already served` — 등록이 남아 새 서버가 45초 뒤 스스로 죽음 | SIGTERM 20초 유예 → 묶음 정지 뒤 5초 → `.err`의 `already served`만 45·90초 백오프로 재시도(야간 1·2·4·6분) |
| 6 | `-c` 서버는 세션이 끝나면 같이 죽는다 | 열린 세션이 있을 때만 `-c`, 없으면 고정=session / 그 밖=server(`--no-create-session-in-dir`) |
| 7 | `-c` 기록 만료(약 4시간)로 기동 실패 | resume·pin은 "실패할 수 있음" 표시 → down이면 기본 모드로 한 번 더 |
| 8 | 기동 성공 오판 | 3초 뒤부터 2초 간격 최대 40초: `.err` served=실패, 프로세스 없음=down, `.out`의 `· Connected ·`/`· Ready ·`=up, 신호 없음=up(미확인) |
| 9 | Not logged in 서버가 "실행 중"으로 남음 | `claude auth status --json` 프리플라이트 — 확실히 out일 때만 막고, 모르면 경고만 하고 진행 |
| 10 | 자격 회복 뒤 죽은 토큰 서버 | `last_out`/`last_in` 기록, `기동 < last_out`인 서버를 "자격 의심"으로 표시(자동 재시작 안 함) |
| 11 | 하위 호출 hang(손자가 파이프를 쥠) | 모든 프로브에 타임아웃 + drain grace |
| 12 | 업데이트 직후 `--version` 지연(Gatekeeper) | 90초까지 재시도 → 설치 경로 폴백, 못 재면 버전 기록을 덮지 않음 |
| 13 | 야간에 내린 서버 방치 | canary(고정 하나 먼저) → 네트워크 확인(IPv4 HTTP) → 재시작 → 07:00까지 백오프 recover → 끝내 실패하면 되살림 표식 |
| 14 | 비고정 자동 기동 반복 | 자동 기동은 `pinned`와 되살림 표식만 한다 |
| 15 | 폴더 개명 | 판정이 cwd 문자열이라 옛 서버가 남는다 — 현황에 "대상 밖 서버"로 보인다(정리는 사람) |

**쉬는 서버 규칙**: 열린 세션이 있고 **그리고** `~/.claude/projects/<dir 이름 변환>/*.jsonl`의 최신 mtime이
야간 60분(낮 재시작 2분) 안이면 바쁜 서버다. 둘 중 하나라도 아니면 쉬는 서버다.

**가져가지 않는 것**:
- 야간의 rocky 버전 확인. rocky 안에서는 `rocky update`가 맡는다.
- `--dry-run`. 화면이 현황을 보여 주므로 필요하면 나중에 넣는다.
- 터미널 컬러 렌더링.

**Antigravity 한 줄**(읽기 전용: `agy remote-control status`)은 현황 화면의 맨 아랫줄로 이어받을지 결정이 필요하다(8절).

## 7. 조각 — PR 단위

1. **현황(읽기 전용)**: `rocky_core::rc` 파싱·판정, 프로브, `GET /api/rc/servers`, `rocky rc status`, 웹 현황 한 줄씩.
   서버를 건드리지 않으니 옛 CLI와 같이 돌아도 안전하다.
2. **띄우기·재시작**: 장수 프로세스 경로(새 그룹), 정지 유예, already served 재시도, 기동 판정,
   `POST …/start|restart`(로컬 전용), 웹 버튼, `rocky rc start|restart`.
3. **감시 루프**: `rc.supervise`, 주기 기동(pinned + 되살림), 자격 프리플라이트·관찰. **이 조각부터 옛 주기 잡을 끈다.**
4. **야간 재시작**: `claude update`, 쉬는 서버, canary, 네트워크 확인, recover. **옛 야간 잡을 끈다.**
5. **확인 관문**(8절 결정에 따라) 및 옛 CLI 은퇴.

조각마다 300~400줄 안팎이 목표다. 2는 판정 테스트가 많아 넘칠 수 있고, 그러면 판정과 배선으로 다시 쪼갠다.

## 8. 결정할 것

1. **프로세스 모델**: 갈래 (가)(새 그룹 + 데몬이 놓기)로 가도 되는가. 결정: (가)로 간다(2026-10-05).
2. **상주 전제**: 3조각(감시) 전에 이 맥에서 `rocky daemon install`로 데몬을 launchd에 올리는 것을 전제로 둬도 되는가.
3. **확인 관문**: 에이전트가 `rocky rc restart`를 부를 때 사람 확인을 어떻게 받을지. 안은 셋이다.
   - (a) 플러그인 `hooks.json`에 PreToolUse(Bash) `rocky hook rc-ask`를 넣는다. 옛 셸 렉서를 Rust로 옮기는 안이고,
     모든 Bash 호출에 훅이 붙는다.
   - (b) 사용자 설정의 `permissions.ask`에 `Bash(rocky rc restart:*)`·`Bash(rocky rc start:*)`를 둔다. 코드는 0줄이지만
     `sh -c` 같은 래퍼로 우회된다.
   - (c) CLI의 변경 명령이 TTY가 아니면 거부하고 웹 버튼만 허용한다.
   - 결정: (b) + 웹 버튼. 옛 관문의 렉서(약 400줄)는 래퍼 우회를 막으려던 것인데, 손잡이가 화면으로
     옮겨 가면 에이전트가 부를 일이 드물다.
4. **Antigravity 현황 한 줄**: 이어받는다(읽기 전용, 1조각에 포함).
5. **웹 위치**: 보드에 묶이지 않는 전역 화면이다. 결정: 둘 다 — "지금" 표 위 요약 한 줄(누르면 펼침)과 전용 화면.
   `web/DESIGN.md`는 설정 화면을 패널에 두지 않는다. 그래서 목록을 고치는 것은 설정 파일로 하고, 화면은 현황과 버튼만 둔다.

## 9. 검증 — 은퇴 기준

- **조건 ①**: launchd(`gui`)로 상주하는 rockyd가 띄운 서버의 세션에서 `git push`(ssh)가 된다.
- **조건 ②**: `rocky daemon restart`와 `rocky update`(버전 교체)를 각각 한 번씩 거친 뒤 서버 pid가 그대로다.
  `launchctl bootout`도 한 번 확인한다.
- **조건 ④**: 2주간 rocky만 켜고, 이벤트 로그에서 기동·재시작·야간 결과를 옛 CLI 로그와 비교한다.
  그동안 옛 잡 둘은 끈다. 둘이 동시에 서버를 띄우지 않게 한쪽만 켠다.
