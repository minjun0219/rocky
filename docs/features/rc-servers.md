# rc 서버 — `claude rc` 서버 보기 · 띄우기 · 재시작

> rocky 를 **고치는** 에이전트용 개발 문서. 설계: [`2026-10-05-rc-server-design.md`](../design/specs/2026-10-05-rc-server-design.md).

## 규칙

- `rc` 블록(사용자 설정만)의 폴더와 떠 있는 `claude rc` 서버를 맞대 `GET /api/rc/servers`·`rocky rc` 로 낸다. 사람이 부르면
  띄우거나 다시 띄운다(`POST /api/rc/servers/:label/{start,restart}` — 로컬 전용, `rocky rc start|restart`). 원격 제어 탭의 동작(띄우기 · 재시작 ·
  닫기 · 야간)은 `access.remoteControl` 이면 검증된 Access 로그인에게도 열린다 — 판정은 [security](security.md)의 예외 절. 꺼진 고정
  서버는 감시가 되살린다(아래).
- **서버 판정은 argv 구조**(`claude` + `rc`/`remote-control` + `--name`). 자식 세션은 같은 `ps` 한 번의 ppid 로, cwd 는 `lsof` 한 번으로.
- **맞대기는 cwd 문자열**이라 정규화하지 않는다.
- 블록이 없거나 `enabled: false`(rc 를 못 쓰는 기기)면 **`claude rc` 프로브를 돌리지 않는다**. *EN: match by the raw cwd string;
  never probe `claude rc` without an `rc` block.*
- **Antigravity(`agy remote-control`)는 rc 블록과 별개다**(2026-10-05 오너 결정): `agy` 가 설치돼 있으면 블록이 없어도
  `agy remote-control status` 만 재서 `antigravity` 에 싣고, 웹은 그때도 원격 제어 탭을 보인다(`web/lib.ts` 의 `rcVisible`).
  켜기·끄기(`POST /api/rc/antigravity/{start,stop}`, `rocky rc agy start|stop`)는 로컬 전용이고, 동작 이름은
  `AgyAction` 둘뿐이다 — agy 에 임의 하위 명령·플래그를 넘기지 않는다. 손잡이는 조회와 **같은 캐시**를 잠그고 돌린 뒤 다시
  잰다(`rc_handles`) — 끈 직후의 `status` 는 launchd 중간값(`SIGTERMed`)이라 자리 잡을 때까지 0.5초 간격으로 최대 6번(`AgyAction::settled`). agy 데몬은 agy 가 올린 launchd 잡이라 rockyd 의 자식이 아니다. 에이전트가 부를 때의 확인은
  `permissions.ask` 에 `Bash(rocky rc agy start:*)` · `Bash(rocky rc agy stop:*)` 로 받는다(설계 8절 3번과 같은 방식 — 상태
  보기 `rocky rc agy` 는 묻지 않는다).
- **서버는 새 프로세스 그룹으로 띄우고 놓는다**(`process_group(0)`, `kill_on_drop` 금지, 핸들은 좀비를 거두는 스레드만). launchd 는
  데몬 잡을 bootout 할 때 잡의 프로세스 그룹만 정리한다 — 새 그룹의 자식은 산다(2026-10-05 임시 LaunchAgent 로 실측). 자식 env 에서
  `XPC_SERVICE_NAME` 을 뗀다. *EN: new process group, never kill_on_drop — servers must outlive daemon restarts.*
- **내리기는 pid 로만** — SIGTERM → 20초 유예 → SIGKILL(SIGKILL 은 claude.ai 쪽 등록을 남긴다). 패턴으로 고르지 않는다.
- **등록은 기동 로그로 판정**한다(`<todo dir>/rc/<라벨>.out`·`.err`) — 프로세스가 떠 있는 것만으로 "떴다" 하지 않는다. `already served`
  면 그 서버를 내리고 60·120초 뒤 다시(45·90초로는 못 풀린 재시작이 있었다 — 2026-10-06 실측, 내린 뒤 3분쯤에 풀렸다), `-c`(이어받기)나
  못 박은 세션이 뜨자마자 내려가면 새로 한 번 더.
