# Codex 감시형 위임 서브에이전트(/codex) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** rocky 에 `/codex <task>` 슬래시 커맨드(Codex 를 격리 worktree 에서 구현자 서브에이전트로 위임하고 Claude 가 게이트·MCP 표면·diff 스코프를 감시)를 추가하고, 그 첫 실사용으로 "rocky 를 Codex 에서 쓰게 하는 문서"를 Codex 가 구현하게 한 뒤 Claude 가 검증·병합한다.

**Architecture:** `/codex` 는 `/finish`·`/curate` 와 같은 순수 markdown host-LLM 오케스트레이션 커맨드(신규 TS/deps 없음). Claude 가 커맨드 절차를 따라 새 git worktree 를 만들고 `codex exec --full-auto --json` 으로 Codex 에 구현을 위임한 뒤, 게이트 3종 + `src/index.test.ts` 표면 무결성 + diff 스코프로 감시하고 깨끗할 때만 현재 브랜치에 병합한다(자동 push 없음). Task 1 이 재사용 하네스를 만들고, Task 2 가 그 하네스를 실제로 돌려 rocky-in-codex 문서(런타임 코드 0, config.toml 스니펫 중심)를 Codex 가 구현한다.

**Tech Stack:** Claude Code 슬래시 커맨드(markdown + front-matter), `codex` CLI 0.44.0 (`codex exec`), git worktree, Bun(게이트: `bun run check` / `bun run typecheck` / `bun test`).

## Global Constraints

- 대화/문서/커밋/PR 언어는 **한국어**. 코드 identifier / 경로 / 명령어 / 라이브러리명은 영어 그대로.
- 커맨드는 **순수 markdown** — 신규 런타임 TS 코드·의존성 없음. `/finish`·`/curate` front-matter 규약(`description` / `argument-hint` / `allowed-tools`) + `$ARGUMENTS` 를 따른다.
- rocky **MCP 도구 표면(개수/이름)을 바꾸지 않는다.** 표면 회귀 가드는 `src/index.test.ts`(도구 개수 + `REMOVED_TOOLS` 누수 + `JOURNAL_TOOLS` 상시).
- **자동 병합/자동 push 없음.** Codex 산출물은 Claude 감시 통과 후 owner 승인 하에만 현재 브랜치로 병합(pr-watch 와 동일 정책).
- 사용자 표면(도구/env/커맨드/handle) 변경 시 두 단일 소스 동기화: `FEATURES.md`(한글) + `AGENTS.md`(영문) + 진입 문서 `README.md`.
- Codex 위임 시 샌드박스는 `--full-auto`(= workspace-write, worktree 범위)로 제한. `danger-full-access` / `--dangerously-bypass-approvals-and-sandbox` 미사용.
- 게이트 실패 시 병합/커밋으로 넘어가지 않는다.

---

## File Structure

**Task 1 (Claude 가 만드는 하네스, branch `minjun0219/codex-plugin`)**

- Create: `commands/codex.md` — `/codex <task>` 커맨드 (worktree 격리 → `codex exec` 위임 → 감시 → 판정).
- Modify: `FEATURES.md` — "## Claude Code 커맨드" intro + `/codex` 절 추가.
- Modify: `AGENTS.md` — *Project in one line* 슬래시 커맨드 목록 + Layout `commands/` 트리에 `codex.md`.
- Modify: `README.md` — 슬래시 커맨드 목록에 `/codex`.

**Task 2 (Codex 가 만드는 산출물, 격리 worktree, Claude 감독)** — 런타임 코드 변경 없음:

- Create: `docs/codex.md` — `~/.codex/config.toml` 등록 스니펫 + `codex mcp add` + 주의점.
- Modify: `FEATURES.md` — Codex 사용 섹션(한글).
- Modify: `AGENTS.md` — 배포 서술 리프레이밍(전체 표면 서버 + N 소비 호스트) + Layout `docs/codex.md`.
- Modify: `README.md` — Codex 소비 호스트 한 줄.

---

## Task 1: `/codex` 커맨드 하네스 + 문서화

**Files:**
- Create: `commands/codex.md`
- Modify: `FEATURES.md:191-210` (Claude Code 커맨드 섹션)
- Modify: `AGENTS.md:9` (Project in one line), `AGENTS.md:27-29` (Layout commands/)
- Modify: `README.md:18` (슬래시 커맨드 목록)

