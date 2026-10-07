---
"@minjun0219/rocky": minor
---

Antigravity(`agy`)에도 사람의 보드 변경을 자동으로 넣는다. 번들에 `hooks.json` 을 더해 `PreInvocation` 마다 `rocky hook notify-todo agy` 가 마지막 확인 이후 사람이 웹에서 바꾼 것을 `ephemeralMessage` 로 싣는다 — Claude Code 의 `UserPromptSubmit` 주입과 같은 블록이다. 커서는 대화(`conversationId`)별로 `hook-cursors-agy.json` 에 따로 두고, agy 자신(`antigravity`)과 다른 에이전트의 변경은 뺀다. 이미 번들을 깐 기기는 `agy plugin install <rocky 레포>/antigravity` 를 다시 돌려야 훅이 들어간다.
