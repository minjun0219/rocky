# 세션 띄우기 — 보드에서 세션을

> rocky 를 **고치는** 에이전트용 개발 문서.

## 규칙

- **rc 가 켜진 기기(`state.rc_control`)는 rc 서버로** — 워크트리 `<boards.path>/.claude/worktrees/todo-<n>`(없으면 데몬이
  `git worktree add -b worktree-todo-<n> … origin/<기본 브랜치>`) 안에서 `claude rc --spawn session --name "<보드>-<n>: <요약>"`
  (`rc::handoff_server_name` · `handoff_server_argv`)을 띄우고, 그 서버의 자식 세션이 받은편지함을 등록하면(`rc::handoff_session` —
  소켓 pid 의 부모가 그 서버이고 서버를 띄운 뒤의 등록) 그 세션 앞으로 핸드오프를 만들고 깨운다. 할 일당 서버 하나(그 워크트리에
  rc 서버가 있으면 409), 내리기는 사람(`rocky rc stop <할 일>` — [rc-servers](rc-servers.md) 의 핸드오프 서버). 띄우기 전에 현황 · 자격(로그아웃이면 409) · 대기 중 핸드오프를 다시 본다. 있는 폴더는
  `rev-parse --show-toplevel` 이 그 폴더일 때만 쓴다(일반 폴더면 세션의 git 이 메인 레포를 잡는다). 서버가 뜬 뒤의 실패(세션을 60초
  안에 못 찾음 · 핸드오프 기록 실패)는 **서버를 남기고 그 이름 · pid 를 알린다**. 서버 · 로그 라벨은 `rc::handoff_log_label`(보드 key
  의 `/` · `.` 를 `_` 로 — key 는 원격에서 바꿀 수 있다). **rc 서버에는 `kill_on_drop` 을 걸지 않는다** — 새 프로세스 그룹으로 띄우고
  놓는다([rc-servers](rc-servers.md)).
- rc 가 꺼진 기기는 `boards.path` 에서 `claude --bg --worktree todo-<n>` 으로 띄우고 응답에 `warning` 을 단다(로그인 세션 밖이라
  ssh · 자격이 끊길 수 있다). **어느 쪽이든 그 워크트리에 살아 있는 세션이 있으면 띄우지 않고 그 세션을 쓴다** — 받은편지함이
  있으면 깨운다.
- 이중 기동은 캐시 없는 세션 목록과 `RecentSpawns` 예약이 막는다 — 예약은 확인과 잡기를 **한 락 안에서**(`try_reserve`, 앞의
  `is_recent` 와 잡기 사이에 세션 목록 await 가 있다), 요청이 끝날 때까지 진행 중으로 잡고 끝난 뒤 60초(창 안의 재요청은 409).
  확실히 아무것도 안 띄웠을 때만 되돌린다(`Reservation::release`). 경로가 키라 자기 예약만 고친다(토큰).
- `boards.path` 는 절대경로여야 하고 canonicalize 한다.
- `--bg` 실행은 비동기, 30초 timeout, `kill_on_drop`(그 갈래만). `--permission-mode` 는 넘기지 않는다(두 갈래 다).
- 프로세스를 띄우는 동작이라 **로컬 요청 전용**([security](security.md)).

## 코드

| 무엇 | 어디 |
| --- | --- |
| 띄우기·예약(`--bg`) | `crates/rockyd/src/spawnctl.rs` |
| rc 갈래 — 워크트리 · 서버 · 세션 기다리기 | `crates/rockyd/src/rc/handoff.rs`, 판정 `crates/rocky-core/src/rc.rs`(핸드오프 서버 절) |
| 세션 목록 | `crates/rockyd/src/sessions_exec.rs`, 판정 `crates/rocky-core/src/sessions.rs` |
| 라우트 | `crates/rockyd/src/server.rs` |

테스트: `crates/rockyd/tests/it/{spawnctl_test,server_spawn_test,server_spawn_rc_test,sessions_swr_test}.rs`.
