# rc 서버 현황 — `claude rc` 서버 보기

> rocky 를 **고치는** 에이전트용 개발 문서. 설계: [`2026-10-05-rc-server-design.md`](../design/specs/2026-10-05-rc-server-design.md).

## 규칙

- 지금은 **보기만** 한다(띄우기·감시는 다음 조각). `rc` 블록(사용자 설정만)의 폴더와 떠 있는 `claude rc` 서버를 맞대
  `GET /api/rc/servers`·`rocky rc` 로 낸다.
- **서버 판정은 argv 구조**(`claude` + `rc`/`remote-control` + `--name`). 자식 세션은 같은 `ps` 한 번의 ppid 로, cwd 는 `lsof` 한 번으로.
- **맞대기는 cwd 문자열**이라 정규화하지 않는다.
- 블록이 없거나 `enabled: false`(rc 를 못 쓰는 기기)면 **프로브를 돌리지 않는다**. *EN: read-only for now; match by the raw cwd
  string; never probe without an `rc` block.*
- rc 서버는 데몬의 자식이 아니다 — 데몬 재시작·교체가 서버에 닿지 않아야 한다.

## 코드

| 무엇 | 어디 |
| --- | --- |
| 판정(순수) | `crates/rocky-core/src/rc.rs` |
| 프로브·라우트 | `crates/rockyd/src/rc.rs`, `crates/rockyd/src/server.rs` |
| CLI | `crates/rocky-cli/src/rc_cmd.rs` |

테스트: `crates/rocky-core/tests/it/rc_test.rs`, `crates/rockyd/tests/it/rc_test.rs`, `crates/rocky-cli/tests/it/rc_cmd_test.rs`.
