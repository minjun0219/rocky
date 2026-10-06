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

## 웹 — 에이전트 탭

화면 모양(자리·문구·폭)의 정본은 [`web/DESIGN.md`](../../web/DESIGN.md) "Layout" 8. 여기는 판정 규칙만.

- **묶음은 상태로 정한다**(`agentPhase`): background `blocked` → 내 차례, `busy`·background `working` → 실행 중,
  나머지(`idle`·`done`) → 쉬는 중. interactive 의 `idle` 은 사람이 칠 차례지만 cmux 가 이미 알리므로 내 차례가 아니다.
- **세션의 보드는 데몬의 `board_key_for_cwd` 와 같은 순서로 고른다**(`boardOfSession`): cwd 가 `path` 아래인 보드 중 가장
  긴 경로, 없으면 key(옛 key 포함)가 cwd 의 경로 세그먼트인 보드 중 가장 긴 key. 처음 맞는 것을 고르면 보드 생성 순서에
  따라 바깥 보드나 이름이 같은 상위 폴더로 간다. 보드가 없으면 레포 폴더 이름(워크트리는 레포로 접는다).
- **보드를 고르면 그 보드의 세션만** 보인다 — 다른 보드의 세션은 "전체" 에서.
- **든 할 일은 `doingSessionId` 로 잇는다** — 전체 `sessionId` 와 짧은 `id` 둘 다(핸드오프로 띄운 세션은 짧은 id 로 귀속된다).
- **셋째 줄은 Claude 가 남긴 요약 한 줄**: 내 차례면 `needs`(없으면 `detail`), 아니면 `detail`. 출력·로그는 두지 않는다.
- **메시지는 받은편지함으로만 보낸다**(`POST /api/sessions/message`) — 큐에 넣는 대안은 없다. 받은편지함을 등록하지 않은
  세션이면 409 와 이유(`받을 세션 등록 없음` 등)를 그대로 보인다. 세션을 움직이는 일이라 **로컬 요청만**(아니면 403, 화면은
  `spawnAllowed` 일 때만 버튼). 끝난 행과 **pid 없이 잠든 background 행**(소켓을 들을 프로세스가 없다)에는 버튼을 그리지 않는다.
  사람이 골라 누른 것이라 "보내지 않기"(mute)는 보지 않는다. 2000자까지, 전달 기록 kind 는 `message` — 기록의 제목에는 본문 대신
  길이만 남긴다(`웹 메시지 (N자)`). 전달 기록은 DB 에도 남고 화면에 보여 비밀이 섞이면 그대로 남는다.
- **본문은 데몬이 아는 사실만 밝힌다**(`web_session_message`) — "이 기기의 로컬 요청(웹 에이전트 탭 경로)". 사람이 쳤다거나
  승인이라고 주장하지 않는다. 받는 쪽에는 다른 세션의 메시지로 보여 권한 허락·결정 답으로 쓰이지 않고, 화면이 그 사실을 밝힌다.
- **멈추기는 살아 있는(pid 있는) background 세션만**(`POST /api/sessions/stop` → `claude stop <짧은 id>`, 판정 `stop_target`). 하던 턴이
  끊기므로 행 아래 한 줄로 한 번 더 묻는다. 로컬 요청만(아니면 403). 데몬은 **캐시 없는 목록**에서 대상을 고르고 명령에는 목록의 짧은 id
  만 넘긴다 — 사람이 보낸 값이 인자가 되지 않는다. 실측(2.1.289): 살아 있는 세션은 transient 서비스(`claude daemon run`)의 자식이라
  stop 할 때 서비스가 늘 떠 있고, stop 뒤 대화·워크트리는 남아 `claude attach` 로 잇는다. 세션은 기본 목록에서 빠진다(`blocked` 는
  `stopped` 가 되어 "내 차례" 에서도 빠진다). **pid 없이 잠든 세션의 stop 은 재 보지 못해 받지 않는다**(409).
- **멈추면 데몬이 세션 목록 캐시를 비운다**(`invalidate_sessions` — `/api/sessions`·statusline 의 SWR). 안 비우면 오래된 값을 최대
  30분 더 줘서 다른 탭·폰·`doingState` 가 멈춘 세션을 산 것으로 본다. 비우기 전에 시작한 조회는 끝나도 캐시에 쓰지 않는다(세대 번호).
  화면은 그 행을 바로 빼고 목록을 다시 읽는다 — 화면 쪽 감춤 상태는 두지 않는다(두면 곧바로 attach 로 이은 세션까지 숨는다).
- **attach 복사는 background 행 전부**(짧은 id 가 있으면) — `claude attach <id>` 를 클립보드에. 아무것도 움직이지 않아 노출된 화면에도
  둔다. pid 없이 잠든 세션에 답하는 길이 이것이다(메시지는 들을 프로세스가 없다). **짧은 id 는 셸 명령이 되므로 웹도 형식을 본다**
  (`isSafeShortId` — 영숫자 64자 이하, 데몬 `is_safe_short_id` 와 같은 규칙). 형식이 다르면 attach·멈추기 둘 다 그리지 않는다.
