# rc 서버 관리 2조각 — 띄우기·재시작 실행 계획

> **날짜**: 2026-10-05 · **보드**: rocky-28 · **설계**: [`specs/2026-10-05-rc-server-design.md`](../specs/2026-10-05-rc-server-design.md) ·
> **앞 조각**: 1조각(읽기 전용 현황) #320 · #321 · #330 머지

2조각은 데몬이 rc 서버를 **띄우고 다시 띄운다**. 사람이 누를 때만 움직이고, 스스로 살리는 감시는 3조각이다.
이 조각부터 데몬이 장수 프로세스를 낳으므로, 데몬이 교체돼도 서버가 사는지(이관 조건 ②)를 코드로 지키고 실측한다.

## 0. 먼저 잰 것 — bootout 에서도 새 프로세스 그룹은 산다

설계의 갈래 (가)는 "새 프로세스 그룹으로 띄우면 launchd 가 잡을 내려도 서버가 산다"에 기댄다. 1조각까지 확인한 것은
옛 CLI 잡이 **스스로 끝날 때**뿐이었다. 데몬 교체 때 실제로 일어나는 것은 `launchctl bootout`(강제로 내림)이라 따로 쟀다
(2026-10-05, 임시 LaunchAgent, KeepAlive).

| 자식 | 프로세스 그룹 | bootout 뒤 |
|---|---|---|
| `sleep 901` | 부모와 같음 | 죽음 |
| `sleep 902` | 새 그룹(`setpgid(0,0)`) | 살아남아 PPID 1 |

데몬이 launchd 밖에서 SIGTERM 으로 내려갈 때(`stop_daemon`)는 그룹을 정리하지 않으므로 더 쉽다 — `kill_on_drop` 만
쓰지 않으면 된다.

## PR 나누기

| PR | 내용 | 예상 크기(테스트 제외) |
|---|---|---|
| 2a | `rocky_core::rc` 판정 — 기동 모드, 등록 결과, 백오프 · argv | 150~200줄 |
| 2b | `rockyd::rc` 기동기 · 정지 · 재시도, `POST` 라우트(로컬 전용), `rocky rc start\|restart`, 이벤트 JSONL | 350~400줄 |
| 2c | 웹 — 행마다 띄우기 · 재시작 버튼, 진행 중 표시 | 150~200줄 |

2b 는 2a 에, 2c 는 2b 의 응답 모양에 기대므로 `gh stack` 으로 쌓는다.

## 2a — 순수 판정 (`rocky_core::rc`)

옛 CLI 의 `decide.go` · `registration.go` 를 옮긴다. 테스트 이름도 그쪽 것을 따른다(동등성 비교가 쉽게).

| 함수 | 규칙 | 설계 6절 |
|---|---|---|
| `restart_mode(pinned, live_session, fresh)` | 열린 세션이 있고 `fresh` 가 아니면 `Resume`(`-c`). 없거나 `fresh` 면 고정=`Session`, 그 밖=`Server`(`--no-create-session-in-dir`) | 6 |
| `START_MODE` | 꺼진 대상을 이름으로 띄우면 고정이든 아니든 `Session` — 옛 CLI 의 "이름 직접 → session". `Server` 는 3·4조각의 되살림에서 쓴다 | 6 |
| `retry_mode(pinned)` | `Resume` 이 안 떴을 때 한 번 더 띄울 모드 — 고정=`Session`, 그 밖=`Server` | 7 |
| `server_argv(label, mode)` | `claude rc --name <label> [-c \| --no-create-session-in-dir]` — 셸을 거치지 않는다 | 2·3 |
| `read_registration(out, err)` | `.err` 에 `already served` → `Served`. `.out` 에 `· Connected ·` / `· Ready ·` → `Connected`. 그 밖 → `Pending` | 5·8 |
| `REGISTRATION_BACKOFF` | 45초 · 90초(합 3분 안) | 5 |
| `RESTART_DELAY` | 내린 뒤 5초 쉬고 띄운다 | 5 |
| `STOP_GRACE` | SIGTERM 뒤 20초, 그래도 살면 SIGKILL | 5 |

`--session-id`(특정 세션으로 고정)는 가져오지 않는다 — 사람이 손으로 쓰던 비상 수단이고, 화면에서 부를 일이 없다.

## 2b — 기동기 · 라우트 · CLI

### 띄우기 (`rockyd::rc::launch`)

- `std::process::Command` + `process_group(0)`(새 그룹), `current_dir(<대상 폴더>)`, stdin `/dev/null`,
  stdout · stderr 는 `<todo dir>/rc/<label>.out` · `.err` 파일(기동마다 덮어쓴다 — 등록 판정이 읽는다).
- **`kill_on_drop` 을 쓰지 않는다.** 핸들은 좀비를 거두는 대기 스레드에만 넘기고 놓는다. 데몬이 먼저 내려가면 서버는
  PPID 1 로 넘어가고 launchd 가 거둔다.
- env 는 데몬 것을 물려주되 `XPC_SERVICE_NAME` 만 뗀다(`launched_by_launchd` 오판 방지 — 1조각 설계 2.1).
  `PATH` 는 plist 가 설치 때의 PATH 를 굽고 있어 `claude` 를 찾는다(이 맥은 `~/.local/bin` 포함, 실측).
