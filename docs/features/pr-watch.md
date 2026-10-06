# PR 감시 — 구독·전이·전달·할 일 정리

> rocky 를 **고치는** 에이전트용 개발 문서. 받은 메시지에 **어떻게 대응하나**는 `/rocky:review-fix`(`plugin/commands/review-fix.md`)와
> 사용자 스킬 [`plugin/skills/pull-request`](../../plugin/skills/pull-request/SKILL.md). 설계: [`2026-09-28-pr-watch-design.md`](../design/specs/2026-09-28-pr-watch-design.md),
> [`2026-10-01-pr-subscriptions-design.md`](../design/specs/2026-10-01-pr-subscriptions-design.md).

## 규칙

### 무엇을 보나
- **구독한 PR 만**(`pr_subscriptions` — `rocky pr subscribe N`, `/rocky:review-request`·`review-fix` 가 구독, 머지·닫힘에서 풀린다)
  `pr.intervalMinutes` 마다 상세 쿼리로 보고 `pr-*` 전이(actor `rocky`)를 그 레포를 둔 보드 히스토리에 남긴다. 레포 목록은 보지
  않는다 — 보드의 `repo` 는 감시 대상을 정하지 않는다. 구독은 `POST/DELETE /api/prs/subscriptions`(로컬 전용), `GET` 은 열려 있다.
- **필터 구독**(`rocky pr subscribe --filter "repo:o/r author:@me"`, `/api/prs/filters`)은 GitHub 검색 조건이다 — tick 마다 검색 한
  번(`is:pr is:open` 은 데몬이 붙인다, `gh` 에는 `-f` 로 — `-F` 는 `@me` 를 파일로 읽는다)으로 걸린 열린 PR 을 그 세션 구독으로
  넣는다. 이미 누가 구독한 PR 은 빼앗지 않고, 필터를 해지하면 그 필터로 들어온 구독도 걷힌다(직접 구독하면 필터 출처가 지워진다).
- **GitHub 은 읽기만 한다.** *EN: the daemon never writes to GitHub.*
- 구독은 그 레포를 기준선이 잡힌 레포로 표시해, 구독한 뒤 첫 tick 이 지금 상태(머지 후보·CI 실패 등)를 알린다. 처음 본 PR 이 이미
  머지·닫힘이면 그 전이(`Merged`/`Closed`)도 첫 tick 에 온다.

### 누구에게 보내나
- 전달 길: macOS 배너(`pr.notify`), 세션 받은편지함(`pr.sessionNotify`), 브릿지(`pr.notifiers[]`, 코드는 `bridges/<name>/` 에만).
- **받은편지함**: 훅이 `CLAUDE_CODE_MESSAGING_SOCKET` 을 `POST /api/sessions/inbox` 로 등록하고, 데몬이 **그 PR 을 구독한 세션**에만
  JSON 한 줄을 쓴다. 그 세션이 끝났거나 "보내지 않기" 면 보내지 않고 다른 세션으로 넘기지 않는다.
- **훅 주입(`notify-todo`)과 rocky 채널도 같은 기준**: 이 세션이 구독한 PR 의 ready·conflict 만. 세션 보드가 풀리면 그 보드 것
  중에서, 안 풀리면 전 보드에서 고른다. 훅은 세션이 받은편지함으로 **이미 받아들인 것**을 뺀다 — 데몬의 전달 `ok`(소켓에 씀)가
  아니라 트랜스크립트의 peer 기록으로 판정한다(권한 우회 모드 세션은 승인 창에 보류하고 거절하면 안 들어간다; `pr_entries_for_session`
  ·`peer_messages`·`drop_absorbed`). 구독·보드 조회가 실패하면 훅은 커서를 붙들고 채널은 다시 붙는다.
- `pr-review` 는 보드의 `reviewFix` 가 켜졌을 때만 세션에 간다. `pr-merged`·`pr-ci-failed`(같은 head 에서 한 번, 재실행이 또 실패하면
  또)는 배너·브릿지 없이 세션에만 간다.
- 보드 `prAuthors`(`@me`·login)에 걸린 전이는 `quiet` 로 기록만 되고 세션·배너·브릿지·훅 주입을 건너뛴다(보기는 넓게, 깨우기는 좁게).
- **받은편지함 등록은 `session_inboxes` 에도 남겨** 데몬이 다시 떠도 되살린다. 되살린 등록은 훅이 다시 등록하기 전까지 그 세션이
  살아 있고 소켓 이름의 pid 가 그 세션의 것일 때만 쓴다(`restored_registration_live`, 캐시 없는 세션 목록; 못 읽으면 보내지 않는다 —
  소켓 경로는 다른 세션이 다시 쓸 수 있다).
- 세션 전달은 `GET /api/deliveries`(받는 세션·최근 50건, 메모리, 못 보낸 건은 `reason`; 끝난 세션의 등록은 세션 목록과 대조해 빼고
  `ended` 로 개수만 — 보이기만 하는 판단이라 캐시 목록이고 못 얻으면 빼지 않는다. 등록 자체는 TTL 로만 걷는다: `/clear` 된 옛 id 도
  목록에서 사라지는데 그 등록이 남아야 다음 등록 때 `superseded_sessions` 가 `/clear` 를 알아본다), `POST /api/deliveries/mute` 로 세션별 "보내지
  않기"(PR 알림은 버리고 수집함 알림은 미룬다, 메모리) — 둘 다 로컬 전용.
