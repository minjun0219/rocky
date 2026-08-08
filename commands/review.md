---
description: 완료를 선언하기 전에 신선한 컨텍스트의 서브에이전트로 지금 작업 diff 를 검토시킨다. 세션 히스토리 대신 diff + 요구사항만 넘겨 "내가 만든 걸 내가 검토하는" 편향을 피한다. PR 리뷰 대응은 /rocky:resolve-reviews.
argument-hint: "[비교 기준 (기본: origin/main 또는 HEAD~1)]"
allowed-tools: Task, Bash(git diff:*), Bash(git log:*), Bash(git rev-parse:*), Bash(git status:*), Bash(git merge-base:*), Read, Glob, Grep
---

# review — 완료 전 셀프 코드 리뷰

지금 브랜치의 작업을 **신선한 컨텍스트의 서브에이전트**에게 검토시킨다. `$ARGUMENTS` 는 비교
기준(생략 가능).

**왜 서브에이전트인가.** 내 세션에는 이 코드를 왜 이렇게 썼는지에 대한 정당화가 이미 쌓여 있어서,
같은 컨텍스트로는 내 판단을 다시 검증하기 어렵다. 리뷰어에게는 **결과물만** 넘긴다 — 사고 과정이
아니라. 덤으로 내 컨텍스트도 아낀다.

**언제 쓰나.** 기능 하나를 끝냈을 때, 머지/PR 직전, 복잡한 버그를 고친 뒤, 막혔을 때.
사소한 수정에는 쓰지 않는다.

**구분.** 이 커맨드는 아직 리뷰받지 않은 **내 작업 diff** 를 본다. 이미 열린 PR 의 리뷰 스레드를
처리하는 건 `/rocky:resolve-reviews` 다.

## 절차

### 1. 범위 확정

```bash
git status --short
git merge-base HEAD origin/main   # 기본 base; 실패하면 HEAD~1
git rev-parse HEAD
```

- `$ARGUMENTS` 가 있으면 그걸 base 로 쓴다.
- **커밋되지 않은 변경이 있으면 리뷰 범위에 포함한다** — base..worktree 가 실제 검토 대상이다.
  서브에이전트에게 `git diff <base>` (unstaged 포함) 를 보게 한다.
- diff 가 비어 있으면 "검토할 변경 없음" 후 종료.

### 2. 요구사항 확보

무엇을 만들려던 것인지 한 문단으로 정리한다. 출처는 순서대로: 사용자의 원 요청 → 관련 스펙/계획
문서(`docs/design/`) → 이슈/PR 본문 → 커밋 메시지. **요구사항 없이 리뷰시키지 않는다** — 그러면
리뷰어가 스타일 지적만 한다.

### 3. 서브에이전트 dispatch

`Task` 로 **`reviewer`** 서브에이전트(`agents/reviewer.md`)를 띄운다. 리뷰 규율(읽기 전용,
검증 후 단언, false pass 함정, 심각도·출력 형식)은 그 에이전트가 갖고 있으므로 **여기서
다시 지시하지 않는다.** 넘길 것은 이 작업에만 해당하는 정보뿐이다:

- **무엇을 만들었나** — 1~2 문단 요약
- **요구사항** — 2단계에서 정리한 것
- **범위** — base SHA, HEAD SHA, 그리고 uncommitted 포함 여부
- **레포 규칙 위치** — 심각도 기준이 따로 있으면(예: `AGENTS.md` 의 *Code review bar*)
  그 경로를 짚어준다

### 4. 피드백 처리

- Critical 은 즉시 고친다.
- Important 는 다음 작업으로 넘어가기 전에 고친다.
- Minor 는 기록만 하고 넘어가도 된다.
- **리뷰어가 틀렸으면 근거를 대고 반박한다.** 코드나 테스트로 반증할 수 있으면 그걸 보인다.
  리뷰어의 지적을 무조건 수용하지 않는다 — 신선한 컨텍스트는 맥락을 모르기도 한다.
- 고친 뒤 게이트를 다시 돌린다(레포의 check / typecheck / test).

### 5. 보고

무엇이 나왔고 무엇을 고쳤는지 한국어로 짧게. 반박한 항목이 있으면 이유와 함께 남긴다. 장문 금지.
