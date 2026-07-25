# `/rocky:review-pr` 커맨드 구현 계획

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** PR 에 붙은 리뷰(Copilot / Codex / 사람)를 미해결 0 까지 자동 처리하고, 반론은 모아 마지막에 상의하며, 머지 가능해지면 알림을 보내는 슬래시 커맨드를 rocky 에 추가한다.

**Architecture:** 순수 마크다운 프롬프트 커맨드 1개(`commands/review-pr.md`) 신설 + 기존 문서 5곳 동기화. `src/` 코드 변경 없음, MCP tool surface 무변경 — `src/index.test.ts` 의 도구 개수 가드에 영향 없다.

**Tech Stack:** Claude Code plugin 슬래시 커맨드 (frontmatter + 마크다운), `gh` CLI (GraphQL `reviewThreads` / `resolveReviewThread`), `Monitor` / `PushNotification` 도구.

**설계 문서:** `docs/superpowers/specs/2026-07-25-rocky-review-pr-command-design.md` (근거·실측·비목표는 전부 여기)

## Global Constraints

- 커맨드 파일 언어는 **한국어**. 코드 identifier / 경로 / 명령어 / API 필드명은 영어 원형.
- 커밋·PR 제목은 Conventional Commits (`type(scope): 한국어 요약`), 요약부 대략 50자 이내.
- 커밋 메시지 말미에 `Co-Authored-By: Claude <noreply@anthropic.com>` trailer.
- 게이트는 `bun run check` → `bun run typecheck` → `bun test` 순서, 전부 통과해야 커밋.
- 봇 계정 로그인명은 정확히 `copilot-pull-request-reviewer` / `chatgpt-codex-connector`.
- 커맨드는 **머지하지 않는다**. `gh pr merge` 를 어떤 경로로도 쓰지 않는다.
- `@codex review` 자동 트리거 금지 — Logan 의 명시 승인 후에만.
- 문서 단일 소스 규칙: 사람용 = `FEATURES.md` (한국어), 에이전트용 = `AGENTS.md` (영문). 새 사이드카 문서를 만들지 않는다.

## File Structure

| 파일 | 역할 | 작업 |
| --- | --- | --- |
| `commands/review-pr.md` | 커맨드 본체 — 리뷰 대응 루프 프롬프트 | **생성** |
| `commands/finish.md` | 7단계 후속 안내를 `/autofix-pr` → `/rocky:review-pr` 로 | 수정 (96행 부근) |
| `FEATURES.md` | 사람용 단일 소스 — 커맨드 개요 문단 + 상세 섹션 + 호스트 매트릭스 | 수정 (295 / 297 / 269 행 부근) |
| `AGENTS.md` | 에이전트용 단일 소스 — *Project in one line* + *Layout* 의 `commands/` 목록 | 수정 |
| `README.md` | 한 페이지 진입 — 슬래시 커맨드 목록 | 수정 (39행) |
| `.claude-plugin/plugin.json` | 플러그인 매니페스트 description 의 커맨드 목록 | 수정 |
| `.changeset/*.md` | 릴리스 의도 선언 (minor) | 생성 |

---

### Task 1: `commands/review-pr.md` 생성

**Files:**
- Create: `commands/review-pr.md`

**Interfaces:**
- Consumes: 없음 (독립 커맨드)
- Produces: 슬래시 커맨드 `/rocky:review-pr` — 이후 Task 2/3 의 문서가 이 이름을 참조한다. 커맨드 이름은 파일명 stem 에서 나온다 (`review-pr.md` → `/rocky:review-pr`).

- [ ] **Step 1: 커맨드 파일 작성**

아래 전문을 `commands/review-pr.md` 로 그대로 쓴다.

````markdown
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