**Interfaces:**
- Produces: 슬래시 커맨드 `/codex <task>` (rocky:codex). Task 2 가 이 커맨드를 실제로 호출해 rocky-in-codex 문서를 Codex 에 위임한다.
- Consumes: 없음(기존 `codex` CLI 0.44.0 + git worktree + Bun 게이트).

- [ ] **Step 1: `commands/codex.md` 생성**

아래 내용을 그대로 작성한다:

```markdown
---
description: task 하나를 Codex(codex exec)에 위임해 격리된 git worktree 에서 구현시키고, Claude 가 게이트·MCP 도구 표면·diff 스코프를 감시해 rocky(Claude Code) 플러그인 동작을 깨지 않는지 검증한 뒤에만 현재 브랜치로 병합한다. 자동 병합·자동 push 없음.
argument-hint: "<Codex 에게 맡길 구현 task>"
allowed-tools: Bash(codex:*), Bash(git:*), Bash(bun:*), Read, Grep, Glob
---

# codex — Codex 위임 + Claude 감시

한 task 를 **Codex(`codex exec`)에 구현자로 위임**하고, 나(Claude)는 **감독자**로서 결과가
rocky 플러그인 동작을 깨지 않는지 검증한다. `$ARGUMENTS` 는 Codex 에게 맡길 구현 task.
출력은 **한국어**(코드 identifier / 경로 / 명령어는 영어 그대로).

## 원칙

1. **역할 분리.** Codex = 구현자, Claude = 감독자. 나는 구현 코드를 직접 쓰지 않고
   위임·게이트·판정만 한다. Codex 가 스코프 밖을 건드리거나 게이트를 못 맞추면 병합하지 않는다.
2. **격리.** Codex 는 항상 새 git worktree 안에서만 작업한다. 현재 작업트리는 건드리지 않는다.
3. **감시 = "플러그인 작동 방해 안 하는지" 의 구체 정의.** (a) 게이트 3종 통과, (b) MCP 도구
   표면(개수/이름) 무결, (c) `.claude-plugin/plugin.json` 의 `mcpServers` 무결, (d) diff 가
   요청 스코프에 한정. 하나라도 어기면 "플러그인 방해" 로 간주하고 병합 보류.
4. **자동 병합·push 없음.** 감시 통과 후 diff 를 사용자에게 제시하고 승인 하에만 병합한다.
   원격 push / PR 은 이 커맨드가 하지 않는다(필요하면 이어서 `/finish`).
5. **샌드박스 제한.** `--full-auto`(workspace-write, worktree 범위)만 쓴다.
   `danger-full-access` / bypass 플래그는 쓰지 않는다.

## 절차

### 1. 사전 점검 & 격리 준비

```bash
git rev-parse --abbrev-ref HEAD        # 현재 브랜치 확인
git status --porcelain                 # 워킹 트리 clean 확인 (더러우면 먼저 정리 안내 후 멈춤)
which codex && codex --version         # codex CLI 존재 확인 (없으면 설치 안내 후 멈춤)
```

- 워킹 트리가 더러우면(커밋 안 된 변경) 병합 시 충돌·혼선이 나므로, 먼저 커밋/스태시하라고
  안내하고 멈춘다.
- task slug 를 정한다(영문 kebab, 예: `codex-mcp-host`). worktree 경로/브랜치:

```bash
WT="../rocky-codex-<slug>"
git worktree add "$WT" -b "codex/<slug>"
```

### 2. Codex 에 위임 (dispatch)

가드레일을 담은 프롬프트로 Codex 를 비대화형 실행한다. `<TASK>` 자리에 `$ARGUMENTS` 를 넣는다:

```bash
codex exec --full-auto -C "$WT" \
  --json --output-last-message "$WT/.codex-last.txt" \
  "너는 rocky 레포에서 한 task 를 구현하는 구현자다. 다음 불변식을 반드시 지켜라:
   (1) rocky 의 MCP 도구 표면(도구 개수/이름)을 바꾸지 마라 — src/index.ts 의 registerTool 목록 불변.
   (2) 게이트를 통과시켜라: bun run check && bun run typecheck && bun test 가 모두 green.
   (3) 요청 스코프 밖 파일(특히 런타임 TS/plugin.json/package.json)을 건드리지 마라.
   (4) 사용자 표면을 바꾸면 FEATURES.md(한글)와 AGENTS.md(영문)를 lockstep 으로 동기화하라.
   (5) 커밋하지 마라 — 변경만 워킹 트리에 남겨라(감독자 Claude 가 검토 후 병합한다).
   TASK: <TASK>"
