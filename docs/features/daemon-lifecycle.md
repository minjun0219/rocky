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
- **main 빌드 모드 — 부트스트랩.** `$XDG_DATA_HOME/rocky/current` 가 `main` 을 가리키면 부트스트랩은 플러그인 버전과 상관없이
  `main/rocky`(그 기기에서 GitHub main 을 release 빌드한 것)를 실행하고, 링크를 버전으로 되돌리지 않는다. `ROCKY_BIN` 이 먼저고,
  `main/rocky` 가 없으면(교체 중·지워짐) 그 한 번은 릴리스 경로로 떨어지지만 링크는 그대로 둔다 — 모드는 링크로만 바뀐다.
  이 링크가 곧 모드다 — 릴리스 모드면 `current` → `v<version>`.
- **`rocky update --main [SHA]`** 가 기기를 main 빌드 모드로 바꾸고 올린다. 자리는 `$XDG_DATA_HOME/rocky/` 의 `main-src/`(전용
  clone — `rocky-marketplace` 가 가리키는 폴더 마켓플레이스)와 `main/`(`rocky`·`rockyd`·`dist/`·`.sha`). 처음이면 마켓플레이스를
  GitHub 에서 그 폴더로 바꾸는데, 마켓플레이스를 빼면 그 플러그인의 저장된 옵션·데이터가 지워지므로 몇 분짜리 빌드 **전에** 확인을
  받는다(`--yes` 로 건너뛰고, 터미널이 아니면 멈춘다). 다른 폴더·원천을 가리키면(개발용 연결) 손대지 않는다. 버전 문자열이 같아도
  커밋이 다르면 빌드하고, 훅은 버전이 같아 교체하지 않으므로 새 빌드로 `daemon restart` 를 직접 부른다(재시작이 커밋까지 확인한다).
  빌드 뒤 단계(재시작·플러그인)가 실패하면 끝까지 갔다는 표식(`main/.finished`)이 안 남아, 다음 실행은 빌드 없이 그 단계만
  마저 한다(`main_build::plan`). 커밋이 같아도 그 커밋에 TUI 번들 스크립트가 있는데 `main/tui.js` 가 없으면 다시 빌드한다 — TUI 를
  모르던 옛 rocky 가 빌드한 설치다. 이 모드에서 `rocky update` 는 main 최신으로 올린다 — 습관대로 불러도 릴리스로 되돌리지 않는다(모드는
  `--main`·`--release` 로만 바뀐다). `--check` 는 어떤 조합이든 아무것도 바꾸지 않는다. 잠금은 `main-sync.lock` — 같은 일을 하던
  기기별 스크립트와 서로 막고, 안에 pid 를 남겨 Ctrl-C 로 끊긴 빌드의 잠금은 넘겨받는다.
- **`rocky update --release`** 가 릴리스로 돌아간다. 마켓플레이스 판정(`main_build::release_step`)과 최신 릴리스 조회를 먼저 끝내고
  확인을 받은 뒤 마켓플레이스를 GitHub 로 되돌려(이미 GitHub 면 갱신만) 플러그인을 다시 깐다. **`current` 는 그다음에 푼다** — 새
  플러그인의 부트스트랩이 main 빌드를 놓고 릴리스를 받게. 받지 못하면 `current` 를 `main` 으로 되돌려 둔다(앞 단계에서 실패해도
  main 빌드는 계속 돈다). 받은 바이너리로 데몬을 재시작하고(버전이 main 빌드와 같으면 훅이 안 바꾸므로) 재시작이 성공했는지와 데몬이
  최신 릴리스 버전을 보고하는지를 같이 확인한다. `--main` 과 같은 잠금을 잡는다.
  다른 폴더 마켓플레이스(개발용)는 건드리지 않는다. `main/`·`main-src/` 는 남긴다.
- **검증 통과 커밋 자동 배포.** `verify.targets[].deploy` 를 켠 main 빌드 기기에서는 검증 잡이 통과한 커밋을
  `<데몬 옆 rocky> update --main <sha>` 로 올린다. update 가 이 데몬을 교체하므로 떨어진 프로세스 그룹으로 띄우고, 끝은 대개 새 데몬이
  `main/.finished` 로 판정한다. 다른 main 빌드가 `main-sync.lock` 을 쥐면 기다린다. 규칙은 [verify](./verify.md) "배포".
