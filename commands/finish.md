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
   **제목은 간결하게** — 핵심 변경 하나만 한 구절로 담고(요약부 대략 50자 이내), 나열·부연·괄호
   덧붙임은 전부 본문으로 내린다. 본문의 상세함은 지금 수준을 유지한다.

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
  (`FEATURES.md` 한국어 · `AGENTS.md` 영문)와 진입 문서(`README.md` 등) 가 갱신됐는지
  `git diff --stat` 로 점검하고, 빠졌으면 한 줄로 지적한다.

### 3. 브랜치 확인

- 현재 브랜치가 `main`(기본 브랜치)이면: 변경 내용에 맞는 이름으로 새 브랜치를 만든다
  (`git switch -c <type>/<짧은-요약>`). 이미 feature 브랜치면 그대로 사용.

### 4. 커밋 초안 → 커밋

- `git diff` 를 읽고 변경의 핵심을 한국어로 요약해 **커밋 제목 + 본문 초안**을 만들어 보여준다.
  커밋 제목도 원칙 4 의 간결성 규칙을 따른다 (핵심 하나만 한 구절로, 세부는 본문으로).
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

```bash
gh pr create --base main --head <브랜치> --title "<Conventional 한국어 제목>" --body "<본문>"
```

- 제목: Conventional Commits 스타일 한국어. **간결하게** — 핵심 하나만 한 구절로, 세부·나열은
  본문으로 (원칙 4).
- 본문(한국어): **요약** / **변경 사항**(bullet) / **검증**(돌린 게이트 결과) 순. 장문 리포트 금지.
- 본문 말미에 반드시:

  ```
  🤖 Generated with [Claude Code](https://claude.com/claude-code)
  ```

- 리뷰를 요청하는 맥락이면 "모든 리뷰 코멘트는 한국어로 작성해 주세요." 를 본문/코멘트에 포함.

### 7. 마무리 & 다음 단계

- 생성된 PR URL 을 출력한다.
- 이어서 CI 실패·리뷰 코멘트 자동 반영까지 맡기려면 Claude Code 빌트인 `/autofix-pr` 을 안내한다 (PR 브랜치를 체크아웃한 상태에서 실행해야 한다 — main 에서는 실행 거부됨).

## 실패 / 예외 처리

- `gh` 미인증 → `gh auth status` 확인 안내 후, 커밋·푸시까지만 하고 PR 단계에서 멈춘다.
- 원격에 upstream 이 없거나 push 거부 → 에러를 그대로 인용하고 멈춘다 (강제 푸시 금지).
- 게이트 실패 → 3단계 이후로 넘어가지 않는다 (커밋 없음).