```bash
gh api graphql -f query='
query($owner:String!, $repo:String!, $num:Int!) {
  repository(owner:$owner, name:$repo) { pullRequest(number:$num) {
    reviewThreads(first:100){nodes{
      id isResolved isOutdated path line
      comments(first:10){nodes{author{login} body diffHunk}}
    }}
  }}
}' -f owner=<owner> -f repo=<repo> -F num=<번호>
```

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
gh api graphql -f query='
mutation($id:ID!){ resolveReviewThread(input:{threadId:$id}){ thread{ isResolved } } }
' -f id=<threadId>
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

`Monitor` 로 60초 간격 폴링한다. 새 미해결 스레드가 생기면 이벤트로 잡는다.

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

  ```bash
  gh api graphql -f query='
  mutation($id:ID!, $body:String!){ addPullRequestReviewThreadReply(input:{pullRequestReviewThreadId:$id, body:$body}){ comment{ url } } }
  ' -f id=<threadId> -f body='<한국어 근거>'
  ```

- 반론 **반려** → 수정 대상으로 전환하고 3단계로 복귀한다 (라운드 카운트는 이어서 센다).
- 이 자리에서 **"Codex 재리뷰를 돌릴까?"** 도 함께 묻는다. 사용자가 승인할 때만 `@codex review`
  코멘트를 달고 7단계로 돌아간다. 승인이 없으면 달지 않는다.

  ```bash
  gh pr comment <번호> --body "@codex review"
  ```

### 10. 머지 가능 판정 + 알림

```bash
gh pr checks <번호>
gh pr view <번호> --json mergeable,mergeStateStatus,reviewDecision
```

- 조건: 미해결 스레드 0 · checks 전부 통과 · `mergeStateStatus` 가 `CLEAN` 또는 `UNSTABLE`.
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
````

- [ ] **Step 2: frontmatter 유효성 확인**

Run:
```bash
bun -e 'const s=await Bun.file("commands/review-pr.md").text(); const m=s.match(/^---\n([\s\S]*?)\n---\n/); if(!m) throw new Error("frontmatter 없음"); for (const k of ["description","argument-hint","allowed-tools"]) if(!m[1].includes(k+":")) throw new Error("키 누락: "+k); console.log("frontmatter OK")'
```
Expected: `frontmatter OK`

- [ ] **Step 3: 게이트 실행**

Run:
```bash
bun run check && bun run typecheck && bun test
```
Expected: 전부 통과. `src/` 를 안 건드렸으므로 테스트 결과는 변경 전과 동일해야 한다.

- [ ] **Step 4: 커밋**

```bash
git add commands/review-pr.md
git commit -m "$(cat <<'EOF'
feat(commands): /rocky:review-pr 추가

PR 리뷰(Copilot / Codex / 사람)를 미해결 0 까지 처리하는 루프 커맨드.
수집→분류→수정→게이트→푸시→resolve 를 반복하고, 반론은 보류 큐에 모아
수렴 후 사용자와 상의한 뒤에만 코멘트한다. 머지는 하지 않고 알림만 보낸다.

Co-Authored-By: Claude <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: `commands/finish.md` 후속 안내 교체

**Files:**
- Modify: `commands/finish.md:96`

**Interfaces:**
- Consumes: Task 1 이 만든 커맨드 이름 `/rocky:review-pr`
- Produces: 없음

- [ ] **Step 1: 7단계 안내 문장 교체**

`commands/finish.md` 의 아래 줄을

```markdown
- 이어서 CI 실패·리뷰 코멘트 자동 반영까지 맡기려면 Claude Code 빌트인 `/autofix-pr` 을 안내한다 (PR 브랜치를 체크아웃한 상태에서 실행해야 한다 — main 에서는 실행 거부됨).
```

아래로 바꾼다.

```markdown
- 이어서 리뷰 대응까지 맡기려면 `/rocky:review-pr` 을 안내한다 — PR 리뷰(Copilot / Codex / 사람)를 미해결 0 까지 처리하고 머지 가능해지면 알린다 (PR 브랜치를 체크아웃한 상태에서 실행). CI 실패 자동 수정만 원하면 Claude Code 빌트인 `/autofix-pr` 이 별도 선택지다.
```

- [ ] **Step 2: 교체 확인**

Run:
```bash
grep -n "review-pr\|autofix-pr" commands/finish.md
```
Expected: 96행 부근 한 줄에 두 이름이 함께 나오고, `/rocky:review-pr` 이 먼저 온다.

- [ ] **Step 3: 커밋**

```bash
git add commands/finish.md
git commit -m "$(cat <<'EOF'
docs(commands): finish 후속 안내를 review-pr 로 교체

