---
description: 현재 변경을 마무리한다 — 게이트(check/typecheck/test) 통과 확인 → 변경 요약 → 브랜치 → 커밋 → 푸시 → PR 생성까지. 게이트가 실패하면 커밋하지 않고 멈춘다.
argument-hint: "[PR/커밋 요약 힌트] (생략 가능)"
allowed-tools: Bash(bun:*), Bash(git:*), Bash(gh:*), Read, Grep, Glob
---

# finish — 변경 마무리 (게이트 → 커밋 → PR)

지금까지의 작업을 저장소 규칙에 맞게 마무리한다. `$ARGUMENTS` 는 커밋/PR 요약에 참고할
힌트(있으면). 출력·커밋·PR 은 **한국어** (코드 identifier / 경로 / 명령어는 영어 그대로).

## 원칙

1. **게이트 먼저.** 하나라도 실패하면 커밋/푸시/PR 로 넘어가지 않고, 실패 내용을 그대로
   보여주고 멈춘다. 실패를 감추거나 `--no-verify` 로 우회하지 않는다.
2. **`main` 에 직접 커밋하지 않는다.** 현재 브랜치가 기본 브랜치면 먼저 새 브랜치를 판다.
3. **사용자가 명시적으로 이 커맨드를 호출한 것 = 커밋·푸시·PR 승인.** 다만 커밋 메시지와
   PR 초안은 만들기 전에 한 번 보여준다.
4. Conventional Commits 스타일 제목 (`type(scope): 한국어 요약` 또는 `type: 한국어 요약`).
   **제목을 장황하게 만들지 않는다** — 핵심 변경 하나를 넘는 나열·부연·괄호 덧붙임을 제목에
   넣지 말고(요약부 대략 50자 초과 금지), 밀려난 세부는 본문으로. 제목을 줄인다고 본문까지
   깎지 않는다.

## 절차

### 1. 현재 상태 파악

```bash
git status
git branch --show-current
git diff --stat HEAD          # 스테이지+워킹 변경 규모
git log --oneline -5
```

- 변경이 전혀 없으면(워킹 트리 clean & main 대비 커밋 없음) 그 사실만 알리고 멈춘다.

### 2. 게이트 실행

이 저장소의 change checklist 순서대로:

```bash
bun run check       # Biome verify
bun run typecheck   # tsc --noEmit
bun test            # 단위 + smoke
```

- 하나라도 실패 → 실패 로그를 인용하고, 무엇을 고쳐야 하는지 한 줄 진단 후 **멈춘다.**
  (직접 코드를 고칠지 여부는 사용자에게 확인.)
- 이번 변경이 사용자 표면(tool / env var / 커맨드 / handle)을 건드렸다면, 두 단일 문서
  (`README.md` 한국어 · `AGENTS.md` 영문) 가 갱신됐는지
  `git diff --stat` 로 점검하고, 빠졌으면 한 줄로 지적한다.

### 3. 브랜치 확인

- 현재 브랜치가 `main`(기본 브랜치)이면: 변경 내용에 맞는 이름으로 새 브랜치를 만든다
  (`git switch -c <type>/<짧은-요약>`). 이미 feature 브랜치면 그대로 사용.

### 4. 커밋 초안 → 커밋

- `git diff` 를 읽고 변경의 핵심을 한국어로 요약해 **커밋 제목 + 본문 초안**을 만들어 보여준다.
  커밋 제목에도 원칙 4 를 적용한다 — 나열·부연을 제목에 넣지 않는다 (세부는 본문으로).
- 승인 흐름상 그대로 커밋한다. 커밋 메시지 말미에 아래 trailer 를 반드시 붙인다:

  ```
  Co-Authored-By: Claude <noreply@anthropic.com>
  ```

- 관련 없는 파일까지 싸잡아 `git add -A` 하지 말고, 이번 작업에 해당하는 변경만 스테이지한다.

### 5. 푸시

```bash
git push -u origin <현재 브랜치>
```

### 6. PR 생성

**PR 을 만든 뒤에 본문을 완성한다.** Files changed 링크에는 PR 번호가 필요해서, 생성 시점에는
아직 링크를 만들 수 없다. 그래서 두 번에 나눠 돈다.

