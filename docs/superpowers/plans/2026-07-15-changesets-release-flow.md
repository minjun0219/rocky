# changesets 릴리스 플로우 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** changesets 로 버전 범프 + CHANGELOG 생성을 자동화하되, `package.json` 과 `.claude-plugin/plugin.json` 두 버전 파일을 lockstep 으로 유지하고 main PR-only 보호와 충돌하지 않는 GitHub Action 자동 "Version Packages" PR 흐름을 도입한다.

**Architecture:** `@changesets/cli`(devDep)로 의도(changeset)를 선언하고, `changesets/action@v1` 이 main push 시 Version PR 을 자동으로 연다. changesets 는 `package.json` 만 범프하므로, `bun run changeset:version` 이 `changeset version` 다음에 후처리 스크립트 `scripts/sync-plugin-version.ts` 를 돌려 `plugin.json` 버전을 일치시킨다.

**Tech Stack:** Bun 1.3.14, `@changesets/cli` ^2.31.0, `changesets/action@v1`, `oven-sh/setup-bun@v2`, GitHub Actions.

## Global Constraints

- npm publish 는 **완전히 범위 밖** — publish 스크립트, `publishConfig`, `NPM_TOKEN`, git 태그 자동화, GitHub Release 생성 모두 넣지 않는다. release.yml 의 changesets/action `publish` 입력은 아예 지정하지 않는다.
- 버전은 두 곳에 lockstep: `package.json:3` 와 `.claude-plugin/plugin.json:4`. 둘은 항상 같아야 한다.
- 코딩 규칙: TypeScript ESM, `type: module`, Bun 이 `.ts` 직접 실행(빌드 없음). `__dirname` 금지 — `import.meta.dir`(Bun) 사용. import 확장자 안 붙임. 새 런타임 dep 금지 (changesets 는 devDep 이라 허용).
- 이 설정 PR 자체는 **버전을 올리지 않는다** — changesets 도입은 tooling chore 이므로 버전 범프용 changeset 을 커밋에 넣지 않는다.
- 현재 버전: `0.10.0`. 브랜치: `feat/changesets-release-flow` (이미 생성됨, spec 커밋 존재).
- 게이트: `bun run check` / `bun run typecheck` / `bun run test` 모두 통과해야 완료.

---

### Task 1: `scripts/sync-plugin-version.ts` — plugin.json 버전 동기화 스크립트

**Files:**
- Create: `scripts/sync-plugin-version.ts`

**Interfaces:**
- Consumes: `package.json` 의 `version` 필드, `.claude-plugin/plugin.json` 텍스트.
- Produces: 실행 시 `.claude-plugin/plugin.json` 의 최초 `"version":` 값을 `package.json` 의 version 으로 치환. 이후 Task 2 의 `changeset:version` 스크립트가 이 파일을 `bun scripts/sync-plugin-version.ts` 로 호출한다.

- [ ] **Step 1: 스크립트 작성**

`scripts/sync-plugin-version.ts` 생성:

```ts
#!/usr/bin/env bun
/**
 * package.json 의 version 을 읽어 .claude-plugin/plugin.json 의 version 에 반영한다.
 * changesets 는 package.json 만 범프하므로, 두 버전 파일을 lockstep 으로 유지하기 위한 후처리 스크립트.
 * plugin.json 은 텍스트로 읽어 최초 "version" 키만 정규식으로 치환한다 (전체 재직렬화/재포맷 없이 최소 diff).
 */
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const repoRoot = join(import.meta.dir, "..");
const pkgPath = join(repoRoot, "package.json");
const pluginPath = join(repoRoot, ".claude-plugin", "plugin.json");

const pkg = JSON.parse(readFileSync(pkgPath, "utf8")) as { version?: string };
const version = pkg.version;
if (!version) {
  throw new Error(`package.json 에 version 이 없다: ${pkgPath}`);
}

const pluginText = readFileSync(pluginPath, "utf8");
const versionRe = /("version":\s*)"[^"]*"/;
if (!versionRe.test(pluginText)) {
  throw new Error(`plugin.json 에서 version 필드를 찾지 못했다: ${pluginPath}`);
}
const next = pluginText.replace(versionRe, `$1"${version}"`);

if (next !== pluginText) {
  writeFileSync(pluginPath, next);
  console.log(`plugin.json version → ${version}`);
} else {
  console.log(`plugin.json version 이미 ${version} (변경 없음)`);
}
```