리뷰 대응은 /rocky:review-pr 이 담당하고, /autofix-pr 은 CI 실패 자동
수정용 별도 선택지로 남긴다.

Co-Authored-By: Claude <noreply@anthropic.com>
EOF
)"
```

---

### Task 3: 문서 4곳 동기화

**Files:**
- Modify: `FEATURES.md` (커맨드 개요 문단 295행 부근 / 상세 섹션 297행 부근 / 호스트 매트릭스 269행 부근)
- Modify: `AGENTS.md` (*Project in one line* 의 커맨드 나열 / *Layout* 의 `commands/` 트리)
- Modify: `README.md:39`
- Modify: `.claude-plugin/plugin.json` (description)

**Interfaces:**
- Consumes: Task 1 의 커맨드 이름과 동작 요약
- Produces: 없음

- [ ] **Step 1: `FEATURES.md` 커맨드 개요 문단 갱신**

295행 부근 문단에서 `/autofix-pr` 위임을 설명하는 문장

```
생성된 PR 의 감시·리뷰 반영은 Claude Code **빌트인 `/autofix-pr`** 에 위임한다 (클라우드 세션 + GitHub App webhook 기반 — rocky 커맨드가 아니며, 구 `/pr-watch` 는 v0.8 에서 제거됨).
```

을 아래로 바꾼다.

```
생성된 PR 의 리뷰 대응은 `/rocky:review-pr` 이 맡는다 — Copilot / Codex / 사람 리뷰를 미해결 0 까지 처리하고 머지 가능해지면 알린다(머지는 하지 않음). CI 실패 자동 수정만 필요하면 Claude Code **빌트인 `/autofix-pr`** 이 별도 선택지다 (클라우드 세션 + GitHub App webhook 기반 — rocky 커맨드가 아니며, 구 `/pr-watch` 는 v0.8 에서 제거됨).
```

- [ ] **Step 2: `FEATURES.md` 에 상세 섹션 추가**

`### /rocky:finish [힌트]` 블록 **바로 뒤**, `### /rocky:recall [주제 힌트]` 앞에 아래를 삽입한다.

```markdown
### `/rocky:review-pr [PR 번호]`

- **What**: PR 에 붙은 리뷰(Copilot `copilot-pull-request-reviewer` / Codex `chatgpt-codex-connector` / 사람)를 **미해결 0 까지** 처리한다 — GraphQL `reviewThreads` 수집 → 수정/반론/무효 분류 → 수정 + 게이트 → 라운드당 커밋 1개 푸시 → 수정·무효 스레드 `resolveReviewThread` → 재리뷰 대기(60초 폴링, 8분 무반응이면 수렴) 반복.
- **Input**: (옵션) PR 번호. 생략 시 현재 브랜치의 PR.
- **반론**: 판단이 갈리는 지적은 **보류 큐**에 모아두고 루프의 미해결 판정에서 제외한다. 수렴 후 사용자와 하나씩 상의해 **승인된 반론만** 근거 코멘트 1개를 남기고 resolve 한다 (반려하면 수정으로 전환).
- **알림**: 미해결 0 + checks 통과 + `mergeStateStatus` 가 `CLEAN`/`UNSTABLE` 이면 `PushNotification` 으로 "머지 가능" 을 알린다.
- **하지 않는 것**: 자동 머지 없음(알림까지가 끝), 게이트 실패 시 푸시 금지, force push 금지, 수정 반영 건에 코멘트 금지(resolve 만), `@codex review` 자동 트리거 금지(사용자 승인 후에만 — Codex 의 수동 게이트는 의도된 설정이다). Copilot 은 푸시마다 자동 재리뷰하므로 트리거가 불필요하다.
- **가드**: 라운드 상한 5, 같은 스레드 2라운드 연속 미해결 시 중단, PR head 브랜치가 아니면 실행 거부.
- **의존성**: 인증된 `gh` CLI.
```

