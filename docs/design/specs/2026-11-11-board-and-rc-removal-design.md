# 보드와 rc 를 걷어내고 지켜보기 · 기록으로 줄이기

- 날짜: 2026-11-11
- 상태: 방향 결정(오너, 2026-11-11 — 로키 기능이 너무 커졌으니 로키는 개인 맥락 저장소 쪽으로 옮기고, 할 일과 노트도 전부 그쪽으로
  옮긴다). 그 저장소 안의 자리는 오너가 정했다. 같은 날 rc 기능도 걷어내기로 했다(오너, 2026-11-11 "로키에서 rc 기능도 걷어내자").
  rocky 코드는 아직 그대로이고, 아래 "PR 순서"대로 나눠 걷어낸다. 걷어내고 나면 규칙 정본은 남는 기능의 `docs/features/` 문서다
- 앞선 문서: [오케스트레이터 설계](./2026-10-29-rocky-orchestrator-design.md)(이 문서로 접는다), [싱크 설계](./2026-10-15-dual-structure-sync-design.md)

## 한 줄

**rocky에서 보드(할 일 · 노트 · 섹션 · 댓글), rc 서버, 로키 에이전트 계획을 빼고, 세션을 지켜보고 기록하는 도구로 줄인다.** 할 일과
노트는 오너의 개인 맥락 저장소로 옮겨 가고, 로키 에이전트도 그쪽에서 산다. rocky는 세션을 지켜보고, PR과 기본 브랜치를 챙기고, 그 모든 것을
기록한다. 세션을 띄우고 살려 두는 일은 rocky 밖의 몫이다.

## 왜

- **로키가 너무 커졌다**(오너). 보드, 노트 CRDT, 싱크 중계소, 수집함, 핸드오프, 맥 할 일 창, 보드 패널이 같은 할 일을 여러 벌로 보인다.
- **보드 화면은 이미 안 쓴다.** 2026-08-12 이후 사용 로그(`~/.config/rocky/usage/*.jsonl`, 2026-11-11 집계):
  - 웹 `GET /api/todos` 2,482회, 마지막 2026-10-16. 웹 `GET /api/notes` 1,355회, 마지막 2026-10-16
  - 웹 싱크 `WS /api/ws` 12회, 마지막 2026-10-16
  - 보드 패널 `hook board-panel` 19회 · `… use` 8회, 마지막 2026-10-14
  - 수집함 `GET /api/inbox` 웹 14회, 마지막 2026-10-10. `rocky next` 12회, 마지막 2026-10-06
  - 맥 할 일 창 `GET /api/todos`(client `mac`) 15회
  - 노트 3개는 모두 보관됨, `note_list` 3회 · `note_write` 2회(마지막 2026-10-09)
- **남은 사용은 에이전트의 MCP와 훅이다.** `todo_list` 311회 · `todo_write` 247회(마지막 2026-11-10), `rocky comment` 63회, 훅 `notify-todo`
  7,287회 · `handoff-stop` 5,883회. 이 일은 개인 맥락 저장소 쪽이 맡는다.
- **rc 는 아직 쓰지만 오너가 뺀다.** 2026-10-12 이후 `rocky rc` 137회(마지막 2026-11-11), `rocky rc start` 61회(마지막 2026-11-08),
  웹 원격 제어 탭 `GET /api/rc/servers` 320회(마지막 2026-11-11). 숫자가 0이 아니므로 rc 를 빼면 rc 서버를 띄우고 살려 두던 일(고정 대상 감시 ·
  야간 재시작 · 워크트리)을 rocky 밖에서 맡아야 한다 — 다른 도구나 손으로.
- **로키 에이전트는 구현 전이다.** 오케스트레이터 설계의 결정 9(rc 대상 + 대상별 설정 폴더)만 `rc.options` 로 들어갔고, 로키의 집 `~/.rocky`
  에는 지시 파일 하나뿐이다.

## 데이터

보드 DB(2026-11-11): 보드 14, 섹션 30, 할 일 265(열린 것 82), 댓글 337, 노트 3, 핸드오프 15, 히스토리 2,448.

- **걷어내기 전에 내보낸다**(오케스트레이터 설계 결정 4). 2026-11-11에 전 테이블 JSON과 보드별 Markdown을 읽기 전용으로 내보냈다.
  옮겨 넣는 일은 개인 맥락 저장소 쪽 세션이 한다 — rocky 코드는 그 저장소를 모른다(AGENTS.md 언어 경계).
- **`todo.db` 는 지우지 않는다.** 걷어내는 PR은 테이블을 떨구는 마이그레이션을 넣지 않는다. 새 코드가 그 테이블을 읽지 않을 뿐이다.

## 빼는 것

