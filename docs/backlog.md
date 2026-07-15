# Backlog

보류 중이거나 아직 착수하지 않은 rocky 항목의 축적처. 항목이 출하되면 여기서 지우고 git 히스토리에 맡긴다. 도메인 재추가의 자세한 절차는 [`AGENTS.md`](../AGENTS.md) 의 *Reintroduction strategy* 절.

## 도메인 재추가 후보 (구 ROADMAP)

각 도메인은 별도 PR. 재추가 시점에 다음 둘 중 하나의 shape 를 정한다:

- **(a) plugin 직접 합류** — 도메인 코드를 `src/core/` 에 두고, 전체 표면 서버 진입점 (`src/index.ts`) 의 surface 에 도구를 등록한다. 별도 CLI 진입점은 만들지 않음.
- **(b) 별도 CLI 진입점 분리** — `openapi-mcp` 옆에 `bin/<domain>-mcp` + `src/<domain>.ts` standalone 진입점을 추가한다. 도메인이 plugin 외 host (Cursor / Continue / Claude Desktop) 에서도 자주 쓰일 때.

결정 기준은 활용 패턴 — host 독립성이 높으면 (b), plugin 안에서만 쓰이면 (a).

| 도메인 | archive 위치 (v0.2 경로) | 후보 shape | 비고 |
| --- | --- | --- | --- |
| `mysql` (read-only inspection) | `lib/mysql-*.ts` + 5 tool (`mysql_*`) + `skills/mysql-query/` | (b) 별도 CLI 진입점 강력 후보 — DB inspector 는 host 독립적. | `mysql2` prod-dep 부활. `rocky.json` 의 `mysql.connections` 키 + `passwordEnv` / `dsnEnv` 정책. |
| `spec-pact` (DRAFT / VERIFY / DRIFT-CHECK / AMEND lifecycle) | `lib/spec-pact-fragments.ts` + 1 tool (`spec_pact_fragment`) + `skills/spec-pact/` + `agents/grace.md` | (a) plugin 합류. fragment loader 자체는 가벼움. | INDEX / SPEC 파일 lifecycle 은 `grace` sub-agent 책임. |
| `pr-review-watch` (polling-only, worklog-backed) | `lib/pr-watch.ts` + 6 tool (`pr_*`) + `skills/pr-review-watch/` + `agents/mindy.md` | (a) plugin 합류. 외부 GitHub MCP 의존. | worklog 재추가 완료 — worklog-backed 이벤트 로그를 얹을 수 있다. 다만 Claude Code 빌트인 `/autofix-pr` 가 같은 자리를 차지해 우선순위 낮음. |

## docs-lifecycle 스킬 (보류, 2026-07-15)

에이전트 생성 작업 문서(설계/조사/플랜)가 대상 레포의 공식 문서와 섞이지 않도록 안내하는 **repo 문서 3단계 lifecycle (draft → accepted → archive)** 번들 스킬.

