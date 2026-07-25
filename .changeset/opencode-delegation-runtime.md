---
"@minjun0219/rocky": minor
---

opencode 위임 런타임 추가 — `/rocky:opencode` 백그라운드 실행 + `/rocky:opencode-jobs`

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