| 표면 | 자리 |
| --- | --- |
| 보드 저장소(보드 · 섹션 · 할 일 · 댓글 · 노트 · 노트 CRDT · 번호 · 별칭) | `rocky_core::{store, note_doc, refs, board_path, summary}` 의 보드 부분, 보드 REST |
| MCP `todo_list` · `todo_write` · `todo_status` · `note_list` · `note_write` | `crates/rockyd/src/mcp.rs` |
| 핸드오프 · doing 귀속 · 할 일에서 세션 띄우기 | `rocky_core::{handoff, doing}`, `rockyd::{spawnctl, rc::handoff}`, 훅 `handoff-stop` · `claim-doing` |
| 사람 댓글 주입 | 훅 `notify-todo`(PR 주입은 남긴다 — 아래) |
| 수집함 | `rocky_core::inbox`, `rockyd::{inbox_exec, inbox_watch}`, `bridges/`, `todo.inbox[]` |
| `/rocky:next` | `rocky_core::next`, `plugin/commands/next.md` |
| 싱크와 클라우드 중계소 | `rocky_core::sync`, `rockyd::ws`, `web/sync/`, `cloud/` |
| 보드 패널 mod | `plugin/hooks/board-panel/` |
| 웹 보드 · 노트 화면 | `web/` 의 보드 · 노트 · 에디터 |
| 맥 할 일 창 | `mac/` 의 `BoardStore` · `BoardWindow` · `BoardTree` |
| 스킬 `board` · `handoff` | `plugin/skills/` |
| rc 서버(대상 · 고정 · 감시 · 야간 · `rc.worktree` · `rc.options` · 핸드오프 서버) | `rocky_core::{rc, rc_targets}`, `rockyd::rc`(`nightly` · `handoff` 포함), `rocky_cli::{rc_cmd, rc_targets_cmd}`, `rocky.json` 의 `rc` 블록 |
| Antigravity 원격 제어 켜기 · 끄기 | `rocky rc agy`, 원격 제어 탭의 Antigravity 줄 |
| 웹 원격 제어 탭과 Cloudflare Access 원격 제어 | `web/components/RcPane.tsx`, `rockyd::access`, `rocky_core::access`(중계소와 함께), `rocky.json` 의 `access` 블록 |
| 로키 에이전트 계획 | 오케스트레이터 설계(접음), README · AGENTS.md 의 방향 단락 |

## 남는 것

세션 목록(웹 에이전트 탭 · `rocky sessions`), PR 감시와 세션 받은편지함 전달 · rocky 채널, 기본 브랜치 검증과 배포, 워크로그 · 로그 색인 · 토큰 색인 ·
`memory_brief`, statusline(cc-usage), `rocky doctor` · `rocky usage`, 데몬 수명, 웹 피드 · 에이전트 탭, 맥 메뉴 막대(돌고 있는 세션 · 머지 후보 PR)와 아바타.

세션 목록은 `claude agents` 를 읽기만 한다 — 띄우기 · 멈추기 · 메시지 보내기 같은 조종 버튼을 남길지는 PR 3에서 사용 숫자를 보고 정한다.

## 보드가 쥐던 레포 목록

PR 감시와 검증이(세션 띄우기도 지금은) 보드를 **레포 목록**으로도 쓴다. 보드를 빼려면 이 역할을 먼저 옮겨야 한다.

| 보드 필드 | 쓰는 곳 |
| --- | --- |
| `repo` | PR 감시의 레포 판정(`rocky_core::prwatch::{review_fix_enabled, author_allowed}`), `@me` 레포 고르기(`rockyd::prwatch`) |
| `path` | 검증의 워크트리 원본(`rockyd::verify` 가 `verify` 대상의 보드 key 로 찾는다), 세션 띄우기(같이 빠진다) |
| `pr_authors` | PR 알림 작성자 필터 |
| `review_fix` | 리뷰가 붙으면 세션에 review-fix 를 시킬지 |

**`rocky.json` 에 `repos` 블록을 둔다**(제안). key 는 지금 보드 key 를 그대로 써서 `verify` 대상이 고치지 않고 이어진다.

```jsonc
"repos": {
  "rocky": { "repo": "minjun0219/rocky", "path": "~/dev/workspaces/rocky", "prAuthors": ["@me"], "reviewFix": true }
}
```

- 프로젝트 > 사용자 병합 규칙은 다른 블록과 같다. `rocky.schema.json` 과 `config.rs` 를 함께 고친다.
- 처음 한 번은 데몬이 보드 행에서 `repos` 를 만들어 주지 않는다 — 설정 파일은 사람이 쓴다. 대신 `rocky doctor` 가 "보드에는 있고 `repos` 에는
  없는 레포"를 알린다(보드가 빠지기 전 PR까지).

## PR 순서

빼는 PR마다 위 사용 숫자를 인용한다(AGENTS.md 변경 체크리스트 8번).

1. **이 설계** — 방향 단락과 오케스트레이터 설계 상태를 고친다.
2. **레포 목록을 `rocky.json` 으로** — `repos` 블록, PR 감시 · 검증이 보드 대신 그것을 읽는다. 보드는 아직 남는다.
3. **화면부터 걷기** — 웹 보드 · 노트 · 원격 제어 탭, 보드 패널, 맥 할 일 창, 싱크와 `cloud/`. 데몬 표면은 그대로라 되돌리기 쉽다.
4. **데몬 표면 걷기** — 보드 REST · MCP · 훅 · 핸드오프 · 수집함 · `bridges/` · `/rocky:next` · 스킬. AGENTS.md "rocky가 무엇인가" 표와
   *범위 → 밖*, README, `docs/features/` 의 board-model · handoff · inbox · sync · board-panel · spawn 을 같이 정리한다.
5. **rc 걷기** — rc 서버 · `rocky rc` · `rc` 블록 · `access` 블록 · 보안 경계의 원격 제어 예외. `docs/features/` 의 rc-servers 와 security 를 정리한다.
   rc 서버를 살려 두던 일을 맡을 곳(오너가 정한다)이 생기기 전에는 머지하지 않는다.

3, 4, 5는 크기에 따라 더 쪼갠다. 개인 맥락 저장소 쪽(할 일을 옮겨 넣고, 그쪽 자동화가 rocky 보드에 쓰던 것을 바꾸는 일)이 끝나기 전에는 4를 머지하지 않는다.

## 열린 질문

- 맥 아바타는 세션 상태를 보이는 얼굴이라 남기는 쪽으로 적었다. 이름이 "로키"인 것만 남고 에이전트 계획은 없다.
- 웹 피드 탭이 보드 히스토리를 섞어 보인다면 PR 3에서 PR · 세션 이벤트만 남긴다.
