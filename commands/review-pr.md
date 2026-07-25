---
description: PR 에 붙은 리뷰(Copilot / Codex / 사람)를 미해결 0 까지 처리한다 — 수정·게이트·푸시·resolve 를 반복하고, 반론은 모아 마지막에 상의한 뒤 코멘트한다. 머지 가능해지면 알림.
argument-hint: "[PR 번호] (생략 시 현재 브랜치의 PR)"
allowed-tools: Bash(gh:*), Bash(git:*), Bash(bun:*), Read, Edit, Write, Grep, Glob, Monitor, PushNotification
---

# review-pr — PR 리뷰 대응 루프

PR 에 붙은 리뷰를 미해결 0 까지 처리한다. `$ARGUMENTS` 는 PR 번호(있으면). 출력·코멘트는
**한국어** (코드 identifier / 경로 / 명령어는 영어 그대로).

## 원칙

1. **이 커맨드 호출 자체가 수정·커밋·푸시·resolve 승인이다.** 라운드마다 채팅에 사후 요약만
   올린다. 매 수정마다 확인을 받지 않는다.
2. **머지는 절대 하지 않는다.** 머지 가능 판정 + 알림까지가 끝. `gh pr merge` 를 쓰지 않는다.
3. **게이트 실패 = 푸시 없음.** `--no-verify` 우회 금지, force push 금지.
4. **코멘트는 필요한 사항만.** 수정 반영 건은 코멘트 없이 resolve 만 한다. 커맨드가 코멘트를
   남기는 경우는 둘뿐 — (a) 사용자가 승인한 반론의 근거, (b) 사용자가 승인한 `@codex review`
   트리거. 둘 다 9단계에서 명시 승인을 받은 뒤에만 단다.
5. **반론은 혼자 결정하지 않는다.** 판단이 갈리는 리뷰는 보류 큐에 쌓고, 루프가 수렴한 뒤
   사용자와 상의해 결정한다.

## 절차

### 0. 준비

```bash
gh auth status
gh pr view $ARGUMENTS --json number,url,headRefName,baseRefName
git branch --show-current
```

- `gh` 미인증 → 중단.
- 인자가 없으면 현재 브랜치의 PR 을 쓴다. PR 이 없으면 중단하고 `/rocky:finish` 를 안내한다.
- 현재 브랜치가 PR 의 `headRefName` 과 다르면 **중단**하고 체크아웃을 안내한다. `main` 에서는
  실행하지 않는다.
- `owner` / `repo` 는 `gh repo view --json owner,name` 으로 얻는다.

### 1. 리뷰 수집

0단계에서 확인한 값을 먼저 변수로 잡는다 (플레이스홀더를 손으로 채우지 않는다).

```bash
OWNER=$(gh repo view --json owner --jq .owner.login)
REPO=$(gh repo view --json name --jq .name)
NUM=$(gh pr view $ARGUMENTS --json number --jq .number)

gh api graphql -f owner="$OWNER" -f repo="$REPO" -F num="$NUM" -f query='
query($owner:String!, $repo:String!, $num:Int!, $after:String) {
  repository(owner:$owner, name:$repo) { pullRequest(number:$num) {
    reviewThreads(first:100, after:$after){
      pageInfo{ hasNextPage endCursor }
      nodes{
        id isResolved isOutdated path line
        comments(first:50){nodes{author{login} body diffHunk}}
      }
    }
  }}
}'
```

- **페이지네이션 필수.** `pageInfo.hasNextPage` 가 `true` 면 `-f after="<endCursor>"` 를 붙여 다음
  페이지를 이어서 조회하고, `false` 가 될 때까지 반복해 전부 모은다. 한 페이지만 보고 판단하면
  스레드가 100개를 넘는 PR 에서 남은 미해결을 놓친 채 "미해결 0" 으로 오판해 조기 종료한다.
- `isResolved: false` 인 스레드만 대상으로 삼는다.
- `isOutdated` 는 참고 정보로만 쓴다 — 자동 제외하지 않는다. 코드가 옮겨졌을 뿐 지적이 유효할
  수 있다.
- 봇(`copilot-pull-request-reviewer` / `chatgpt-codex-connector`)과 사람 스레드를 모두 모은다.
  **사람 리뷰어의 지적이 봇보다 우선순위가 높다.**
