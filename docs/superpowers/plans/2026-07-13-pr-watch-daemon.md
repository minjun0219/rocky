# `/pr-watch` 데몬 감시 진화 — 구현 계획

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `/pr-watch` 슬래시 커맨드를, PR 을 감시하며 새 리뷰를 코드와 대조해 자율 처리(타당→게이트 통과시 수정·푸시·답글·resolve)하고 애매한 건 어드바이저·호출자에게 물어 판단하며 머지 가능해지면 알리는(자동 머지 X) **데몬 감시 사이클**로 진화시킨다.

**Architecture:** 이 작업은 **프롬프트 엔지니어링 + 문서 동기화**다 — TypeScript 코드나 MCP tool surface 변경 없음. 산출물은 개정된 `commands/pr-watch.md` 프롬프트와, 그에 정합하도록 갱신된 4개 문서(`FEATURES.md` / `README.md` / `AGENTS.md` / 메모리)다. 실행 모델은 "1 호출 = 1 완결 사이클"이고, 반복은 `/loop` 가, 사이클 사이의 idle 페이싱은 `ScheduleWakeup` 이 담당한다. 사이클 간 상태는 GitHub 의 리뷰 스레드 `isResolved` 를 작업 큐로 삼아 별도 저장 없이 처리한다.

**Tech Stack:** Claude Code 슬래시 커맨드(markdown + frontmatter), `gh` CLI (REST + GraphQL), `git`, Bun 게이트(`bun run check` / `typecheck` / `test`), `Agent`(어드바이저 서브에이전트), `ScheduleWakeup`(자기 페이싱).

## Global Constraints

- **머지 금지**: `gh pr merge` 를 절대 실행하지 않는다. 종착점은 "머지 가능" 알림까지.
- **게이트 통과시에만 푸시**: 코드 수정 후 `bun run check` / `bun run typecheck` / `bun test` 를 모두 통과했을 때만 커밋·푸시. 실패 시 푸시 금지, 해당 건은 호출자에게 알림.
- **GitHub 접근은 `gh` CLI 로만**: `curl` / 직접 API fetch 금지. GitHub MCP 도 쓰지 않는다.
- **출력·커밋·답글·PR 은 한국어**: 코드 identifier / 경로 / 명령어 / API path 는 영어 그대로.
- **커밋 trailer**: 커밋 메시지 말미에 `Co-Authored-By: Claude <noreply@anthropic.com>`.
- **데몬 서버 없음**: webhook / headless claude / 새 배포 타깃 금지. 세션 수명, `gh` 폴링.
- **MCP tool surface 불변**: 이 커맨드는 슬래시 커맨드이므로 `src/index.ts` / `src/index.test.ts` / `.claude-plugin/plugin.json` 의 tool 등록은 건드리지 않는다.
- **AGENTS.md 단일 소스 규칙**: 사용자 표면(커맨드 동작)이 바뀌므로 `FEATURES.md`(한국어) + `AGENTS.md`(영문) + `README.md` 를 lockstep 으로 갱신한다.

---

## 파일 구조

- **Modify**: `commands/pr-watch.md` — 커맨드 프롬프트 전면 개정 (frontmatter `description`/`allowed-tools` + 본문). 이 작업의 핵심 산출물.
- **Modify**: `FEATURES.md` (203~209행 `### /pr-watch [PR]` 블록) — 한국어 사용자 문서.
- **Modify**: `README.md` (18행) + `AGENTS.md` (9행 *Project in one line*, 29행 Layout) — 진입 문서 / 에이전트 문서.
- **Modify**: `/Users/minjun/.claude/projects/-Users-minjun-dev-workspaces-agent-toolkit-rocky/memory/pr-watch-bot-review-policy.md` — 새 동작(에스컬레이션 사다리 · 게이트 통과시 푸시 · 자기 페이싱 종료 조건)과 정합.

검증은 자동 단위 테스트가 아니라 (슬래시 커맨드엔 테스트가 없음): frontmatter YAML 파싱 · `gh`/GraphQL 스니펫 유효성 · 리포 게이트 무회귀(`bun run check`) · 문서 상호 정합 재검토로 한다.

---

## Task 1: `commands/pr-watch.md` 전면 개정

**Files:**
- Modify: `commands/pr-watch.md` (전체 교체)