- **기동 로그는 비우고 append 로 연다**(`server_log`) — 세션이 붙은 서버는 화면을 다시 그릴 때마다 덧붙여 분당 수십 KB 씩 자란다
  (2026-10-07 실측: 2시간 40분에 4.25MB). 데몬이 10분마다 1MiB 를 넘은 `rc/*.out` · `*.err`(핸드오프 서버 것 포함)를 비운다
  (`trim_logs` · `spawn_rc_log_trim` — 감시 · 야간 설정과 상관없이 `rc` 블록만 있으면 돈다). append 가 아니면 비운 뒤에도 서버가
  옛 위치에 이어 써 앞이 빈 큰 파일이 된다(배포 전부터 떠 있던 서버는 다음 재시작까지 그렇다 — 겉보기 크기만 크고 디스크는 안 쓴다).
  로그는 띄운 직후에만 읽으니(등록 판정 40초, 핸드오프는 세션 확인까지 길어야 100초쯤) 그 안에는 닿지 않는 크기이고, 크기를 잰
  순간과 비우는 순간 사이에 서버가 새로 뜨는 것은 기동(`spawn_logged` — 대상 · 핸드오프 공통)과 정리가 같은 잠금(`log_lock`)을
  쥐어 막는다. 링크는 따라가지 않는다. 비웠거나 못 비운 것이 있으면 이벤트 `log-trim`.
- 재시작은 열린 세션이 있을 때만 이어받는다 — **내리기 전에 그 서버의 자식 세션 명령줄에서 세션 id 를 읽어 `--session-id` 로
  못 박는다**(`rc::resume_session` · `session_id_of`). 세션이 여럿이면 받은편지함 등록 시각(훅이 턴마다 갱신)이 가장 최근인 것, 등록이
  없으면 가장 늦게 열린 것. id 를 못 읽을 때만 `-c` — `-c` 는 "그 폴더에서 마지막 서버가 처음 만든 세션" 을 되살려, 서버에 나중에 열린
  대화를 놓친다(2026-10-06 실측: 하던 대화 대신 9월에 시작한 첫 세션이 돌아왔다). 둘 다 단일 세션 모드다. 이름으로 띄우면 고정이
  아니어도 세션까지.
- **핸드오프 서버**(보드의 새 세션 띄우기 — [spawn](spawn.md)): 할 일의 워크트리에서 `claude rc --spawn session` 으로 띄운다(로그
  `rc/handoff-<보드>-<n>.out` · `.err`, 이벤트 `handoff-*`). 대상이 아니라 **감시 · 야간이 건드리지 않고** 현황에는 대상 밖 서버로
  보인다 — 띄울 때 남긴 기록(`rc/handoff/<라벨>.json`: 이름 · pid · 폴더 · 할 일 참조)과 **pid · 폴더가 둘 다 맞는** 대상 밖 서버는
  현황의 `handoffs` 로 가린다(`rc::split_handoffs` · `handoff_is_live`, pid 만 보면 재사용된 pid 를 잡는다). 현황은 5초 캐시라 낡을
  수 있다 — 기록은 프로브가 성공했고 **그 pid 가 정말 없을 때만** 지운다(막 띄운 서버가 낡은 스냅숏에 아직 없어도 지키게), 그 사이
  끝난 대상 밖 서버는 현황에서 뺀다. 지울 파일은 읽은 경로로 고른다(기록 안의 라벨로 경로를 만들지 않는다). 내리는 것은 사람이다
  (`POST /api/rc/handoffs/:ref/stop`, `rocky rc stop <할 일>`) — 캐시 없는 프로브로 지금 그 폴더에서 그 pid 로 도는 rc 서버일 때만
  pid 로 내리고, 프로브가 실패하면 손대지 않는다. pid 가 다른 폴더의 서버면 신호 없이 기록만 지운다. 워크트리는 남긴다. CLI 는 자기
  조상 사슬에 그 서버가 있으면 거절한다(재시작과 같은 자가-살해 가드).
- **대상 밖 서버 닫기**(`POST /api/rc/strays/:ref/stop`, `rocky rc stop <이름|pid>`, 웹 "대상 밖" 줄의 닫기): 폴더 이름이나 pid 로
  고르고(`rc::find_stray` — 이름이 겹치면 pid 를 대라고 거절), 캐시 없이 다시 잰 현황의 대상 밖 서버일 때만 pid 로 내린다. 설정 대상은
  대상 밖이 아니라 고를 수 없다 — 감시 · 재시작이 다룬다. 프로브가 실패하면 손대지 않는다. CLI 는 핸드오프 서버를 먼저 찾고(`stop_target`),
  자기 조상 사슬의 서버는 거절한다. `already served`(같은 폴더의 서버를
  방금 내렸다)는 다시 해 보지 않는다 — 요청이 몇 분씩 붙잡히지 않게 그 서버는 내리고 "3분쯤 뒤 다시" 를 알린다.
