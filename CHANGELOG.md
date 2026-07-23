# @minjun0219/rocky

## 0.14.0

### Minor Changes

- [#100](https://github.com/minjun0219/rocky/pull/100) [`65aa2ea`](https://github.com/minjun0219/rocky/commit/65aa2ea25b6376056c163ec897215bf5d11ec1e8) Thanks [@minjun0219](https://github.com/minjun0219)! - rocky-todo 공유 todo/스크래치패드 데몬 추가 — 시스템 유일 상주 데몬(127.0.0.1:8636, bun:sqlite)이 계층/섹션/보드 todo + 스티커 메모 + 전 변경 히스토리(아카이브만, 삭제 없음)를 들고, 에이전트는 `/mcp`(streamable HTTP, `todo_list`/`todo_write`/`todo_status`/`note_list`/`note_write`) 또는 `rocky-todo` CLI(온디맨드 자동 기동, `daemon install` launchd 등록)로, 호출자는 React 웹 UI(SSE 실시간, 처리중 actor 뱃지)로 같은 보드를 본다. 역방향(사람→에이전트)은 `UserPromptSubmit` 훅이 데몬의 `/api/changes` 피드를 세션별 커서로 읽어 호출자의 웹 편집분만 자동 주입한다 (`todo.watch`/`ROCKY_TODO_WATCH` 토글, fail-open). 노출은 `todo.expose` 채널(`lan` 내부망 0.0.0.0 / `tailscale-serve` 테일넷 serve, 배열 조합 또는 단일 문자열, 기본 없음 = 이 머신만 — tailscale 채널이 없으면 tailscale 을 일절 안 건드림; 수동 `rocky-todo tailscale on|off|status`). 전체 기능은 마스터 스위치 `todo.enabled`(기본 off — 상주 데몬 opt-in, env `ROCKY_TODO_ENABLED` 우선)로 게이트된다. `rocky.json` 에 `todo.enabled`/`todo.port`/`todo.dir`/`todo.expose`/`todo.watch` 키, env `ROCKY_TODO_PORT`/`ROCKY_TODO_DIR`/`ROCKY_TODO_ACTOR`/`ROCKY_TODO_WATCH`/`ROCKY_TODO_EXPOSE`, 번들 스킬 `todo`, `docs/rocky-todo.md` 추가. 기존 full-surface MCP 표면(`src/index.ts`)은 불변.

## 0.13.0

### Minor Changes

- [#96](https://github.com/minjun0219/rocky/pull/96) [`e30f9d6`](https://github.com/minjun0219/rocky/commit/e30f9d6688273d29bb59f76b13c2cdda0b567efc) Thanks [@minjun0219](https://github.com/minjun0219)! - feat(statusline): 번들 statusline 추가 — statusLine 템플릿 3종(`statusline/<name>.sh`: `duo` 2줄 기본 / `mini` 1줄 / `full` 3줄+세션 비용·변경량·경과)을 플러그인이 소유하고, `/rocky:statusline` 커맨드가 고른 템플릿을 안정 경로 `~/.config/rocky/statusline.sh` 로 설치(user `settings.json` 의 `statusLine` 1회 지정, 초안 확인 + 타임스탬프 백업). 새 `SessionStart` 훅(`src/hooks/sync-statusline.ts`)이 설치본 헤더의 템플릿 마커를 읽어 플러그인 업데이트를 같은 템플릿에서 자동 전파한다 (미설치 시 no-op, fail-open). MCP tool 표면 변화 없음.

- [#99](https://github.com/minjun0219/rocky/pull/99) [`d16592a`](https://github.com/minjun0219/rocky/commit/d16592a9fa30e6b0e0d1512dae0c0b1a25777514) Thanks [@minjun0219](https://github.com/minjun0219)! - statusline full 템플릿 고도화 — git dirty(`*`)·ahead/behind(`↑↓`) 세그먼트, ctx/left 임계값 경고색(안전 dim / 70·30 경고 / 90·10 위험), 경과 5분 이상일 때 시간당 비용(`($N.N/h)`) 표시. 템플릿 3종 표시 내용 문서 `docs/statusline.md` 신설.

- [#98](https://github.com/minjun0219/rocky/pull/98) [`f488c79`](https://github.com/minjun0219/rocky/commit/f488c79fde2665c65c586ef94a18d56006f4a121) Thanks [@minjun0219](https://github.com/minjun0219)! - todoist 번들 스킬 추가 — 세션에 연결된 Todoist MCP 로 현재 레포의 작업 목록을 파악(다음 작업 제안: Todoist + git + worklog 교차)·등록(컨벤션 + 차등 확인 게이트)·마감하는 Claude Code 전용 스킬. rocky 는 Todoist 접근을 배포하지 않으며 도구 부재 시 중단·안내한다.

## 0.12.0

### Minor Changes

- [#91](https://github.com/minjun0219/rocky/pull/91) [`246243c`](https://github.com/minjun0219/rocky/commit/246243c3104ce96c4bd023aacb6d7f0e255bfcca) Thanks [@minjun0219](https://github.com/minjun0219)! - 소울이 사용자를 부르는 호칭(`callsign`) 설정 지원 — `rocky.json` 최상위 `callsign` 키(한 줄, 1~40자, project > user)를 `SessionStart` 훅이 소울 컨텍스트에 함께 주입하고, `/rocky:soul <name>` 세팅 플로우가 호칭을 물어보며, 새 `call` 서브커맨드로 호칭만 조회/변경/제거할 수 있다.

### Patch Changes

- [#95](https://github.com/minjun0219/rocky/pull/95) [`cf7dc50`](https://github.com/minjun0219/rocky/commit/cf7dc500868e21bd3b476c0e448d1bea47c89a47) Thanks [@minjun0219](https://github.com/minjun0219)! - docs(finish): PR·커밋 제목 장황화 금지 규칙 추가 — 제목에 핵심 하나를 넘는 나열·부연을 넣지 않는다(요약부 대략 50자 초과 금지), 밀려난 세부는 본문으로. `/finish` 커맨드와 AGENTS.md / FEATURES.md 의 출력 규칙에 금지형으로 반영 (본문 상세함은 기존 유지).

- [#94](https://github.com/minjun0219/rocky/pull/94) [`d4c127c`](https://github.com/minjun0219/rocky/commit/d4c127ca6a1f75660f897e00512d8be0f9ea79d1) Thanks [@minjun0219](https://github.com/minjun0219)! - rocky 소울 시그니처 다듬기 — 이해 선언을 "이해해." → "Understand!" / "이해 못 해." → "이해 못 함." 으로 바꾸고, "Amaze!" 는 항상 느낌표 종결임을 명시하고, 질문은 항상 "질문." 으로 종결하도록("커밋할까? 질문.") 규칙을 뒤집음.

## 0.11.0

### Minor Changes

- a881fb8: changesets 기반 버전 자동화 도입 — main 병합 시 `changesets/action` 이 "Version Packages" PR 을 자동으로 열어 `package.json` + `.claude-plugin/plugin.json` 버전 범프와 `CHANGELOG.md` 를 관리한다. 두 버전 파일은 `scripts/sync-plugin-version.ts` 로 lockstep 유지. (npm publish 는 자동화 대상 아님)