**Interfaces:**
- Consumes: 없음 (독립 프롬프트 파일).
- Produces: `/pr-watch` 커맨드의 새 계약 — 나머지 문서 태스크가 이 동작을 서술한다:
  - frontmatter `allowed-tools: Bash(gh:*), Bash(git:*), Bash(bun:*), Bash(printf:*), Read, Grep, Glob, Edit, Write, Agent, ScheduleWakeup`
  - 실행 모델: 1 호출 = 1 완결 사이클, `/loop` 로 반복, `ScheduleWakeup` 로 idle 페이싱
  - 리뷰 처리: 에스컬레이션 사다리(타당→게이트 통과시 수정·푸시 / 반박→답글+resolve / 애매→어드바이저→호출자 보류)
  - 머지 판정: 모든 스레드 resolved + `mergeStateStatus ∈ {CLEAN, HAS_HOOKS}`, 자동 머지 X

- [ ] **Step 1: 아래 내용으로 `commands/pr-watch.md` 전체를 교체한다**

````markdown
---
description: 열린 PR 을 감시하며 새 리뷰를 코드와 대조해 처리(타당→게이트 통과시 수정·푸시·답글·resolve)하고, 애매한 건 어드바이저·호출자에게 물어 판단하며, 머지 가능해지면 알린다. 자동 머지는 하지 않는다.
argument-hint: "[PR 번호 | URL | owner/repo#123] (생략 시 현재 브랜치의 PR)"
allowed-tools: Bash(gh:*), Bash(git:*), Bash(bun:*), Bash(printf:*), Read, Grep, Glob, Edit, Write, Agent, ScheduleWakeup
---

# pr-watch — PR 머지까지 감시하는 데몬 사이클

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
````

- [ ] **Step 2: frontmatter 가 유효한 YAML 인지 확인한다**

Run:
```bash
head -6 commands/pr-watch.md
bun -e 'const s=require("fs").readFileSync("commands/pr-watch.md","utf8"); const m=s.match(/^---\n([\s\S]*?)\n---/); if(!m) throw new Error("no frontmatter"); for(const k of ["description","argument-hint","allowed-tools"]) if(!m[1].includes(k+":")) throw new Error("missing "+k); console.log("frontmatter OK");'
```
Expected: `frontmatter OK` 출력. (3개 키 모두 존재하고 `---` 블록이 파싱됨.)

- [ ] **Step 3: 리포 게이트 무회귀 확인 (문서 변경이 아무것도 깨지 않음)**

Run:
```bash
bun run check
```
Expected: PASS (Biome 는 `.md` 를 린트하지 않으므로 통과해야 한다. 만약 실패하면 이전부터 있던 무관한 문제 — 커맨드 파일 때문이 아님).

- [ ] **Step 4: 커밋**

```bash
git add commands/pr-watch.md
git commit -m "$(cat <<'EOF'
feat(pr-watch): 데몬 감시 사이클로 진화 — 리뷰 자율 처리 + 에스컬레이션

1 호출 = 1 완결 사이클(/loop 로 반복, ScheduleWakeup 로 idle 페이싱).
미해결 리뷰 스레드를 큐로 삼아 사이즈 기반 사다리로 처리: 타당→게이트 통과시
수정·푸시·답글·resolve / 반박→근거 답글+resolve / 애매→어드바이저→호출자 보류.
머지 판정은 전부 resolved + CLEAN/HAS_HOOKS 일 때만. 자동 머지는 여전히 안 함.

Co-Authored-By: Claude <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: 사용자 문서 동기화 (`FEATURES.md` / `README.md` / `AGENTS.md`)

**Files:**
- Modify: `FEATURES.md` (203~209행 `### /pr-watch [PR]`)
- Modify: `README.md` (18행)
- Modify: `AGENTS.md` (9행 *Project in one line* 의 슬래시 커맨드 구절, 29행 Layout)

**Interfaces:**
- Consumes: Task 1 의 새 커맨드 계약 (에스컬레이션 사다리 · 게이트 통과시 푸시 · 자기 페이싱 · 자동 머지 X).
- Produces: 없음 (문서 종단).

- [ ] **Step 1: `FEATURES.md` 의 `### /pr-watch [PR]` 블록(203~209행)을 아래로 교체**

기존 5개 bullet 을 아래로 바꾼다 (`### /pr-watch [PR]` 헤더는 유지):