- [ ] **Step 3: `FEATURES.md` 호스트 매트릭스에 행 추가**

269행 부근 표의 `| /rocky:finish, /rocky:issue |` 행 **바로 아래**에 삽입한다.

```markdown
| `/rocky:review-pr` | ✅ | ◐ 커버 가능 (skill) | ◐ 커버 가능 (command) | `gh` CLI + 폴링 의존, 로직은 호스트 중립 |
```

- [ ] **Step 4: `README.md:39` 갱신**

슬래시 커맨드 나열에서 `/rocky:finish (…)` 바로 뒤에 `/rocky:review-pr` 을 끼우고, 문단 끝의

```
PR 감시·리뷰 반영은 Claude Code 빌트인 `/autofix-pr` 에 위임.
```

을 아래로 바꾼다.

```
CI 실패 자동 수정은 Claude Code 빌트인 `/autofix-pr` 이 별도 선택지.
```

삽입할 항목:

```
`/rocky:review-pr` (PR 리뷰를 미해결 0 까지 처리 — 수정·게이트·푸시·resolve 반복, 반론은 모아 상의, 머지 가능 시 알림),
```

- [ ] **Step 5: `AGENTS.md` 갱신 2곳**

(a) *Layout* 의 `commands/` 트리에서 `finish.md` 줄 바로 아래에 삽입:

```
│   ├── review-pr.md                        `/rocky:review-pr` — PR 리뷰(Copilot/Codex/사람)를 미해결 0 까지 처리 (수집→분류→수정→게이트→푸시→resolve 루프), 반론은 보류 후 상의, 머지 가능 시 알림 (gh CLI 기반, 자동 머지 X)
```

(b) *Project in one line* 의 슬래시 커맨드 나열에서 `/rocky:finish` 설명 뒤에 삽입하고, 같은 문단의 "PR watching / review handling is delegated to Claude Code's built-in `/autofix-pr`" 문장을 아래로 교체:

```
`/rocky:review-pr` — `gh` CLI based, drives the PR review loop (collect `reviewThreads` → classify fix/rebut/moot → fix + gates → one commit per round → `resolveReviewThread` → poll for re-review) until zero unresolved threads, holding rebuttals in a queue for a final confirm-then-comment pass and pushing a "mergeable" notification at the end (never merges);
```

```
PR review handling is `/rocky:review-pr`; Claude Code's built-in `/autofix-pr` remains a separate option for CI-failure autofixes (the former `/pr-watch` command was removed in v0.8).
```

- [ ] **Step 6: `.claude-plugin/plugin.json` description 갱신**

description 문자열 안에서

```
/rocky:finish (gates → commit → push → PR, gh CLI),
```

뒤에 아래를 잇고,

```
/rocky:review-pr (drive the PR review loop until zero unresolved threads, rebuttals confirmed at the end, notify when mergeable, never merges),
```

같은 description 안의

```
PR watching is delegated to Claude Code's built-in /autofix-pr.
```

를 아래로 바꾼다.

```
PR review handling is /rocky:review-pr; the built-in /autofix-pr remains a separate option for CI-failure autofixes.
```

- [ ] **Step 7: JSON 유효성 + 게이트**

Run:
```bash
bun -e 'const j=await Bun.file(".claude-plugin/plugin.json").json(); if(!j.description.includes("/rocky:review-pr")) throw new Error("description 미갱신"); console.log("plugin.json OK", j.version)'
bun run check && bun run typecheck && bun test
```
Expected: `plugin.json OK 0.16.0` 후 게이트 전부 통과.

