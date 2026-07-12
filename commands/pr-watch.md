---
description: PR 상태(CI 체크·리뷰·머지 가능성)를 점검하고, 열린 리뷰 코멘트를 코드와 대조해 정리한 뒤, 머지 가능한 상태가 되면 알려준다. 머지는 하지 않는다.
argument-hint: "[PR 번호 | URL | owner/repo#123] (생략 시 현재 브랜치의 PR)"
allowed-tools: Bash(gh:*), Bash(git:*), Bash(printf:*), Read, Grep, Glob
---

# pr-watch — PR 머지 대기 감시

너는 지금 **하나의 열린 GitHub PR** 을 대상으로 상태를 점검하고, 리뷰를 정리하고,
**머지 가능한 상태가 되면 사용자에게 알려주는** 역할을 한다.

- 대상 PR: `$ARGUMENTS` (비어 있으면 현재 브랜치에 연결된 PR 을 자동으로 찾는다)
- 모든 GitHub 접근은 `gh` CLI 로만 한다. `curl` / 직접 API fetch 금지.
- 출력은 **한국어**. 코드 identifier / 경로 / 명령어 / API path 는 영어 그대로.

## 핵심 원칙 (반드시 지킬 것)

1. **머지하지 않는다.** `gh pr merge` 를 절대 실행하지 마라. 이 커맨드의 종착점은
   "머지 가능해졌다"는 **알림**까지다. 실제 머지는 사용자가 직접 한다.
2. **코드를 수정하지 않는다.** 리뷰 코멘트에 대해서는 "이 위치를 이렇게 고치면 된다"는
   **권고**까지만. 실제 commit / push 는 사용자 몫.
3. **사람 리뷰를 강제 대기하지 않는다.** CI 는 `gh pr checks --watch` 로 이 턴 안에서
   끝까지 기다릴 수 있지만, reviewer 의 승인은 기다릴 수 없으니 재실행을 안내한다.
4. 한 번 호출 = 한 번의 완결된 점검. 무한 루프를 스스로 돌지 않는다.

## 절차

### 1. 대상 PR 확정

- `$ARGUMENTS` 가 있으면 그것을 PR 핸들로 쓴다 (번호 / URL / `owner/repo#123` 모두 허용).
- 비어 있으면 현재 브랜치의 PR 을 찾는다:
  `gh pr view --json number,url,headRefName 2>/dev/null`
  - 실패하면(= 현재 브랜치에 PR 없음) 사용자에게 PR 번호/URL 을 한 번 묻고 멈춘다.

### 2. 스냅샷 수집

한 번의 호출로 필요한 필드를 모두 가져온다:

```bash
gh pr view <PR> --json number,title,url,state,isDraft,mergeable,mergeStateStatus,reviewDecision,statusCheckRollup,headRefName,baseRefName,reviewRequests
```

- `state != OPEN` (이미 MERGED / CLOSED) 이면: 그 사실만 한 줄로 알리고 멈춘다.
  watch 할 대상이 없다.
- `isDraft == true` 이면: draft 라는 점을 알리고, ready 로 바꾸기 전엔 머지 불가임을
  안내한다 (draft 해제는 사용자가).

### 3. 현재 상태 리포트

아래 신호를 사람이 읽기 쉽게 한국어로 요약한다:

- **CI 체크** — `statusCheckRollup` 을 집계해 `성공 N / 실패 M / 진행중 K` 로. 실패 항목은
  이름과 함께 나열.
- **리뷰** — `reviewDecision` (`APPROVED` / `CHANGES_REQUESTED` / `REVIEW_REQUIRED` / null) 과
  대기 중인 `reviewRequests`.
- **머지 상태** — `mergeStateStatus` 를 해석한다:
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

### 4. 열린 리뷰 코멘트 정리 (있을 때만)

리뷰/코멘트가 있으면 코드와 대조해 사용자의 대응을 돕는다.

- 리뷰 본문과 **미해결 리뷰 스레드**를 가져온다. REST(`/pulls/<n>/comments`)는 스레드의
  `isResolved` 를 주지 않으므로, 미해결만 정확히 거르려면 GraphQL `reviewThreads` 를 쓴다:
  ```bash
  gh pr view <PR> --json reviews          # 리뷰 본문 (approve / changes / comment)
  gh api graphql -F owner=<owner> -F repo=<repo> -F num=<number> -f query='
  query($owner:String!,$repo:String!,$num:Int!){
    repository(owner:$owner, name:$repo){ pullRequest(number:$num){
      reviewThreads(first:50){ nodes {
        isResolved isOutdated
        comments(first:1){ nodes { author{login} path line body } } } } } } }'
  # → isResolved==false 인 스레드만 아래에서 다룬다 (resolved 는 이미 처리됨)
  ```
  주의: `{owner}`/`{repo}` 자동 치환은 gh 의 **REST 엔드포인트** 기능이라 GraphQL query 문자열엔
  적용되지 않는다. GraphQL 은 위처럼 `$owner`/`$repo`/`$num` **변수**로 넘겨라 —
  값은 `gh repo view --json owner,name` 과 1단계의 `gh pr view <PR> --json number` 로 확보한다.