```

- `.codex-last.txt`(최종 메시지)와 종료 코드를 확인한다. 비정상 종료면 로그를 인용하고
  worktree 를 남긴 채 사용자에게 보고한다.

### 3. 감시 (supervise)

worktree 안에서 직접 검증한다.

```bash
cd "$WT"
git status --porcelain                 # Codex 가 만든 변경 목록
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
- [ ] diff 파일 집합이 요청 task 스코프에 한정.

### 4. 판정 & 병합 / 에스컬레이션

- **모두 통과** → 변경 요약 + `git diff --stat` 을 사용자에게 제시하고 승인 하에 현재 브랜치로
  가져온다:

  ```bash
  cd -                                 # 원래 작업트리로
  git merge --no-ff "codex/<slug>"     # 또는 squash: git merge --squash
  git worktree remove "$WT"
  git branch -d "codex/<slug>"
  ```

  Codex 가 커밋을 안 남겼으면(원칙 5) merge 대신 worktree 의 변경을 원래 트리로 가져와
  Claude 가 직접 커밋한다:

  ```bash
  cd -
  git --no-ff은 불가 → 변경을 적용: (worktree diff 를 원 트리에 반영 후 커밋)
  ```

  실무상: worktree 에서 `git add -A && git commit` 후 `git merge --squash` 로 가져오거나,
  간단히 원 트리에서 동일 파일을 반영 커밋한다. 어느 쪽이든 **사용자에게 diff 를 먼저 보여주고**
  승인받은 뒤 커밋한다.
- **하나라도 실패** → 무엇을 깼는지(게이트/표면/스코프)를 로그 인용과 함께 보고하고
  **병합하지 않는다.** 선택지: (a) 가드레일을 보강해 같은 worktree 에서 Codex 재위임
  (`codex exec ... resume` 또는 새 프롬프트), (b) worktree 폐기 후 사용자 에스컬레이션.

  ```bash
  # 폐기할 때
  git worktree remove --force "$WT"
  git branch -D "codex/<slug>"
  ```

### 5. 마무리

- 병합 여부, 변경 파일, 돌린 게이트 결과를 한국어 한두 줄로 요약한다. 장문 리포트 금지.

## 예외 처리

- `codex` 미설치 → 설치 안내 후 멈춤(위임 없음).
- 워킹 트리 더러움 → 먼저 정리 안내 후 멈춤.
- Codex 비정상 종료 → worktree 보존 + 로그 인용 + 사용자 보고.
- 게이트/표면 실패 → 병합 없음(위 4단계).
- worktree 정리 실패 → 경로를 알리고 수동 `git worktree remove --force` 안내.
```

- [ ] **Step 2: 커맨드 파일 형식 검증**

Run:
```bash
head -5 commands/codex.md
```
Expected: 1행 `---`, `description:` / `argument-hint:` / `allowed-tools:` 존재, 5행 `---` — `finish.md`/`curate.md` 와 동일한 front-matter 형태.

- [ ] **Step 3: `FEATURES.md` 갱신 — 커맨드 섹션 intro + `/codex` 절**

`FEATURES.md` 의 "## Claude Code 커맨드" intro(현재 193행: "…`/finish` 는 `gh` CLI 기반… `/curate` 는… 짝 커맨드다.")에 `/codex` 를 한 문장 덧붙인다. intro 문장 끝에 다음을 추가:

```
그리고 `/codex` 는 task 하나를 Codex(`codex exec`)에 위임해 격리 worktree 에서 구현시키고 Claude 가 게이트·MCP 표면·diff 스코프를 감시하는 위임 커맨드다(자동 병합 없음).
```

그리고 `### /curate [주제 힌트]` 절(현재 203–209행) **뒤에** 새 절을 추가:

```markdown
### `/codex <task>`

- **What**: task 하나를 **Codex(`codex exec`)에 구현자로 위임**하고, Claude 가 **감독자**로서
  결과를 검증하는 오케스트레이션 커맨드. Codex 는 새 git worktree(격리)에서 `--full-auto`
  (workspace-write) 로 구현하고, Claude 는 게이트(`check`/`typecheck`/`test`) + MCP 도구 표면
  무결성(`src/index.test.ts`) + `plugin.json` mcpServers 무결 + diff 스코프를 감시한다.
- **감시 = "플러그인 작동 방해 안 하는지"**: 위 4가지가 모두 통과할 때만 "방해 없음" 으로 보고
  현재 브랜치에 병합한다. 하나라도 어기면 병합하지 않고 무엇을 깼는지 보고·에스컬레이션.
- **하지 않는 것**: 자동 병합·자동 push·PR 없음(승인 하 병합만, 이어서 `/finish`).
  `danger-full-access` 미사용. Claude 가 구현 코드를 직접 쓰지 않음(위임·게이트·판정만).
- **전제**: `codex` CLI(0.44+) 설치, 워킹 트리 clean.
```

- [ ] **Step 4: `AGENTS.md` 갱신 — Project in one line + Layout**

`AGENTS.md:9` 의 *Project in one line* 에서 슬래시 커맨드를 열거하는 부분
`(`/finish` — `gh` CLI based; `/curate` — reads `journal_*` and writes markdown to the configured wiki; none are MCP tools)` 를 다음으로 교체:

```
(`/finish` — `gh` CLI based; `/curate` — reads `journal_*` and writes markdown to the configured wiki; `/codex` — delegates one task to Codex (`codex exec`) in an isolated worktree and supervises gates / MCP-surface / diff-scope before merging (no auto-merge); none are MCP tools)
```

`AGENTS.md:27-29` 의 Layout `commands/` 트리에 줄을 추가(`curate.md` 줄 뒤):

```
│   ├── curate.md                           `/curate` — journal_* 기록을 설정된 wiki(Obsidian 등)로 증분 증류 (정리 레이어, gh 불필요)
│   └── codex.md                            `/codex` — task 를 Codex(codex exec)에 위임(격리 worktree) + Claude 감시(게이트/표면/스코프), 자동 병합 X
```

(기존 `curate.md` 줄의 트리 문자 `└──` 를 `├──` 로 바꾸고 새 `codex.md` 를 `└──` 로 둔다.)

- [ ] **Step 5: `README.md` 갱신 — 슬래시 커맨드 목록**

`README.md:18` 의 `— `/finish` (…) 와 `/curate` (…)` 부분을 `/codex` 를 포함하도록 확장:

```
— `/finish` (게이트→커밋→푸시→PR 생성, `gh` CLI 기반), `/curate` (`journal_*` 를 읽어 wiki 로 정리), 그리고 `/codex` (task 를 Codex `codex exec` 에 위임해 격리 worktree 에서 구현시키고 Claude 가 게이트·MCP 표면·diff 스코프를 감시, 자동 병합 없음).
```

- [ ] **Step 6: 게이트 실행**

Run:
```bash
bun run check && bun run typecheck && bun test
```
Expected: 3종 모두 PASS(회귀 없음 — markdown/문서만 추가, 런타임 코드·표면 불변이므로 `src/index.test.ts` 도 그대로 통과).

- [ ] **Step 7: 커밋**

