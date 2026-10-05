---
name: branch-verify
description: Use when the user asks whether the default branch (main) is still green after merges, or when a rocky "검증 실패 / 다시 초록" banner or a red `rocky verify` result comes up ("main 깨졌어?", "머지된 거 다 통과했어?", "검증 결과 봐줘"). Covers reading rocky's default-branch verification (the daemon re-runs the repo's gates on every new commit in its own detached worktree), telling a real failure from an environmental false red using the per-commit log, and what not to touch (the daemon's state files and the user's verify config).
---

# 기본 브랜치 검증 — main 이 여전히 초록인가

`rocky.json` 의 `verify.targets[]` 가 있으면, 데몬이 원격 브랜치를 1분마다 보다가 새 커밋이 들어오면 **자기 전용 워크트리**
(`~/.config/rocky/todo/verify/…/tree`, detached)에서 그 레포의 게이트를 차례로 돈다. 세션의 작업 트리·브랜치는 건드리지 않는다.
실패와 복구(실패 → 통과)만 macOS 배너로 알린다.

## 보는 법

- `rocky verify` — 대상마다 한 줄: `✓ 통과` · `✗ <단계> 실패 — 이유` + 로그 경로 · `… 검증 중` · `⚠` 시작 못 한 이유.
- `GET /api/verify` — 같은 내용(JSON).
- 대상이 없다고 나오면 꺼진 것이다 — 켜는 건 사용자의 설정(`rocky.json` 의 `verify`)이다.

## 빨강일 때

1. **로그를 연다** — `rocky verify` 가 보여 주는 `<커밋>.log`. 단계별로 `### <단계> — <실행 파일>` 머리가 있다.
2. **진짜 실패인지 가른다.**
   - 그 커밋이 건드린 코드에서 실패 → 진짜다. 사용자에게 커밋·단계·원인을 알리고, 고치는 PR 을 낼지 묻는다.
   - 그 커밋과 무관한 테스트가 **시간 초과**로 실패(예: 정확히 5초에 끊김), 단독으로 다시 돌리면 통과 → 환경 탓 거짓 빨강일
     수 있다(머신이 바쁠 때). 사용자에게 그렇게 알린다.
3. **같은 커밋의 실패는 데몬이 다시 돌지 않는다** — 다음 커밋까지 빨강으로 남는다. 다시 돌리는 건 사용자 결정이다.

## 하지 않는 것

- 데몬의 상태 파일(`last.json`·`finished.json`·`signals.log`)이나 검증 워크트리를 손대지 않는다 — 사용자가 시킬 때만.
- `rocky.json` 의 `verify` 블록(실행할 명령)을 바꾸지 않는다 — 명령은 사용자의 설정이다.
- 검증 워크트리에서 커밋·푸시하지 않는다.
