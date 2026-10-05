---
"@minjun0219/rocky": minor
---

`rocky statusline --full` 이 usage API 로 한도·크레딧을 읽는다. `source: api`·`auto`(stdin 에 한도가 없을 때)나 한도가 소진됐을 때 statusline 이 갱신 프로세스를 detached 로 띄우고 기다리지 않는다 — 결과(5h/7d, 남은 크레딧, 실패 이유·stale)는 다음 렌더에 나온다. 계정은 세션의 `CLAUDE_CONFIG_DIR` 로 판단하고, 토큰은 `tokenEnv` → keychain(기본 설정 폴더만) → `.credentials.json` 순으로 읽기만 한다. 캐시(`~/.cache/rocky/statusline/`)는 설정 폴더와 로그인된 계정별로 갈라 계정을 바꿔도 남의 숫자를 그리지 않는다. 설정: `statusline` 블록의 `configDir` · `pollSeconds` · `creditPollSeconds` · `creditDivisor` · `currency` · `alwaysShowCredits` · `keychainService` · `credentialsFile` · `tokenEnv`. cc-usage 의 refresh 와 같은 동작이다.
