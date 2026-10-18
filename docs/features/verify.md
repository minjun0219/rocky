# 기본 브랜치 검증 — main 에 들어온 커밋을 다시 돈다

> rocky 를 **고치는** 에이전트용 개발 문서. **쓰는** 법은 README "기본 브랜치 검증"과 사용자 스킬
> [`plugin/skills/branch-verify`](../../plugin/skills/branch-verify/SKILL.md).

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
  커밋별 `<sha12>.log`(0600, 최근 10개 — 같은 커밋을 또 돌면 비우지 않고 구분선과 함께 이어 쓴다). 단계 argv 는 로그에 남기지 않는다(토큰이 있을 수 있다) — 이름과 실행 파일만.
- **이력** `runs.jsonl`: 끝난 실행(`passed`·`failed`, 자동 재시도 전의 첫 실패 포함), 끊긴 실행(`interrupted` — 다음 실행이 시작될 때),
  검증을 못 한 이유(`error` — 이유가 바뀔 때만, 기동 때 마지막 줄에서 되살려 재시작을 넘어서도 중복하지 않는다). 256KB 를 넘으면
  `.1` 로 한 번 돌린다(`signals.log` 와 같은 헬퍼).
- **자동 재시도**: 실패하면 그 자리에서 한 번 더(`MAX_ATTEMPTS`, 처음 단계부터, 출력은 같은 로그에 이어서). 첫 실패는 이력에만 —
  두 번 연속일 때만 Failed 기록·배너. 기록의 `attempt` 가 2 면 `rocky verify` 가 "통과(다시 돌려서)" 로 보인다.
- **다시 돌리기**: 그 뒤 같은 커밋의 실패는 `rocky verify --rerun`/`POST /api/verify/rerun`(로컬 전용 — 프로세스를 띄운다)으로만.
  필터(`board`·`branch`)를 빼면 전부, 잘못 준 값은 400. 요청은 메모리(데몬이 다시 뜨면 다시 부탁), `verify_wake` 로 잡을 바로 깨운다
  (기동 직후 첫 대기 포함). "도는 중" 은 `last.json` 의 `running` 이 아니라 잡이 실제로 단계를 도는지(`VerifyQueue.in_flight`)로 —
  `running` 은 데몬이 죽은 뒤에도 남는다. 요청 맡기기와 돌기 시작(요청 흡수)은 한 락 아래(틈에 맡긴 요청이 같은 커밋을 또 돌린다).
  다시 돌린 실행은 기록의 `rerun` 으로 남는다.
- 실패·복구만 배너. 조회는 `GET /api/verify`·`rocky verify`. 히스토리·웹 "지금"·SSE(`/api/events`)에는 아직 싣지 않는다.
- **구독한 세션에는 끝난 실행마다(통과도) 받은편지함으로 한 줄**(`session_notice` → `server::notify_verify_subscriber`) — 배포를 맡은
  세션이 폴링하지 않고 이것으로 움직인다. 같은 커밋은 한 번: 직전 판정과 커밋·결과가 같으면(다시 돌렸는데 그대로) 보내지 않고,
  결과가 바뀌면 보낸다. 자동 재시도 전 첫 실패는 판정이 아니라 보내지 않는다. 전달 기록 kind 는 `verify-passed`·`verify-failed`.
- **구독은 대상(보드·브랜치)마다 세션 하나**(`verify_subscriptions`, 다시 구독하면 넘겨받기) — `rocky verify subscribe [BOARD]
  [--branch B]`(세션 안에서만, `CLAUDE_CODE_SESSION_ID`)·`unsubscribe`·`subscriptions`, `GET|POST|DELETE /api/verify/subscriptions`(바꾸기는
  로컬 전용, 원격 조회는 세션 id 를 가린다). 보드는 별칭까지 풀어 설정의 대상 key 로 적는다 — 설정에 없는 대상은 404.
  보내는 규칙은 PR 알림과 같다: "보내지 않기" 면 보내지 않고, `/clear` 된 세션·받은편지함 등록 없는 세션이면 사유만 남긴다.
  `/clear` 결정에서 넘기기는 후계로 옮기고, 지켜보기만·해지는 걷는다(세션 없이는 보낼 곳이 없다 — 수집함 구독과 같다).
- **넘겨받기는 조용하지 않다** — 구독 응답에 이전 세션(`previousSessionId`)과 지금 결과(`record`)를 싣고 CLI 가 찍는다(구독 전에 끝난
  통과는 다시 오지 않는다). 세션 안의 `unsubscribe` 는 그 세션 것만 걷는다(`DELETE …?sessionId=`).
