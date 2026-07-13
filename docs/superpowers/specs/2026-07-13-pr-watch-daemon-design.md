# `/pr-watch` 데몬 감시 진화 — 설계

- 날짜: 2026-07-13
- 대상: `commands/pr-watch.md` (Claude Code plugin 슬래시 커맨드)
- 상태: 설계 승인됨 → 구현 계획 대기

## 배경 / 문제

현재 `/pr-watch` 는 **1회성 점검** 커맨드다: 스냅샷 수집 → 상태 리포트 → 미해결 리뷰 정리(권고까지) → 머지 가능하면 알림. 명시적으로 "머지 X / 코드 수정 X / 자동 답글 X / 스스로 루프 X".

한편 메모리 정책 `pr-watch-bot-review-policy` 는 이미 이 커맨드를 확장해 두었다: 타당한 지적은 직접 수정·푸시, 애매한 지적은 호출자와 논의, 어드바이저(독립 subagent)로 교차검증, 반박은 답글+resolve, **모든 리뷰 스레드가 resolved 됐을 때만** 머지 판정.

원하는 것: 여기에 **데몬 감시 + 자동 반복 처리 + 종료 조건(머지 가능 알림)** 을 얹는다. PR 을 지속 감시하다가 새 코멘트/리뷰가 달리면 답글·resolve·코드 수정·푸시까지 하고, 더 개선할 여지가 없거나 리뷰가 없으면 호출자에게 머지 가능하다고 알린다. 리뷰 중 애매한 건 어드바이저와 호출자에게 물어 판단한다.

## 확정된 설계 결정 (브레인스토밍 결과)

1. **지속성**: 이 세션 살아있는 동안만. **데몬 서버(webhook·headless claude·새 배포 타깃)는 짓지 않는다.** `gh` 폴링, 인프라 0.
2. **드라이버 재구성**: 순수 백그라운드 서브에이전트는 도중에 호출자에게 못 묻는다 → 애매 처리("어드바이저 + 호출자")와 구조적으로 충돌. 그래서 **메인 스레드 코디네이터 + ScheduleWakeup 자기 페이싱** 으로 재구성한다. "논블로킹(딴 일 하기)"의 이득은 detached 에이전트가 아니라 **폴링 사이 잠들기(ScheduleWakeup)** 로 얻는다 — 잠든 사이 세션은 자유.
3. **코드 수정 자율성**: 타당한 지적은 자율 수정하되 **게이트(check/typecheck/test) 통과시에만 푸시**. 게이트 실패 시 푸시 안 하고 그 건을 호출자에게 알림. (`/finish` 게이트와 일관)
4. **애매 처리 = 사이즈 기반 에스컬레이션 사다리** (감시자가 사안 크기 보고 rung 을 자율 선택):
   - 작고 명백 → 직접 판단·처리 (게이트 내)
   - 판단 필요 → 어드바이저 상담 → 명확해지면 진행 (필요시 그 뒤 호출자 확인)
   - 크거나 끝까지 갈림 → 호출자에게 직접 의견 요청 → 결정 전까지 그 건 **보류**
5. **머지는 안 한다**: 종착점은 "머지 가능" **알림**. `gh pr merge` 자동 실행 X. (원 요청 "머지 가능하다고 알리고" 준수)
6. **기존 커맨드 진화**: 별도 커맨드 신설 없이 `/pr-watch` 자체를 이 동작으로 바꾼다. 1 호출 = 1 완결 사이클(1회성 사용 보존), `/loop` 로 감싸면 데몬.

## 실행 모델