- **최근 활동**(`GET /api/rc/servers?activity=1`, `rocky rc --activity`): 대상마다 git 사실(저장소인지 · `status --porcelain -uno` ·
  HEAD 브랜치 · `origin/HEAD` — 없으면 `main` · `master` 순으로 짐작 · 마지막 커밋 시각과 제목)을 **부를 때만** 잰다 — 5초 폴링
  현황에는 없다. 대상마다 git 을 띄우므로 `activity=1` 은 **로컬 전용**(403). 대상끼리는 동시에 재고, git 은
  `--no-optional-locks` 로 부른다(사람이 작업 중인 저장소의 인덱스 잠금을 잡지 않는다). 꺼진 비고정 대상이 활동이 없으면(기본 브랜치에서 14일 넘게 조용하고 작업 중이 아님) "정박" — 판정 못
  한 것(git 아님 · 커밋 없음 · 미래 커밋)은 활성으로 둔다(`rc::is_active`). 옛 CLI `-s` 와 같은 기준이고, 기동 조건이 아니라 사람이
  "뭘 열까" 고를 때 보는 표시다.
- **일괄 띄우기**(`rocky rc start --all`, 옛 CLI `-a`): 꺼짐이 확실한 대상(`running: false`)마다 `start` 를 보낸다 — 비고정은 본문
  `serverOnly` 로 세션 없이 서버만(`Revive(Server)`, 떠 있으면 그대로), 고정은 감시가 되살릴 때처럼 세션과 함께. 현황을 못 읽으면
  (`probeError`) 아무것도 띄우지 않는다. 비상용이다.
- **턴 대기**: 내리기 직전에 열린 세션이 2분 안에 대화했으면(그 폴더 최상위 대화 기록 `*.jsonl` 의 mtime) 막 답하는 중이라 보고
  15초마다 캐시 없이 다시 재며 10분까지 기다린다(행 상태 `waiting`). 끝내 안 끝나면 **내리지 않고** 실패로 남긴다. 기다린 뒤에는
  설치 버전을 다시 잰다. **한계**: 2분 넘게 아무것도 쓰지 않는 도구 호출 · 서브에이전트는 "조용함"으로 보인다. 그래서 **자기 서버
  재시작은 CLI 가 거절한다** — `rocky rc restart` 가 자기 조상 사슬(ppid)에 그 서버 pid 가 있으면 멈춘다(옛 CLI 의 자가-살해 가드,
  설계 6절 1번). 기다리는 사이 서버가 내려가면 그냥 띄운다. **야간은 기다리지 않는다**(`Policy.turn_wait` 0) — 판정 뒤 대화가 다시
  시작된 서버는 손대지 않고 건너뛰고, 그것이 canary 였으면 다음 것이 canary 가 된다(바쁨을 canary 실패로 세지 않는다).
- **세션 못 박기**(`restart` 본문 `session`, `rocky rc restart <라벨> --session <id>`): `claude rc --session-id <id>` 로 그 세션을
  이어받는다. id 는 claude.ai 쪽(`cse_…` · `session_…`)만 받는다(`valid_session_id` — 로컬 전사본 UUID 는 서버가 400 으로 내려간다).
  `fresh` 와 같이 줄 수 없다(400), 문자열이 아니면 400. 라우트는 본문을 보기 전에 로컬부터 확인한다. 꺼져 있던 대상이면 그
  세션으로 띄운다(옛 CLI 는 무시하고 새로 띄웠다). 못 박은 id 는 `start` 이벤트의 `session` 에 남는다.
- **현황 프로브가 실패하면 손대지 않는다** — 꺼짐이 모름일 때 띄우면 떠 있는 서버를 하나 더 띄운다.
- **데몬 맥락이 확실히 로그아웃이면 손대지 않는다**(재시작이면 내리지도 않는다). launchd 로 도는 데몬은 자격을 키체인에서 읽어
  셸과 다를 수 있다 — 셸이 로그인돼 있어도 데몬이 띄운 서버는 "You must be logged in" 으로 곧 내려간다(2026-10-05 실측).