```bash
git add commands/codex.md FEATURES.md AGENTS.md README.md
git commit -m "feat(codex): /codex — Codex 위임 + Claude 감시 슬래시 커맨드

task 를 codex exec 에 격리 worktree 로 위임하고 게이트·MCP 표면·diff 스코프를
감시해 rocky 플러그인 동작을 깨지 않을 때만 병합(자동 병합 X). host-LLM
오케스트레이션 커맨드(순수 markdown), /finish·/curate 패턴.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: 라이브 실행 — Codex 에게 rocky-in-codex 문서 위임 + 감시 + 병합

> 이 task 는 Task 1 이 만든 `/codex` 를 **실제로 실행**한다. Codex 가 구현자, Claude 가 감독자.
> 산출물은 rocky 를 Codex 에서 쓰게 하는 **문서/설정만**(런타임 코드 0).

**Files (Codex 가 worktree 에서 만듦):**
- Create: `docs/codex.md`
- Modify: `FEATURES.md` (Codex 사용 섹션), `AGENTS.md` (배포 리프레이밍 + Layout), `README.md` (Codex 한 줄)

**Interfaces:**
- Consumes: Task 1 의 `/codex <task>` 커맨드(절차 = worktree → codex exec → 감시 → 판정).
- Produces: rocky-in-codex 문서. 감시 통과 시 현재 브랜치 병합.

- [ ] **Step 1: 워킹 트리 clean 확인**

Run:
```bash
git status --porcelain
```
Expected: 빈 출력(Task 1 커밋 완료 상태). 더러우면 정리 후 진행.

- [ ] **Step 2: 격리 worktree 생성**

```bash
WT="../rocky-codex-mcp-host"
git worktree add "$WT" -b "codex/mcp-host"
```
Expected: `Preparing worktree` + `HEAD is now at 333a274 …`(Task 1 하네스 커밋 포함).

- [ ] **Step 3: Codex 에 rocky-in-codex 문서 task 위임**

아래를 실행한다(가드레일 + 부속 스코프를 프롬프트에 명시). 이 프롬프트가 `/codex` 절차의
`<TASK>` 에 해당한다:

```bash
codex exec --full-auto -C "$WT" \
  --json --output-last-message "$WT/.codex-last.txt" \
  "너는 rocky 레포에서 문서 task 를 구현하는 구현자다. 불변식: (1) MCP 도구 표면 불변,
   (2) bun run check && bun run typecheck && bun test 통과, (3) 런타임 TS/plugin.json/
   package.json/rocky.schema.json 을 건드리지 마라 — 이 task 는 문서/설정만이다,
   (4) FEATURES.md(한글)와 AGENTS.md(영문)를 lockstep 동기화, (5) 커밋하지 마라.
   TASK: rocky 를 OpenAI Codex CLI 에서도 MCP 서버로 쓸 수 있게 문서화하라. 핵심 사실:
   rocky 의 src/index.ts 는 이미 호스트 무관 stdio MCP 서버라 Codex 는 CC 플러그인과 동일한
   프로세스(bun run <repo>/src/index.ts)를 그대로 띄우면 전체 도구(openapi 7 + seo_validate 1
   + journal 4, ntn 있으면 notion 4)를 쓴다 — 런타임 코드 변경 0. 구체 산출물:
   (a) docs/codex.md 신설(docs/openapi-mcp.md 대칭): ~/.codex/config.toml 의
       [mcp_servers.rocky] command='bun' args=['run','/abs/path/to/rocky/src/index.ts'] 스니펫,
       동등 CLI 'codex mcp add rocky -- bun run <abs>/src/index.ts', 주의점(cwd 의존:
       rocky.json project scope & journal 프로젝트 키가 cwd 기반 / env 로 ROCKY_JOURNAL_DIR 등
       오버라이드 / bun PATH 전제 / notion 은 ntn 탐지 시만 / 슬래시커맨드·스킬은 CC 전용이라 비노출).
   (b) FEATURES.md 에 Codex 사용 섹션(한글).
   (c) AGENTS.md 의 Project in one line + 배포 서술을 '2 배포 타깃 → 전체 표면 서버(src/index.ts)
       + 이를 소비하는 N 호스트(CC 플러그인 + Codex)' 로 리프레이밍하고 Layout 에 docs/codex.md 추가.
   (d) README.md 에 Codex 소비 호스트 한 줄."
```
Expected: Codex 가 (a)~(d) 파일을 worktree 에 생성/수정. 종료 코드 0 + `.codex-last.txt` 요약.

- [ ] **Step 4: 감시 — diff 검토 + 게이트**

```bash
cd "$WT"
git status --porcelain
git --no-pager diff --stat
git --no-pager diff docs/codex.md FEATURES.md AGENTS.md README.md
bun run check
bun run typecheck
bun test
cd -
```
Expected(판정 기준, 모두 만족해야 병합):
- `check`/`typecheck`/`test` 모두 PASS.
- `src/index.test.ts` PASS → MCP 도구 표면 무결.
- diff 가 `docs/codex.md` / `FEATURES.md` / `AGENTS.md` / `README.md` 4개에만 한정. 런타임 TS·
  `plugin.json`·`package.json`·`rocky.schema.json` 변경 **없음**.
- `docs/codex.md` 의 config.toml 스니펫이 `bun run <repo>/src/index.ts` 를 가리킴.

- [ ] **Step 5: (감시 보강) 등록 스니펫 실제 부팅 스모크**

문서가 안내하는 명령이 실제로 뜨는지 확인한다(Codex 없이 stdio 왕복):

```bash
printf '%s\n%s\n' \
 '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}}' \
 '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}' \
 | bun run "$WT/src/index.ts" 2>/dev/null | grep -o '"name":"[a-z_]*"' | sort -u