- **경위**: PR [#85](https://github.com/minjun0219/rocky/pull/85) 에서 `skills/docs-lifecycle/SKILL.md` 로 구현 완료(봇 리뷰 5건 반영)했으나, 오너 판단으로 스킬 출하는 보류 — rocky 에 적용하기엔 과함. 당시 구현 diff 는 PR #85 의 force-push 이전 타임라인에 남아 있고, 아래 완성본이 그 최종본에 후속 오너 피드백까지 반영한 결정판이다.
- **부활 방법**: 아래 완성본을 `skills/docs-lifecycle/SKILL.md` 로 옮기고 문서 동기화 4곳 — `FEATURES.md` 스킬 블록 / `README.md` 스킬 bullet / `AGENTS.md` Layout + *Project in one line* / `.claude-plugin/plugin.json` description — 을 갱신하면 된다 (스킬은 자동 발견, `.claude-plugin/plugin.json` 에 skills 필드 없음).
- **수용 기준 (원 요청)**: ① 3단계 분류·라우팅·승격·위생 규칙을 표/목록으로 정리 ② repo 구조 하드코딩 금지 (디렉터리 이름은 관례 기본값 + 대상 repo 기존 구조 존중) ③ 사용자 확인 없는 승격 경로 금지.
- **반영된 오너 피드백 (PR #85 이후)**: ① 디렉토리 사전 스캐폴딩 제거 — 첫 문서 작성 직전 `mkdir -p` 지연 생성 ② draft 는 기본값일 뿐 강제 아님 — 이미 사용자와 합의된 내용은 accepted 직행 허용 (게이트의 본질은 사용자 확인이지 draft 경유가 아님) ③ ignore 규칙 위치 자유 — 루트 `.gitignore` (`docs/.draft/`) 또는 `docs/.gitignore` (`.draft/`), 레포 기존 관례 우선.

### SKILL.md 완성본

````markdown
---
name: docs-lifecycle
description: Use when creating, saving, or organizing any markdown working document in a repository — a design doc, implementation plan, investigation note, ADR, spec, or runbook — and deciding where in docs/ it belongs. Routes new agent-generated docs to a git-ignored docs/.draft/ area by default, gates promotion into accepted docs (docs/spec/, docs/adr/, docs/runbook/) behind explicit user confirmation, retires superseded docs to docs/archive/, and keeps accepted docs free of references into the draft area. Includes a setup snippet for the draft-area ignore rule.
---

# Repo Docs Lifecycle

Keep agent-generated working documents from mixing into a repo's official docs by running every
repo document through a **three-stage lifecycle: draft → accepted → archive**. Working docs are
disposable scratch output; accepted docs are curated engineering knowledge; archived docs are
retired knowledge worth keeping. This applies to **whatever repo you are working in** — it is a
convention you carry, not a structure owned by any one repo.

**Core rule — new working docs go to the repo's draft area (default: `docs/.draft/`).** Whenever
you generate a design doc, implementation plan, or investigation note, route it to the draft area —
`docs/.draft/` by default, or the repo's own working-docs convention when one already exists (see
*Respect the target repo's structure*). Do not write a new document directly into an accepted area
(`docs/spec/`, `docs/adr/`, …) **unless the user has already agreed to that document going there** —
accepted status is granted by the user; the draft detour is the default route to that grant, not
the only one.

## The three stages

| Stage | Default location | What lives there | Source control |
| :-- | :-- | :-- | :-- |
| Draft | `docs/.draft/` (subdirs like `spec/`, `adr/`, `runbook/`, `investigation/` — created on demand) | agent/tool-generated working docs: designs, plans, investigations, ADR sketches | ignored — rule in the root `.gitignore` (`docs/.draft/`) or `docs/.gitignore` (`.draft/`) |
| Accepted | `docs/adr/`, `docs/spec/`, `docs/runbook/`, `docs/architecture.md`, … | official engineering knowledge the team relies on | committed |
| Archive | `docs/archive/` | superseded docs no longer active reference but worth preserving | committed |

## Routing new docs

| You are writing… | Default destination |
| :-- | :-- |
| a feature/API design or spec draft | `docs/.draft/spec/` |
| an architecture-decision sketch | `docs/.draft/adr/` |
| an operational procedure draft | `docs/.draft/runbook/` |
| an investigation / debugging / research note, an implementation plan | `docs/.draft/investigation/` |

- Create directories lazily — `mkdir -p` the exact path right before writing the first doc there;
  never scaffold the whole tree up front.
- Content the user has already agreed on may skip the draft stage and go straight to its accepted
  path — that agreement is exactly the confirmation the promotion gate exists to collect.
- Prefer date-prefixed filenames (`2026-07-15-<slug>.md`) so drafts sort chronologically and never
  collide.
- Drafts need no ceremony: rewrite, split, or delete them freely — no user confirmation required
  inside the draft area.

## Respect the target repo's structure

The directory names above are **conventional defaults, not mandates**.

- Before creating anything, look at the repo's existing docs tree (`ls docs/`, existing ADR/spec
  directories, conventions in CONTRIBUTING / AGENTS.md / CLAUDE.md) and map the three stages onto
  what already exists.
- If the repo already has a working-docs convention (a `drafts/`, `wip/`, `notes/` area, or a
  documented plans directory), use that as the draft stage instead of introducing `docs/.draft/`.
- **Never** restructure a repo's existing accepted docs to fit this skill's layout.

## Promotion gate (draft → accepted)

1. Identify the candidate: a spec worth reusing → `docs/spec/`; a settled architecture decision →
   a new ADR in `docs/adr/`; a proven operational procedure → `docs/runbook/`.
2. Propose it to the user — target path, and what will be rewritten or stripped. Ask an
   explicit question, and **wait for confirmation**.
3. Only after explicit confirmation: rewrite the doc for permanence — remove session-specific
   context and any links into `docs/.draft/` — then write it to the accepted path.
4. Delete or keep the source draft per the user's preference.

**Never auto-promote.** There is no path where promotion runs without user confirmation —
"the user seemed to imply it" does not count. (Content the user already agreed on, written
straight to its accepted path, is not auto-promotion — the confirmation happened up front.)

## Demotion / archiving (accepted → archive)

When an accepted doc is superseded, propose moving it (`git mv`) to `docs/archive/` — also
user-confirmed. **Never** silently delete an accepted doc.

## Hygiene rules

- Accepted and archived docs **must not reference specific files under `docs/.draft/`** — drafts
  are git-ignored and can vanish at any time. If an accepted doc wants to cite a draft, that is a
  promotion signal: promote the draft first, then cite the accepted copy.
- Drafts may reference anything, including other drafts.
- Because the draft area is git-ignored, never rely on it as cross-session or cross-machine
  knowledge storage — anything that must persist belongs in an accepted doc (after the gate).

## Setup helper

The draft area needs no scaffolding — directories are created on demand. The only setup is the
ignore rule, and adding it is a repo modification: do it **only when the user asks for it or
confirms your offer**. Follow the repo's existing ignore style — either location works:

```bash
echo 'docs/.draft/' >> .gitignore     # root .gitignore
# or
echo '.draft/' >> docs/.gitignore     # docs-local .gitignore (created if missing)
```

Before running it: check whether an existing rule already covers the draft area (don't append a
duplicate), and if the target file exists without a trailing newline, add one first so the rule
lands on its own line.

## Guardrails

- **Never** write a new working doc directly into an accepted area unless the user has already
  agreed to it landing there.
- **Never** promote, archive, add the ignore rule, or restructure `docs/` without explicit user
  confirmation.
- **Never** impose this skill's default paths over an existing repo convention — defaults yield to
  the target repo's structure.
````

## 비전 메모 (구 ROADMAP)

작업 컨텍스트를 들고 코드까지 굴리는 에이전트 오케스트레이션 toolkit. 세 갈래 방향 — 도메인이 다시 모인 뒤 본격 추진:

1. **업무 / 코딩 파트너로 단독 충분한 토대** — agent / skill / command / MCP / tool 다섯 종 primitive 을 적재적소에 섞어 쓰는 composition foundation.
2. **외부 primary 와의 시너지** — OmO Sisyphus / Superpowers 같은 외부 primary agent 가 동일 host 에 있을 때 description-driven routing 이 깨지지 않고 자연스럽게 위임이 흐른다.
3. **회사 맞춤 토킷의 base** — plugin (현재 형태) + library (`src/core/` exports) 두 형태로 패키징해 의존성으로 가져다 쓰는 토대.
