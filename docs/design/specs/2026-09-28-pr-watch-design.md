# PR 감시를 데몬으로 — 설계

날짜: 2026-09-28 · 결정자: 오너 · 상태: 구현 중 (3 PR 스택)

## 왜

PR 이 "확인·머지해도 되는" 상태가 됐는지, 머지·닫힘·충돌로 바뀌었는지는 지금 **세션**이
본다 — `pr-threads.ts watch/ready/transitions` 를 Monitor 에 물려서. 그래서 30분마다 다시
걸어야 하고, 세션이 닫히면 감시도 끝나고, 알림은 세션이 살아 있을 때만 간다. 오너 결정:
**전이 감지 + 머지 가능 판정 + 알림을 데몬이 맡는다.** 세션에 남는 건 리뷰 지적을 고치는
일뿐이다.

## 결정

1. **대상은 `repo` 가 설정된 보드의 PR** 이다(이슈 생성과 같은 기준). 보드 없는 레포는 보지
   않는다 — 보드가 곧 "내가 관장하는 레포" 의 목록이다.
2. **조회는 데몬이 `gh api graphql` 로** 한다(이슈 생성이 이미 데몬 사용자의 `gh` 를 쓴다).
   레포당 한 번의 쿼리: 최근 갱신 순 PR 30건(OPEN·MERGED·CLOSED)의 상태·`mergeStateStatus`·
   head·`statusCheckRollup`(CI)·미해결 리뷰 스레드의 첫 코멘트에 내(viewer)가 단 리액션.
   기본 3분 간격(`pr.intervalMinutes`). 외부 API 를 초당 도는 경로가 아니다.
   **비용은 실제 노드가 아니라 `first:` 로 요청한 노드 수다** (릴리스 뒤 추가, 2026-09-28 실측
   — 열린 PR 이 0개인 레포에서도 첫 판 쿼리가 263 포인트). 레포 10개 × 3분이면 두 tick 에 시간당
   5,000 포인트가 바닥나고, 한도는 계정 단위라 세션의 `gh` 까지 막혔다. 그래서 레포당 호출을
   둘로 나눈다: **목록**은 열린 50·닫힌 30의 상태 조각만(1 포인트대), **상세**는 실제로 열린
   PR 번호에만 `p<번호>: pullRequest(number:) { CI·스레드 }` 별칭 배치로(PR 당 2 포인트대) —
   비용이 현실의 열린 PR 수에 비례하고 열린 것이 없으면 목록 한 번이다. 리액션은
   `reactions(content:EYES) { viewerHasReacted }` 로 노드 없이 묻고, 응답마다 `rateLimit` 을 읽는다.
3. **판정은 순수 코드(`rocky_core::prwatch`)** — 세션 스크립트 `readyVerdict`/`transitionsBetween`
   의 Rust 판. `ready` = OPEN · draft 아님 · CI 통과 · 👀 도 🚀 도 없는 미해결 스레드 0 · 🚀 0.
   전이 = 직전 스냅숏과의 차이: `opened`·`ready`·`unready`·`conflict`·`merged`·`closed`.
4. **기억은 `pr_watch` 테이블**(user_version 8) — `(repo, number)` 당 마지막 스냅숏. 데몬이
   재기동돼도 전이를 다시 알리지 않는다.
5. **알림은 셋으로 간다.**
   - 사람: macOS 알림(`osascript -e 'display notification …'`, `pr.notify` 기본 켬) —
     `ready`·`conflict` 만. `merged` 는 대개 오너 자신이 한 일이라 조용히 기록만.
   - 세션: 전이가 보드 히스토리(actor `rocky`, action `pr-ready`/`pr-conflict`/`pr-merged`/
     `pr-closed`)로 남고 `/api/changes` 를 타므로, `notify-todo` 훅이 다음 턴에
     "#179 확인·머지해도 된다" 를 additionalContext 로 넣는다 — 에이전트가 감시하지 않아도 안다.
     **채널**(2026-09-29 추가): 훅 주입은 턴이 열려야 실리므로 idle 세션엔 닿지 않는다. 플러그인의
     worklog stdio 서버가 Claude Code channels(`claude/channel` capability,
     `notifications/claude/channel`)를 선언하고 데몬 SSE 를 구독해 ready·conflict 를 밀어 넣으면
     세션이 그 자리에서 깨어난다. 리서치 프리뷰라 `--dangerously-load-development-channels
     plugin:rocky@rocky-marketplace` 로 띄운 세션만 받는다. 규칙은 훅 주입과 같고 모양만 다르다
     (`rocky_core::notify::pr_channel_events`).
   - 보드: "지금" 표에 `ready`·`conflict` PR 행. `GET /api/prs[?board=]` 가 재료.
6. **세션 스크립트는 남긴다** — `list`/`react` 는 리뷰 대응에 여전히 필요하고, `ready`/
   `transitions` 는 데몬이 없는 곳(다른 레포)의 폴백이다. `/rocky:resolve-reviews` 9단계의
   "Monitor 로 기다린다" 만 "데몬이 알린다" 로 바뀐다.

## 스택

| PR | 층 | 내용 |
|---|---|---|
| 1 | 코어 | `pr` 설정 블록(스키마 lockstep) · `rocky_core::prwatch`(파싱·판정·전이·알림 문구) · `pr_watch` 테이블과 스토어 |
| 2 | 데몬 | `rockyd::prwatch` 주기 잡(주입 가능 runner) · osascript 알림 · 히스토리/SSE · `GET /api/prs` |
| 3 | 표면 | 훅 주입(`build_pr_context`) · 웹 "지금" 표 PR 행 · `rocky pr` · 문서·changeset |

## 경계

- 리뷰 지적을 **고치는** 일, 👀/🚀 리액션, 코멘트·resolve·머지는 여전히 세션/사람 몫이다.
  데몬은 읽기만 한다(`gh api graphql` 쿼리뿐, 변이 없음).
- 스택 순서 판단은 하지 않는다 — GitHub 이 base 를 옮기고 리베이스하므로 "맨 아래" 는 base 가
  기본 브랜치인 PR 로 자연히 드러난다(`ready` 는 base 가 main 인 것에만 준다).
- 사람 리뷰어의 승인 여부는 조건이 아니다(이 레포는 1인). 필요해지면 `reviewDecision` 을 더한다.

## 실패 처리

- `gh` 가 없거나 인증이 없으면 그 tick 을 건너뛰고 `/api/health` 에 `prWatch: { available: false,
  reason }` 을 싣는다 — 핸드오프의 `claude` 부재와 같은 모양.
- 쿼리 한 번의 실패는 스냅숏을 바꾸지 않는다(전이 없음). 레포가 사라졌거나 403 이면 그 레포만
  건너뛰고 사유를 health 에 남긴다.
- **예산이 바닥이면 쉰다** — 잔여가 1,000(`RATE_LIMIT_FLOOR`) 밑이거나 한도 에러(`errors[].type ==
  RATE_LIMIT`)를 받으면 그 tick 의 남은 레포는 묻지 않고(똑같이 실패한다) 리셋 시각(+1분)까지
  다음 tick 을 미룬다. 리셋 시각을 모르면 15분. 남긴 1/5 은 세션·터미널의 `gh` 몫이다. health 의
  `prWatch.pausedUntil`·`rateLimit` 이 그 상태를 말한다.