- [ ] **Step 2: idempotent 실행 검증 (현재 버전과 동일 → 변경 없음)**

Run: `bun scripts/sync-plugin-version.ts`
Expected: `plugin.json version 이미 0.10.0 (변경 없음)` 출력, `git status` 에 변경 없음.

Run: `git status --short .claude-plugin/plugin.json`
Expected: 빈 출력 (변경 없음).

- [ ] **Step 3: 실제 치환 동작 검증 (임시로 package.json version 변경 후 되돌림)**

Run (macOS BSD sed 는 `sed -i ''`, Linux GNU sed 는 `sed -i` — 이식성 위해 Bun 한 줄로):
```bash
bun -e "const f='package.json';require('fs').writeFileSync(f,require('fs').readFileSync(f,'utf8').replace('\"version\": \"0.10.0\"','\"version\": \"9.9.9\"'))"
bun scripts/sync-plugin-version.ts
grep '"version"' .claude-plugin/plugin.json
```
Expected: `plugin.json version → 9.9.9` 출력 + grep 결과 `  "version": "9.9.9",`.

Run (되돌리기):
```bash
git restore package.json .claude-plugin/plugin.json
grep '"version"' package.json .claude-plugin/plugin.json
```
Expected: 두 파일 모두 `0.10.0` 으로 복구.

- [ ] **Step 4: 게이트 (biome 가 새 스크립트를 받아들이는지)**

Run: `bun run check && bun run typecheck`
Expected: 둘 다 통과 (No fixes / 에러 없음). biome 가 `scripts/**` 를 포맷하면 `bun run fix` 후 재확인.

- [ ] **Step 5: Commit**

```bash
git add scripts/sync-plugin-version.ts
git commit -m "feat(release): plugin.json 버전 동기화 스크립트 (sync-plugin-version)"
```

---

### Task 2: `@changesets/cli` 설치 + init + `changeset:version` 스크립트 배선

**Files:**
- Modify: `package.json` (devDependencies 에 `@changesets/cli`, scripts 에 `changeset` / `changeset:version`)
- Create: `.changeset/config.json` (init 후 편집)
- Create: `.changeset/README.md` (init 이 생성)
- Modify: `bun.lock` (install 결과)

**Interfaces:**
- Consumes: Task 1 의 `scripts/sync-plugin-version.ts`.
- Produces: `bun run changeset:version` = `changeset version && bun scripts/sync-plugin-version.ts` (Task 3 의 release.yml 가 이 스크립트를 versioning 커맨드로 호출).

- [ ] **Step 1: changesets CLI 설치**

Run: `bun add -D @changesets/cli`
Expected: `package.json` devDependencies 에 `@changesets/cli`(약 `^2.31.0`) 추가, `bun.lock` 갱신.

- [ ] **Step 2: changesets 초기화**

Run: `bunx changeset init`
Expected: `.changeset/config.json` + `.changeset/README.md` 생성 출력.

- [ ] **Step 3: `.changeset/config.json` 편집**

`.changeset/config.json` 를 아래로 설정 (단일 패키지, publish 안 함):

```json
{
  "$schema": "https://unpkg.com/@changesets/config@3.1.1/schema.json",
  "changelog": "@changesets/cli/changelog",
  "commit": false,
  "fixed": [],
  "linked": [],
  "access": "restricted",
  "baseBranch": "main",
  "updateInternalDependencies": "patch",
  "ignore": []
}
```

