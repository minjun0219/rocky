# 세션 목록 — `claude agents`

> rocky 를 **고치는** 에이전트용 개발 문서. 핸드오프·doing 귀속은 [handoff](./handoff.md), 세션 띄우기는 [spawn](./spawn.md),
> REST 모양은 [`docs/rewrite/contract.md`](../rewrite/contract.md) 의 `GET /api/sessions`.

## 규칙

- **세션 목록의 출처는 `claude agents --json` 하나다.** 데몬이 돌리고(캐시는 `rockyd::sessions_exec`), 파싱은 순수 함수
  `parse_sessions`. 못 돌리면 `available: false` + `reason` 이고 보드 나머지는 정상이다.
- **pid 없는 행을 버리지 않는다.** 사람 답을 기다리며 잠든 background 세션(`state: blocked`)은 `pid`·`status` 없이 오고, cwd 는
  워크트리가 아니라 **레포 루트**다(Claude Code 2.1.289 — 워크트리 경로는 `state.json` 의 `worktreePath` 에만 있다). 버리면 그
  세션이 든 doing 이 `gone` 이 되어 자동 해제된다. 필수 필드는 `cwd`·`sessionId`·`name` 뿐이다.
- **잠든·끝난 세션은 목록에 남되 일을 새로 넘길 곳은 아니다.** doing 생존(`resolve_doing_state`)·자동 해제(`board_has_session`)
  판정에는 보이고, 핸드오프 **자동** 대상에서만 뺀다(`takes_handoff` — `blocked`·`done`).
- **background 행의 작업 요약(`job`)은 Claude Code 의 내부 파일에서 고른 것만 싣는다.** `<CLAUDE_CONFIG_DIR 또는 ~/.claude>/jobs/<짧은
  id>/state.json` 의 `detail`·`needs`·`updatedAt` 셋뿐 — 같은 파일의 제안 답장·토큰·첫 프롬프트(`intent`)·환경(`providerEnv`)은
  싣지 않는다. `/api/sessions` 는 원격(테일넷·터널)에서도 읽히는 경로다. 필드를 늘리려면 이 목록부터 고친다.
- **내부 파일이라 fail-open 이다.** 못 읽거나 형식이 다르면 **그 행만** `job` 이 없다 — 목록 전체를 실패시키지 않는다.
- **짧은 id 로 경로를 만들기 전에 검증한다.** id 는 CLI 출력에서 온 값이다 — 영숫자 64자 이하만 받는다(`job_state_path`).
  `Path::join` 은 인자가 절대경로면 앞을 버리고 `..` 는 폴더를 벗어난다.

## 코드

| 무엇 | 어디 |
| --- | --- |
| 파싱·보드 매칭·핸드오프 후보·작업 요약 | `crates/rocky-core/src/sessions.rs` |
| `claude agents --json` 실행·캐시 | `crates/rockyd/src/sessions_exec.rs` |
| `GET /api/sessions`(`job` 붙이기)·핸드오프 자동 매칭 | `crates/rockyd/src/server.rs`(`read_job_summary`, 핸드오프 라우트) |

테스트: `crates/rocky-core/tests/it/sessions_test.rs`(`background_rows_without_pid_are_kept`, `job_state_*`),
`crates/rocky-core/tests/it/doing_test.rs`(`dormant_blocked_background_is_idle_not_gone`),
`crates/rockyd/tests/it/server_handoff_test.rs`(`background_sessions_carry_job_summary`, `auto_match_skips_dormant_background_sessions`).

## 함정

- 테스트 픽스처의 잠든 행 cwd 를 워크트리 경로로 두면 핸드오프 후보 회귀가 테스트에서 안 드러난다 — 실제처럼 레포 루트로.
- spawn 가드(`find_live_session_at`)는 cwd 를 워크트리 경로와 정확히 비교해 잠든 세션을 못 본다(이전부터의 한계, 후속).
