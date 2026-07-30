---
description: PR 에 이미 붙어 있는 리뷰(Copilot / Codex / 사람)를 해소한다 — 판단이 필요 없는 명백한 오류는 즉시 고치고 코멘트 없이 스레드 resolve, 호출자가 확인해야 하는 건은 열어 둔 채 채팅으로 보고. GitHub 코멘트는 호출자가 승인할 때만 만든다. 머지 가능해지면 알리고 머지는 하지 않는다. 새로 리뷰하는 게 아니라 받은 리뷰에 대응하는 쪽(내 diff 를 검토받는 건 /rocky:review). 재리뷰를 기다리지 않는다.
argument-hint: "[PR 번호] (생략 시 현재 브랜치의 PR)"
allowed-tools: Bash(gh:*), Bash(git:*), Bash(bun:*), Read, Edit, Write, Grep, Glob, PushNotification
---

# resolve-reviews — PR 리뷰 해소

PR 에 지금 붙어 있는 리뷰를 처리한다. `$ARGUMENTS` 는 PR 번호(있으면). 출력·코멘트는
**한국어** (코드 identifier / 경로 / 명령어는 영어 그대로).

**PR 을 리뷰하는 커맨드가 아니다** — 이미 받은 리뷰 스레드를 해소하는 쪽이다. 내 작업
diff 를 검토받는 것은 `/rocky:review`, GitHub PR 을 리뷰하는 것은 빌트인 `/review`.

**한 번 돌고 끝난다.** 재리뷰를 폴링하지 않는다 — 리뷰를 한 번 더 받고 싶으면 사용자가
`@copilot review` / `@codex review` 를 직접 달고 이 커맨드를 다시 부른다. 봇 자동 재리뷰를 켜 두고
루프를 도는 방식은 라운드가 계속 쌓여 피로해서 걷어냈다.

## 원칙

1. **이 커맨드 호출 자체가 수정·커밋·푸시·resolve 승인이다.** 채팅에 사후 요약만 올린다. 매
   수정마다 확인을 받지 않는다.
2. **머지는 절대 하지 않는다.** 머지 가능 판정 + 알림까지가 끝. `gh pr merge` 를 쓰지 않는다.
3. **게이트 실패 = 푸시 없음.** `--no-verify` 우회 금지, force push 금지.
4. **코멘트는 가급적 달지 않는다.** GitHub 에 남는 글은 되돌려도 알림이 이미 갔고 남이 읽는다.
   수정한 건도 무효인 건도 코멘트 없이 resolve 만 한다 — 무엇을 고쳤는지는 커밋과 diff 가
   말한다. **코멘트는 호출자가 승인한 경우에만 만든다** (7단계). 그 밖에는 한 줄도 남기지
   않는다.
5. **모든 판정의 축은 "호출자 판단이 필요한가" 다.**
   - 필요 없다 → 즉시 고치고 resolve. 물어보지 않는다.
   - 필요하다 → **resolve 하지 않고** 채팅으로 보고한다. 스레드는 열린 채 남긴다 — 열려
     있는 스레드가 곧 "아직 결정되지 않았다" 는 표시이고, 닫아 버리면 그 사실이 사라진다.

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
- **미해결이 0 이면 8단계(머지 가능 판정)로 바로 간다.** 봇 리뷰가 아직 안 왔을 수도 있으니, 그
  사실을 보고에 적는다.

### 2. 분류

각 스레드를 셋 중 하나로 판정한다. **판정 근거는 지적이 가리키는 코드를 실제로 읽고 확인한
뒤에 정한다** — 리뷰 문구만 보고 정하지 않는다.

| 판정 | 조건 | 처리 |
| --- | --- | --- |
| **즉시 수정** | 호출자 판단이 필요 없는 명백한 오류 | 고치고 코멘트 없이 resolve |
| **무효** | 이미 해결됐거나 대상 코드가 사라짐 | 코멘트 없이 resolve |
| **확인 필요** | 호출자가 확인해야 하는 사항 | **resolve 하지 않고** 채팅으로 보고 (코멘트도 안 남긴다) |

**"명백한 오류" 는 셋을 다 만족할 때만이다.** 하나라도 아니면 [확인 필요]다.

1. 지적이 사실인 것을 코드로 확인했다 (리뷰어가 틀렸을 가능성을 실제로 배제했다).
2. 고치는 방법이 사실상 하나다 — 설계 선택이나 취향이 끼어들지 않는다.
3. 고쳐도 다른 결정을 건드리지 않는다 — 공개 계약(API·CLI·스키마), 문서화된 동작, 성능
   트레이드오프, 이 PR 의 범위 밖 코드가 걸리지 않는다.

전형적인 [확인 필요]: 지적이 틀렸다고 보는 건(반론), 고치는 방향이 둘 이상 갈리는 건, 맞는
지적이지만 이 PR 범위를 넘어 별도 작업이어야 하는 건, 사용자가 의도적으로 그렇게 둔 것으로
보이는 건.

**애매하면 [확인 필요]로 보낸다.** 판단이 필요한 것을 임의로 고쳐 resolve 하면 호출자는
그 결정이 있었다는 사실조차 모른다 — 반대 방향의 실수(물어봐서 한 번 더 확인받는 것)는
비용이 훨씬 싸다.

### 3. 수정 + 게이트

- [즉시 수정] 건을 구현한다. [확인 필요] 건은 손대지 않는다 — 어느 방향으로 고칠지가 곧
  호출자에게 물을 내용이다.
- 게이트를 순서대로 전부 실행한다.

```bash
bun run check
bun run typecheck
bun test
```

