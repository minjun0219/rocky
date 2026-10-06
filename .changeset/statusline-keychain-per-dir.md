---
"@minjun0219/rocky": patch
---

`rocky statusline --full` 의 갱신이 `CLAUDE_CONFIG_DIR` 로 띄운 세션의 토큰도 keychain 에서 찾는다 — Claude Code 의 이름 규칙(`Claude Code-credentials-<sha256(그 값) 앞 8자리>`)을 따르므로 설정 없이 두 번째 계정의 크레딧·api 숫자가 나온다. keychain 토큰이 만료됐으면 유효한 `.credentials.json` 을 대신 쓴다. `rocky statusline doctor` 의 keychain 줄도 그 이름을 보인다.