(`$schema` 의 버전은 init 이 생성한 값 그대로 두어도 무방 — 위 값은 참고. `changelog` 은 기본 생성기, `access: "restricted"` 는 publish 를 안 하므로 실질 무의미하지만 scoped 패키지 기본값.)

- [ ] **Step 4: `package.json` scripts 배선**

`package.json` 의 `scripts` 에 두 줄 추가 (`typecheck` 아래, `prepare` 위 등 적당한 위치):

```json
    "changeset": "changeset",
    "changeset:version": "changeset version && bun scripts/sync-plugin-version.ts",
```

- [ ] **Step 5: config 유효성 확인**

Run: `bunx changeset status --since=main`
Expected: 에러 없이 "No changesets present" 류 메시지 (아직 changeset 없음). config 파싱 성공이 핵심.

- [ ] **Step 6: 로컬 파이프라인 검증 (더미 changeset → version → 되돌림)**

Run (더미 changeset 파일 직접 작성):
```bash
cat > .changeset/zz-dummy-verify.md <<'EOF'
---
"@minjun0219/rocky": minor
---

파이프라인 검증용 더미 (커밋하지 않음)
EOF
bun run changeset:version
```
Expected: `changeset version` 이 `package.json` 을 `0.11.0` 으로 올리고 `CHANGELOG.md` 생성, 이어서 `plugin.json version → 0.11.0` 출력.

Run (두 파일 일치 + CHANGELOG 생성 확인):
```bash
grep '"version"' package.json .claude-plugin/plugin.json
head -20 CHANGELOG.md
```
Expected: 두 version 모두 `0.11.0`, `CHANGELOG.md` 에 `0.11.0` 항목 존재.

Run (전부 되돌리기 — 설정 PR 에는 버전 변경/더미/CHANGELOG 안 넣음):
```bash
git restore package.json .claude-plugin/plugin.json
rm -f CHANGELOG.md .changeset/zz-dummy-verify.md
git status --short
```
Expected: `package.json`/`plugin.json` 복구(0.10.0), `CHANGELOG.md`·더미 삭제. `git status` 에는 Task 2 의 의도된 변경(package.json scripts+devDep, .changeset/config.json, .changeset/README.md, bun.lock)만 남음.

- [ ] **Step 7: 게이트**

Run: `bun run check && bun run typecheck && bun run test`
Expected: 셋 다 통과 (test 246 pass 유지 — 이 작업은 런타임 코드 무변경).

- [ ] **Step 8: Commit**

```bash
git add package.json bun.lock .changeset/config.json .changeset/README.md
git commit -m "feat(release): changesets CLI 도입 + changeset:version 배선 (plugin.json sync 포함)"
```

---

### Task 3: `.github/workflows/release.yml` — Version PR 자동화 워크플로

**Files:**
- Create: `.github/workflows/release.yml`

**Interfaces:**
- Consumes: Task 2 의 `bun run changeset:version` 스크립트, `bun install --frozen-lockfile`.
- Produces: main push 시 pending changeset 이 있으면 `changeset-release/main` 브랜치로 "Version Packages" PR 을 자동 생성/갱신.

- [ ] **Step 1: 워크플로 작성**

`.github/workflows/release.yml` 생성:

```yaml
name: Release

on:
  push:
    branches: [main]

# changesets/action 이 Version PR 브랜치를 push 하고 PR 을 열 수 있어야 함
permissions:
  contents: write
  pull-requests: write

concurrency:
  group: release-${{ github.ref }}
  cancel-in-progress: false

jobs:
  version:
    name: version packages PR
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0

      - uses: oven-sh/setup-bun@v2
        with:
          bun-version: 1.3.14

      - name: Install dependencies
        run: bun install --frozen-lockfile

      # publish 입력을 지정하지 않음 → Version PR 생성/갱신만, npm publish/태그/release 없음.
      # version 커맨드를 오버라이드해 changeset version 뒤 plugin.json 을 sync.
      - name: Create Version Packages PR
        uses: changesets/action@v1
        with:
          version: bun run changeset:version
          title: "chore(release): 버전 범프"
          commit: "chore(release): 버전 범프 (CHANGELOG + plugin.json sync)"
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
```