- 이전 라운드에서 보류 큐에 넣은 스레드는 다시 분류하지 않는다.

### 2. 분류

각 스레드를 셋 중 하나로 판정한다. 판정 근거는 실제 코드를 읽고 확인한 뒤에 정한다.

| 판정 | 처리 |
| --- | --- |
| **수정** | 이번 라운드에서 고친다 |
| **반론** | 보류 큐에 넣는다 — 이 라운드에서 코멘트도 resolve 도 하지 않는다 |
| **무효** | 이미 해결됐거나 대상 코드가 사라짐 → 코멘트 없이 resolve |

### 3. 수정 + 게이트

- [수정] 건을 구현한다.
- 게이트를 순서대로 전부 실행한다.

```bash
bun run check
bun run typecheck
bun test
```

- 하나라도 실패하면 실패 로그를 그대로 인용하고 **푸시 없이 멈춘다**.

### 4. 커밋 · 푸시

- 라운드당 커밋 1개. Conventional Commits 한국어 제목(`fix(review): …`), 본문에 처리한 스레드 요약.
- 커밋 메시지 말미에 반드시:

  ```
  Co-Authored-By: Claude <noreply@anthropic.com>
  ```

- 이번 라운드에 해당하는 변경만 스테이지한다 (`git add -A` 금지).

```bash
git push
```

### 5. resolve

[수정] 과 [무효] 스레드를 **코멘트 없이** resolve 한다. [반론] 스레드는 건드리지 않는다.

```bash
# THREAD_ID = 2단계에서 [수정] 또는 [무효] 로 판정한 스레드의 id (1단계 수집 결과의 nodes[].id)
for THREAD_ID in $RESOLVE_IDS; do
  gh api graphql -f id="$THREAD_ID" -f query='
  mutation($id:ID!){ resolveReviewThread(input:{threadId:$id}){ thread{ isResolved } } }
  '
done
```

### 6. 라운드 보고 (채팅)

```
라운드 N — 처리 3 / 반론 보류 1 / 무효 1
  ✅ src/core/worklog.ts:42  …지적 요약… → …어떻게 고쳤는지…
  ⏸  AGENTS.md:12           …지적 요약… → 보류 (근거: …)
  ⊘  docs/codex.md:8        outdated, 코드 이동됨 → resolve
  게이트 통과 · 커밋 abc1234 푸시
```

장문 리포트를 쓰지 않는다. 스레드당 한 줄.

### 7. 재리뷰 대기

`Monitor` 로 60초 간격 폴링한다. 5단계에서 resolve 를 마쳤으니 이 시점의 미해결은 **보류 큐를
제외하면 0** 이다. 따라서 **미해결 개수가 0보다 크면 새 리뷰**로 보면 된다 — 스레드 id 목록을
들고 비교하지 않는다. 그 목록은 라운드마다 낡아서 이미 처리한 스레드를 다시 새것으로 알린다.

```bash
for i in $(seq 1 8); do
  sleep 60
  n=$(gh api graphql -f owner="$OWNER" -f repo="$REPO" -F num="$NUM" -f query='
  query($owner:String!, $repo:String!, $num:Int!) {
    repository(owner:$owner, name:$repo) { pullRequest(number:$num) {
      reviewThreads(first:100){nodes{isResolved}}
    }}
  }' --jq '[.data.repository.pullRequest.reviewThreads.nodes[]|select(.isResolved==false)]|length') || n=0
  if [ "${n:-0}" -gt 0 ]; then echo "NEW REVIEW: 미해결 ${n}건 (${i}분 경과)"; exit 0; fi
done
echo "CONVERGED: 8분간 새 리뷰 없음"
```

- 보류 큐에 넣은 스레드가 있으면 그 개수를 빼고 센다.
- **8분간 새 스레드가 없으면 리뷰 종료로 판정**하고 9단계로 간다.
  (Copilot 은 푸시마다 자동 재리뷰하며 실측 지연이 2~5분이다.)
- **트리거 코멘트를 달지 않는다.** Copilot 은 이미 자동으로 돌고, Codex 는 사용자가 의도적으로
  수동 게이트로 막아둔 것이다.

### 8. 루프

