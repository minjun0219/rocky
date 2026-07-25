# `/rocky:review-pr` — PR 리뷰 대응 루프 커맨드 설계

- 날짜: 2026-07-25
- 대상: rocky 레포에 슬래시 커맨드 `commands/review-pr.md` 신설 + `commands/finish.md` 연계 수정
- 상태: 설계 협의 완료 (Logan 승인 방향), 계획 대기
- 코드 변경: 없음 (커맨드는 마크다운 프롬프트, MCP tool surface 무변경)

## 배경 / 동기

`/rocky:finish` 는 게이트 → 커밋 → 푸시 → PR 생성까지 하고 멈춘다. 그 뒤 **PR 에 붙는 봇 리뷰를
받아 처리하는 구간**은 지금 매번 수작업이다 — Logan 이 세션마다 같은 지시를 붙여넣고 있었다:

> 리뷰 확인 시 채팅 창에 작업 내용을 요약하고, 코멘트는 필요한 사항만 달고 수정 및 반론을
> 채팅창에 공유해. 코파일럿 리뷰와 코덱스 리뷰를 포함해 리뷰가 0이 될 때까지 반복하고,
> 수정되었거나 반론한 이슈는 리졸브 처리해. 최종 머지가 가능한 상황일 때 알림 보내.

이 반복 지시를 커맨드로 굳힌다.

### 빌트인 `/autofix-pr` 로 안 되는 이유

`finish.md` 7단계는 현재 빌트인 `/autofix-pr` 을 안내한다. 겹치는 건 "리뷰 코멘트 반영" 하나뿐이고
아래 네 가지가 없다.

1. **반론** — 리뷰를 거절하고 근거를 남기는 경로
2. **resolve 처리** — GraphQL `resolveReviewThread`
3. **미해결 0 까지의 루프** — 재리뷰 대기 + 재수렴
4. **머지 가능 시점 알림**

## 확정 사실 (실측)

rocky 레포 PR #102 / #104 / #105 를 `gh api graphql` 로 조회해 확인했다.

- **봇 계정 로그인명**
  - Copilot = `copilot-pull-request-reviewer`
  - Codex = `chatgpt-codex-connector`
- **둘 다 `reviewThreads` 로 잡히고 `isResolved` 필드를 가진다.** 스레드 resolve 는 REST 로 불가,
  GraphQL `resolveReviewThread` mutation 이 유일한 경로다.