- **쓰던 메시지는 세션 id 로 탭에 둔다** — 행이 다른 묶음으로 옮겨 가면(실행 중 → 쉬는 중) 다시 마운트돼 입력칸이 사라진다.
- **세션 목록 폴링은 앱 한 곳**(`useAgentsPolling`, `main.tsx`) — 탭을 보는 동안 15초, 아니면 60초(피드 숫자는 어느 탭에서나
  보인다). ⋯ 메뉴에서 탭을 끄면 폴링도 멈추고 `?view=agents` 도 피드로 돌아간다.

## 웹 — 피드 "내 차례"

- **사람 답을 기다리는 background 세션(`blocked`)이 행이 된다** — 멈춘 진행과 같은 순위(`MINE_RANK.stuck`). 제목은 `needs`(없으면
  `detail`), 누르면 에이전트 탭.
- **진행 중 할 일을 든 세션이면 빼고 그 진행 행(멈춤)만 남긴다** — 같은 일을 두 줄로 세지 않는다. 대신 기다리는 것 문구는 그
  경우 에이전트 탭에만 보인다.
- **에이전트 탭을 끄면 이 행도 없다**(GitHub 탭과 같은 규칙 — 끄면 그 표면은 어디에도 안 보인다).
- **기간으로 자르지 않는다** — 몇 달 잠든 세션도 뜬다. 숨기기(×)는 없고, 치우는 길은 그 세션에 답하거나, 살아 있으면 에이전트 탭의 멈추기, 아니면 `claude rm`.

## 코드

| 무엇 | 어디 |
| --- | --- |
| 파싱·보드 매칭·핸드오프 후보·작업 요약 | `crates/rocky-core/src/sessions.rs` |
| `claude agents --json` 실행·캐시 | `crates/rockyd/src/sessions_exec.rs` |
| `GET /api/sessions`(`job` 붙이기)·핸드오프 자동 매칭 | `crates/rockyd/src/server.rs`(`read_job_summary`, 핸드오프 라우트) |
| 에이전트 탭의 판정(상태·보드·묶음) | `web/agents.ts` |
| 에이전트 탭 화면·폴링 | `web/components/AgentsPane.tsx`(`useAgentsPolling`), 탭 전환 `web/components/ViewSwitch.tsx` |
| 세션에 메시지 | `POST /api/sessions/message` `crates/rockyd/src/server.rs`(`wake_session` — 핸드오프와 같은 길), 본문 `crates/rocky-core/src/peer_inbox.rs`(`web_session_message`), 화면 `AgentsPane.tsx`(`MessageToggle`) |
| 세션 멈추기 · attach 복사 | 판정 `crates/rocky-core/src/sessions.rs`(`stop_target`), 실행 `crates/rockyd/src/sessions_exec.rs`(`stop_session`), 라우트 `POST /api/sessions/stop` `server.rs`, 캐시 비우기 `sessions_exec.rs`(`swr_sessions_with_invalidate`), 화면 `AgentsPane.tsx`(`useStopAction` · `AttachCopy`) |
| 피드 행 | `web/lib.ts`(`nowRows`), 누르면 탭으로 `web/components/NowTable.tsx` |

테스트: `crates/rocky-core/tests/it/sessions_test.rs`(`background_rows_without_pid_are_kept`, `job_state_*`),
`crates/rocky-core/tests/it/doing_test.rs`(`dormant_blocked_background_is_idle_not_gone`),
`crates/rockyd/tests/it/server_handoff_test.rs`(`background_sessions_carry_job_summary`, `auto_match_skips_dormant_background_sessions`),
`crates/rockyd/tests/it/prwatch_test.rs`(`web_message_goes_to_a_registered_session_only_from_a_local_request`),
`crates/rocky-core/tests/it/sessions_test.rs`(`stop_target_takes_only_live_background_sessions`),
`crates/rockyd/tests/it/server_handoff_test.rs`(`stop_runs_claude_stop_for_live_background_sessions_only`),
`crates/rockyd/tests/it/sessions_swr_test.rs`(`invalidate_drops_the_cache_and_ignores_lookups_started_before_it`),
`web/agents.test.ts`, `web/components/AgentsPane.test.tsx`, `web/lib.test.ts`("nowRows — 답을 기다리는 에이전트").

## 함정

- 테스트 픽스처의 잠든 행 cwd 를 워크트리 경로로 두면 핸드오프 후보 회귀가 테스트에서 안 드러난다 — 실제처럼 레포 루트로.
- spawn 가드(`find_live_session_at`)는 cwd 를 워크트리 경로와 정확히 비교해 잠든 세션을 못 본다(이전부터의 한계, 후속).