- 같은 대상에 겹친 요청은 409, 다른 대상은 동시에. 결과는 현황 행의 `action`·`lastResult`(메모리), 기록은 `rc/events.jsonl`.
- **감시(`rc.supervise`, 기본 꺼짐)**: 2분마다 캐시 없이 재서 **고정이면서 꺼진** 대상만 사람과 같은 길(`begin` → `run`)로 띄운다 —
  비고정은 띄우지 않는다(옛 CLI 에서 비고정 자동 기동이 반복되던 함정). 프로브 실패 · 확실한 로그아웃이면 아무것도 고르지 않는다.
  연달아 못 뜬 대상은 2 · 4 · 8 · 16 · 30분 쉰다. 같은 일을 하는 다른 주기 잡이 있으면 **둘을 동시에 켜지 않는다**(켜는 날 그쪽을 끈다).
- **되살림 표식**(`rc/<라벨>.revive`, 야간 재시작이 내리고 못 띄운 대상): 감시가 **비고정이어도 서버 모드로** 띄우고(고정은 그대로 세션까지),
  뜨면 지운다. 이미 떠 있거나 설정에서 뺀 라벨의 표식도 지운다 — 단 프로브가 실패한 바퀴에는 아무것도 지우지 않는다(`stale_revive_marks`).
  표식 없는 비고정은 여전히 띄우지 않는다. 감시의 기동(`RcCommand::Revive`)은 그새 누가 띄운 서버를 실패로 치지 않는다.
- **기동 버전 기록**(`rc/<라벨>.version`): 띄울 때마다 `claude --version` 을 재 남긴다(프로브와 같이 — 내린 뒤에 재지 않는다). **못 재면
  지운다** — 옛 값을 남기면 새 바이너리로 뜬 서버를 야간이 구버전으로 보고, 기록이 없으면 야간은 "모름" 으로 건드리지 않는다.
  새 바이너리의 첫 실행은 Gatekeeper 검사로 수십 초 멎어 한도가 40초다. 쓰기 · 지우기가 실패하면 기동은 그대로
  두고 `version-record` 이벤트(경로 · 원인)를 남긴다.
- **야간 리허설**(`GET /api/rc/nightly/preview`, `rocky rc nightly --dry-run`): 지금 설치 버전(`claude --version`, 못 재면
  `~/.local/bin/claude` 링크 대상 → `versions/` 중 가장 높은 것)으로 떠 있는 **설정 대상**마다 `decide_nightly` 를 돌려 보인다.
  update · 내리기 · 띄우기 · 기다리기 없이, `rc/` 에 아무것도 쓰지 않는다. 쉬는지는 `~/.claude/projects/<project_dir_name>/*.jsonl` 의
  최신 mtime 으로 잰다. 프로브 실패 · 로그아웃이면 `blocked`. 읽기지만 프로세스를 캐시 없이 띄우므로 **로컬 전용**(403).