- **`/clear` 뒤의 구독은 웹에서 사람이 정한다.** `/clear`·세션 안 `/resume` 은 세션 id 만 바꾸고 프로세스·소켓(`<pid>.sock`)은
  그대로다(Claude Code 2.1.288 실측). 받은편지함 등록 때 같은 소켓으로 **지금 프로세스가 뜬 뒤**(소켓 파일 mtime = 프로세스 시작, 실측)
  등록했던 다른 id 가 있으면, 구독이 남은 그 세션을 `cleared_sessions` 에 적고 등록을 걷는다(`peer_inbox::superseded_sessions`·
  `store.mark_session_cleared`; 그 전의 등록은 pid 재사용이라 적지 않는다). 적힌 세션은 **깨우지 않는다** — 그대로 두면 맥락 없는
  새 세션이 옛 PR 의 "리뷰가 붙었다" 를 받는다. 구독은 남아 감시·보드 상태·머지 시 할 일 완료는 이어지고, 전달 기록에는
  "세션이 /clear 됨" 으로 남는다. 웹 "세션 전달" 카드(`GET /api/deliveries` 의 `cleared`)에서 **넘기기**(PR·필터·수집함 구독과 안
  집힌 핸드오프, 겹치면 합친다) · **지켜보기만**(PR·필터는 세션만 떼고 수집함 구독은 지운다) · **해지**(전부)를 고른다
  (`POST /api/sessions/cleared`, 로컬 전용; `store.resolve_cleared_session`).
- **기본 브랜치 검증 결과 구독도 같은 전달 규칙이다**(`verify-passed`·`verify-failed`) — 받은편지함으로, "보내지 않기" 면 안 보내고,
  `/clear` 된 세션이면 사유만 남기고 웹 "세션 전달" 카드의 결정(넘기기·지켜보기만·해지)을 따른다. 세부는 [verify](./verify.md).
- **늦게 끝나는 쓰기는 지금 주인으로**: 필터 검색 결과는 넣는 순간의 필터 행에서 세션을 읽고(해지됐으면 넣지 않는다), 수집함 본
  항목·기준선은 그 세션의 구독이 아직 있을 때만 쓴다 — 기다리는 사이 주인이 바뀌어도 옛 id 로 쓰지 않는다.
- **전달 기록은 DB 에도 남는다**(`deliveries`, 최근 50건 — `store.save_delivery`·`load_deliveries`). 데몬은 배포마다 다시 뜨는데, 메모리에만
  두면 "예전에 못 보냈나" 를 볼 수 없었다. 못 남겨도 전달과 메모리 기록은 그대로다.

### 무엇을 뜻하나
- `ready` 는 **머지 후보**다 — 세션이 사용자에게 알리기 전에 판단한다(`/rocky:review-fix` 8단계). 머지 뒤에 붙은 리뷰는 다음 PR 로
  간다(`after-merge`).
- **PR ↔ 할 일**: 머지·닫힘 전이가 오면 그 PR 주소를 `links` 에 둔 할 일(보관 제외, 전 보드)을 정리한다 — 머지면 완료 + 댓글(링크한
  다른 PR 이 아직 구독 중이면 완료하지 않고 댓글만), 머지 없이 닫히면 댓글만, 이미 끝낸 할 일은 그대로, 한 tick 에 같은 할 일의 PR 이
  함께 머지돼도 완료는 한 번. 링크는 `/rocky:review-request` 가 세션이 든 할 일에 붙인다.

### 예산
- GraphQL 비용은 돌려받은 노드가 아니라 `first:` 로 **요청한** 노드 수다. 레포당 `PR_LIST_QUERY`(상태 조각) 한 번 + 실제로 열린 PR
  에만 `detail_query`. 잔여가 `RATE_LIMIT_FLOOR`(1,000) 밑이거나 한도 에러면 리셋까지 쉰다(`pause_for`). **주기를 바꾸기 전에
  `rateLimit { cost }` 를 잰다.** *EN: the budget is shared with every session's `gh`.*

## 코드

| 무엇 | 어디 |
| --- | --- |
| 순수 판정(전이·쿼리·예산·할 일 정리 `linked_todo_action`) | `crates/rocky-core/src/prwatch.rs` |
| 주입·채널 이벤트(`build_pr_context`·`pr_channel_events`·`pr_entries_for_session`) | `crates/rocky-core/src/notify.rs` |
| 받은편지함 문구·흡수 판정(`peer_messages`·`drop_absorbed`) | `crates/rocky-core/src/peer_inbox.rs` |
| `/clear` 판정(`superseded_sessions`)·기록·결정(`mark_session_cleared`·`resolve_cleared_session`) | `crates/rocky-core/src/peer_inbox.rs`, `crates/rocky-core/src/store.rs` |
| 잡·전달기·`settle_linked_todos` | `crates/rockyd/src/prwatch.rs` |
| 훅·채널 | `crates/rocky-cli/src/hooks.rs`, `crates/rocky-cli/src/channel.rs` |

테스트: `crates/rocky-core/tests/it/{prwatch_test,notify_test,peer_inbox_test}.rs`, `crates/rockyd/tests/it/prwatch_test.rs`,
`crates/rocky-cli/tests/it/{hooks_test,hook_wiring_test}.rs`(배선 — 가짜 데몬으로 훅 본체 `notify_todo_context`·채널 `read_page`).

## 함정

- **GraphQL 한도 사고(2026-09-28)**: 첫 쿼리가 20분에 5,000 포인트를 써 세션들의 `gh` 가 막혔다. `gh api rate_limit` 은 믿을 수
  없다 — 응답의 `rateLimit { cost remaining }` 을 본다.
- **재시작 직후 알림 유실(2026-10-05)**: 다시 뜬 뒤 첫 tick 이 세션의 재등록보다 먼저 돌아 알림이 버려졌다 — 그래서 등록을 DB 에 남긴다.