```markdown
- **What**: 열린 GitHub PR 하나를 **감시**하며, 새 리뷰 코멘트를 코드와 대조해 처리하고, **머지 가능한 상태가 되면 알려준다.**
- **Input**: PR 번호 / URL / `owner/repo#123`. 생략하면 현재 브랜치에 연결된 PR 을 자동으로 찾는다.
- **동작**: 1 호출 = 1 완결 사이클. 미해결 리뷰 스레드를 작업 큐로 삼아 **에스컬레이션 사다리**로 처리 — 타당·명백하면 직접 수정 후 **게이트(`bun run check`/`typecheck`/`test`) 통과시에만** 커밋·푸시·답글·resolve, 반박이면 근거 답글+resolve, 애매하면 **어드바이저(독립 서브에이전트) 교차검증 → 그래도 갈리면 호출자에게 보류**. 모든 스레드가 resolved 되고 `mergeStateStatus ∈ {CLEAN, HAS_HOOKS}` 이면 머지 가능 알림.
- **반복·페이싱**: CI 진행 중이면 턴 내 `gh pr checks --watch` 로 대기 후 재판정. `/loop /pr-watch <PR>` 로 감싸면 데몬처럼 반복되며, 사람 리뷰 대기 중엔 `ScheduleWakeup` 으로 20~30분 간격 자기 페이싱한다.
- **하지 않는 것**: **자동 머지 금지** (`gh pr merge` 실행 X — 머지 가능 알림까지만), **게이트 실패 시 푸시 금지**, 데몬 서버/webhook 금지(세션 수명·`gh` 폴링만).
- **의존성**: 인증된 `gh` CLI. GitHub MCP 는 쓰지 않는다.
```

- [ ] **Step 2: `README.md` 18행의 `/pr-watch` 설명 구절을 갱신**

기존: `` `/pr-watch` (그 PR 을 머지 가능 상태까지 감시·알림) `` 을 아래로 바꾼다:

```
`/pr-watch` (그 PR 을 감시하며 리뷰를 자율 처리하고 머지 가능해지면 알림 — 자동 머지 X)
```

- [ ] **Step 3: `AGENTS.md` 갱신 — 9행 *Project in one line* 과 29행 Layout**

9행 *Project in one line* 안의 슬래시 커맨드 나열 구절에서 `/pr-watch` 설명을 현행화한다.
기존 구절: `` `/pr-watch` — `gh` CLI based `` (괄호 설명 없음) 은 그대로 두되, 그 문장이
"머지 가능 상태까지 감시·알림" 뉘앙스면 "리뷰를 자율 처리하고 머지 가능 시 알림(자동 머지 X)"
로 맞춘다. (해당 줄에 pr-watch 세부 설명이 없으면 건드리지 않아도 된다 — Layout 이 단일 소스.)

29행 Layout 의 pr-watch 줄을 아래로 교체:

```
│   ├── pr-watch.md                         `/pr-watch` — 열린 PR 감시: 리뷰 자율 처리(게이트 통과시 수정·푸시) + 애매 건 어드바이저/호출자 에스컬레이션, 머지 가능 알림 (자동 머지 X)
```

- [ ] **Step 4: 문서 간 상호 정합 재검토**

Run:
```bash
grep -n "pr-watch" FEATURES.md README.md AGENTS.md
```
Expected: 세 문서 모두 "자동 머지 X" 와 "감시/자율 처리" 뉘앙스로 일관. "머지 가능 상태까지
감시·알림"만 남고 자율 처리/게이트 언급이 빠진 곳이 없어야 한다.

- [ ] **Step 5: 게이트 무회귀 + 커밋**

```bash
bun run check
git add FEATURES.md README.md AGENTS.md
git commit -m "docs: /pr-watch 데몬 감시 진화 문서 동기화 (FEATURES/README/AGENTS)