- **버전 인식 재기동.** 훅이 `/api/health` 의 `version` 을 자기 `CARGO_PKG_VERSION` 과 **정확한 문자열**로 비교해 낡은 데몬을
  교체한다 — pid 로 SIGTERM, 상주 중이면 launchd 가 다시 띄우게 한다(아래 launchd 재시작). `name` 은 `"rocky"` 여야 한다. 버전이 같으면 경로가 달라도
  두는 것이 의도다. `UserPromptSubmit`(`notify-todo`)이 `RestartPolicy::OnlyIfOlder` 로 같은 검사를 해서 `/reload-plugins` 가
  세션끼리 뒤집히지 않고 올린다.
  - **`/api/health` 의 `commit`(빌드한 커밋의 짧은 해시)은 훅의 교체 판단에 넣지 않는다** — main 빌드끼리는 버전이 같아 커밋으로
    비교하면 훅이 세션마다 서로 다른 빌드를 번갈아 올린다. 웹 하단·`rocky version`·`daemon status` 가 버전 오른쪽 괄호에 붙이고,
    커밋까지 보는 곳은 둘뿐이다: `daemon restart` 의 "내 빌드가 떴나" 확인(`version::same_build`, 둘 다 알 때만)과 웹의 새로고침
    권유(화면 번들이 바뀌니까).
  - **해시는 release 빌드에만 굽는다**(`crates/rocky-core/build.rs`). rocky-core 는 모든 크레이트의 바닥이라 해시가 바뀌면 워크스페이스
    전체가 다시 컴파일된다 — dev 빌드(테스트·clippy·pre-push·`rocky verify`)까지 구우면 Rust 를 안 고친 커밋 뒤에도 매번 통째로
    빌드한다. 설치되는 바이너리(릴리스 tarball·main 빌드)는 모두 release 다.
  - **옛 데몬을 못 내리면 재기동하지 않는다** — 구버전 보드가 보드 없음보다 낫다. *EN: an old board beats no board.*
  - 교체의 각 단계는 실패를 삼키지 않고 보고한다. 데몬이 사라지고 말았으면 launchd 밖에서라도 띄우고 `⚠ rocky 데몬: …` 경고를 주입한다.
- **launchd 밖의 고아를 만들지도 두지도 않는다.** job 이 로드돼 있으면 CLI·훅은 따로 띄우지 않고 `launchctl kickstart` 로 맡긴다.
  교체 뒤엔 포트의 pid 가 job 의 pid 와 같고 목표 버전인지 확인한다(다르면 그 고아를 pid 로 내린다). `daemon restart`·`update` 는
  버전이 목표와 다르면 실패로 끝나고, `daemon status` 는 둘이 다르면 ⚠. launchd 가 띄운 데몬은 포트가 차 있으면 끝나지 않고
  기다렸다 이어받는다. 그래도 kickstart 가 시간 안에 못 띄워 CLI 가 직접 띄우는 마지막 수단에는 plist 에 구운 PATH 를 앞에
  붙여 넘긴다(`launchd::spawn_path`) — 좁은 PATH 의 앱(로그인 때 뜬 GUI 앱의 자식)이 띄운 고아가 `claude` 를 못 찾아 rc 기동이
  `os error 2` 로 실패했다.