- 새 리뷰가 있으면 1단계로 복귀한다.
- 보류 큐를 제외한 미해결이 0 이고 8분간 새 리뷰가 없으면 수렴으로 본다.
- **라운드 상한 5.** 초과하면 멈추고 현재 상태를 보고한다.
- **진전 없음 감지**: 같은 스레드를 두 라운드 연속 못 고치면 중단하고 보고한다.

### 9. 반론 상의 (수렴 후)

보류 큐를 채팅에서 하나씩 사용자와 정리한다. 스레드마다 지적 요약 + 반론 근거를 보여주고 묻는다.

- 반론 **승인** → 한국어 근거 코멘트 1개를 그 스레드에 남기고 resolve.

  본문은 **heredoc 으로 변수에 담아** 넘긴다. 작은따옴표로 인라인하면 근거에 아포스트로피나
  줄바꿈이 들어갈 때 셸에서 깨진다.

  ```bash
  BODY=$(cat <<'EOF'
  <한국어 근거>
  EOF
  )
  gh api graphql -f id="$THREAD_ID" -f body="$BODY" -f query='
  mutation($id:ID!, $body:String!){ addPullRequestReviewThreadReply(input:{pullRequestReviewThreadId:$id, body:$body}){ comment{ url } } }
  '
  ```

- 반론 **반려** → 수정 대상으로 전환하고 3단계로 복귀한다 (라운드 카운트는 이어서 센다).
- 이 자리에서 **"Codex 재리뷰를 돌릴까?"** 도 함께 묻는다. 사용자가 승인할 때만 `@codex review`
  코멘트를 달고 7단계로 돌아간다. 승인이 없으면 달지 않는다.

  ```bash
  gh pr comment "$NUM" --body "@codex review"
  ```

### 10. 머지 가능 판정 + 알림

```bash
gh pr checks "$NUM"
gh pr view "$NUM" --json mergeable,mergeStateStatus,reviewDecision
```

- 조건: 미해결 스레드 0 · checks 전부 통과 · `mergeStateStatus` 가 `CLEAN` 또는 `UNSTABLE`.
- **`BLOCKED` 을 곧바로 "머지 불가" 로 단정하지 않는다.** 마지막 푸시에 대한 봇 리뷰가 아직
  안 끝났을 때도 `BLOCKED` 이 나온다 (레포에 `copilot_code_review` ruleset 이 걸린 경우). 사유를
  먼저 확인한다.

  ```bash
  gh api "repos/$OWNER/$REPO/rulesets" --jq '.[].id' \
    | xargs -I{} gh api "repos/$OWNER/$REPO/rulesets/{}" --jq '{name, rules:[.rules[].type]}'
  ```

  - 승인 리뷰 부족(`required_approving_review_count` 미달)처럼 **사람이 풀어야 하는 사유**면 그대로
    보고한다.
  - 봇 리뷰 대기처럼 **시간이 풀어주는 사유**면 7단계로 돌아가 한 번 더 대기한 뒤 재판정한다
    (이 재판정도 라운드 상한에 포함).
- 충족 → `PushNotification` 으로 알리고 채팅에 최종 요약을 남긴다.

  ```
  PR #<번호> 머지 가능 — 리뷰 <X>건 처리 / 반론 <Y>건
  ```

- 미충족 → 무엇이 막고 있는지(실패한 check 이름, `BLOCKED` 사유) 채팅에 보고한다. 알림은 보내지
  않는다.
- **머지는 하지 않는다.** 머지는 사용자 몫이다.

## 실패 / 예외 처리

| 상황 | 처리 |
| --- | --- |
| `main` 브랜치 / PR head 불일치 | 즉시 중단, 체크아웃 안내 |
| `gh` 미인증 | 즉시 중단 |
| PR 없음 | 중단하고 `/rocky:finish` 안내 |
| 게이트 실패 | 푸시 없이 중단, 실패 로그 인용 |
| push 거부 (원격이 앞서 있음) | 에러 그대로 인용하고 중단. force push 금지 |
| 라운드 상한 5 초과 | 중단 + 상태 보고 |
| 같은 스레드 2라운드 연속 미해결 | 중단 + 보고 |
| 폴링 8분 무반응 | 정상 수렴으로 간주하고 9단계로 |