Co-Authored-By: Claude <noreply@anthropic.com>"
```
Expected: `bun run check` PASS 후 커밋 생성.

---

## Task 3: 메모리 정책 정합 + 최종 검증

**Files:**
- Modify: `/Users/minjun/.claude/projects/-Users-minjun-dev-workspaces-agent-toolkit-rocky/memory/pr-watch-bot-review-policy.md`

**Interfaces:**
- Consumes: Task 1 의 새 커맨드 계약.
- Produces: 없음 (종단).

- [ ] **Step 1: 메모리 `pr-watch-bot-review-policy.md` 본문에 새 동작을 반영**

기존 5개 항목 정책(타당→수정, 애매→호출자 논의, 어드바이저 상의, 반박→답글+resolve, 전부
resolved 후 머지 판정)은 유지하되, **커맨드가 이제 이 정책을 데몬 사이클로 구현한다**는 점과
아래 3개를 명시적으로 덧붙인다 (기존 프론트매터 `name`/`description`/`metadata` 는 유지):

1. **게이트 통과시에만 푸시** — 타당한 지적을 고친 뒤 `bun run check`/`typecheck`/`test`
   모두 통과했을 때만 커밋·푸시. 실패하면 푸시 안 하고 호출자에게 알림, 스레드는 미해결 유지.
2. **에스컬레이션은 사이즈 기반 사다리** — 작고 명백하면 직접, 판단 필요하면 어드바이저,
   크거나 끝까지 갈리면 호출자. rung 은 감시자가 자율 선택.
3. **자기 페이싱 종료 조건** — `/loop` 로 구동 시 사람 리뷰 대기는 `ScheduleWakeup`
   20~30분 페이싱, 머지 가능/죽은 PR/보류는 루프 종료(재예약 X). 데몬 서버는 안 짓는다.

- [ ] **Step 2: 메모리 파일이 여전히 유효한 frontmatter 를 갖는지 확인**

Run:
```bash
head -8 "/Users/minjun/.claude/projects/-Users-minjun-dev-workspaces-agent-toolkit-rocky/memory/pr-watch-bot-review-policy.md"
```
Expected: `---` frontmatter 블록에 `name: pr-watch-bot-review-policy`, `description:`,
`metadata:` 가 그대로 존재.

- [ ] **Step 3: 전체 게이트 최종 실행 (무회귀 확인)**

Run:
```bash
bun run check && bun run typecheck && bun test
```
Expected: 3개 모두 PASS. (이 작업은 코드 변경이 없으므로 typecheck/test 는 이전과 동일하게
통과해야 한다. 실패하면 이 작업과 무관한 기존 문제.)

- [ ] **Step 4: 커맨드 프롬프트 최종 일관성 재검토 (수동)**

`commands/pr-watch.md` 를 처음부터 다시 읽고 아래를 확인한다:
- 핵심 원칙 6개 ↔ 절차 4·5단계의 동작이 서로 모순되지 않는가.
- "머지 금지"가 원칙·절차·6단계 알림에 일관되게 유지되는가 (`gh pr merge` 실행 지시가 없어야 함).
- allowed-tools 에 나열된 도구(`Agent`/`ScheduleWakeup`/`Edit`/`Write`/`Bash(bun:*)`)가 본문에서 실제로 쓰이는가, 반대로 본문이 요구하는 도구가 allowed-tools 에 다 있는가.
- 모순 발견 시 인라인 수정 후 이 Step 재확인.

- [ ] **Step 5: 메모리 커밋 안내 (메모리는 리포 밖이라 별도 관리)**

메모리 파일(`~/.claude/.../memory/pr-watch-bot-review-policy.md`)은 이 리포지토리 밖이므로
`git` 대상이 아니다. 수정 사실만 사용자에게 알린다. 리포 변경(Task 1·2)은 이미 각 태스크에서
커밋됨. 필요하면 `/finish` 로 브랜치·PR 마무리를 안내한다.

---

## Self-Review (작성자 체크)

**1. Spec coverage** — spec 각 결정 ↔ 태스크 매핑:
- 지속성(데몬 서버 X, 세션 수명, gh 폴링) → Task 1 원칙5·핵심원칙, Global Constraints. ✅
- 드라이버 재구성(메인+ScheduleWakeup) → Task 1 실행 모델·5단계 자기 페이싱. ✅
- 코드 수정 자율성(게이트 통과시 푸시) → Task 1 원칙2·4단계, Global Constraints. ✅
- 애매 처리 에스컬레이션 사다리 → Task 1 4단계·어드바이저. ✅
- 머지 안 함 → Task 1 원칙1·6단계, Global Constraints. ✅
- 기존 커맨드 진화(신설 X) → Task 1 (전면 개정), Task 2·3 문서 정합. ✅
- 문서 동기화(FEATURES/README/AGENTS/메모리) → Task 2·3. ✅

**2. Placeholder scan** — 전 태스크가 실제 최종 콘텐츠를 담음. "적절히 처리" 류 없음.
게이트 명령·gh/GraphQL 스니펫·커밋 메시지 모두 구체값. ✅

**3. Type consistency** — 커맨드 계약 용어가 태스크 간 일관:
`mergeStateStatus ∈ {CLEAN, HAS_HOOKS}` / `isResolved` / `threadId` / `in_reply_to` /
`ScheduleWakeup 1200~1800s` / 게이트 `bun run check`·`typecheck`·`test` 가 Task 1·2·3 에서
동일 표기. ✅