```bash
# 1) 링크 없는 본문으로 생성 → 번호 확보
NUM=$(gh pr create --base main --head <브랜치> \
  --title "<Conventional 한국어 제목>" --body "<본문 초안>" | grep -o '[0-9]*$')

# 2) 그 번호로 링크를 만들어 본문을 갱신
bun "${CLAUDE_PLUGIN_ROOT:-.}/scripts/permalink.ts" --pr "$NUM" <포인터> ...
gh pr edit "$NUM" --body "<링크까지 채운 본문>"
```

- 제목: Conventional Commits 스타일 한국어. 나열·부연으로 늘리지 않는다 — 세부는 본문으로
  (원칙 4).
- **PR 템플릿이 있으면 템플릿이 우선이다.** 본문을 쓰기 전에 먼저 찾는다.

  ```bash
  ls .github/pull_request_template.md .github/PULL_REQUEST_TEMPLATE.md \
     pull_request_template.md docs/pull_request_template.md 2>/dev/null
  ls .github/PULL_REQUEST_TEMPLATE/ 2>/dev/null   # 여러 개짜리 (디렉토리형)
  ```

  - 있으면 **그 절 제목과 순서를 그대로 두고 내용만 채운다.** 아래 세 부분 형식을 덮어씌우지
    않고, 템플릿에 없는 절을 억지로 끼워 넣지도 않는다. 레포가 정해 둔 형식이 먼저다.
  - 짧게 쓰는 원칙은 템플릿 **안에서도** 지킨다 — 파일별 나열 금지, 읽어야 할 곳은 1~3곳,
    링크는 `[경로:줄](Files changed 위치)`.
  - **체크리스트는 실제로 한 것만 체크한다.** 안 한 항목은 빈 칸으로 두고, 왜 못 했는지
    한 줄 적는다. 통과 못 한 게이트를 체크해 두면 본문 전체가 못 믿을 글이 된다.
  - `.github/PULL_REQUEST_TEMPLATE/` 처럼 여러 개면 어느 것을 쓸지 사용자에게 묻는다.
  - `gh pr create --body` 는 템플릿을 자동으로 채워 주지 않는다 (`--body` 를 주는 순간
    템플릿은 무시된다). 파일을 직접 읽어 본문을 만들어야 한다.