- **야간 재시작(`rc.nightly`, 블록이 있으면 켜짐)**: 1분마다 "오늘 `at` 이 지났고 아직 안 돌았나"(`nightly_due`)를 보고, 돌 차례면
  **그 날을 먼저 `rc/nightly.json` 에 남긴 뒤** 돈다 — 도중에 데몬이 다시 떠도 같은 날 두 번 돌지 않는다. 기록이 없으면(처음 켠 날)
  이미 돈 것으로 친다(`nightly_first_mark`). 순서: `claude update`(실패해도 그때 설치 버전으로 판정, 버전은 90초까지 다시 재고 못
  재면 설치 경로) → 판정(`decide_nightly`) → 다시 띄울 대상을 **한꺼번에 잠금**(`begin`, 사람이 하던 대상은 건너뜀) → canary(고정
  하나)를 먼저, 뜬 뒤에 나머지 — **canary 가 못 뜨면 나머지는 내리지 않고 canary 부터 회복**해 보고, 마감 안에 뜨면 나머지를 이어서
  한다(끝내 안 뜨면 나머지는 그대로 — `canary_failed`) → 못 뜬 것 회복(`busyUntil` 까지 1 · 2 · 4 · 8 · 16분, **처음에 못 박은 세션
  (`--session-id`, `RcController.pins`)으로 다시** — 다음 대기가 마감을 넘는 마지막 시도만 새로; `already served` 는 그 자리에서
  기다리지 않고 다음 간격으로, 그새 떠 있으면 다시 띄우지 않는다) → 바쁜 것
  대기(5분마다, 풀리면 같은 순서). 마감이 지나 도는 실행(깬 뒤 따라잡기)은 기다리지 않는다.
  **내리기 직전마다 `curl -4` 로 네트워크**를 보고(10초 간격 3분), 안 닿으면 내리지 않는다. **내리기 전에 표식을 찍고** 뜨면 지운다.
  못 뜬 대상의 잠금은 **회복이 끝날 때까지 쥔다** — 놓으면 감시가 표식을 보고 같은 대상을 띄워 회복과 겹친다. 마감까지 못 띄운 것은
  표식을 남긴 채 놓는다 — 감시가 서버 모드로 띄운다. `already served` 재시도는 1 · 2 · 4 · 6분(낮보다 길게). 대상은 **설정 대상 중 떠 있는 것**뿐이다(strays 는 기록이 없다). 프로브 실패 · 로그아웃이면
  전체를 건너뛴다(`nightly_blocked`). 배너는 못 띄운 서버나 canary 실패가 있을 때만. 보고의 방식은 **실제로 뜬 방식**
  (`launched_mode` — 첫 방식이 안 떠 새로 띄웠으면 그것)이고 판정 때 정한 방식이 아니다(2026-10-07: 네트워크 장애로 회복이 새 세션으로
  떴는데 보고는 "이어받기(-c)" 라 적었다). 보고(`NightlyReport.rocky`)에 rocky 세 층 버전
  (`claude plugin list --json` · `ROCKY_USAGE=0 rocky --version` — 사용 로그를 오염시키지 않게 · 데몬 자기 버전)과 최신 릴리스 태그(`git ls-remote` — 30초 간격 4번)를 남긴다. 설치는
  하지 않는다. agy 도 남긴다(`NightlyReport.agy` — `agy --version` · `remote-control status`(둘 다 실패해야 미설치 — 상태만 실패하면 상태 칸만 빈다), 데몬 pid 의 `ps etime` 으로 기동 시각,
  PATH 의 실행 파일 mtime(`RcOps.binary_mtime`), 실행 파일이 기동보다 나중에 바뀌었으면 `oldBinary`) — 켜거나 끄지 않는다. 리허설은
  둘 다 싣지 않는다. 마지막 보고는 `rocky rc report`(읽기만 — `nightly` 와 이름을 갈라 `rocky rc nightly:*` 확인 규칙에 걸리지 않게 했다).
  마지막 보고는 처음 현황을 낼 때 `rc/nightly.json` 에서 읽는다 — 일정이 꺼진 기기에서 손으로 돌린 보고도 데몬을 다시 띄운 뒤에 보인다. 손 실행(`POST /api/rc/nightly`, `rocky rc nightly`)은 같은 길로 한 번 — 배너 없이, 날짜 기록(`lastRun`)도 건드리지 않는다.
- **자격 관찰**: 끊김(`In → Out`)과 회복(`Out → In`)을 바뀐 바퀴에 한 번씩만 배너로 알린다 — 같은 상태가 이어지면 다시 울리지 않는다.
  기록은 `rc/auth.json`(데몬을 다시 띄워도 회복을 알아채게). 회복 뒤 로그아웃보다 먼저 뜬 서버는 `authSuspect` — 자동으로 재시작하지
  않는다(세션이 붙어 있을 수 있다). launchd 로 도는 데몬은 키체인을 읽어 셸과 자격이 갈릴 수 있다(2026-10-06 두 번째 재발).

## 코드

| 무엇 | 어디 |
| --- | --- |
| 판정(순수) | `crates/rocky-core/src/rc.rs` |
| 프로브·기동기(`RcController`)·감시(`supervise_tick`)·라우트·agy 손잡이 | `crates/rockyd/src/rc.rs`, `crates/rockyd/src/server.rs` |
| 야간 재시작(`run_nightly` · `spawn_rc_nightly` · 리허설 `nightly_preview`) | `crates/rockyd/src/rc/nightly.rs` |
| CLI | `crates/rocky-cli/src/rc_cmd.rs` |
| 웹 | `web/components/RcPane.tsx`, `web/lib.ts`(`rcVisible`) |

테스트: `crates/rocky-core/tests/it/rc_test.rs`, `crates/rockyd/tests/it/rc_test.rs`, `crates/rockyd/tests/it/rc_launch_test.rs`(가짜 프로세스 세계 + 진짜 `sleep` 으로 새 그룹 확인), `crates/rockyd/tests/it/rc_log_test.rs`(커진 로그만 비움 · 비운 뒤 append 로 처음부터),
`crates/rockyd/tests/it/rc_nightly_test.rs`(가짜 프로세스 세계 + 가짜 시계 — canary · 회복 · 마감 · 네트워크 · 바쁨 · 일정),
`crates/rocky-cli/tests/it/rc_cmd_test.rs`.
