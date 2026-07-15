# changesets 기반 릴리스 플로우 도입 — 설계

- 날짜: 2026-07-15
- 상태: 구현됨 (PR #88)
- 범위: 접근 **B** — GitHub Action 자동 "Version Packages" PR + CHANGELOG 자동 생성. **npm publish 는 포함 안 함** (별도 후속 PR).

## 배경 / 목적

rocky 는 버전이 두 곳(`package.json`, `.claude-plugin/plugin.json`)에 lockstep 으로 존재하고, 지금까지 버전 범프는 손으로 두 파일을 고쳐 왔다 (오늘 `0.9.0 → 0.10.0` 이 그 방식). 릴리스 태깅/CHANGELOG 도 없다.

목표는 **버전 범프 + CHANGELOG 생성을 changesets 로 자동화**하되:
- 두 버전 파일을 항상 일치시키고,
- main 이 PR-only 로 보호된 현 상태와 충돌하지 않으며,
- npm publish 자동화는 (AGENTS.md 스코프상 별도 PR 항목이므로) 이번엔 건드리지 않고 깔끔한 seam 만 남긴다.

## 비목표 (out of scope)

- npm publish 자동화 (후속 접근 C 에서 `NPM_TOKEN` + publish 스텝 추가).
- ~~git 태그 자동 생성~~ → **후속으로 추가됨**: Version PR 병합으로 버전이 오르면 release.yml 의 독립 스텝(`scripts/release-github.ts`)이 `v<version>` 태그 + GitHub Release(노트=CHANGELOG 섹션)를 멱등하게 생성한다. npm 은 여전히 손대지 않는다 (GitHub Release ≠ npm publish).
- `@changesets/changelog-github` 등 향상된 changelog 생성기 (기본 생성기로 시작, 나중에 교체 가능).
- 이 설정 PR 로 버전을 올리는 것 (tooling chore 라 버전 범프 changeset 을 넣지 않음).

## 컴포넌트

| 파일 | 역할 |
|---|---|
| `@changesets/cli` (devDep) | changesets 코어. CHANGELOG 생성기는 기본 `@changesets/cli/changelog` (dep 최소화, 후일 교체 가능) |
| `.changeset/config.json` | `baseBranch: "main"`, `commit: false`, `access: "restricted"` (publish 안 하니 사실상 무의미), 기본 changelog, 단일 패키지 |
| `.changeset/README.md` | `changeset init` 이 생성하는 안내 파일 |
| `scripts/sync-plugin-version.ts` | `package.json` 의 `version` 을 읽어 `.claude-plugin/plugin.json` 의 `version` 에 반영. 포맷(들여쓰기/개행) 보존, idempotent. **lockstep 해결의 핵심** |
| `package.json` scripts | `"changeset:version": "changeset version && bun scripts/sync-plugin-version.ts"` 추가 (publish 스크립트 없음) |
| `.github/workflows/release.yml` | `on: push: main` → setup-bun + `bun install --frozen-lockfile` + `changesets/action@v1` (`version: bun run changeset:version`, publish 입력 비움). `permissions: contents: write, pull-requests: write` |

### 핵심 동작

- changesets/action 은 versioning 시 우리가 지정한 `bun run changeset:version` 을 호출한다. 그 스크립트 안에서 `changeset version`(package.json + CHANGELOG 갱신) 다음에 `bun scripts/sync-plugin-version.ts`(plugin.json 갱신)가 이어 돌아 **두 버전이 항상 일치**한다.
- Action 은 `changeset-release/main` **새 브랜치**를 만들어 Version PR 을 연다 → main PR-only 보호와 충돌 없음 (새 브랜치는 보호 대상 아님).
- publish 입력을 비워서 publish 경로 자체를 타지 않는다 (release 생성/태그/npm 없음).

## 릴리스 플로우 (도입 후 상시)

```
1. 기능 PR 작업 → user-facing 변경이면 `bunx changeset` 로 의도 선언
   (patch/minor/major + 한 줄 요약 → .changeset/<name>.md 커밋)
2. PR 병합 → release.yml 감지 → "Version Packages" PR 자동 생성/갱신
   (package.json + plugin.json 범프 + CHANGELOG.md 갱신 + .changeset/*.md 소비)
3. Version PR 병합 → 다음 버전 확정 (예: 0.10.0 → 0.11.0)
4. 태그 + GitHub Release → release.yml 의 `scripts/release-github.ts` 스텝이 자동 생성 (`v<version>` + CHANGELOG 노트, 멱등)
```

## Bun ↔ changesets/action 마찰 (알려진 리스크, 선제 처리)

- `changesets/action@v1` 은 lockfile 로 패키지 매니저를 감지한다. bun 지원이 과거 들쭉날쭉했으므로:
  - **install 은 워크플로에서 명시적으로 `bun install --frozen-lockfile` 스텝으로 먼저 수행**한다.
  - action 에는 `version` 커맨드만 오버라이드해서 넘긴다 (action 이 자체 install 을 재시도하지 않도록).
  - publish 입력을 비워 publish 경로를 아예 타지 않는다.
- `@changesets/cli` 는 devDep → install 후 `bunx changeset` / `bun run changeset:version` 으로 접근 가능.

## 테스트 / 검증

`scripts/sync-plugin-version.ts` 유닛테스트를 넣으려면 CI 의 `test: "bun test ./src"` glob 을 건드려야 한다(스크립트가 `src/` 밖). 이번엔 **유닛테스트 생략**하고 다음으로 커버:
1. 구현 중 수동 검증 (아래).
2. Version PR diff 가 머지 전 사람 리뷰를 거친다 (plugin.json 변경이 눈에 보임).
3. 스크립트가 ~15줄로 작아 리스크가 낮다.

구현 중 실제로 돌려 확인할 것:
1. `bunx changeset status` — config 유효성.
2. 더미 changeset 하나 만들고 `bun run changeset:version` 로컬 실행 → `package.json` + `.claude-plugin/plugin.json` 둘 다 바뀌고 `CHANGELOG.md` 생성되는지 확인 → **`git restore` 로 되돌려** 설정 PR 에는 넣지 않음.
3. sync 스크립트 idempotent 확인 (두 번 돌려도 동일 결과).
4. 게이트 3종(`bun run check` / `typecheck` / `test`) 통과.

## 문서 동기화

- `AGENTS.md`: Common commands 에 changeset 흐름 한 줄 + Change checklist 에 "user-facing 변경은 `bunx changeset` 로 의도 선언" 한 줄.
- `FEATURES.md`: 릴리스 플로우 짧게 (한국어, 사람 대상).

## 오픈 이슈 / 후속

- 접근 C (npm publish + 자동 태그): `NPM_TOKEN` secret + `files`/`publishConfig` 점검 + release.yml publish 입력 채우기. 별도 PR.
- ~~changelog 를 `@changesets/changelog-github` 로 교체 (PR 링크 포함)~~ → **적용됨**: `.changeset/config.json` 의 `changelog` 를 `["@changesets/changelog-github", { "repo": "minjun0219/rocky" }]` 로 교체. 이후 CHANGELOG/릴리스 노트에 커밋·PR·작성자 링크가 자동으로 붙는다 (커밋된 changeset + 연결된 PR 기준). CI 는 `changesets/action` 이 넘기는 `GITHUB_TOKEN` 으로 동작; 로컬에서 `bun run changeset:version` 을 직접 돌릴 땐 `GITHUB_TOKEN` (예: `$(gh auth token)`) 이 필요하다.
