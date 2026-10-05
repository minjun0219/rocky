# rc 서버 — `claude rc` 서버 보기 · 띄우기 · 재시작

> rocky 를 **고치는** 에이전트용 개발 문서. 설계: [`2026-10-05-rc-server-design.md`](../design/specs/2026-10-05-rc-server-design.md).

## 규칙

- `rc` 블록(사용자 설정만)의 폴더와 떠 있는 `claude rc` 서버를 맞대 `GET /api/rc/servers`·`rocky rc` 로 낸다. 사람이 부르면
  띄우거나 다시 띄운다(`POST /api/rc/servers/:label/{start,restart}` — 로컬 전용, `rocky rc start|restart`). 스스로 되살리는
  감시는 다음 조각.
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

## 코드

| 무엇 | 어디 |
| --- | --- |
| 판정(순수) | `crates/rocky-core/src/rc.rs` |
| 프로브·기동기(`RcController`)·라우트·agy 손잡이 | `crates/rockyd/src/rc.rs`, `crates/rockyd/src/server.rs` |
| CLI | `crates/rocky-cli/src/rc_cmd.rs` |
| 웹 | `web/components/RcPane.tsx`, `web/lib.ts`(`rcVisible`) |

테스트: `crates/rocky-core/tests/it/rc_test.rs`, `crates/rockyd/tests/it/rc_test.rs`, `crates/rockyd/tests/it/rc_launch_test.rs`(가짜 프로세스 세계 + 진짜 `sleep` 으로 새 그룹 확인),
`crates/rocky-cli/tests/it/rc_cmd_test.rs`.
