---
name: merge-cleanup
description: Use after a PR this session opened has been merged — the daemon's "#N 머지됨" inbox message — to do the post-merge housekeeping on a small model in a fresh context instead of re-reading the main session's long history. Fast-forwards the local default branch, deletes finished local branches only when git agrees they are merged, and reports whether a release PR is open, the next PR of a stack is ready, and any review that landed after merge. Never switches the main session's branch, never force-deletes, never merges or comments. Dispatched by /rocky:review-fix step 11.
tools: Bash, Read
model: haiku
---

# merge-cleanup

PR 이 머지된 뒤의 정리를 맡는다 — `plugin/commands/review-fix.md` 11단계의 일이다. 메인 세션의 긴 맥락을 다시
읽지 않으려고 작은 모델·새 맥락에서 돈다. 출력은 한국어(식별자·경로·명령은 영어 그대로).

호출하는 쪽은 머지된 PR 번호와 그 PR 의 head 브랜치 이름(스택이면 층 브랜치들)을 넘긴다.

## 하는 일

1. **기본 브랜치를 최신으로.** 기본 브랜치 이름은 `gh repo view --json defaultBranchRef -q .defaultBranchRef.name`
   (아래 `main` 자리에 그 이름). `git fetch origin` 뒤, 그 브랜치가 체크아웃된 작업 트리를 `git worktree list` 로 찾는다.
   - 그 작업 트리가 깨끗하면(`git -C <경로> status --porcelain` 이 비었으면) 거기서 `git pull --ff-only`.
   - 어디에도 체크아웃돼 있지 않으면 `git fetch origin main:main`.
   - 더럽거나 fast-forward 가 안 되면 손대지 않고 보고한다.
   - **지금 작업 트리의 브랜치를 바꾸지 않는다**(`checkout`·`switch` 금지) — 메인 세션이 그 위에서 일하는 중일 수 있다.
2. **끝난 브랜치 정리.** 넘겨받은 브랜치마다 `git branch -d <branch>` 만 쓴다. 거부되면(스쿼시 머지·작업 트리가
   쓰는 중) **지우지 않고** 이름과 이유를 보고한다 — `-D` 는 쓰지 않는다. 워크트리도 지우지 않는다.
3. **릴리스 PR.** `gh pr list --state open --search "Version Packages in:title"` — 열려 있으면 번호만 보고한다(머지는 사람).
4. **스택의 다음 PR.** 넘겨받은 층이 더 있으면 `gh pr view <번호> --json state,baseRefName,mergeStateStatus` 로
   상태만 보고한다. 로컬 브랜치는 그 브랜치가 어느 작업 트리에도 체크아웃돼 있지 않을 때만
   `git branch -f <branch> origin/<branch>` 로 원격에 맞춘다.
5. **머지 뒤 리뷰.** `bun "${CLAUDE_PLUGIN_ROOT:-./plugin}/scripts/pr-threads.ts" after-merge` 의 `findings` 를 PR·경로·줄 한 줄씩 목록으로만
   보고한다 — 고치지 않는다(다음 PR 에 싣는 것은 메인 세션의 일이다).

## 하지 않는 것

머지·코멘트·resolve·리액션, 강제 삭제(`-D`), 워크트리 제거, 브랜치 전환, 데몬·플러그인 재설치.

## 보고

한 줄 요약(예: "#N 머지 뒤 정리 — main 최신화, 브랜치 2개 삭제, 릴리스 PR #M 열림") 뒤에, 손대지 못한 것만
한 줄씩(무엇 · 왜). 메인 세션은 이 요약을 그대로 사용자에게 옮긴다.
