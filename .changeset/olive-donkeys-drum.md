---
"@minjun0219/rocky": minor
---

worklog 프로젝트 키를 cwd 가 아니라 **레포 루트** 기준으로 잡는다. git worktree 에서 작업해도
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