- **끝난 세션의 구독은 걷지 않는다** — 다음 세션이 다시 구독하면 넘겨받고, 그 사이 결과는 전달 기록에 "받을 세션 등록 없음" 으로
  남는다(수집함처럼 TTL 로 걷으면 배포 세션이 잠깐 끊긴 사이에 구독이 사라진다).

## 배포(`deploy`)

검증을 통과한 커밋을 데몬이 `rocky update --main <sha>` 로 올린다. 결정과 버린 것은
[설계](../design/specs/2026-10-18-verify-auto-deploy-design.md)에 있고 규칙의 정본은 여기다.

- **조건 넷을 모두 채울 때만**(`deploy_gate`): 대상의 `deploy: true`, main 빌드 모드(`current` → `main`), 보드 `repo` 가
  `main_build::REPO`, 브랜치 `main`(`update --main` 은 origin/main 위의 커밋만 받는다). 데몬은 다른 블록처럼 사용자 `rocky.json` 만
  읽고, 스위치는 기동 때 읽는다(바꾸면 `rocky daemon restart`).
- **대상은 마지막으로 통과한 커밋**(`passed.json`, 없으면 통과로 끝난 `finished.json`)이다. 뒤 커밋이 실패해도 그대로 대상이다. 통과한
  순간이 아니라 바퀴마다 검증 뒤에 맞춘다(데몬이 다시 떠도 놓치지 않게).
- **판정 순서**(`deploy_plan`): 조건 → 도는 배포 → 통과 없음 → 40자 16진이 아님 → 이미 깔림(`main/.finished`) → 이미 띄움(`deploy.json`)
  → 깔린 커밋을 못 읽음(`.finished`, 없으면 `.sha`) → 앞으로만 → 잠금 → 띄운다. 조상 여부와 잠금은 띄울 수 있을 때만 본다(평소에는
  git 을 부르지 않는다).
- **앞으로만 간다.** 깔린 빌드가 통과한 커밋의 조상일 때만 띄운다(보드 레포에서 `git merge-base --is-ancestor`). 확인하지 못하면 띄우지
  않는다. 사람이 더 새 커밋을 손으로 올렸으면 옛 통과 커밋으로 되돌리지 않는다.
- **같은 커밋은 한 번만 띄운다.** 성공이든 실패든 다시 띄우지 않고, 사람이 `.finished` 를 그보다 옛 커밋으로 되돌려도 다시 덮지 않는다.
  다음 커밋이 통과하면 그 커밋이 대상이다. 실패한 커밋을 다시 올리는 것은 사람의 `rocky update --main <sha>` 다.
- **다른 main 빌드가 `main-sync.lock` 을 쥐면 기다린다**(`main_build::lock_busy`). update 의 잠금과 같은 규칙이다: pid 가 없거나 못
  읽으면 쥔 것, 죽은 pid 면 빈 것. 그때 띄우면 잠금에 걸린 실패가 그 커밋에 남아 다시 띄우지 못한다.
- **실행**: 고정 argv `<데몬 옆 rocky> update --main <sha>`(데몬 실행 파일의 실제 경로 옆). `--yes` 는 넘기지 않는다(마켓플레이스를
  바꿔야 하는 기기에서는 확인을 받지 못해 실패한다). rc 서버처럼 떨어뜨린다: `process_group(0)`, `kill_on_drop` 없음,
  `XPC_SERVICE_NAME` 지움, stdin 비움, cwd 는 `<데이터 홈>/rocky`. 출력은 대상 디렉터리의 `deploy-<sha12>.log`(0600, 최근 5개).
  REST·화면에는 배포를 시작하는 길이 없다.
- **띄우자마자 기록한다**: `deploy.json` 에 `running`, pid, 그 프로세스의 시작 시각(`ps -o lstart=`). 띄우지 못하면 그 자리에서 `failed`.
- **배포가 도는 동안 검증 잡은 새 실행을 시작하지 않는다.** 배포를 켠 대상 전부를 본다(main 빌드는 기기에 하나다). 시작해도 update 가
  데몬을 교체하면서 끊긴다. 도는지는 pid 와 시작 시각으로 본다(재사용된 pid 를 도는 것으로 보지 않는다).
- **끝 판정**(`settle_deploy`): 띄운 데몬은 종료 코드를 받는다(기다리는 스레드가 잡을 깨운다). update 가 교체한 새 데몬은 종료 코드를
  모르고 다음 바퀴에 `main/.finished` 만 본다. 그 커밋이면 `succeeded`, 아니면 `failed` 다(이유는 종료 코드, 또는 끝났는데 `.finished`
  가 다름 — 플러그인·agy 단계).
