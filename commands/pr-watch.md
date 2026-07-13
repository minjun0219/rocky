---
description: 열린 PR 을 감시하며 새 리뷰를 코드와 대조해 처리(타당→게이트 통과시 수정·푸시·답글·resolve)하고, 애매한 건 어드바이저·호출자에게 물어 판단하며, 머지 가능해지면 알린다. 자동 머지는 하지 않는다.
argument-hint: "[PR 번호 | URL | owner/repo#123] (생략 시 현재 브랜치의 PR)"
allowed-tools: Bash(gh:*), Bash(git:*), Bash(bun:*), Bash(printf:*), Read, Grep, Glob, Edit, Write, Agent, ScheduleWakeup
---

# pr-watch — PR 감시 데몬 사이클 (자동 머지 X)

너는 **하나의 열린 GitHub PR** 을 대상으로 **한 번의 완결된 감시 사이클**을 돈다:
상태 스냅샷 → 새 리뷰 처리(수정 / 답글 / resolve) → 머지 판정 → 자기 페이싱.
`/loop /pr-watch <PR>` (인터벌 생략 = dynamic) 로 감싸면 데몬처럼 반복된다.

- 대상 PR: `$ARGUMENTS` (비어 있으면 현재 브랜치에 연결된 PR 을 자동으로 찾는다)
- 모든 GitHub 접근은 `gh` CLI 로만. `curl` / 직접 API fetch 금지.
- 출력·커밋·답글은 **한국어**. 코드 identifier / 경로 / 명령어 / API path 는 영어 그대로.

## 핵심 원칙 (반드시 지킬 것)

1. **머지하지 않는다.** `gh pr merge` 를 절대 실행하지 마라. 종착점은 "머지 가능"
   **알림**까지다. 실제 머지는 사용자가 직접 한다.
2. **코드 수정은 게이트 통과시에만 푸시한다.** 타당한 지적은 직접 고치되,
   `bun run check` / `typecheck` / `test` 를 **모두 통과**했을 때만 커밋·푸시.
   하나라도 실패하면 푸시하지 말고, 그 건을 호출자에게 알린다.
3. **리뷰는 사이즈 기반 에스컬레이션 사다리로 처리한다** (아래 4단계).
4. **애매하면 어드바이저 → 그래도 갈리면 호출자.** 판단이 갈리는 지적은 독립
   서브에이전트(어드바이저)로 교차검증하고, 그래도 갈리면 호출자에게 물어 보류한다.
5. **한 호출 = 한 완결 사이클.** 스스로 무한 루프를 돌지 않는다. 반복은 `/loop`,
   사이클 사이의 대기 페이싱은 `ScheduleWakeup` 이 담당한다.
6. **머지 판정은 모든 리뷰 스레드가 resolved 됐을 때만.** 미해결/보류 스레드가
   남아 있으면 아직 머지 판정하지 않는다.

## 절차

### 1. 대상 PR 확정

- `$ARGUMENTS` 가 있으면 그것을 PR 핸들로 쓴다 (번호 / URL / `owner/repo#123`).
- 비어 있으면 현재 브랜치의 PR 을 찾는다:
  `gh pr view --json number,url,headRefName 2>/dev/null`
  - 실패하면(현재 브랜치에 PR 없음) 사용자에게 PR 번호/URL 을 한 번 묻고 멈춘다.

### 2. 스냅샷 수집

```bash
gh pr view <PR> --json number,title,url,state,isDraft,mergeable,mergeStateStatus,reviewDecision,statusCheckRollup,headRefName,baseRefName,reviewRequests,reviews
```

미해결 리뷰 스레드 + threadId + 첫 comment databaseId 를 GraphQL 로 가져온다 (REST
`/pulls/<n>/comments` 는 `isResolved` 를 안 주므로 GraphQL `reviewThreads` 를 쓴다):

```bash
gh api graphql -F owner=<owner> -F repo=<repo> -F num=<number> -f query='
query($owner:String!,$repo:String!,$num:Int!){
  repository(owner:$owner, name:$repo){ pullRequest(number:$num){
    reviewThreads(first:50){ nodes {
      id isResolved isOutdated
      comments(first:1){ nodes { databaseId author{login} path line body } } } } } } }'
# → isResolved==false 인 스레드만 아래 4단계에서 다룬다.
```

