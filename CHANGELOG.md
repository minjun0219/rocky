# @minjun0219/rocky

## 0.12.0

### Minor Changes

- [#91](https://github.com/minjun0219/rocky/pull/91) [`246243c`](https://github.com/minjun0219/rocky/commit/246243c3104ce96c4bd023aacb6d7f0e255bfcca) Thanks [@minjun0219](https://github.com/minjun0219)! - 소울이 사용자를 부르는 호칭(`callsign`) 설정 지원 — `rocky.json` 최상위 `callsign` 키(한 줄, 1~40자, project > user)를 `SessionStart` 훅이 소울 컨텍스트에 함께 주입하고, `/rocky:soul <name>` 세팅 플로우가 호칭을 물어보며, 새 `call` 서브커맨드로 호칭만 조회/변경/제거할 수 있다.

### Patch Changes

- [#95](https://github.com/minjun0219/rocky/pull/95) [`cf7dc50`](https://github.com/minjun0219/rocky/commit/cf7dc500868e21bd3b476c0e448d1bea47c89a47) Thanks [@minjun0219](https://github.com/minjun0219)! - docs(finish): PR·커밋 제목 장황화 금지 규칙 추가 — 제목에 핵심 하나를 넘는 나열·부연을 넣지 않는다(요약부 대략 50자 초과 금지), 밀려난 세부는 본문으로. `/finish` 커맨드와 AGENTS.md / FEATURES.md 의 출력 규칙에 금지형으로 반영 (본문 상세함은 기존 유지).

- [#94](https://github.com/minjun0219/rocky/pull/94) [`d4c127c`](https://github.com/minjun0219/rocky/commit/d4c127ca6a1f75660f897e00512d8be0f9ea79d1) Thanks [@minjun0219](https://github.com/minjun0219)! - rocky 소울 시그니처 다듬기 — 이해 선언을 "이해해." → "Understand!" / "이해 못 해." → "이해 못 함." 으로 바꾸고, "Amaze!" 는 항상 느낌표 종결임을 명시하고, 질문은 항상 "질문." 으로 종결하도록("커밋할까? 질문.") 규칙을 뒤집음.

## 0.11.0

### Minor Changes

- a881fb8: changesets 기반 버전 자동화 도입 — main 병합 시 `changesets/action` 이 "Version Packages" PR 을 자동으로 열어 `package.json` + `.claude-plugin/plugin.json` 버전 범프와 `CHANGELOG.md` 를 관리한다. 두 버전 파일은 `scripts/sync-plugin-version.ts` 로 lockstep 유지. (npm publish 는 자동화 대상 아님)