- [ ] **Step 2: YAML 유효성 로컬 확인**

Run: `python3 -c "import yaml,sys; yaml.safe_load(open('.github/workflows/release.yml')); print('yaml ok')"`
Expected: `yaml ok`.

- [ ] **Step 3: 게이트 (biome 는 .github 를 안 건드리지만 전체 확인)**

Run: `bun run check`
Expected: 통과 (`.github/**` 는 biome 대상 아님, 회귀 없음 확인용).

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/release.yml
git commit -m "ci(release): changesets Version PR 자동화 워크플로 (release.yml)"
```

---

### Task 4: 문서 동기화 (AGENTS.md + FEATURES.md)

**Files:**
- Modify: `AGENTS.md` (Common commands 섹션 + Change checklist)
- Modify: `FEATURES.md` (릴리스 플로우 짧게)

**Interfaces:**
- Consumes: Task 1–3 의 최종 스크립트/명령 이름 (`bunx changeset`, `bun run changeset:version`, release.yml 동작).
- Produces: 없음 (문서).

- [ ] **Step 1: AGENTS.md — Common commands 에 changeset 항목 추가**

`AGENTS.md` 의 "## Common commands" 코드블록(``bun test`` 줄 뒤)에 추가:

```
bunx changeset          # user-facing 변경의 버전 의도 선언 (patch/minor/major)
bun run changeset:version  # (로컬 수동 시) 버전 범프 + CHANGELOG + plugin.json sync
```

그리고 코드블록 아래에 한 줄 설명 문단 추가:

```
**Release (changesets).** user-facing 변경이 있는 PR 은 `bunx changeset` 으로 의도를 선언한다(`.changeset/*.md` 커밋). main 병합 시 `.github/workflows/release.yml` 의 `changesets/action` 이 pending changeset 을 모아 "Version Packages" PR 을 자동으로 열고, 그 PR 이 `package.json` + `.claude-plugin/plugin.json` 범프와 `CHANGELOG.md` 갱신을 담는다 (버전 sync 는 `bun run changeset:version` 안의 `scripts/sync-plugin-version.ts` 가 처리). 그 Version PR 을 병합하면 릴리스가 확정된다. npm publish/태그는 자동화 대상이 아니다 (태그는 수동).
```

- [ ] **Step 2: AGENTS.md — Change checklist 에 한 줄 추가**

`AGENTS.md` 의 "## Change checklist" 목록 끝에 항목 추가:

```
9. If the change is user-facing (tools / commands / hooks / config surface), declare the version intent with `bunx changeset` (patch / minor / major) so the release workflow can bump on merge. Tooling-only chores need no changeset.
```

(기존 마지막 번호가 8 이면 9, 다르면 다음 번호로.)

- [ ] **Step 3: FEATURES.md — 릴리스 플로우 짧게 (한국어)**

`FEATURES.md` 의 적당한 말미(Quick start 이후 또는 개발 섹션)에 짧은 소절 추가:

```
## 릴리스 (changesets)

