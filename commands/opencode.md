---
description: task 하나를 opencode(opencode run)에 위임해 격리된 git worktree 에서 구현시키고, Claude 가 게이트·MCP 도구 표면·diff 스코프를 감시해 rocky(Claude Code) 플러그인 동작을 깨지 않는지 검증한 뒤에만 현재 브랜치로 병합한다. 자동 병합·자동 push 없음.
argument-hint: "<opencode 에게 맡길 구현 task>"
allowed-tools: Bash(opencode:*), Bash(git:*), Bash(bun:*), Bash(which:*), Read, Grep, Glob
---

# opencode — opencode 위임 + Claude 감시

한 task 를 **opencode(`opencode run`)에 구현자로 위임**하고, 나(Claude)는 **감독자**로서 결과가
rocky 플러그인 동작을 깨지 않는지 검증한다. `$ARGUMENTS` 는 opencode 에게 맡길 구현 task.
출력은 **한국어**(코드 identifier / 경로 / 명령어는 영어 그대로).

## 원칙

1. **역할 분리.** opencode = 구현자, Claude = 감독자. 나는 구현 코드를 직접 쓰지 않고
   위임·게이트·판정만 한다. opencode 가 스코프 밖을 건드리거나 게이트를 못 맞추면 병합하지 않는다.
2. **격리.** opencode 는 항상 새 git worktree 안에서만 작업한다. 현재 작업트리는 건드리지 않는다.
3. **감시 = "플러그인 작동 방해 안 하는지" 의 구체 정의.** (a) 게이트 3종 통과, (b) MCP 도구
   표면(개수/이름) 무결, (c) `.claude-plugin/plugin.json` 의 `mcpServers` 무결, (d) diff 가
   요청 스코프에 한정. 하나라도 어기면 "플러그인 방해" 로 간주하고 병합 보류.
4. **자동 병합·push 없음.** 감시 통과 후 diff 를 사용자에게 제시하고 승인 하에만 병합한다.
   원격 push / PR 은 이 커맨드가 하지 않는다(필요하면 이어서 `/finish`).
5. **격리로만 봉쇄.** opencode 에는 codex `-s workspace-write` 같은 OS 레벨 파일시스템 샌드박스가
   없다. 봉쇄는 **worktree 격리 + 병합 전 Claude 의 diff 전량 검토**로만 보장된다. `--auto`(권한 자동
   승인)는 worktree 안으로만 국한되고 어떤 변경도 Claude 검토 없이는 원 브랜치에 들어가지 않는다.

## 절차

### 1. 사전 점검 & 격리 준비

```bash
git rev-parse --abbrev-ref HEAD        # 현재 브랜치 확인
git status --porcelain                 # 워킹 트리 clean 확인 (더러우면 먼저 정리 안내 후 멈춤)
which opencode && opencode --version   # opencode CLI 존재 확인 (없으면 설치 안내 후 멈춤)
```

- 워킹 트리가 더러우면(커밋 안 된 변경) 병합 시 충돌·혼선이 나므로, 먼저 커밋/스태시하라고
  안내하고 멈춘다.
- task slug 를 정한다(영문 kebab, 예: `opencode-docs`). worktree 경로/브랜치:

```bash
WT="../rocky-opencode-<slug>"
git worktree add "$WT" -b "opencode/<slug>"
```

### 2. opencode 에 위임 (dispatch)

가드레일을 담은 프롬프트로 opencode 를 비대화형 실행한다. `$ARGUMENTS` 변수를 통해 태스크를 전달한다.
opencode 에는 `--output-last-message` 등가물이 없으므로 stdout/stderr 를 파일로 캡처한다:

```bash
opencode run --dir "$WT" --auto \
  "너는 rocky 레포에서 한 task 를 구현하는 구현자다. 다음 불변식을 반드시 지켜라:
   (1) rocky 의 MCP 도구 표면(도구 개수/이름)을 바꾸지 마라 — src/index.ts 의 registerTool 목록 불변.
   (2) 게이트를 통과시켜라: bun run check && bun run typecheck && bun test 가 모두 green.
   (3) 요청 스코프 밖 파일(특히 런타임 TS/plugin.json/package.json)을 건드리지 마라.
   (4) 사용자 표면을 바꾸면 FEATURES.md(한글)와 AGENTS.md(영문)를 lockstep 으로 동기화하라.
   (5) 커밋하지 마라 — 변경만 워킹 트리에 남겨라(감독자 Claude 가 검토 후 병합한다).
   TASK: $ARGUMENTS" > "$WT/.opencode-last.txt" 2>&1
```