```
Expected: 최소 `openapi_get/refresh/status/search/envs/endpoint/tags` + `seo_validate` +
`journal_append/read/search/status` = 12 도구가 목록에 보인다(`ntn` 설치 시 `notion_*` 4 추가).
(주: stdio 서버가 요청 처리 후 대기하면 `head`/timeout 이 필요할 수 있다 — 도구명이 확인되면 충분.)

- [ ] **Step 6: 판정 & 병합 (통과 시)**

diff 요약을 사용자에게 제시하고 승인 하에 병합한다. Codex 는 커밋을 안 남겼으므로
(원칙 5) worktree 에서 커밋 후 squash 로 가져온다:

```bash
cd "$WT"
git add docs/codex.md FEATURES.md AGENTS.md README.md
git commit -m "docs(codex): rocky 를 Codex CLI MCP 호스트로 사용하는 문서 (Codex 구현, Claude 감독)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
cd -
git merge --squash "codex/mcp-host"
git commit -m "docs(codex): rocky 를 Codex CLI MCP 호스트로 사용하는 문서

src/index.ts(호스트 무관 stdio MCP)를 Codex ~/.codex/config.toml 에 등록하는 방법.
런타임 코드 변경 없음. Codex 가 codex exec 로 구현, Claude 가 게이트·MCP 표면·diff
스코프 감시 통과 확인 후 병합.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

- [ ] **Step 7: worktree 정리**

```bash
git worktree remove "$WT"
git branch -d "codex/mcp-host"
git worktree list
```
Expected: worktree 목록에서 `rocky-codex-mcp-host` 사라짐.

- [ ] **Step 8: 실패 시 에스컬레이션 (통과 못 하면)**

Step 4/5 판정 실패면 병합하지 않고 보고한다: 무엇을 깼는지(게이트 로그 / 표면 diff / 스코프 이탈)
인용 + 선택지 제시(가드레일 보강 재위임 vs worktree 폐기 후 owner 결정). 폐기:

```bash
git worktree remove --force "$WT"
git branch -D "codex/mcp-host"
```

- [ ] **Step 9: 최종 게이트 & 요약**

병합 후 원 브랜치에서 최종 확인:
```bash
bun run check && bun run typecheck && bun test
git log --oneline -3
```
Expected: 게이트 통과. 로그에 Task 1(하네스) + Task 2(Codex 문서) 커밋. 한국어 한두 줄 요약
(무엇을 Codex 가 만들고 Claude 가 무엇을 검증했는지).

---

## Self-Review

**Spec coverage:**
- Codex 서브에이전트(형태=슬래시커맨드) → Task 1. ✓
- 격리(새 worktree) → Task 1 커맨드 절차 + Task 2 Step 2. ✓
- 감시(게이트/표면/plugin.json/스코프) → Task 1 §3 + Task 2 Step 4–5. ✓
- 라이브 실행(하네스로 codex-plugin 구현) → Task 2. ✓
- rocky-in-codex 부속 스코프(docs/codex.md + FEATURES/AGENTS/README, 런타임 0) → Task 2 Step 3 프롬프트 + 판정. ✓
- 실행 순서(하네스 커밋 → worktree 분기 → Codex → 감시 → 병합) → Task 1 커밋 후 Task 2. ✓
- 자동 병합·push 없음 → Global Constraints + Task 1 원칙 4 + Task 2 Step 6(승인 하). ✓
- 비대상(rocky-mcp bin / 슬래시커맨드 Codex 포팅 / npm publish) → 계획에 포함 안 함. ✓

**Placeholder scan:** `<slug>` / `<TASK>` / `<abs>` 는 커맨드 템플릿의 의도된 치환자(실행 시 확정). Task 2 는 구체값(`mcp-host` slug, 실제 프롬프트, 실제 파일명) 사용 — TODO/미정 없음.

**Type consistency:** worktree 경로 변수 `$WT`, 브랜치 `codex/<slug>`(Task 2 = `codex/mcp-host`), 게이트 3종 명령(`bun run check`/`bun run typecheck`/`bun test`), 표면 가드(`src/index.test.ts`)가 Task 1·2 전반에서 일관.

> 주의: Task 2 Step 6 은 "Codex 가 커밋 안 남김(원칙 5)" 전제로 worktree 커밋 후 squash 한다.
> Task 1 커맨드 §4 의 병합 서술과 정합(어느 쪽이든 사용자에게 diff 먼저 제시 후 승인 병합).
