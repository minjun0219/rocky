# rc 서버 현황 — `claude rc` 서버 보기

> rocky 를 **고치는** 에이전트용 개발 문서. 설계: [`2026-10-05-rc-server-design.md`](../design/specs/2026-10-05-rc-server-design.md).

## 규칙

- 지금은 **보기만** 한다(띄우기·감시는 다음 조각). `rc` 블록(사용자 설정만)의 폴더와 떠 있는 `claude rc` 서버를 맞대
  `GET /api/rc/servers`·`rocky rc` 로 낸다.
- **서버 판정은 argv 구조**(`claude` + `rc`/`remote-control` + `--name`). 자식 세션은 같은 `ps` 한 번의 ppid 로, cwd 는 `lsof` 한 번으로.
- **맞대기는 cwd 문자열**이라 정규화하지 않는다.
- 블록이 없거나 `enabled: false`(rc 를 못 쓰는 기기)면 **`claude rc` 프로브를 돌리지 않는다**. *EN: read-only for now; match by
  the raw cwd string; never probe `claude rc` without an `rc` block.*
- **Antigravity(`agy remote-control`)는 rc 블록과 별개다**(2026-10-05 오너 결정): `agy` 가 설치돼 있으면 블록이 없어도
  `agy remote-control status` 만 재서 `antigravity` 에 싣는다.
  켜기·끄기(`POST /api/rc/antigravity/{start,stop}`, `rocky rc agy start|stop`)는 로컬 전용이고, 동작 이름은
  `AgyAction` 둘뿐이다 — agy 에 임의 하위 명령·플래그를 넘기지 않는다. 손잡이는 조회와 **같은 캐시**를 잠그고 돌린 뒤 다시
  잰다(`rc_handles`) — 끈 직후의 `status` 는 launchd 중간값(`SIGTERMed`)이라 자리 잡을 때까지 0.5초 간격으로 최대 6번(`AgyAction::settled`). agy 데몬은 agy 가 올린 launchd 잡이라 rockyd 의 자식이 아니다. 에이전트가 부를 때의 확인은
  `permissions.ask` 에 `Bash(rocky rc agy:*)` 로 받는다(설계 8절 3번과 같은 방식).
- rc 서버는 데몬의 자식이 아니다 — 데몬 재시작·교체가 서버에 닿지 않아야 한다.

## 코드

| 무엇 | 어디 |
| --- | --- |
| 판정(순수) | `crates/rocky-core/src/rc.rs` |
| 프로브·라우트·agy 손잡이 | `crates/rockyd/src/rc.rs`, `crates/rockyd/src/server.rs` |
| CLI | `crates/rocky-cli/src/rc_cmd.rs` |

테스트: `crates/rocky-core/tests/it/rc_test.rs`, `crates/rockyd/tests/it/rc_test.rs`, `crates/rocky-cli/tests/it/rc_cmd_test.rs`.