- **재리뷰 트리거 (PR #105 / #102 시각 대조)**

  | 봇 | 푸시 후 자동 재리뷰 | 지연 |
  |---|---|---|
  | `copilot-pull-request-reviewer` | O — 커밋마다 매번 | 2~5분 (실측 3회: 4분49초 / 2분33초 / 2분18초) |
  | `chatgpt-codex-connector` | X — PR 오픈 시 1회만 | — |

  이 차이는 사고가 아니라 **Logan 의 의도적 설정**이다. Copilot 은 매 푸시 재리뷰하도록 켜뒀고,
  Codex 는 "어느 정도 끊는 지점"이 필요해서 일부러 명시 트리거만 받게 해뒀다. → **커맨드는
  `@codex review` 를 자동으로 달지 않는다.** 수렴 시점(=그 끊는 지점)에 Logan 에게 제안만 한다.

## 형태 결정

**슬래시 커맨드** (`commands/review-pr.md`), 스킬 아님.

- 스킬은 모델이 상황을 보고 자동 발동하는 형태(`todoist` 처럼)다. 이 워크플로우는 Logan 이
  "지금 리뷰 대응 시작" 하고 명시적으로 호출하는 절차형 — `/rocky:finish`·`/rocky:codex` 와 같은 결.
- **루프는 메인 세션이 직접 돈다** (서브에이전트·백그라운드 에이전트 아님). 요구의 무게중심이
  "채팅창에 수정·반론 실시간 공유"인데, 서브에이전트의 최종 리포트는 사용자에게 직접 보이지
  않고 라운드별 공유가 불가능하다. 또 무승인 커밋·푸시를 감시하는 주체가 없어진다.
- 컨텍스트 소모가 이 선택의 유일한 비용이다. 실제로 터지면 그때 "수정 구현만 서브에이전트 위임"
  최적화를 붙인다 — 지금 넣으면 YAGNI.

## 커맨드 사양

### frontmatter

```yaml
---
description: PR 에 붙은 리뷰(Copilot / Codex / 사람)를 미해결 0 까지 처리한다 — 수정·게이트·푸시·resolve 를 반복하고, 반론은 모아 마지막에 상의한 뒤 코멘트한다. 머지 가능해지면 알림.
argument-hint: "[PR 번호] (생략 시 현재 브랜치의 PR)"
allowed-tools: Bash(gh:*), Bash(git:*), Bash(bun:*), Read, Edit, Write, Grep, Glob, Monitor, PushNotification
---
```

### 원칙

1. **커맨드 호출 자체가 수정·커밋·푸시·resolve 승인이다.** 라운드마다 채팅에 사후 요약만 올린다.
   (`finish.md` 원칙 3 과 동일한 자율성 계약.)
2. **머지는 절대 하지 않는다.** 머지 가능 판정 + 알림까지가 끝.
3. **게이트 실패 = 푸시 없음.** `--no-verify` 우회 금지, force push 금지.
4. **코멘트는 필요한 사항만.** 수정 반영 건은 코멘트 없이 resolve 만. 커맨드가 코멘트를 남기는
   경우는 둘뿐이다 — (a) Logan 이 승인한 반론의 근거, (b) Logan 이 승인한 `@codex review` 트리거.
   둘 다 9단계에서 명시 승인을 받은 뒤에만 단다.
5. **반론은 혼자 결정하지 않는다.** 판단이 갈리는 리뷰는 보류 큐에 쌓고, 루프가 수렴한 뒤
   Logan 과 상의해 결정한다.

### 절차

#### 0. 준비

- PR 식별: 인자가 있으면 그 번호, 없으면 `gh pr view --json number,url,headRefName`.
- 현재 브랜치가 PR 의 head 브랜치가 아니면 **중단**하고 체크아웃을 안내한다. `main` 에서는 실행하지
  않는다.
- `gh auth status` 실패 시 중단.

#### 1. 수집

```bash
gh api graphql -f query='
{ repository(owner:"<owner>", name:"<repo>") { pullRequest(number: <N>) {
  reviewThreads(first:100){nodes{
    id isResolved isOutdated path line
    comments(first:10){nodes{author{login} body diffHunk}}
  }}
}}}'
```

- `isResolved: false` 인 스레드만 대상. `isOutdated` 는 참고 정보로만 쓴다(자동 제외하지 않음 —
  코드가 옮겨졌을 뿐 지적이 유효할 수 있다).
- 봇 스레드와 사람 스레드를 모두 수집한다. **사람 리뷰어의 지적이 봇보다 우선순위가 높다.**
- 이전 라운드에서 이미 보류 큐에 넣은 스레드는 재분류하지 않는다.

#### 2. 분류

각 스레드를 셋 중 하나로 판정하고, 채팅에 표 한 줄씩 요약한다.

| 판정 | 처리 |
|---|---|
| **수정** | 이번 라운드에서 고친다 |
| **반론** | 보류 큐에 넣는다 — 이 라운드에서 코멘트도 resolve 도 하지 않는다 |
| **무효** | 이미 해결됐거나 대상 코드가 사라짐 → 코멘트 없이 resolve |

#### 3. 수정 + 게이트

- [수정] 건을 구현한다.
- `bun run check` → `bun run typecheck` → `bun test` 순서로 전부 실행.
- 하나라도 실패하면 실패 로그를 인용하고 **푸시 없이 멈춘다**.

#### 4. 커밋 · 푸시

- 라운드당 커밋 1개. Conventional Commits 한국어 제목 (`fix(review): …`), 본문에 처리한 스레드 요약.
- `Co-Authored-By: Claude <noreply@anthropic.com>` trailer 필수.
- `git push`.

#### 5. resolve

- [수정] 스레드: **코멘트 없이** `resolveReviewThread`.
- [무효] 스레드: 동일.
- [반론] 스레드: 건드리지 않는다.

```bash
gh api graphql -f query='mutation($id:ID!){ resolveReviewThread(input:{threadId:$id}){ thread{ isResolved } } }' -f id='<threadId>'
```

#### 6. 라운드 보고 (채팅)

```
라운드 N — 처리 3 / 반론 보류 1 / 무효 1
  ✅ src/core/worklog.ts:42  …지적 요약… → …어떻게 고쳤는지…
  ⏸  AGENTS.md:12           …지적 요약… → 보류 (근거: …)
  ⊘  docs/codex.md:8        outdated, 코드 이동됨 → resolve
  게이트 통과 · 커밋 abc1234 푸시
```

#### 7. 재리뷰 대기

- `Monitor` 로 60초 간격 폴링 — 새 미해결 스레드가 생기면 이벤트.
- **8분 무반응이면 리뷰 종료로 판정** (Copilot 실측 최대 5분 + 여유).
- 트리거 코멘트는 달지 않는다 (원칙 4 + Codex 는 의도적 수동 게이트).

#### 8. 루프

- 새 리뷰가 있으면 1로 복귀. 미해결(보류 큐 제외) 0 이 되고 8분간 새 리뷰가 없으면 수렴.
- **라운드 상한 5.** 초과 시 멈추고 현재 상태를 보고한다.
- **진전 없음 감지**: 같은 스레드를 두 라운드 연속 못 고치면 중단하고 보고한다.

#### 9. 반론 상의 (수렴 후)

보류 큐를 채팅에서 하나씩 Logan 과 정리한다.

- 반론 **승인** → 한국어 근거 코멘트 1개를 스레드에 남기고 resolve.
- 반론 **반려** → 수정 대상으로 전환하고 3으로 복귀 (라운드 카운트 계속).
- 이 자리에서 **"Codex 재리뷰 돌릴까?"** 를 함께 묻는다. Logan 이 승인할 때만 `@codex review`
  코멘트를 달고 7로 돌아간다. 승인 없으면 달지 않는다.

#### 10. 머지 가능 판정 + 알림

```bash
gh pr checks <N>
gh pr view <N> --json mergeable,mergeStateStatus,reviewDecision
```

- 조건: 미해결 스레드 0 · checks 전부 통과 · `mergeStateStatus` 가 `CLEAN`/`UNSTABLE`.
- 충족 → `PushNotification("PR #N 머지 가능 — 리뷰 X건 처리 / 반론 Y건")` + 채팅 최종 요약.
  (`PushNotification` 은 터미널 데스크톱 알림과 폰 푸시를 동시에 처리한다 — 별도 osascript 배선
  불필요. Logan 이 터미널 앞에 있으면 중복이라 스킵되고 그 사실을 알려준다.)
- 미충족 → 무엇이 막고 있는지(실패한 check, `BLOCKED` 사유) 채팅에 보고. 알림은 보내지 않는다.
- **머지는 하지 않는다.**

## 실패 / 예외 처리

| 상황 | 처리 |
|---|---|
| `main` 브랜치 / PR head 불일치 | 즉시 중단, 체크아웃 안내 |
| `gh` 미인증 | 즉시 중단 |
| 게이트 실패 | 푸시 없이 중단, 실패 로그 인용 |
| push 거부 (원격이 앞서 있음) | 에러 그대로 인용하고 중단. force push 금지 |
| 라운드 상한 초과 | 중단 + 상태 보고 |
| 같은 스레드 2라운드 연속 미해결 | 중단 + 보고 |
| 폴링 8분 무반응 | 정상 수렴으로 간주하고 9단계로 |

## 연계 변경

- `commands/finish.md` 7단계: `/autofix-pr` 안내를 `/rocky:review-pr` 로 교체.
- `FEATURES.md` (한국어, 사람용) 슬래시 커맨드 표에 `/rocky:review-pr` 추가.
- `AGENTS.md` *Project in one line* + *Layout* 의 `commands/` 목록에 추가.
- `README.md` 커맨드 목록/카운트 갱신.
- `.claude-plugin/plugin.json` 의 description 커맨드 목록 갱신.
- changeset: minor (user-facing 커맨드 추가).

## 명시적 비목표

- **자동 머지** — 알림까지가 끝.
- **MCP 도구화** — `review_*` 같은 tool 을 만들지 않는다. 슬래시 커맨드 하나로 끝난다.
  (Codex / opencode 에는 노출되지 않는다 — 다른 Claude Code 전용 커맨드와 동일.)
- **CI 실패 자동 수정** — 리뷰 스레드 처리가 대상이다. CI 는 머지 가능 판정의 입력으로만 읽는다.
- **`@copilot review` 재트리거** — 이미 푸시마다 자동으로 돈다. 불필요.
- **서브에이전트 위임** — 초판은 메인 세션 단독. 컨텍스트가 실제로 문제가 될 때 재검토.
