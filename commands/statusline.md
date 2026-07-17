---
description: rocky statusline 을 설치/점검/해제한다. 번들 템플릿(statusline/<name>.sh — duo/mini/full)에서 하나를 골라 안정 경로 ~/.config/rocky/statusline.sh 로 복사하고 user settings.json 의 statusLine 을 그 경로로 지정. 이후 플러그인 업데이트는 SessionStart 훅이 자동 전파.
argument-hint: "[install [<template>] | list | status | off]"
allowed-tools: Read, Write, Edit, Bash(ls:*), Bash(cp:*), Bash(chmod:*), Bash(mkdir:*), Bash(command -v:*), Bash(diff:*), Bash(head:*)
---

# statusline — rocky statusline 설치/관리

Claude Code 의 `statusLine` 은 user `settings.json` 에만 살 수 있다 (플러그인 `settings.json`
은 `agent`/`subagentStatusLine` 만 지원). rocky 는 스크립트 원본을 번들 템플릿
`${CLAUDE_PLUGIN_ROOT%/}/statusline/<name>.sh` 로 소유하고, 이 커맨드가 고른 템플릿을 안정
경로 `~/.config/rocky/statusline.sh` 로 복사한 뒤 settings 를 1회 지정한다. 이후 플러그인
업데이트는 `SessionStart` 훅(`src/hooks/sync-statusline.ts`)이 설치본 헤더의
`# rocky-statusline-template: <name>` 마커를 읽어 같은 템플릿에서 자동 전파한다.
`$ARGUMENTS` 로 서브커맨드를 받는다.

> **플러그인 캐시 경로(버전별)를 settings 에 직접 쓰지 않는다** — 업데이트 때마다 경로가
> 바뀌어 statusline 이 깨진다. 반드시 안정 경로 간접화를 유지한다. 번들 템플릿 나열/존재
> 확인은 `ls "${CLAUDE_PLUGIN_ROOT%/}/statusline"` 으로 하고, 그 출력의 실제 절대경로를
> 이후 단계(Read/cp)에 쓴다 (Read/cp 는 shell 확장을 하지 않으므로).

## 템플릿

각 템플릿 파일 상단에 `# rocky-statusline-template: <name>` (훅 sync 용 마커, 절대 제거
금지) 와 `# description: <한 줄>` 이 있다. 목록을 보여줄 때는 `head` 로 이 두 줄을 읽는다.
번들 기본 3종: `duo` (2줄, 기본) / `mini` (1줄 컴팩트) / `full` (3줄, 세션 비용·변경량·경과 포함).

## 서브커맨드

- **(없음) 또는 `install [<template>]`** — 설치/재설치한다.
  1. `<template>` 인자가 있으면 **먼저** `^[a-zA-Z0-9_-]+$` 검증(아니면 사용법 후 즉시 멈춤 — 경로를 구성하지 않는다) 후 번들 존재 확인. **인자가 없으면** 템플릿 목록(이름 + description)을 보여주고 하나를 고르게 한다 (기본 추천: `duo`).
  2. 의존성 확인: `command -v jq` — 없으면 설치 안내 후 멈춘다 (스크립트가 jq 를 쓴다).
  3. `~/.claude/settings.json` 을 읽어 현재 `statusLine` 값을 확인하고 사용자에게 보여준다.
     이미 다른 statusline 이 설정돼 있으면 교체 여부를 명시적으로 확인한다.
  4. **초안을 보여주고 승인 받은 뒤에만** 진행한다:
     - `mkdir -p ~/.config/rocky` 후 고른 템플릿을 `~/.config/rocky/statusline.sh` 로 복사, `chmod +x`.
     - `~/.claude/settings.json` 을 타임스탬프 백업(`settings.json.bak-<YYYYMMDDHHmmss>`)으로
       복사해 두고, `statusLine` 키만 갱신한다 (다른 필드 보존):
       ```json
       { "type": "command", "command": "bash ~/.config/rocky/statusline.sh" }
       ```
     - 이미 rocky statusline 이 설치된 상태에서 템플릿만 바꾸는 경우 settings 는 그대로 두고
       복사만 한다 (경로 동일).
  5. 완료 후 "새 세션부터 적용됨" 과, 이후 커스터마이징은 번들 `statusline/<template>.sh` 를
     고치면 훅이 전파한다는 점을 알린다.

- **`list`** — 번들 템플릿 목록(이름 + description)과 현재 설치본의 템플릿(마커)을 보여준다.

- **`status`** — 현재 상태를 점검해 보여준다.
  - `~/.claude/settings.json` 의 `statusLine` 값 (rocky 경로인지 여부).
  - `~/.config/rocky/statusline.sh` 존재 여부와 설치본 마커의 템플릿 이름.
  - 번들 해당 템플릿과 설치본의 동기화 여부: `diff -q` 로 비교 (다르면 다음 세션 시작 때 훅이 동기화함을 알린다).

- **`off`** — statusline 을 해제한다.
  1. **확인 후** `~/.claude/settings.json` 을 타임스탬프 백업하고 `statusLine` 키를 제거한다 (다른 필드 보존).
  2. `~/.config/rocky/statusline.sh` 는 지우지 않는다 — 파일 삭제는 사용자가 원할 때만,
     별도 확인을 받아서 한다 (남겨두면 재설치 없이 훅 sync 만으로 복귀 가능하다는 점을 알린다).

## 원칙

- `~/.claude/settings.json` 을 쓰기 전 반드시 초안 확인 + 타임스탬프 백업. `statusLine` 키만 건드린다 — 다른 필드(permissions/hooks/model 등)는 보존한다.
- 스크립트 커스터마이징의 단일 원본은 번들 `statusline/<template>.sh` (레포) — 설치본을 직접 고치면 다음 훅 sync 때 덮여 사라진다는 점을 안내한다. 템플릿 마커 줄은 훅 sync 의 열쇠이므로 유지한다.
