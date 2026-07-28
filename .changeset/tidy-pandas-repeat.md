---
"@minjun0219/rocky": minor
---

opencode 위임 런타임을 걷어냈다. `/rocky:opencode` · `/rocky:opencode-jobs` 커맨드, companion CLI,
잡 저장소, `SessionStart`/`SessionEnd` 잡 배선 훅, `rocky.json` 의 `opencode` 블록과
`ROCKY_OPENCODE_*` 환경 변수가 사라진다 (코드 1,737 LOC + 테스트 9 파일).

도입(v0.17) 이후 실제로 돈 위임 잡이 1 건뿐이었고, 커맨드 실행 흔적도 없었다. Codex 위임
(`/rocky:codex`)은 그대로 남는다. MCP 도구 16 종(openapi_* 7 / seo_validate / notion_* 4 /
worklog_* 4)과 소울·statusline·`Stop` 훅도 전부 유지된다.

기존 `rocky.json` 에 `opencode` 블록이 남아 있으면 이제 unknown key 로 거부되니 지워야 한다.
`~/.config/rocky/jobs/` 의 기존 잡 기록 파일은 삭제하지 않았다.
