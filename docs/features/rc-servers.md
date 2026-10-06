# rc 서버 — `claude rc` 서버 보기 · 띄우기 · 재시작

> rocky 를 **고치는** 에이전트용 개발 문서. 설계: [`2026-10-05-rc-server-design.md`](../design/specs/2026-10-05-rc-server-design.md).

## 규칙

- `rc` 블록(사용자 설정만)의 폴더와 떠 있는 `claude rc` 서버를 맞대 `GET /api/rc/servers`·`rocky rc` 로 낸다. 사람이 부르면
  띄우거나 다시 띄운다(`POST /api/rc/servers/:label/{start,restart}` — 로컬 전용, `rocky rc start|restart`). 꺼진 고정
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
  `permissions.ask` 에 `Bash(rocky rc agy:*)` 로 받는다(설계 8절 3번과 같은 방식).
- **서버는 새 프로세스 그룹으로 띄우고 놓는다**(`process_group(0)`, `kill_on_drop` 금지, 핸들은 좀비를 거두는 스레드만). launchd 는
  데몬 잡을 bootout 할 때 잡의 프로세스 그룹만 정리한다 — 새 그룹의 자식은 산다(2026-10-05 임시 LaunchAgent 로 실측). 자식 env 에서
  `XPC_SERVICE_NAME` 을 뗀다. *EN: new process group, never kill_on_drop — servers must outlive daemon restarts.*
- **내리기는 pid 로만** — SIGTERM → 20초 유예 → SIGKILL(SIGKILL 은 claude.ai 쪽 등록을 남긴다). 패턴으로 고르지 않는다.
- **등록은 기동 로그로 판정**한다(`<todo dir>/rc/<라벨>.out`·`.err`) — 프로세스가 떠 있는 것만으로 "떴다" 하지 않는다. `already served`
  면 그 서버를 내리고 45·90초 뒤 다시, `-c`(이어받기)가 뜨자마자 내려가면 새로 한 번 더.
- 재시작은 열린 세션이 있을 때만 이어받는다(`-c` 는 단일 세션 모드). 이름으로 띄우면 고정이 아니어도 세션까지.
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
  하나)를 먼저, 뜬 뒤에 나머지. 바쁜 것은 다음 날로.
  **내리기 직전마다 `curl -4` 로 네트워크**를 보고(10초 간격 3분), 안 닿으면 내리지 않는다. **내리기 전에 표식을 찍고** 뜨면 지운다.
  못 뜬 것은 표식을 남긴 채 잠금을 놓는다 — 감시가 서버 모드로 띄운다. `already served` 재시도는 1 · 2 · 4 · 6분(낮보다 길게). 대상은 **설정 대상 중 떠 있는 것**뿐이다(strays 는 기록이 없다). 프로브 실패 · 로그아웃이면
  전체를 건너뛴다(`nightly_blocked`). 배너는 못 띄운 서버나 canary 실패가 있을 때만. 손 실행(`POST /api/rc/nightly`, `rocky rc nightly`)은 같은 길로 한 번 — 배너 없이, 날짜 기록(`lastRun`)도 건드리지 않는다.
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

테스트: `crates/rocky-core/tests/it/rc_test.rs`, `crates/rockyd/tests/it/rc_test.rs`, `crates/rockyd/tests/it/rc_launch_test.rs`(가짜 프로세스 세계 + 진짜 `sleep` 으로 새 그룹 확인),
`crates/rockyd/tests/it/rc_nightly_test.rs`(가짜 프로세스 세계 + 가짜 시계),
`crates/rocky-cli/tests/it/rc_cmd_test.rs`.