- 각 **미해결(isResolved==false)** 코멘트에 대해 `Read` / `Grep` / `Glob` 으로 해당 파일·라인을 확인하고,
  다음 셋 중 하나로 **분류 + 한 줄 근거**를 제시한다:
  - **타당** — 지적이 코드와 일치. 어떻게 고치면 되는지 `file:line` 인용과 함께 권고.
  - **반박** — 지적이 사실과 다르거나 scope 밖. 근거(코드 라인 / 기존 합의)를 들어 반박안 제시.
  - **보류** — 합리적이나 이 PR scope 밖. 후속 이슈/PR 제안.
- **답글을 자동으로 달지 않는다.** 위 정리를 사용자에게 보여주고, 사용자가 답글/수정 여부를 결정.
  (테스트 / 타입 / lint 결과로 갈리는 코멘트는 스스로 굴리지 말고, 필요하면 사용자에게
  `bun test` / `bun run typecheck` 결과를 물어본다.)

### 5. 머지 가능 여부 판정 & 대기

- **이미 머지 가능** (`mergeStateStatus` ∈ {`CLEAN`, `HAS_HOOKS`}):
  → 6단계 "머지 가능 알림" 으로 간다.

- **CI 가 진행 중** (`statusCheckRollup` 에 `PENDING`/`IN_PROGRESS`, 또는 `mergeStateStatus == UNKNOWN`):
  → CI 완료까지 이 턴 안에서 기다린다:
  ```bash
  gh pr checks <PR> --watch --fail-fast
  ```
  (이 명령은 모든 체크가 끝나면 반환한다. 실패 시 non-zero.)
  완료 후 **2단계 스냅샷을 다시 찍어** 재판정한다. 이제 머지 가능하면 6단계로,
  여전히 막혀 있으면 아래 "사람 리뷰 대기" 로.

- **사람 리뷰 대기로 막힘** (`BLOCKED` / `reviewDecision ∈ {REVIEW_REQUIRED, CHANGES_REQUESTED}` 이고
  CI 는 통과):
  → reviewer 승인은 강제할 수 없다. 무엇이 막고 있는지(누구의 리뷰 대기 / changes requested)
  명확히 알리고, **재실행 안내**로 멈춘다:
  > 사람 리뷰 대기 중이라 지금은 머지 불가. 승인이 올라온 뒤 `/pr-watch <PR>` 를 다시 돌리거나,
  > 주기적으로 지켜보려면 `/loop 5m /pr-watch <PR>` 로 백그라운드 폴링을 걸 수 있다.

- **머지 충돌 / BEHIND / DIRTY**:
  → 원인과 사용자가 할 일(rebase/merge base, 충돌 해소)을 한 줄로 안내하고 멈춘다.

### 6. 머지 가능 알림

머지 가능 상태(`CLEAN` / `HAS_HOOKS`, 또는 사용자가 수용한 `UNSTABLE`)가 되면 **눈에 띄게** 알린다:

- 터미널 벨을 울려 주의를 끈다: `printf '\a'`
- 아래 형식으로 결론을 출력한다:

```markdown
## ✅ 머지 가능 — <title> (#<number>)

<PR URL>

- CI: 전체 통과 (N/N)
- 리뷰: <APPROVED 등>
- 머지 상태: <CLEAN 등>

지금 머지할 수 있다. 머지는 직접:
`gh pr merge <PR> --squash` (또는 --merge / --rebase, repo 정책에 맞게)
```

- **여기서 멈춘다.** 머지는 실행하지 않는다.

## 실패 / 예외 처리

- `gh` 미인증 → `gh auth status` 로 확인하라고 한 줄 안내하고 멈춘다.
- PR 핸들 파싱 실패 → 입력을 그대로 인용하고 한 번 묻고 멈춘다.
- `gh pr checks --watch` 중 체크 실패(non-zero) → 실패한 체크 이름을 나열하고,
  로그 확인(`gh pr checks <PR>` 또는 `gh run view`)을 안내한 뒤 멈춘다. (자동 재시도 금지)