- **알림**: 실패만 배너 한 번(`deploy_notification`, 다시 하는 명령을 싣는다). 성공은 알리지 않는다. 데몬이 배포하는 대상(조건 넷을
  채움)이면 구독한 세션의 통과 문구가 세션에게 `rocky update` 를 돌리지 말라고 한다(같은 커밋을 둘이 올리지 않게). 구독은 그대로다.
- **보이기**: `GET /api/verify` 의 대상에 `deploy`(마지막 배포)와 `deployNote`(손을 봐야 풀릴 수 있는 건너뜀 이유만, `DeploySkip::note`).
  `rocky verify` 는 대상 아래에 한 줄(`↑ 배포 중`·`↑ 배포 <sha> HH:MM`·`⚠ 배포 실패`와 이유·로그·`⚠ 배포: <이유>`). 데몬 로그에는 건너뛴
  이유가 바뀔 때만(기동 뒤 처음 한 번 포함) 한 줄.
- **이력**: `runs.jsonl` 의 `deployed`·`deploy-failed` 줄(기록은 `deploy` 칸). 검증을 못 한 이유를 기동 때 되살릴 때 배포 줄은 건너뛴다.
  사람이 부르는 표면이 없어 사용 로그에는 넣지 않고, 쓰임은 이 줄로 센다.

## 코드

| 무엇 | 어디 |
| --- | --- |
| 판정(원격 커밋·다시 돌지·알릴지·디렉터리 이름·배포 `deploy_plan`·`settle_deploy`) | `crates/rocky-core/src/verify.rs` |
| 배포의 잠금 판정(`lock_busy`)·레포 이름(`REPO`) | `crates/rocky-core/src/main_build.rs` |
| 설정 | `crates/rocky-core/src/config.rs`(`load_verify_block`) + `rocky.schema.json` |
| 잡·git·프로세스 그룹·기록·배포(`DeployEnv`·`spawn_update`·`settle_deploys`) | `crates/rockyd/src/verify.rs` |
| CLI | `crates/rocky-cli/src/verify_cmd.rs` |
| 구독 저장·`/clear` | `crates/rocky-core/src/store.rs`(`subscribe_verify` · `cleared_sessions`) |
| 구독한 세션에 보내기 | `crates/rockyd/src/server.rs`(`notify_verify_subscriber` · 구독 라우트) |

테스트: `crates/rocky-core/tests/it/verify_test.rs`, `crates/rockyd/tests/it/verify_test.rs`(임시 bare 원격 + clone 으로 통과·실패·
같은 커밋 생략·복구·시간 초과·손자 정리·번호 재사용·준비 실패, `subscribed_session_hears_each_finished_run`, 배포는 가짜 `rocky` 로
한 번만 띄움·실패 배너·잠금 기다림과 배포 중 검증 멈춤·새 데몬의 판정·main 빌드 모드 아님), `crates/rocky-cli/tests/it/verify_cmd_test.rs`,
`crates/rocky-core/tests/it/main_build_test.rs`(`the_build_lock_is_held_like_update_main_takes_it`).

## 함정

- **자동 재시도는 진짜 실패를 한 바퀴만큼 늦게 알린다** — `cargo test` 가 길면 그만큼. 거짓 실패(부하로 부트스트랩 테스트 시간 초과,
  2026-10-05)를 줄이려고 감수했다.
- `cargo test` 단계 뒤 그룹에 프로세스가 남는 기록이 반복된다 — 남기는 테스트를 찾는 중(`signals.log` 의 `남은 손자`).
- 검증 트리의 `target/` 은 수 GB 다.
- **배포는 launchd 데몬의 환경에서 돈다.** plist 는 설치 때의 PATH 만 굽고(`crates/rocky-cli/src/launchd.rs`), update 의
  `daemon restart` 는 데몬에게 물려받은 그 PATH 로 다시 굽는다(같은 항목은 한 번만, `join_path`). update 가 부르는
  git·cargo·bun·claude·agy 가 그 PATH 에 없으면 배포가 실패한다(로그에 남는다) — PATH 가 맞는 셸에서 `rocky daemon install` 을 다시 한다.
  셸의 `XDG_DATA_HOME` 도 plist 에 굽지 않아서, 데이터 홈을 바꾼 기기에서는 데몬이 `~/.local/share/rocky` 를 보고 "main 빌드 모드가
  아니다" 로 건너뛴다.
- **배포 결과는 대개 한 바퀴 늦게 보인다.** update 가 데몬을 교체하면 새 데몬은 종료 코드를 모르고 다음 바퀴(`intervalSeconds`)에
  `.finished` 로 판정한다.