- **1 호출 = 1 완결 사이클.** 기존 1회성 동작을 보존한다. `/loop /pr-watch <PR>`(인터벌 생략 = dynamic 모드)로 감싸면 데몬처럼 반복된다.
- **메인 스레드 코디네이터.** tick 사이에는 PR 상태에 맞춰 `ScheduleWakeup` 으로 자기 페이싱한다. 잠든 사이 세션은 자유롭게 쓸 수 있어 실질적으로 논블로킹.
- **자기 페이싱 정책** (ScheduleWakeup `delaySeconds`):
  | PR 상태 | 다음 깨어남 | 이유 |
  |---|---|---|
  | CI 진행중 (`PENDING`/`IN_PROGRESS`, `mergeStateStatus==UNKNOWN`) | ~240s | 캐시 윈도우 유지, 곧 바뀔 상태 |
  | 사람 리뷰 대기 (`BLOCKED`/`REVIEW_REQUIRED`/`CHANGES_REQUESTED`, CI 통과, 할 일 없음) | 1200~1800s | 몇 분 안에 안 바뀜 |
  | 머지 가능 / 죽은 PR (closed·merged·conflict·BEHIND·DIRTY) | 재예약 안 함 = **루프 종료** | 종착 |
  | 보류 건만 남음 (호출자 결정 대기) | 재예약 안 함 = 루프 종료 | 호출자 차례 |
- 모든 GitHub 접근은 `gh` CLI 로만. `curl` / 직접 API fetch 금지.

## 사이클 플로우

한 사이클(= 1 호출)은 다음을 순서대로 수행한다:

### 1. 대상 PR 확정
- `$ARGUMENTS` 있으면 PR 핸들(번호 / URL / `owner/repo#123`). 없으면 현재 브랜치의 PR (`gh pr view --json number,url,headRefName`). 실패 시 한 번 묻고 멈춤.

### 2. 스냅샷 수집
- `gh pr view <PR> --json number,title,url,state,isDraft,mergeable,mergeStateStatus,reviewDecision,statusCheckRollup,headRefName,baseRefName,reviewRequests,reviews`
- 미해결 리뷰 스레드 + threadId + 첫 comment databaseId: GraphQL `reviewThreads`(`isResolved==false` 만). `$owner`/`$repo`/`$num` 변수로 넘긴다 (GraphQL query 문자열엔 `{owner}` 자동 치환 안 됨).
- `state != OPEN` → 사실만 알리고 종료. `isDraft` → draft 안내(해제는 사용자).

### 3. 새 이벤트 감지
- 지난 사이클 대비 새 코멘트 / 리뷰 / CI 상태 변화를 식별한다. (사이클 간 비교 기준을 어떻게 들고 갈지는 계획 단계에서 — 예: 마지막 처리 시각 / 이미 resolve 한 threadId 집합.)

### 4. 리뷰 처리 (미해결 스레드마다, 에스컬레이션 사다리)
각 미해결 스레드를 코드와 대조(`Read`/`Grep`/`Glob`)하고 사이즈 기반으로 rung 을 선택:

- **타당·명백** → 코드 수정 → **게이트(check/typecheck/test) 통과 확인** → 통과시 커밋·푸시 → 스레드에 답글 + `resolveReviewThread`.
  - 게이트 실패 → 푸시 안 함, 그 건을 호출자에게 알림, 해당 스레드는 미해결로 남김.
- **반박** → (사이즈에 따라 어드바이저 교차검증) 근거(가능하면 실증 결과) 답글 + resolve.
- **애매** → 어드바이저 상담 → 여전히 갈리면 **보류 + 호출자 알림**. 그 스레드만 멈추고 나머지 스레드는 계속 처리.

답글은 REST `POST /repos/{o}/{r}/pulls/{n}/comments` `in_reply_to=<cid>`, resolve 는 GraphQL `resolveReviewThread(input:{threadId})`.

### 5. 머지 판정
- **모든 리뷰 스레드가 resolved** 이고 `mergeStateStatus` ∈ {`CLEAN`, `HAS_HOOKS`} → "머지 가능".
- 미해결/보류 스레드가 남아 있으면 아직 머지 판정하지 않는다.
- CI 진행중이면 이 턴 안에서 `gh pr checks <PR> --watch --fail-fast` 로 기다린 뒤 스냅샷 재수집(또는 자기 페이싱으로 다음 tick 에 재확인 — 계획 단계에서 택일).