- [ ] **Step 8: 커밋**

```bash
git add FEATURES.md AGENTS.md README.md .claude-plugin/plugin.json
git commit -m "$(cat <<'EOF'
docs: review-pr 커맨드를 단일 소스 문서에 반영

FEATURES.md(개요·상세·호스트 매트릭스) / AGENTS.md(Layout·요약) /
README.md / plugin.json description 을 lockstep 으로 갱신.

Co-Authored-By: Claude <noreply@anthropic.com>
EOF
)"
```

---

### Task 4: changeset + 최종 검증

**Files:**
- Create: `.changeset/<자동 생성 이름>.md`

**Interfaces:**
- Consumes: Task 1~3 의 변경 전체
- Produces: 릴리스 의도 (minor)

- [ ] **Step 1: changeset 파일 작성**

`bunx changeset` 은 대화형이라 파일을 직접 만든다. `.changeset/review-pr-command.md`:

```markdown
---
"@minjun0219/rocky": minor
---

`/rocky:review-pr` 슬래시 커맨드 추가 — PR 에 붙은 리뷰(Copilot / Codex / 사람)를 미해결 0 까지 처리한다. 수집 → 분류 → 수정 + 게이트 → 라운드당 커밋 1개 푸시 → resolve → 재리뷰 대기를 반복하고, 판단이 갈리는 지적은 보류 큐에 모아 수렴 후 사용자와 상의해 승인된 반론만 코멘트 + resolve 한다. 미해결 0 + checks 통과 시 머지 가능 알림을 보내며, 머지 자체는 하지 않는다. `/rocky:finish` 의 후속 안내도 이 커맨드로 교체.
```

- [ ] **Step 2: 최종 게이트 + 표면 무변경 확인**

Run:
```bash
bun run check && bun run typecheck && bun test
git diff main --stat
```
Expected: 게이트 전부 통과. diff 에 `src/` 파일이 **하나도 없어야 한다** (MCP tool surface 무변경).

- [ ] **Step 3: 커밋**

```bash
git add .changeset/review-pr-command.md
git commit -m "$(cat <<'EOF'
chore(changeset): review-pr 커맨드 추가 (minor)

Co-Authored-By: Claude <noreply@anthropic.com>
EOF
)"
```

- [ ] **Step 4: 로컬 로드 확인 안내**

커맨드는 마크다운이라 단위 테스트로 검증할 수 없다. 실제 로드는 새 세션에서 확인한다.

```bash
claude --plugin-dir /Users/minjun/dev/workspaces/agent-toolkit-rocky
```

새 세션에서 `/rocky:review-pr` 이 목록에 뜨고 `argument-hint` 가 보이면 성공. (마켓플레이스
설치본은 GitHub `main` clone 이라 push 전에는 반영되지 않는다.)

이 확인은 사용자가 직접 해야 하므로, 여기까지 끝나면 사용자에게 알리고 멈춘다. push / PR 생성은
`/rocky:finish` 로 별도 진행한다.

## Self-Review 결과

- **Spec 커버리지**: spec 의 절차 0~10 전부 Task 1 의 커맨드 본문에 있음. 연계 변경 5곳(finish /
  FEATURES / AGENTS / README / plugin.json) = Task 2~3. changeset = Task 4. 비목표 5개는 커맨드
  본문의 원칙·예외 표로 강제됨.
- **플레이스홀더**: `<owner>` / `<repo>` / `<번호>` / `<threadId>` 는 런타임 치환 자리로 의도된
  것. TBD/TODO 없음.
- **이름 일관성**: 파일명 `review-pr.md` ↔ 커맨드 `/rocky:review-pr` ↔ changeset 설명 ↔ 문서
  4곳 표기 전부 일치. 봇 로그인명은 spec 실측값과 동일.