- 대상이 설정(`pinned` · `targets`)에 있을 때만 띄운다. 이름은 라벨로 받고 경로는 설정에서 푼다 — 화면이 경로를 넘기지 않는다.

### 내리기 · 재시작

- 내리기: 서버 pid 에 SIGTERM → 200ms 간격으로 확인하며 20초 → 살아 있으면 SIGKILL. **pid 로만** 다룬다(패턴 kill 없음).
- 재시작: 내리기 → 5초 → 띄우기 → 등록 판정.
- 등록 판정: 3초 뒤부터 2초 간격, 최대 40초. `Served` → 45 · 90초 뒤 다시 띄운다. 프로세스가 사라졌고 모드가 `Resume`
  이면 `retry_mode` 로 한 번 더. 신호 없이 끝까지 떠 있으면 `Up(미확인)`.
- 대상마다 `tokio::sync::Mutex` — 같은 대상에 겹친 요청은 앞 것이 끝날 때까지 409(`진행 중`). 다른 대상은 동시에 된다.
- 결과는 백그라운드로 돈다(재시도까지 최대 몇 분). 라우트는 바로 202 를 내고, 진행 상태는 현황 응답의 `action`
  (`starting` · `restarting` · `retrying`)과 마지막 결과(`lastResult`)로 보인다.

### 라우트 · CLI

- `POST /api/rc/servers/:label/start` · `/restart`(`{"fresh": true}` 선택) — **로컬 전용**(`is_local_request`, 프로세스를
  띄우는 동작이라 spawn 과 같은 등급). cross-site 차단은 이미 라우팅 전에 있다. rc 가 꺼진 기기면 404.
- `KNOWN_SURFACES` 에 둘을 넣고, `normalize_route` 가 `/api/rc/servers/:label/start` 를 접게 `("rc","servers")` 를
  넷째 자리 접기에 더한다(1조각 리뷰가 미리 짚은 것).
- `rocky rc start <label>` · `rocky rc restart <label> [--fresh]` — 데몬에 요청하고, `--wait` 면 결과가 날 때까지 현황을
  읽어 찍는다. 확인 관문(`permissions.ask`)은 5조각에서 사용자 설정에 둔다 — 이 조각에서 코드로 막지 않는다.

### 이벤트 로그

`<todo dir>/rc/events.jsonl` 에 한 줄씩 — `start` · `stop` · `retry` · `result`(라벨 · 모드 · pid · 판정 · 걸린 시간).
3·4조각의 동등성 비교(이관 조건 ④)에 쓴다. 로그 색인에 싣는 것은 필요해질 때.

## 2c — 웹

- 원격 제어 탭의 대상 행 오른쪽에 주 액션 하나(`web/DESIGN.md` "행"): 꺼졌으면 **띄우기**, 떠 있으면 **재시작**.
  대상 밖 서버에는 버튼이 없다(설정에 없는 것은 화면이 움직이지 않는다).
- **재시작은 확인 창을 띄운다** — 그 서버에 붙은 원격 세션이 끊기고 되돌릴 수 없다. 열린 세션이 있으면 "세션 N 개가
  끊기고 이어받기(-c)로 다시 뜬다"를 적는다. 띄우기는 확인 없이 바로.
- 진행 중이면 행에 `띄우는 중…` · `재시작 중…` · `다시 시도(45초 뒤)…`, 버튼은 잠근다. 진행 중일 때만 폴링을 3초로.
- 버튼은 로컬에서 연 화면에서만 보인다(`/api/health` 의 `spawnAllowed` — 기존 세션 띄우기와 같은 판정).
- 요약 줄(피드)에는 버튼을 두지 않는다 — 누르면 탭으로 간다.

## 하지 않는 것(이 조각)

- 스스로 되살리기 · 주기 기동(`rc.supervise`) — 3조각. 야간 재시작 · `claude update` · 버전 기록 — 4조각.
- 서버 내리기만 하는 버튼 — 옛 CLI 에도 없었다.
- 자가-살해 가드 — 데몬은 어느 세션의 조상도 아니다. 세션이 자기 서버를 재시작하면 그 세션이 끊기는 것은 사람이 고른
  결과라 막지 않고, 확인 창(웹)과 CLI 출력이 그 사실을 알린다.
- 옛 CLI 와의 잠금 공유 — 옛 주기 잡(5분)과 겹쳐 같은 폴더에 둘이 뜨면 한쪽이 `already served` 로 내려가고 재시도가
  푼다. 옛 잡은 3조각에서 끈다.

## 확인 — 이 조각이 머지되기 전에

1. 게이트 6개.
2. **조건 ② 실측**: 개발 데몬을 전용 launchd 라벨(`ROCKY_LAUNCHD_LABEL` + 전용 `ROCKY_CONFIG`)로 상주시키고, 빈 시험
   폴더 하나를 대상으로 띄운 뒤 `rocky daemon restart` → 서버 pid 가 그대로인지 본다.
3. **조건 ① 실측(오너)**: 데몬이 띄운 서버에 앱으로 붙어 `git push` 를 한 번 해 본다. 실패하면 3조각 전에 원인부터 본다.
4. 이 맥의 실제 서버 하나(가벼운 비고정 대상)를 웹에서 재시작해 옛 CLI `-r` 과 같은 결과(이어받기 · 등록 확인)가 나는지.