- 본문(한국어): **템플릿이 없을 때의 기본 형식.** 전체 열 줄 안팎, 아래 세 부분만.

  1. **무엇을·왜** — 1~2줄.
  2. **변경 사항** — **파일별 나열이 아니다.** 리뷰어가 실제로 읽어야 할 1~3곳만 고른다:
     위험한 변경 · 판단이 갈린 곳 · 확신 없는 곳. 항목마다 **`###` 제목 → 설명 →
     (코드 스니펫) → 링크** 순으로 쓰고, 스니펫은 선택이다.

     ````markdown
     ## 변경 사항

     ### 스레드를 닫는 주체를 사용자로 되돌림
     gh 가 오너 토큰을 쓰므로 에이전트가 닫든 오너가 닫든 밖에서는 구분되지 않는다.
     나누는 방법이 "안 하기로 정해 두는 것"뿐이라 resolve 를 사용자 행위로 돌렸다.
     [commands/resolve-reviews.md:25-32](https://github.com/<owner>/<repo>/blob/<sha>/commands/resolve-reviews.md#L25-L32)

     ### 리액션 대상이 스레드가 아니라 첫 코멘트
     여기가 틀리면 👀 가 통째로 실패해서, 1단계 GraphQL 에 comments.id 를 추가했다.

     ```graphql
     addReaction(input:{subjectId:$commentId, content:EYES}){ reaction{ content } }
     ```

     [commands/resolve-reviews.md:146-160](https://github.com/<owner>/<repo>/blob/<sha>/commands/resolve-reviews.md#L146-L160)
     ````

     - **제목**: 그 자리에서 무엇이 바뀌었는지 한 줄. 목록만 훑어도 변경의 지형이 잡혀야 한다.
     - **설명**: 무엇이 걸리는지(위험한 이유 · 갈린 판단 · 확신 없는 부분) 한두 줄.
     - **코드 스니펫**(선택): 설명과 링크 사이에 짧은 코드 블록을 넣는다. **항상은 아니다** —
       링크만으로 안 되는 때에만 쓴다: 떨어져 있는 줄을 나란히 보여야 할 때, before / after 를
       대비시킬 때, 본문에서 대안 코드를 제안할 때. 10줄 안쪽으로 자르고, 그냥 읽으라고 붙이는
       덤프는 만들지 않는다.
     - **링크**: `[경로:줄](URL)` 형태로, 보이는 것은 경로와 줄뿐이고 URL 은 뒤에 숨긴다.
       URL 은 **그 PR 의 Files changed 위치**(`…/pull/<번호>/files#diff-<해시>R<줄>`)로 건다 —
       리뷰어가 누르면 어차피 보던 리뷰 화면에서 그 줄로 간다. blob permalink 로 걸면 PR 밖
       파일 뷰로 튕겨 나가 다시 돌아와야 한다. 이 파일이 diff 에 없으면(이 PR 이 건드리지 않은
       코드를 가리킬 때) 그때만 blob permalink 로 건다.

     **포인터를 앞세우지 않는다.** 경로가 먼저 오면 읽는 사람 눈에는 경로부터 들어와 정작
     무엇이 바뀌었는지가 묻힌다. 제목이 먼저고 링크는 맨 아래다.

     링크는 손으로 조립하지 말고 스크립트에 포인터를 넘긴다 — 인자는 `경로:심볼`,
     `경로:42`, `경로:42-58` 셋 다 받고, 포인터마다 `[경로:줄](URL)` 를 한 줄씩 출력한다
     (`--url` 이면 날 URL).

     ```bash
     # PR 번호를 주면 Files changed 위치로, 안 주면 blob permalink 로 건다
     bun "${CLAUDE_PLUGIN_ROOT:-.}/scripts/permalink.ts" --pr 127 \
       src/core/handlers.ts:handleOpenapiSearch commands/finish.md:12-18
     ```

     현재 `HEAD` 의 SHA 로 고정해 준다 (브랜치명으로 걸면 머지 후 브랜치가 지워질 때 깨지고,
     후속 커밋이 붙으면 가리키는 줄이 밀린다). 심볼 후보가 여럿이면 링크를 임의로 고르지
     않고 후보 목록과 함께 실패하므로, 그때는 줄 번호를 직접 넘긴다. **본문을 쓰기 직전,
     푸시한 커밋에서 실행한다** — 아직 원격에 없는 커밋이면 경고가 뜬다.

     **인라인 리뷰 코멘트로 대신하지 않는다.** 그건 리뷰 스레드를 만들고, 열린 스레드는
     `required_review_thread_resolution` 이 걸린 레포에서 머지를 막는다 — 읽으라고 찍은 것이
     사용자의 resolve 클릭 작업으로 돌아온다.

     짚을 곳이 없으면 `없음` 이라고 쓴다 — 억지로 만들지 않는다.
  3. **검증** — 안 돌린 것 · 실패한 것 · 수동 확인이 필요한 것만. 전부 통과면 한 줄.

  diff 를 파일별로 다시 서술하지 않고, 돌린 명령을 전부 나열하지 않으며, 읽는 사람이 이미
  아는 배경을 되풀이하지 않는다. 근거: 긴 본문은 그냥 안 읽힌다 — 본문의 일은 리뷰어의
  10초를 어디에 쓸지 정해 주는 것이다.
- 본문 말미에 반드시:

  ```
  🤖 Generated with [Claude Code](https://claude.com/claude-code)
  ```

- 리뷰를 요청하는 맥락이면 "모든 리뷰 코멘트는 한국어로 작성해 주세요." 를 본문/코멘트에 포함.

### 7. 마무리 & 다음 단계

- 생성된 PR URL 을 출력한다.
- 이어서 리뷰 대응까지 맡기려면 `/rocky:resolve-reviews` 을 안내한다 — PR 리뷰(Copilot / Codex / 사람) 중 판단이 필요 없는 건을 고치고, 스레드에는 👀 만 남긴 채 전부 열어 둔 뒤 채팅으로 보고한다(resolve 는 사용자 몫). 머지 가능해지면 알린다 (PR 브랜치를 체크아웃한 상태에서 실행). CI 실패 자동 수정만 원하면 Claude Code 빌트인 `/autofix-pr` 이 별도 선택지다.

## 실패 / 예외 처리

- `gh` 미인증 → `gh auth status` 확인 안내 후, 커밋·푸시까지만 하고 PR 단계에서 멈춘다.
- 원격에 upstream 이 없거나 push 거부 → 에러를 그대로 인용하고 멈춘다 (강제 푸시 금지).
- 게이트 실패 → 3단계 이후로 넘어가지 않는다 (커밋 없음).
