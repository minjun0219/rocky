---
"@minjun0219/rocky": minor
---

feat(statusline): 번들 statusline 추가 — statusLine 템플릿 3종(`statusline/<name>.sh`: `duo` 2줄 기본 / `mini` 1줄 / `full` 3줄+세션 비용·변경량·경과)을 플러그인이 소유하고, `/rocky:statusline` 커맨드가 고른 템플릿을 안정 경로 `~/.config/rocky/statusline.sh` 로 설치(user `settings.json` 의 `statusLine` 1회 지정, 초안 확인 + 타임스탬프 백업). 새 `SessionStart` 훅(`src/hooks/sync-statusline.ts`)이 설치본 헤더의 템플릿 마커를 읽어 플러그인 업데이트를 같은 템플릿에서 자동 전파한다 (미설치 시 no-op, fail-open). MCP tool 표면 변화 없음.