- `--dir "$WT"` 로 worktree 를 작업 디렉터리로, `--auto` 로 권한 자동 승인. `.opencode-last.txt`(최종
  출력)와 종료 코드를 확인한다. 비정상 종료면 로그를 인용하고 worktree 를 남긴 채 사용자에게 보고한다.

### 3. 감시 (supervise)

worktree 안에서 직접 검증한다.

```bash
cd "$WT"
git status --porcelain                 # opencode 가 만든 변경 목록
git --no-pager diff --stat             # 변경 규모
git --no-pager diff                    # 실제 내용 (직접 읽어 스코프/의도 확인)
bun run check
bun run typecheck
bun test                               # src/index.test.ts 표면 가드 포함
```

판정 체크리스트(모두 통과해야 함):
- [ ] `bun run check` / `typecheck` / `bun test` 모두 통과.
- [ ] `src/index.test.ts` 통과 → MCP 도구 표면(개수/이름/누수 가드) 무결.
- [ ] `git diff` 에 `.claude-plugin/plugin.json` `mcpServers` 파손 없음, 예상 밖 런타임 코드
      변경 없음.
- [ ] diff 파일 집합이 요청 task 스코프에 한정. (`.opencode-last.txt` 는 커밋 전에 지운다.)

### 4. 판정 & 병합 / 에스컬레이션

- **모두 통과** → 변경 요약 + `git diff --stat` 을 사용자에게 제시하고 승인받는다. opencode 는 커밋을
  남기지 않으므로(원칙 5) 변경은 worktree 에 uncommitted 로 있다. 승인 후 worktree 에서 커밋한 뒤
  원 브랜치로 squash 병합하고 worktree 를 정리한다:

  ```bash
  # (사용자 승인 후) 임시 캡처 파일 정리 + worktree 에서 opencode 변경을 커밋
  git -C "$WT" clean -fq -- .opencode-last.txt   # untracked 캡처 파일 제거 (rm 불필요 → allowed-tools 의 git 만 사용)
  git -C "$WT" add -A
  git -C "$WT" commit -m "<한국어 커밋 제목>"
  # 메인 worktree 경로를 git 으로 계산해(OLDPWD/cwd 비의존) 그쪽에서 squash 병합
  MAIN="$(git -C "$WT" worktree list --porcelain)"   # 첫 항목이 메인 worktree
  MAIN="${MAIN%%$'\n'*}"; MAIN="${MAIN#worktree }"    # 첫 줄만 → 'worktree ' 접두 제거 (git+shell 만, sed 불필요)
  git -C "$MAIN" merge --squash "opencode/<slug>"
  git -C "$MAIN" commit -m "<한국어 커밋 제목>"
  # 정리
  git -C "$MAIN" worktree remove "$WT"
  git -C "$MAIN" branch -D "opencode/<slug>"
  ```
- **하나라도 실패** → 무엇을 깼는지(게이트/표면/스코프)를 로그 인용과 함께 보고하고
  **병합하지 않는다.** 선택지: (a) 가드레일을 보강해 같은 worktree 에서 opencode 재위임
  (`opencode run --dir "$WT" -c --auto ...` 로 직전 세션 이어가기 또는 새 프롬프트), (b) worktree
  폐기 후 사용자 에스컬레이션.

  ```bash
  # 폐기할 때 (메인 worktree 에서 실행 — OLDPWD/cwd 비의존)
  MAIN="$(git -C "$WT" worktree list --porcelain)"   # 첫 항목이 메인 worktree
  MAIN="${MAIN%%$'\n'*}"; MAIN="${MAIN#worktree }"    # 첫 줄만 → 'worktree ' 접두 제거 (git+shell 만, sed 불필요)
  git -C "$MAIN" worktree remove --force "$WT"
  git -C "$MAIN" branch -D "opencode/<slug>"
  ```

### 5. 마무리

- 병합 여부, 변경 파일, 돌린 게이트 결과를 한국어 한두 줄로 요약한다. 장문 리포트 금지.

## 예외 처리

- `opencode` 미설치 → 설치 안내 후 멈춤(위임 없음).
- 워킹 트리 더러움 → 먼저 정리 안내 후 멈춤.
- opencode 비정상 종료 → worktree 보존 + 로그 인용 + 사용자 보고.
- 게이트/표면 실패 → 병합 없음(위 4단계).
- worktree 정리 실패 → 경로를 알리고 수동 `git worktree remove --force` 안내.