### 6. 자기 페이싱 / 종료
- 위 "자기 페이싱 정책" 표대로 ScheduleWakeup 예약 또는 종료.

## 어드바이저

- 판단(특히 반박 / 애매)을 **독립 서브에이전트(`Agent`)** 로 교차검증한다.
- 중첩 서브에이전트 제약이 있을 수 있으므로(메인 스레드가 스폰하면 문제없지만 detached 워커 내부 스폰은 불확실), 코디네이터가 메인 스레드에서 어드바이저를 스폰하는 형태를 기본으로 한다. 세부 폴백(적대적 2차 검토 등)은 **구현 계획 단계에서 확정**.

## 종료 & 알림 (머지는 안 함)

- **머지 가능** → 터미널 벨(`printf '\a'`) + 눈에 띄는 한국어 알림. 머지 명령(`gh pr merge <PR> --squash` 등)은 **안내만**, 실행 X.
- **죽은 PR** (closed / merged / conflict / BEHIND / DIRTY) → 원인 + 사용자가 할 일 한 줄 안내 후 종료.
- **보류 건 있음** → 호출자에게 결정 요청 알림(무엇이 왜 갈리는지 근거 포함).

## 기존 커맨드와의 관계

- **`/pr-watch` 를 이 동작으로 진화** — 별도 커맨드 신설 없음.
- 현 커맨드의 제약 중 **"머지 X" 는 유지**, **"코드 수정 X / 자동 답글 X / 스스로 루프 X" 는 해제**(이미 메모리 정책이 override 한 방향과 정합).
- 1회성으로 호출해도 여전히 완결 동작(한 사이클).
- `allowed-tools` 를 갱신해야 한다: 현재 `Bash(gh:*), Bash(git:*), Bash(printf:*), Read, Grep, Glob`. 코드 수정·푸시·게이트·어드바이저·자기 페이싱을 위해 최소 `Edit`/`Write`, `Bash(bun:*)`(게이트), `Agent`(어드바이저), `ScheduleWakeup`, 그리고 답글용 `gh api` 가 필요 — 구체 목록은 계획 단계에서 확정.

## 문서 동기화 (AGENTS.md 체크리스트)

- `commands/pr-watch.md` — 본문 전면 개정 + frontmatter `description` / `allowed-tools` 갱신.
- `FEATURES.md` — 슬래시 커맨드 설명 갱신(한국어, humans 단일 소스).
- `AGENTS.md` — *Project in one line* / Layout 의 `/pr-watch` 설명 갱신(자동 머지 X·감시 데몬화 반영).
- 메모리 `pr-watch-bot-review-policy` — 새 동작과 정합 확인(에스컬레이션 사다리 · 게이트 통과시 푸시 · 자기 페이싱 종료 조건 추가).
- 이 커맨드는 MCP tool surface 가 아니므로 `src/index.ts` / `src/index.test.ts` / `plugin.json` 의 tool 등록은 건드리지 않는다.

## 범위 밖 (Out of scope)

- 자동 머지(`gh pr merge` 실행).
- 데몬 서버 / webhook 수신 / headless claude 호출 / 새 장기 실행 배포 타깃.
- 세션을 넘어 지속되는 감시(cron/routine 승격).
- MCP tool 화(`pr_watch_*` 같은 도구). 이건 슬래시 커맨드로 유지.

## 열린 구현 세부 (계획 단계에서 확정)

1. 사이클 간 상태(이미 처리한 threadId / 마지막 처리 시각)를 어떻게 들고 갈지 — 대화 컨텍스트 vs `/loop` 재주입.
2. 어드바이저 스폰 형태와 폴백.
3. CI 대기: 턴 내 `--watch` vs 다음 tick 재확인.
4. `allowed-tools` 최종 목록.