user-facing 변경이 있는 PR 은 `bunx changeset` 으로 버전 의도(patch/minor/major)를 선언한다. main 에 병합되면 GitHub Action 이 "Version Packages" PR 을 자동으로 열어 `package.json` + `.claude-plugin/plugin.json` 버전 범프와 `CHANGELOG.md` 갱신을 모아준다. 그 PR 을 병합하면 새 버전이 확정된다. (npm publish 는 자동화하지 않는다.)
```

- [ ] **Step 4: 문서 게이트 (biome 가 md 포맷)**

Run: `bun run check`
Expected: 통과. biome 가 md 를 포맷하면 `bun run fix` 후 재확인.

- [ ] **Step 5: Commit**

```bash
git add AGENTS.md FEATURES.md
git commit -m "docs(release): changesets 릴리스 플로우 문서화 (AGENTS + FEATURES)"
```

---

### Task 5: 마무리 — 전체 게이트 + PR 생성

**Files:** 없음 (검증 + PR).

- [ ] **Step 1: 최종 게이트 3종**

Run: `bun run check && bun run typecheck && bun run test`
Expected: check 통과 / typecheck 통과 / test 246 pass, 0 fail.

- [ ] **Step 2: 최종 상태 확인 — 버전/CHANGELOG 미변경**

Run: `grep '"version"' package.json .claude-plugin/plugin.json && ls CHANGELOG.md 2>&1`
Expected: 두 version 모두 `0.10.0` (설정 PR 은 버전 안 올림), `CHANGELOG.md` 없음 (`ls: CHANGELOG.md: No such file or directory`).

- [ ] **Step 3: diff 스코프 확인**

Run: `git diff --stat main...HEAD`
Expected 변경 파일: `docs/superpowers/specs/...`, `docs/superpowers/plans/...`, `scripts/sync-plugin-version.ts`, `package.json`, `bun.lock`, `.changeset/config.json`, `.changeset/README.md`, `.github/workflows/release.yml`, `AGENTS.md`, `FEATURES.md`. (런타임 `src/**` 무변경.)

- [ ] **Step 4: push + PR 생성**

```bash
git push -u origin feat/changesets-release-flow
gh pr create --base main --head feat/changesets-release-flow \
  --title "feat(release): changesets 기반 버전 자동화 (Version PR + CHANGELOG + plugin.json sync)" \
  --body "$(cat <<'EOF'
## 요약
changesets 로 버전 범프 + CHANGELOG 생성을 자동화한다 (접근 B). npm publish 는 범위 밖.

- `@changesets/cli`(devDep) + `.changeset/config.json`
- `scripts/sync-plugin-version.ts` — package.json → plugin.json 버전 lockstep
- `bun run changeset:version` = `changeset version` + plugin.json sync
- `.github/workflows/release.yml` — main push 시 "Version Packages" PR 자동 생성 (새 브랜치라 main PR-only 보호와 무충돌, publish 없음)
- AGENTS.md / FEATURES.md 릴리스 플로우 문서화
- 설계/계획: `docs/superpowers/{specs,plans}/2026-07-15-changesets-*`

이 PR 자체는 버전을 올리지 않음 (tooling chore). 첫 자동 범프는 다음 기능 PR 부터.

## 게이트
- \`bun run check\` / \`typecheck\` / \`test\` (246 pass) 통과

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```
Expected: PR URL 출력.

---

## Self-Review

**Spec coverage:**
- changesets CLI + config → Task 2 ✓
- plugin.json sync 스크립트 → Task 1 ✓
- `changeset:version` 배선 → Task 2 Step 4 ✓
- release.yml (자동 Version PR, publish 없음, Bun install 명시, version 오버라이드) → Task 3 ✓
- Bun↔action 마찰 처리(명시적 frozen install + version-only 오버라이드) → Task 3 Step 1 주석 ✓
- 로컬 파이프라인 검증(더미→version→restore) → Task 2 Step 6 ✓
- 유닛테스트 생략 + 수동검증 → Task 1 Step 2–3 ✓
- 설정 PR 버전 안 올림 → Task 5 Step 2 ✓
- 문서(AGENTS + FEATURES) → Task 4 ✓
- npm publish 완전 배제 → Global Constraints + Task 3 (publish 입력 미지정) ✓

**Placeholder scan:** 모든 step 에 실제 코드/명령/기대출력 포함. TBD/TODO 없음.

**Type consistency:** `scripts/sync-plugin-version.ts` 이름/경로가 Task 1·2·3·문서에서 일관. `changeset:version` 스크립트명 일관. 패키지명 `@minjun0219/rocky` 일관(Task 2 더미 changeset).