- 하나라도 실패하면 실패 로그를 그대로 인용하고 **푸시 없이 멈춘다**.

### 4. 커밋 · 푸시

- 커밋 1개. Conventional Commits 한국어 제목(`fix(review): …`), 본문에 처리한 스레드 요약.
- 커밋 메시지 말미에 반드시:

  ```
  Co-Authored-By: Claude <noreply@anthropic.com>
  ```

- 이번 처리에 해당하는 변경만 스테이지한다 (`git add -A` 금지).

```bash
git push
```

### 5. resolve

[즉시 수정] 과 [무효] 스레드를 **코멘트 없이** resolve 한다. [확인 필요] 스레드는 열린 채로
둔다 — resolve 도, 코멘트도 하지 않는다.

```bash
# THREAD_ID = 2단계에서 [즉시 수정] 또는 [무효] 로 판정한 스레드의 id (1단계 수집 결과의 nodes[].id)
for THREAD_ID in $RESOLVE_IDS; do
  gh api graphql -f id="$THREAD_ID" -f query='
  mutation($id:ID!){ resolveReviewThread(input:{threadId:$id}){ thread{ isResolved } } }
  '
done
```

### 6. 처리 보고 (채팅)

```
수정 3 / 확인 필요 1 / 무효 1
  ✅ src/core/worklog.ts:42  …지적 요약… → …어떻게 고쳤는지…
  ⏸  AGENTS.md:12           …지적 요약… → 확인 필요 (무엇을 정해야 하는지: …)
  ⊘  docs/codex.md:8        outdated, 코드 이동됨 → resolve
  게이트 통과 · 커밋 abc1234 푸시 · 스레드 4건 resolve, 1건 열림
```

장문 리포트를 쓰지 않는다. 스레드당 한 줄. `⏸` 줄에는 **호출자가 무엇을 정해야 하는지**를
적는다 — "보류" 만 적으면 사용자가 스레드를 직접 열어 봐야 한다.

### 7. 확인 필요 건 상의

[확인 필요] 를 채팅에서 하나씩 정리한다. 스레드마다 지적 요약 + 왜 판단이 필요한지 + 갈래를
보여주고 묻는다. 갈래가 둘뿐이면 그대로, 열려 있으면 선택지를 제시한다.

사용자 결정에 따라:

- **지적이 틀렸다(반론 승인)** → 한국어 근거 코멘트 1개를 그 스레드에 남기고 resolve.
  **이것이 이 커맨드가 코멘트를 만드는 유일한 경우다.**

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

- **고쳐야 한다 / 어느 방향으로 고칠지 정해졌다** → [즉시 수정] 으로 전환하고 3단계로 돌아가
  처리한다. 고친 뒤에는 코멘트 없이 resolve.
- **맞는 지적이지만 이 PR 범위 밖이다** → 후속 작업으로 남긴다(보드 항목 / 이슈). 스레드를
  닫을지는 사용자가 정한다 — 닫기로 하면 어디로 옮겼는지 한 줄 코멘트 후 resolve.
- **사용자가 답하지 않고 넘어갔다** → 스레드를 열린 채 남기고 끝낸다. 임의로 resolve 하지
  않는다. 8단계는 "미해결 0" 이 아니므로 머지 가능 판정이 나지 않는다 — 그 사실을 보고한다.

### 8. 머지 가능 판정 + 알림

```bash
gh pr checks "$NUM"
gh pr view "$NUM" --json mergeable,mergeStateStatus,reviewDecision
```

- 조건: 미해결 스레드 0 · checks 전부 통과 · `mergeStateStatus` 가 `CLEAN` 또는 `UNSTABLE`.
- **푸시 직후의 첫 조회를 믿지 않는다.** GitHub 은 `mergeable` / `mergeStateStatus` 를 비동기로
  다시 계산해서, 방금 푼 충돌이 몇 초간 `CONFLICTING` / `DIRTY` 로 남아 있다. 값이 부정적이면
  몇 초 간격으로 두어 번 더 조회해 안정된 값으로 판정한다.
- **`BLOCKED` 을 곧바로 "머지 불가" 로 단정하지 않는다.** 마지막 푸시에 대한 봇 리뷰가 아직
  안 끝났을 때도 `BLOCKED` 이 나온다 (레포에 `copilot_code_review` ruleset 이 걸린 경우). 사유를
  먼저 확인한다.

  ```bash
  gh api "repos/$OWNER/$REPO/rulesets" --jq '.[].id' \
    | xargs -I{} gh api "repos/$OWNER/$REPO/rulesets/{}" --jq '{name, rules:[.rules[].type]}'
  ```

  - 승인 리뷰 부족(`required_approving_review_count` 미달)처럼 **사람이 풀어야 하는 사유**면 그대로
    보고한다.
  - 봇 리뷰 대기처럼 **시간이 풀어주는 사유**면 그 사실을 보고하고 끝낸다. 기다리지 않는다 —
    잠시 뒤 이 커맨드를 다시 부르면 된다.
- 충족 → `PushNotification` 으로 알리고 채팅에 최종 요약을 남긴다.

  ```
  PR #<번호> 머지 가능 — 리뷰 <X>건 처리 / 확인 필요 <Y>건
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
| 미해결 스레드 0 | 8단계로 바로 이동, 보고에 명시 |
| 게이트 실패 | 푸시 없이 중단, 실패 로그 인용 |
| push 거부 (원격이 앞서 있음) | 에러 그대로 인용하고 중단. force push 금지 |
| 확인 필요 건의 3단계 복귀가 3회 넘음 | 중단 + 상태 보고 |
