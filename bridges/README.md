# bridges/ — 수집함 어댑터 · 알림 브릿지

외부 서비스에 닿는 **명령**들. 둘 다 `rocky.json` 에 argv 로 등록하고 데몬이 셸 없이 실행한다:

- **수집함**(`todo.inbox[]`) — 외부 투두 앱을 읽어 `GET /api/inbox` 로 합친다. 동기화가 아니다 — 읽어서
  보여주고 링크로 참조만 한다.
- **알림**(`pr.notifiers[]`) — PR 이 "확인·머지해도 되는" 상태가 되거나 충돌이 나면 데몬이 stdin 에
  전이 JSON 을 주고 실행한다. 환경마다 채널을 고른다(폰이면 텔레그램…).

규약(입출력 형식·실패 처리)의 정본은 [`docs/board.md` "수집함"](../docs/board.md#수집함--외부-투두-앱-읽기-todoinbox--get-apiinbox)
과 같은 문서의 "PR 감시 — 알림 브릿지".
설계 근거는 [`docs/design/specs/2026-09-27-bridges-and-tui-design.md`](../docs/design/specs/2026-09-27-bridges-and-tui-design.md).

| 디렉터리 | 무엇 |
| --- | --- |
| `file/` | JSON 파일을 그대로 내는 참조 구현 — 테스트·수동 확인용 |
| `todoist/` | Todoist 미완료 작업(API v1, 필터 지원). 토큰은 `--op op://Agent Vault/<uuid>/credential` 로 `op read`. `python3` stdlib 만 |
| `github-project/` | GitHub 프로젝트 보드의 열린 이슈를 보드 필터(`--assignee @me --type Bug --field "Component/s=Web"`)로 거른다 — 보드를 훑지 않고 이슈 검색으로 좁힌다. `--filter` 로 보드 필터 문자열을 그대로 받는다. 로그인된 `gh`(`read:project`)를 쓴다. Bun, 의존 없음 |
| `telegram/` | **알림** — Bot API `sendMessage` 로 전이 한 건을 보낸다(굵은 머리 + PR 링크, mdwire `telegram-html` 로 이스케이프). 토큰은 `--op` 로 `op read`, `--chat` 이 대상. Bun + `@minjun0219/mdwire` |

새 어댑터는 `bridges/<name>/` 에 두고, 토큰은 1Password Agent Vault 에서 `op read` 로 읽는다
(홈의 평문 파일 금지). 공개 레포이므로 계정 식별자를 코드에 박지 않는다.
