---
"@minjun0219/rocky": patch
---

rc 야간 재시작이 열린 세션을 이어받지 않고 서버만(`--no-create-session-in-dir`) 띄운다. 이어받기(`--session-id`)로 띄운 서버는 단일 세션 모드라 앱에서 두 번째 세션을 열면 `already running 1 of 1 sessions` 로 거절됐고, 그 세션이 끝나면 서버도 같이 내려갔다. 원격 세션의 대화는 claude.ai 쪽에 남아 새 서버에 다시 붙는다. 회복 재시도도 서버만이다. 낮의 `rocky rc restart` 는 그대로 이어받는다.