- **launchd 재시작은 plist 가 그대로면 건드리지 않는다.** 교체(훅·`daemon restart`·`update`)는 디스크의 plist 가 지금 구울 내용과
  같고 launchd 가 쥔 job 의 `program`(`launchctl print`)이 같은 실행 파일이면 `launchctl kickstart -k` 로 데몬만 다시 띄운다
  (`launchd::restart_launchd`·`plan_restart`). plist 가 다르거나, job 이 로드되지 않았거나, 쥔 정의가 다르거나, kickstart 가
  실패하면 plist 를 (다를 때만) 쓰고 bootout → bootstrap 으로 다시 등록하고, 그 사유를 stderr 에 한 줄 남긴다(`rocky: launchd job 을
  다시 등록한다 — …`). 재시작은 plist 에 구워 둔 PATH 를 그대로 쓴다 — 부른 쪽(세션·터미널·데몬이 띄운 배포)마다 PATH 가 달라,
  다시 구우면 재시작마다 plist 가 바뀐다. PATH 를 새로 굽는 길은
  `rocky daemon install` 이다(늘 쓰고 다시 등록한다). plist 의 데몬 경로도 버전이 바뀌어도 그대로다(`exec_through_link`): main
  빌드는 `main/rockyd`, 릴리스는 `current` 가 그 설치본을 가리킬 때 `current/rockyd`. 다른 설치본을 가리키면 자기 옆 `rockyd` 를
  굽는다 — 그 링크로 띄우면 다른 버전이 뜬다. 그래서 rockyd 는 dist 를 실제 파일 옆에서 찾는다(링크는 도는 동안 바뀔 수 있다).
  *EN: Restarts never rewrite an unchanged plist — they `kickstart -k`; only `rocky daemon install` re-bakes PATH.*
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
| main 빌드 모드(`update --main`·`--release`) | `crates/rocky-cli/src/update_main.rs`, 판정은 `crates/rocky-core/src/main_build.rs` |
| 링크·설정 점검(`config show/link`) | `crates/rocky-cli/src/config_cmd.rs`, `crates/rocky-core/src/setup.rs` |
| 기동 순서(포트 → 옛 데몬 대기 → DB), `dbIntegrity` | `crates/rockyd/src/daemon.rs` |
| 부트스트랩(sh) | `plugin/bin/rocky` |

테스트: `crates/rocky-cli/tests/it/{hooks_test,launchd_test}.rs`, `crates/rocky-core/tests/it/setup_test.rs`,
`crates/rockyd/tests/it/shutdown_test.rs`, `scripts/bootstrap.test.ts`(부트스트랩·`current` 링크·main 빌드 모드),
`crates/rocky-core/tests/it/main_build_test.rs`(모드·다시 빌드할지·마켓플레이스 판정).

## 함정

- **DB 손상(2026-09-30)**: 업그레이드 때 종료 중인 옛 데몬과 새 데몬의 마이그레이션이 겹쳐 실제 DB 가 망가졌다 — 그래서 "포트 먼저,
  옛 데몬이 끝난 뒤 DB".
- **개발·데모 데몬**은 `ROCKY_CONFIG=<전용 파일> cargo run -p rockyd`(자기 포트·`dir`·`expose: "off"`) — 전역 `expose` 를 물려받지
  않게. launchd 동작 재현은 `ROCKY_LAUNCHD_LABEL` + 전용 `ROCKY_CONFIG`(기본 포트면 `rocky daemon` 이 거부한다).
- **폴더 마켓플레이스도 설치 때 캐시로 복사한다(2026-10-03 실측)** — 버전이 같으면 `claude plugin update` 가 "이미 최신" 이라 새
  커밋을 안 가져온다. 그래서 main 빌드는 플러그인을 빼고 다시 깐다.
- 프로세스는 **pid 로만** 내린다 — 패턴(`pkill -f`)은 다른 세션·서버를 같이 죽인 적이 있다.
- **보안 솔루션 탐지(2026-10-19)**: 회사 Mac 의 행위 기반 EDR 이 main 빌드의 `rocky daemon restart` 를 악성코드로 분류했다. 링커의
  ad-hoc 서명뿐인 방금 만든 바이너리가 LaunchAgent plist 를 다시 쓰고 `launchctl bootout`/`bootstrap` 하는 흐름이 macOS 지속 실행
  악성코드(MITRE T1543.001)와 같은 모양이라 그렇게 본 것으로 보인다(탐지 근거는 보안팀 콘솔에만 있다). 그래서 재시작은 plist 를
  쓰지 않는다 — 재시작마다 plist 를 다시 굽게 되돌리지 않는다.
