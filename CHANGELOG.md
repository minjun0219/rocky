# @minjun0219/rocky

## 0.20.1

### Patch Changes

- [#120](https://github.com/minjun0219/rocky/pull/120) [`d75aee4`](https://github.com/minjun0219/rocky/commit/d75aee485a7fa36a34294348c67c57136fb4f97d) Thanks [@minjun0219](https://github.com/minjun0219)! - `/rocky:recall` 이 읽은 프로젝트와 다른 워크로그에 쓰지 못하게 막는다

  worklog 의 프로젝트 키는 MCP 서버 프로세스의 `process.cwd()` 에서 나오는데, rocky MCP 서버는
  프로젝트마다 따로 뜬다. 세션이 붙은 인스턴스가 실행 도중 갈아끼워지면 **한 번의 recall 안에서도
  읽는 프로젝트와 쓰는 프로젝트가 갈린다** — 실제로 A 를 읽고 B 에 digest 를 써서 B 의 watermark 를
  오염시키는 사고가 났다. 그 프로젝트의 digest 가 그것 하나뿐이면 이전 항목 전부가 다음 증분에서
  영구히 건너뛰어진다.

  커맨드 절차에 방어를 넣었다 — 1단계에서 `projectKey` 를 적어두고, append 직전 `worklog_status` 를
  다시 불러 대조한 뒤 불일치면 중단한다. 이미 오염된 경우의 복구 절차(정정 note + 전체 범위 재-digest)도
  예외 처리에 명시했다.

## 0.20.0

### Minor Changes

- [#118](https://github.com/minjun0219/rocky/pull/118) [`9f3e68d`](https://github.com/minjun0219/rocky/commit/9f3e68df2d61f87c9ff0ce9f53c76ebca9b29582) Thanks [@minjun0219](https://github.com/minjun0219)! - `/rocky:review-pr` 에서 재리뷰 폴링 루프를 걷어냈다. PR 에 지금 붙어 있는 리뷰를 **한 번에
  처리하고 끝난다** — 60초 간격 `Monitor` 폴링, 8분 수렴 판정, 라운드 상한, 진전 없음 감지가 모두
  사라졌다 (249줄 → 205줄, `Monitor` 도구 의존도 제거).

  원래 이 루프는 **Copilot 이 푸시마다 자동 재리뷰한다**는 전제 위에 있었다. 그 설정을 끄면서
  전제가 사라졌고, 그대로 두면 오지 않을 재리뷰를 매번 8분씩 기다리게 된다. 무엇보다 라운드가
  계속 쌓이는 방식 자체가 피로했다 — 한 PR 에서 라운드 6까지 간 적도 있다.

  리뷰를 한 번 더 받고 싶으면 `@copilot review` / `@codex review` 를 직접 달고 커맨드를 다시 부른다.
  미해결 스레드가 0 이면 머지 가능 판정으로 바로 넘어가고, 봇 리뷰 대기로 `BLOCKED` 이면 그 사실만
  보고하고 끝낸다(기다리지 않는다).

### Patch Changes

- [#117](https://github.com/minjun0219/rocky/pull/117) [`459f5b8`](https://github.com/minjun0219/rocky/commit/459f5b89aa566d9ddb4a07a2f4a8abea718ae630) Thanks [@minjun0219](https://github.com/minjun0219)! - `/rocky:recall` 의 서브에이전트 모델 선택을 특정 모델 고정에서 **등급 선택**으로 일반화한다.
  Haiku/Sonnet 은 예시(기본값)로 남고, 배치 크기 기준(작은 배치 → 더 저렴한 쪽, 큰 배치 → 더 큰
  컨텍스트/품질)은 그대로다. 세션 환경이 다른 저비용 백엔드(예: 로컬 모델 위임)를 제공하면 스킬
  지시와 충돌 없이 그쪽을 고를 수 있다. 다이제스트 출력 형식과 `kind:"digest"` 기록 방식은 변경 없다.

## 0.19.0

### Minor Changes

- [#115](https://github.com/minjun0219/rocky/pull/115) [`9fda5c4`](https://github.com/minjun0219/rocky/commit/9fda5c44b08df5ab5de6457a5e493bc1c6a96abe) Thanks [@minjun0219](https://github.com/minjun0219)! - worklog 프로젝트 키를 cwd 가 아니라 **레포 루트** 기준으로 잡는다. git worktree 에서 작업해도
  원본 워크스페이스와 같은 워크로그에 쌓인다.

  `git rev-parse --git-common-dir` 는 linked worktree 안에서도 주 워크트리의 `.git` 을 가리킨다 —
  그 경로를 해시하면 worktree 와 원본이 한 키로 접힌다. git 레포가 아니면 예전처럼 cwd 기준이고,
  git 호출은 실패해도 throw 하지 않는다 (워크로그 기록이 git 유무로 깨지면 안 된다).

  경로는 `realpathSync` 로 정규화한다 — worktree 의 common dir 은 realpath 로 나오는데 cwd 는
  아닐 수 있어(macOS 의 `/tmp` → `/private/tmp`), 정규화하지 않으면 같은 레포가 여전히 두 해시로
  갈린다.

  **왜 고쳤나**: 실측 결과 `~/.config/rocky/worklog` 에 디렉터리가 58 개 쌓여 있었는데 실제
  프로젝트는 15 개였다. 나머지는 worktree 마다 갈라진 조각과, 그 worktree 가 삭제된 뒤 남은
  고아였다. 이 상태에서는 worktree 에서 `/rocky:recall` 을 돌려도 본체 히스토리를 못 읽어,
  "프로젝트를 넘나드는 기억"이라는 워크로그의 존재 이유가 깨진다.

  기존 디렉터리는 이름에서 원본 cwd 를 역산할 수 없어(sha1) 자동 마이그레이션이 제공되지 않는다.
  본체에서 쌓은 워크로그는 키가 그대로라 영향이 없고, worktree 조각만 새 키로 다시 시작된다.

- [#113](https://github.com/minjun0219/rocky/pull/113) [`5b08c06`](https://github.com/minjun0219/rocky/commit/5b08c062e0a2e1095fb7d98e189b37aaf9a40963) Thanks [@minjun0219](https://github.com/minjun0219)! - 소울(페르소나)과 statusline, 그리고 `/rocky:codex` · `/rocky:issue` 커맨드를 걷어냈다.

  - **소울** — `souls/*.md` 3종, `soul.ts`, `inject-soul` 훅, `/rocky:soul`, `rocky.json` 의
    `soul` / `callsign` 키. 재미로 넣은 기능이었고, 동시에 rocky 가 세션 컨텍스트에 넣던
    **유일한** 것이었다 (주입 1,310자 → 605자로 압축했다가 기능째 제거).
  - **statusline** — 템플릿 3종, `statusline.ts`, `sync-statusline` 훅, `/rocky:statusline`,
    `docs/statusline.md`. 컨텍스트 비용은 0 이었지만 함께 정리했다.
  - **커맨드** — `/rocky:codex` (Codex 위임은 공식 `openai/codex-plugin-cc` 가 덮는다),
    `/rocky:issue`.

  훅은 `Stop`(턴 자동 기록) 하나만 남는다 — **SessionStart 가 사라져 이제 플러그인이 세션
  컨텍스트에 넣는 것이 아무것도 없다.** MCP 도구 16 종과 `/rocky:brainstorm` · `/rocky:review` ·
  `/rocky:finish` · `/rocky:review-pr` · `/rocky:recall` 커맨드, 스킬 2종은 그대로다.

  기존 `rocky.json` 에 `soul` / `callsign` 이 남아 있으면 unknown key 로 거부되니 지워야 한다.
  `~/.claude/settings.json` 의 `statusLine` 설정과 `~/.config/rocky/statusline.sh` 도 직접 정리해야
  한다 (rocky 는 사용자 설정을 건드리지 않는다).

- [#113](https://github.com/minjun0219/rocky/pull/113) [`5b08c06`](https://github.com/minjun0219/rocky/commit/5b08c062e0a2e1095fb7d98e189b37aaf9a40963) Thanks [@minjun0219](https://github.com/minjun0219)! - opencode 위임 런타임을 걷어냈다. `/rocky:opencode` · `/rocky:opencode-jobs` 커맨드, companion CLI,
  잡 저장소, `SessionStart`/`SessionEnd` 잡 배선 훅, `rocky.json` 의 `opencode` 블록과
  `ROCKY_OPENCODE_*` 환경 변수가 사라진다 (코드 1,737 LOC + 테스트 9 파일).

  도입(v0.17) 이후 실제로 돈 위임 잡이 1 건뿐이었고, 커맨드 실행 흔적도 없었다. Codex 위임
  (`/rocky:codex`)은 그대로 남는다. MCP 도구 16 종(openapi*\* 7 / seo_validate / notion*\_ 4 /
  worklog\_\_ 4)과 소울·statusline·`Stop` 훅도 전부 유지된다.

  기존 `rocky.json` 에 `opencode` 블록이 남아 있으면 이제 unknown key 로 거부되니 지워야 한다.
  `~/.config/rocky/jobs/` 의 기존 잡 기록 파일은 삭제하지 않았다.

## 0.18.0

### Minor Changes

- [#110](https://github.com/minjun0219/rocky/pull/110) [`a16bfa8`](https://github.com/minjun0219/rocky/commit/a16bfa8b764a9f06f9377085f1fbe4f9c48a0d54) Thanks [@minjun0219](https://github.com/minjun0219)! - `/rocky:brainstorm` · `/rocky:review` 슬래시 커맨드 추가 — superpowers 플러그인을 걷어내면서 실제로 쓰던 두 발상만 rocky 자체 커맨드로 재작성했다. `/rocky:brainstorm` 은 아이디어를 설계로 다듬는다(맥락 파악 → 한 번에 하나씩 질문 → 접근안 2~3개 → 설계 → 규모가 클 때만 스펙 문서). `/rocky:review` 는 완료 선언 전 신선한 컨텍스트의 서브에이전트로 현재 작업 diff 를 검토시킨다(이미 열린 PR 의 스레드 대응인 `/rocky:review-pr` 과 별개). 원본과 달리 **강제 게이트가 아니다** — 사용자가 부를 때만 돌고, 작은 수정에는 요구하지 않는다. 설계·계획 산출물 디렉터리는 `docs/superpowers/` 에서 `docs/design/` 으로 개명(기존 문서는 경로만 갱신해 보존).

## 0.17.0

### Minor Changes

- [#107](https://github.com/minjun0219/rocky/pull/107) [`efab755`](https://github.com/minjun0219/rocky/commit/efab75579b221dc87780c3265589c3da14c27ecf) Thanks [@minjun0219](https://github.com/minjun0219)! - opencode 위임 런타임 추가 — `/rocky:opencode` 백그라운드 실행 + `/rocky:opencode-jobs`

  `/rocky:opencode` 의 dispatch 를 companion 런타임(`src/opencode-companion.ts`)이 맡는다. 프롬프트를
  `--prompt-file` 로 넘겨 셸 인용 문제를 없애고, `--format json` NDJSON 을 파싱해 최종 텍스트와
  opencode 세션 id 를 뽑는다. `--background` 를 붙이면 자기 자신을 detached `job-worker` 로 재실행해
  즉시 잡 id 를 돌려주고, 잡 조회·회수·취소는 새 커맨드 `/rocky:opencode-jobs` 가 담당한다.

  - 잡 상태는 `~/.config/rocky/jobs/<project-key>` 에 인덱스 + payload + 진행 로그로 저장
    (`ROCKY_OPENCODE_JOBS_DIR` / `rocky.json` 의 `opencode.dir`, `opencode.maxJobs` 기본 50)
  - `SessionStart`/`SessionEnd` 훅이 세션 id 를 주입해 잡을 세션별로 격리하고, 세션 종료 시
    진행 중이던 워커의 프로세스 그룹을 정리한다 (잡 기록은 보존)
  - 취소는 `kill(-pid)` 로 프로세스 그룹 전체를 끊어 opencode 자식까지 함께 종료
  - `rocky.json` 에 `opencode` 블록 추가 (`dir` / `maxJobs` / `model` / `agent`)
  - MCP 도구 표면은 변경 없음 — Codex / opencode 호스트에 영향 없다

- [#108](https://github.com/minjun0219/rocky/pull/108) [`83c9777`](https://github.com/minjun0219/rocky/commit/83c9777a55de5aad5f3eca10aff10d0134e3276f) Thanks [@minjun0219](https://github.com/minjun0219)! - `/rocky:review-pr` 슬래시 커맨드 추가 — PR 에 붙은 리뷰(Copilot / Codex / 사람)를 미해결 0 까지 처리한다. 수집 → 분류 → 수정 + 게이트 → 라운드당 커밋 1개 푸시 → resolve → 재리뷰 대기를 반복하고, 판단이 갈리는 지적은 보류 큐에 모아 수렴 후 사용자와 상의해 승인된 반론만 코멘트 + resolve 한다. 미해결 0 + checks 통과 시 머지 가능 알림을 보내며, 머지 자체는 하지 않는다. `/rocky:finish` 의 후속 안내도 이 커맨드로 교체.

## 0.16.0

### Minor Changes

- [#105](https://github.com/minjun0219/rocky/pull/105) [`2fa89e1`](https://github.com/minjun0219/rocky/commit/2fa89e1b0b74220cde056d71371eb3570222ddd0) Thanks [@minjun0219](https://github.com/minjun0219)! - `delegating-to-codex` 번들 스킬을 제거하고 `/rocky:codex` 를 자기완결화

  공식 [`openai/codex-plugin-cc`](https://github.com/openai/codex-plugin-cc) 플러그인이 Codex 위임
  영역을 공유 app-server 런타임 기반으로 더 넓게 덮으면서, rocky 가 같은 메커니즘을 스킬로 중복
  배포할 근거가 사라졌다. 공식 쪽 커버 범위는 스킬 3종(`codex-cli-runtime`, `codex-result-handling`,
  `gpt-5-4-prompting`)과 커맨드(`/codex:rescue`, `/codex:review`, `/codex:transfer`)다.

  `/rocky:codex` 에는 공식 플러그인에 없는 고유 가치가 남아 있어 유지한다 — **격리 git worktree** 와
  **rocky 플러그인 표면 무결 검증**(MCP 도구 개수/이름 + `.claude-plugin/plugin.json` 의 `mcpServers`).
  스킬에 있던 자기완결 프롬프트 원칙, 감독자 규칙, 샌드박스·모델 선택 가드레일은 커맨드 본문으로
  흡수했고, 공식 플러그인을 써야 할 상황을 커맨드 상단에 명시했다.

## 0.15.0

### Minor Changes

- [#102](https://github.com/minjun0219/rocky/pull/102) [`c67167c`](https://github.com/minjun0219/rocky/commit/c67167c648808f56b540eb48cff85f1217fde3b5) Thanks [@minjun0219](https://github.com/minjun0219)! - rocky-todo(공유 보드 데몬)를 별도 레포/플러그인 `minjun0219/rocky-todo` 로 분리했다. rocky 본체에서 todo 코드·데몬·웹 UI·CLI·`notify-todo` 훅·`todo` 스킬·`docs/rocky-todo.md` 를 제거하고 react/react-dom/zustand 의존을 걷어냈다. `rocky.json` 의 `todo` 키는 관용한다(rocky 는 무시, rocky-todo 데몬이 소비 — 공유 파일이라 거부하지 않음). rocky 마켓플레이스가 rocky-todo 를 github source 2번째 entry 로 서빙하므로 `claude plugin install rocky-todo@rocky-marketplace` 로 설치할 수 있다.

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