주의: `{owner}`/`{repo}` 자동 치환은 gh 의 **REST 엔드포인트** 기능이라 GraphQL query
문자열엔 적용되지 않는다. GraphQL 은 `$owner`/`$repo`/`$num` **변수**로 넘겨라 — 값은
`gh repo view --json owner,name` 과 1단계 `gh pr view <PR> --json number` 로 확보한다.

- `state != OPEN` (이미 MERGED / CLOSED) → 그 사실만 한 줄로 알리고 멈춘다 (종료).
- `isDraft == true` → draft 라는 점을 알리고, ready 로 바꾸기 전엔 머지 불가임을 안내
  (draft 해제는 사용자).

### 3. 현재 상태 리포트

- **CI 체크** — `statusCheckRollup` 을 `성공 N / 실패 M / 진행중 K` 로 집계. 실패 항목은 이름과 함께 나열.
- **리뷰** — `reviewDecision` (`APPROVED`/`CHANGES_REQUESTED`/`REVIEW_REQUIRED`/null) + 대기 중 `reviewRequests`.
- **머지 상태** — `mergeStateStatus` 해석:
  | 값 | 의미 | 머지 가능? |
  |---|---|---|
  | `CLEAN` | 모든 조건 충족 | ✅ 가능 |
  | `HAS_HOOKS` | 통과 (hook 있음) | ✅ 가능 |
  | `UNSTABLE` | 비필수 체크 실패 있음 | ⚠️ 가능하나 확인 권장 |
  | `BLOCKED` | 필수 리뷰/체크 미충족 | ❌ |
  | `BEHIND` | base 보다 뒤처짐 (업데이트 필요) | ❌ |
  | `DIRTY` | 머지 충돌 | ❌ |
  | `DRAFT` | draft 상태 | ❌ |
  | `UNKNOWN` | GitHub 계산 중 | 재확인 필요 |

### 4. 새 리뷰 처리 (미해결 스레드마다, 에스컬레이션 사다리)

**미해결(`isResolved==false`) 리뷰 스레드 = 이번 사이클의 작업 큐다.** GitHub 의
resolved 상태를 그대로 큐로 쓴다 — 이미 resolve 한 건 다시 보지 않는다 (별도 상태 파일
없음). 각 스레드를 `Read`/`Grep`/`Glob` 로 코드·라인과 대조하고, **사안 크기에 따라 아래
사다리에서 rung 을 스스로 고른다**:

- **작고 명백 (타당)** → 직접 처리:
  1. 코드/문서 수정 (`Edit`/`Write`).
  2. 게이트 실행: `bun run check` → `bun run typecheck` → `bun test`.
  3. **모두 통과** → 이번 건에 해당하는 파일만 스테이지 → 커밋 (Conventional 한국어 제목
     + 말미에 `Co-Authored-By: Claude <noreply@anthropic.com>`) → `git push`.
  4. 스레드에 무엇을 어떻게 고쳤는지 한국어 답글 → resolve.
  5. **게이트 실패** → 푸시하지 않는다. 실패 로그를 인용해 호출자에게 알리고, 그 스레드는
     미해결로 남긴다 (머지 판정 보류 사유가 된다).
- **반박 (지적이 사실과 다르거나 scope 밖)** → 사안이 크면 먼저 어드바이저로 교차검증
  (아래 "어드바이저"). 근거(코드 라인 / 기존 합의 / 가능하면 실증 결과)를 든 한국어 답글
  → resolve.
- **애매 (판단이 갈림)** → 어드바이저 상담 → 여전히 갈리면 **보류**: 호출자에게 무엇이 왜
  갈리는지 근거와 함께 결정을 요청하고, 그 스레드는 손대지 않는다. 나머지 스레드는 계속
  처리한다.

**답글 / resolve 방법:**

```bash
# 답글 (in_reply_to = 스레드 첫 comment 의 databaseId)
gh api -X POST /repos/<owner>/<repo>/pulls/<num>/comments \
  -f body='<한국어 답글>' -F in_reply_to=<comment_databaseId>

# resolve (threadId = reviewThreads 노드의 id)
gh api graphql -f query='mutation($id:ID!){
  resolveReviewThread(input:{threadId:$id}){ thread{ isResolved } } }' -F id=<threadId>
```

### 어드바이저 (독립 교차검증)

반박·애매 판단을 독립 서브에이전트로 교차검증한다:

- `Agent` (general-purpose) 를 띄워 **해당 리뷰 코멘트 + 관련 코드 인용 + 나의 잠정 판단**을
  주고, 회의적으로 **반증**하도록 요청한다. 예: *"이 리뷰 지적에 대한 내 잠정 결론은 X 다.
  근거는 Y. 이 결론을 반박해봐 — 근거가 약하거나 놓친 게 있으면 지적하고, 타당하면 그렇게
  말해줘."*
- 어드바이저가 내 판단을 지지 → 진행. 판단이 갈리거나 어드바이저도 불확실 → 호출자 보류
  rung 으로 내려간다.
- 코드 수정 자체는 인라인(메인)에서 하고, 어드바이저는 **판단 교차검증에만** 쓴다.

### 5. 머지 판정 & 자기 페이싱

미해결 스레드가 하나라도 남아 있으면(4단계 보류·게이트실패 포함) **아직 머지 판정하지 않는다.**

- **CI 진행중** (`statusCheckRollup` 에 `PENDING`/`IN_PROGRESS`, 또는 `mergeStateStatus==UNKNOWN`):
  이 턴 안에서 완료까지 기다린 뒤 **2단계 스냅샷을 다시 찍어 재판정**한다:
  ```bash
  gh pr checks <PR> --watch --fail-fast
  ```
- **모든 스레드 resolved + `mergeStateStatus ∈ {CLEAN, HAS_HOOKS}`** → 6단계 머지 가능
  알림으로 간다. (루프 종료 — 재예약하지 않는다.)
- **사람 리뷰 대기로 막힘** (`BLOCKED` / `reviewDecision ∈ {REVIEW_REQUIRED, CHANGES_REQUESTED}`,
  CI 통과, 내가 할 일 없음):
  - **`/loop` 로 구동 중이면** `ScheduleWakeup` 으로 20~30분 뒤(1200~1800s) 다음 사이클을
    예약한다. `reason` 에 무엇을 기다리는지(누구의 승인 / changes requested) 명시.
  - **1회성 호출이면** 무엇이 막고 있는지 알리고, 재실행 / `/loop /pr-watch <PR>` 폴링을
    안내한 뒤 멈춘다.
- **보류 건 있음** (애매 판단을 호출자에게 넘긴 경우) → 호출자 결정 대기. 재예약하지 않고
  (루프 종료), 무엇을 왜 결정해야 하는지 알린다.
- **머지 충돌 / BEHIND / DIRTY** → 원인과 사용자가 할 일(rebase/update-branch, 충돌 해소)을
  한 줄로 안내하고 멈춘다. (자동 rebase 하지 않는다.)

**자기 페이싱 요약:**
| 상태 | 다음 행동 |
|---|---|
| CI 진행중 | 턴 내 `gh pr checks --watch` → 재판정 |
| 사람 리뷰 대기 (`/loop` 구동) | `ScheduleWakeup` 1200~1800s |
| 사람 리뷰 대기 (1회성) | 안내 후 멈춤 |
| 머지 가능 / 죽은 PR / 보류 | 종료 (재예약 X) |

### 6. 머지 가능 알림

머지 가능 상태가 되면 **눈에 띄게** 알린다:

- 터미널 벨: `printf '\a'`
- 아래 형식으로 결론 출력:

```markdown
## ✅ 머지 가능 — <title> (#<number>)

<PR URL>

- CI: 전체 통과 (N/N)
- 리뷰: <APPROVED 등> / 모든 스레드 resolved
- 머지 상태: <CLEAN 등>

지금 머지할 수 있다. 머지는 직접:
`gh pr merge <PR> --squash` (또는 --merge / --rebase, repo 정책에 맞게)
```

- **여기서 멈춘다.** 머지는 실행하지 않는다.

## 실패 / 예외 처리

- `gh` 미인증 → `gh auth status` 로 확인하라고 한 줄 안내하고 멈춘다.
- PR 핸들 파싱 실패 → 입력을 그대로 인용하고 한 번 묻고 멈춘다.
- 게이트 실패(코드 수정 후) → 푸시하지 않고, 실패한 게이트/로그를 인용해 호출자에게 알린다.
  해당 스레드는 미해결로 남긴다. (실패를 감추거나 `--no-verify` 로 우회하지 않는다.)
- `gh pr checks --watch` 중 체크 실패(non-zero) → 실패한 체크 이름을 나열하고, 로그 확인
  (`gh pr checks <PR>` 또는 `gh run view`)을 안내한 뒤 멈춘다. (자동 재시도 금지)
- push 거부 / upstream 없음 → 에러를 그대로 인용하고 멈춘다 (강제 푸시 금지).
