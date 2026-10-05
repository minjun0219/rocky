# 기본 브랜치 검증 — main 에 들어온 커밋을 다시 돈다

> rocky 를 **고치는** 에이전트용 개발 문서. **쓰는** 법은 README "기본 브랜치 검증".

## 규칙

- **opt-in.** `rocky.json` 의 `verify.targets[]`(보드 · 브랜치 · 단계 argv)가 있을 때만. 명령은 설정 파일에만 — 화면·REST 가 바꾸지 않는다.
- **감지**는 `git ls-remote`(GitHub API 예산을 쓰지 않는다). 새 커밋이면 `<todo dir>/verify/<board>/<branch>/tree`(보드 레포의
  **detached** 워크트리 — 사람·세션의 작업 트리·브랜치를 건드리지 않는다)에서 단계를 차례로 돈다. 디렉터리 이름은 읽기 쉬운 부분 +
  원래 이름의 SHA-256 앞 8자(`release/a` 와 `release_a` 가 겹치지 않게).
- 레포 전체 `worktree prune` 은 하지 않는다(사용자의 다른 워크트리 등록을 지울 수 있다) — 깨진 그 트리만 지우고 `--force` 로 다시.
  보드 레포의 git 훅은 돌리지 않는다(`core.hooksPath=/dev/null`).
- **잡 하나가 대상을 차례로** — 동시 1개, 몰린 커밋은 최신 하나.
- **준비(fetch·워크트리) 실패는 커밋을 빨강으로 남기지 않는다** — `error` 만 싣고 다음 바퀴에 다시.
- **단계는 자기 프로세스 그룹**으로 띄운다(cargo·bun 이 띄운 손자까지). 남지 않게 세 겹: 시간 초과면 TERM → 5초 → KILL, 데몬이 작업을
  버리면 가드가 KILL, 데몬이 죽어 남은 그룹은 `running.pgid`(그룹 id + 리더 시작 시각 — 리더가 살아 있는데 시각이 다르면 번호
  재사용이라 건드리지 않는다)로 다음 실행 전에 끝낸다. 그룹을 건드린 판단은 전부 데몬 로그와 대상의 `signals.log` 에 남는다.
- **기록**: `last.json`(도는 중이면 다음 기동에 같은 커밋을 다시), `finished.json`(알림 기준 — 끊겼다 다시 돈 실행도 복구를 알린다),
  커밋별 `<sha12>.log`(0600, 최근 10개). 단계 argv 는 로그에 남기지 않는다(토큰이 있을 수 있다) — 이름과 실행 파일만.
- 실패·복구만 배너. 조회는 `GET /api/verify`·`rocky verify`. 히스토리·웹 "지금"·세션 받은편지함에는 아직 싣지 않는다.

## 코드

| 무엇 | 어디 |
| --- | --- |
| 판정(원격 커밋·다시 돌지·알릴지·디렉터리 이름) | `crates/rocky-core/src/verify.rs` |
| 설정 | `crates/rocky-core/src/config.rs`(`load_verify_block`) + `rocky.schema.json` |
| 잡·git·프로세스 그룹·기록 | `crates/rockyd/src/verify.rs` |
| CLI | `crates/rocky-cli/src/verify_cmd.rs` |

테스트: `crates/rocky-core/tests/it/verify_test.rs`, `crates/rockyd/tests/it/verify_test.rs`(임시 bare 원격 + clone 으로 통과·실패·
같은 커밋 생략·복구·시간 초과·손자 정리·번호 재사용·준비 실패), `crates/rocky-cli/tests/it/verify_cmd_test.rs`.

## 함정

- **같은 커밋의 실패는 다시 돌지 않는다** — 환경 탓 거짓 실패(부하로 부트스트랩 테스트 시간 초과, 2026-10-05)가 나면 다음 커밋까지
  빨강으로 남는다. 지금은 `last.json` 을 지워 다시 돌린다(다시 돌리기 기능은 후속).
- `cargo test` 단계 뒤 그룹에 프로세스가 남는 기록이 반복된다 — 남기는 테스트를 찾는 중(`signals.log` 의 `남은 손자`).
- 검증 트리의 `target/` 은 수 GB 다.
