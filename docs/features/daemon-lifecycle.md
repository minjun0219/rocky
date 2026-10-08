# 데몬 수명 — 설치·기동·업데이트·교체

> rocky 를 **고치는** 에이전트용 개발 문서. 쓰는 법은 [`README.md`](../../README.md), 왜 이렇게 됐는지(사고 경위)는
> [`docs/daemon.md`](../daemon.md). 규칙을 바꾸면 이 문서와 코드를 같이 고친다.

## 규칙

- **설치 = 활성화.** `todo.enabled` 는 없다. 끄려면 `claude plugin disable rocky`.
- **터미널의 `rocky`.** 부트스트랩이 `~/.local/bin/rocky` → `~/.local/share/rocky/current/rocky` 링크를 건다(`link_cli`,
  남의 실제 파일은 덮지 않는다). `rocky config show` 가 링크·PATH 누락을 알려 주고 `rocky config link` 가 고친다. 옆 바이너리는
  canonicalize 한 실제 파일 옆에서 찾는다.
- **기동.** SessionStart 의 `hook ensure-daemon` 이 health 가 없으면 detached 로 띄운다. CLI 도 필요할 때 띄운다.
  `rocky daemon install` 이 상주시킨다(launchd KeepAlive). `rocky daemon restart` 는 버전과 상관없이 같은 교체 경로
  (`RestartPolicy::Always`)를 탄다.
- **`rocky update [--check]`** 는 마켓플레이스 갱신 → `claude plugin update` → **새 버전 폴더의 부트스트랩**
  (`<cache>/rocky/<최신>/bin/rocky hook ensure-daemon`)으로 데몬 교체를 한 번에 한다 — 지금 도는 `rocky` 는 옛 바이너리라 자기
  자신으로는 새 바이너리를 못 받는다. 목표 버전은 GitHub 최신 릴리스 태그. 할 일 수정은 `rocky edit`(예전 이름이 `update` —
  REF·수정 플래그가 붙은 `rocky update` 는 업데이트를 돌리지 않고 `edit` 으로 안내한다). 0.36.0 의 `rocky upgrade` 는 숨은 별칭으로
  아직 남아 있다("한 릴리스만" 이라던 것 — 걷는 일은 보드 할 일).
- **버전 인식 재기동.** 훅이 `/api/health` 의 `version` 을 자기 `CARGO_PKG_VERSION` 과 **정확한 문자열**로 비교해 낡은 데몬을
  교체한다 — pid 로 SIGTERM, 상주 중이면 launchd job 을 다시 설치. `name` 은 `"rocky"` 여야 한다. 버전이 같으면 경로가 달라도
  두는 것이 의도다. `UserPromptSubmit`(`notify-todo`)이 `RestartPolicy::OnlyIfOlder` 로 같은 검사를 해서 `/reload-plugins` 가
  세션끼리 뒤집히지 않고 올린다.
  - **옛 데몬을 못 내리면 재기동하지 않는다** — 구버전 보드가 보드 없음보다 낫다. *EN: an old board beats no board.*
  - 교체의 각 단계는 실패를 삼키지 않고 보고한다. 데몬이 사라지고 말았으면 launchd 밖에서라도 띄우고 `⚠ rocky 데몬: …` 경고를 주입한다.
- **launchd 밖의 고아를 만들지도 두지도 않는다.** job 이 로드돼 있으면 CLI·훅은 따로 띄우지 않고 `launchctl kickstart` 로 맡긴다.
  교체 뒤엔 포트의 pid 가 job 의 pid 와 같고 목표 버전인지 확인한다(다르면 그 고아를 pid 로 내린다). `daemon restart`·`update` 는
  버전이 목표와 다르면 실패로 끝나고, `daemon status` 는 둘이 다르면 ⚠. launchd 가 띄운 데몬은 포트가 차 있으면 끝나지 않고
  기다렸다 이어받는다.
- **첫 세션 순서**(SessionStart ↔ http MCP 초기화)는 보장되지 않는다 — 첫 세션의 MCP `failed` 는 `/mcp` 재시도, 다음 세션,
  launchd 로 풀린다.
- **전역 단일 인스턴스.** 포트가 락이다. 사용자 `rocky.json` 의 `todo` 블록만 적용된다. 새 데몬은 **포트를 먼저 잡고**
  `daemon.pid` 의 옛 rockyd 가 끝난 뒤에야 DB 를 연다(마이그레이션 포함, 15초 넘게 안 끝나면 DB 를 열지 않고 멈춘다). 기동 때
  `quick_check` 결과는 `/api/health` 의 `dbIntegrity`.

## 코드

| 무엇 | 어디 |
| --- | --- |
| 훅 입구(`ensure-daemon`·`notify-todo`의 재기동 검사, `RestartPolicy`) | `crates/rocky-cli/src/hooks.rs` |
| `daemon install/restart/status`, `update` | `crates/rocky-cli/src/commands.rs`, `crates/rocky-cli/src/launchd.rs` |
| 링크·설정 점검(`config show/link`) | `crates/rocky-cli/src/config_cmd.rs`, `crates/rocky-core/src/setup.rs` |
| 기동 순서(포트 → 옛 데몬 대기 → DB), `dbIntegrity` | `crates/rockyd/src/daemon.rs` |
| 부트스트랩(sh) | `plugin/bin/rocky` |

테스트: `crates/rocky-cli/tests/it/{hooks_test,launchd_test}.rs`, `crates/rocky-core/tests/it/setup_test.rs`,
`crates/rockyd/tests/it/shutdown_test.rs`, `scripts/bootstrap.test.ts`(부트스트랩·`current` 링크).

## 함정

- **DB 손상(2026-09-30)**: 업그레이드 때 종료 중인 옛 데몬과 새 데몬의 마이그레이션이 겹쳐 실제 DB 가 망가졌다 — 그래서 "포트 먼저,
  옛 데몬이 끝난 뒤 DB".
- **개발·데모 데몬**은 `ROCKY_CONFIG=<전용 파일> cargo run -p rockyd`(자기 포트·`dir`·`expose: "off"`) — 전역 `expose` 를 물려받지
  않게. launchd 동작 재현은 `ROCKY_LAUNCHD_LABEL` + 전용 `ROCKY_CONFIG`(기본 포트면 `rocky daemon` 이 거부한다).
- 프로세스는 **pid 로만** 내린다 — 패턴(`pkill -f`)은 다른 세션·서버를 같이 죽인 적이 있다.
