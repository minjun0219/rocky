---
"@minjun0219/rocky": minor
---

rocky-todo 를 흡수한다 (hail-mary D-046). Rust 데몬 `rocky-todod` · CLI `rocky-todo` · 코어가
`crates/` 로 들어오고(히스토리 보존, `--allow-unrelated-histories`), 플러그인은 하나 `rocky` 가
된다 — MCP 서버는 데몬 http(`rocky`, 보드 5 도구) + 임시 stdio(`worklog`, 4 도구), 훅은
SessionStart(데몬 기동) · UserPromptSubmit(보드 주입) · Stop(핸드오프 → 워크로그 기록),
커맨드에 `/rocky:next`, 스킬에 `board` 가 추가된다. 보드 도구 id 는
`mcp__plugin_rocky_rocky__todo_*`, worklog 는 당분간 `mcp__plugin_rocky_worklog__worklog_*`.

안 가져온 것: React 웹 UI · Tauri 앱 · `rocky-todo app` 서브커맨드 · TS 참조 구현 —
rocky-todo 히스토리에 남는다. GUI 는 Swift 또는 TUI 로 별도 결정. 이름(`rocky-todo` /
`rocky-todod` / 크레이트)은 아직 옛것이며 개명은 별도 PR.

릴리스 tarball 은 바이너리 둘만 담고, `bin/rocky-todo` 부트스트랩은 이 레포의 Release 에서
받는다. `package.json` · `plugin.json` · `Cargo.toml` · `Cargo.lock` 버전이 lockstep 이어야
한다(`ensure-daemon` 이 정확 일치로 구버전을 판정).

worklog 도 Rust 로 간다 — `worklog_*` 4 도구는 CLI 의 stdio MCP 서버(`rocky-todo mcp worklog`),
Stop 훅의 턴 기록은 `rocky-todo hook log-turn`. 저장 형식(JSONL)·경로·프로젝트 키
(`<basename>-<sha1[:8]>`)는 TS 판과 바이트 동일해 기존 앵커가 그대로 이어진다. 데몬이 아니라
CLI 인 이유는 워크로그가 프로젝트별인데 데몬은 호출자의 cwd 를 모르기 때문. 이로써 런타임 TS
가 사라지고 `package.json` 은 개발 도구(biome·changesets·릴리스 스크립트)만 남는다.

플러그인 표면은 `plugin/` 로 모이고 마켓플레이스 소스가 `./plugin` 이 된다 — 설치본에
`crates/`·`target/`·`node_modules` 가 더 이상 복사되지 않는다.
