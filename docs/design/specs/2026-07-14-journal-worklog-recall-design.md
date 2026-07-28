# Design — worklog 자동 기록 + `/recall` 히스토리 다이제스트

> Status: 설계 확정 (승인) · Date: 2026-07-14 · Scope: rocky journal → worklog 도메인 개명 + 자동 기록

## 배경 / 문제

rocky 의 기존 `journal` 도메인은 "기록(record) 레이어"를 표방하지만 **자동 캡처 트리거가
없다** — `journal_append` 를 명시적으로 호출할 때만 한 줄이 쌓인다. 그래서 실사용에서 저널이
아예 비어 있고(디렉터리 미생성), "메모리가 안 쌓인다"는 증상이 나온다.

동시에 스스로 내세운 정체성(네이티브 메모리와 구분되는 **완전한·결정론적 로그**)과 실제
동작(에이전트가 판단해 남기는 **큐레이션**)이 모순이다. 자동 캡처가 없으면 "네이티브
메모리인데 컨텍스트 자동 로딩도 안 되는 열등한 중복"이 된다.

## 목표

기존 `journal` 도메인을 **`worklog`(블랙박스/로그북)** 로 개명하고, 자동 기록을 붙인다:

0. **개명** — `journal_*` 도구·`AgentJournal` 코드·`journal.*` config·`ROCKY_JOURNAL_*` env 를
   전부 `worklog_*` / `Worklog` / `worklog.*` / `ROCKY_WORKLOG_*` 로 바꾼다. 도구 이름이 바뀌는
   breaking change 지만, 개인 플러그인이고 기존 저널 데이터가 없어 마이그레이션 불필요.
1. **기계적으로 쌓는다** — 매 응답 종료 시 `Stop` hook 이 자동으로 워크로그 한 줄(`kind:"turn"`)을
   남긴다 (LLM 개입 0, 사용자가 아무것도 안 해도 누적).
2. **필요할 때 정리해 앵커로 쓴다** — 히스토리 파악이 필요한 시점에 `/recall` 이 워크로그를
   요약해 **앵커(드릴다운 가능한 히스토리 색인, `kind:"digest"`)** 를 만들고 보고한다.
   위키(별도 지식 문서) 로 증류하지 않는다.

## 비목표 (Out)

- **위키 증류 제거** — 기존 `/curate` → Obsidian wiki 파이프라인과 `wikiDir` 개념 전면 삭제.
- **자동 read-back 없음** — 다음 세션에 워크로그/다이제스트를 자동 주입하지 않는다.
  히스토리는 `/recall` / `worklog_read` / `worklog_search` 로 on-demand 조회만.
- **자동 정리(폴링/임계 자동 발화) 없음** — `/recall` 은 호출식(반자동: 스킬이 필요 시점에
  부른다). 백그라운드 데몬 아님.
- 네이티브 메모리로의 승격/동기화 없음.
- standalone CLI 에는 worklog 미추가 (plugin 전용, 기존 journal 과 동일 정책).

## 네이밍 규칙 (개명 후)

| 구분 | 이전 (journal) | 이후 (worklog) |
| :-- | :-- | :-- |
| MCP 도구 | `journal_append/read/search/status` | `worklog_append/read/search/status` |
| 클래스 | `AgentJournal` | `Worklog` |
| 코어 파일 | `src/core/journal.ts` / `journal-handlers.ts` | `src/core/worklog.ts` / `worklog-handlers.ts` |
| 팩토리 | `createJournalFromEnv` | `createWorklogFromEnv` |
| 핸들러 | `handleJournal*` | `handleWorklog*` |
| 타입 | `JournalEntry/Kind/Status/…` | `WorklogEntry/Kind/Status/…` |
| config 키 | `journal.{dir,wikiDir}` | `worklog.{dir,autoCapture,captureMaxChars,digestThreshold}` |
| env | `ROCKY_JOURNAL_DIR` / `ROCKY_JOURNAL_WIKI_DIR` | `ROCKY_WORKLOG_DIR` / `ROCKY_WORKLOG_AUTO_CAPTURE` |
| 저장 | `~/.config/rocky/journal/<key>/journal.jsonl` | `~/.config/rocky/worklog/<key>/worklog.jsonl` |
| surface 가드 | `JOURNAL_TOOLS` (index.test.ts) | `WORKLOG_TOOLS` |
| 커맨드 | `commands/curate.md` (`/curate`) | `commands/recall.md` (`/recall`) |

`kind` 값(`turn` / `digest` / `decision` / `blocker` / `answer` / `note`)은 **내용 종류**라
도메인 개명과 무관하게 유지한다.

## 아키텍처 (2층, 위키 없음)

```
[매 응답 종료]  Stop hook → src/hooks/log-turn.ts
                  createWorklogFromEnv().append(kind:"turn")   ← 기계적, LLM 0
                        │
                        ▼
                  worklog.jsonl  ── 기록 레이어 (worklog + 수동 decision/blocker)
                        ▲
                        │  worklog_read(since 마지막 digest)
[스킬이 히스토리 필요]  /recall  → 적응적 Haiku/Sonnet 서브에이전트
                                    → 앵커 다이제스트 생성 (id 참조 포함)
                                    → append(kind:"digest")  ← 다음 /recall 의 watermark
                                    → 앵커 요약 보고
```

두 레이어 모두 `Worklog` / `worklog.jsonl` 를 재사용한다. hook 은 MCP 를 우회해
`createWorklogFromEnv()` 로 같은 파일에 직접 append 한다 (rocky MCP 서버 기동 불필요).

## 컴포넌트

### 1. `hooks/hooks.json` (신규)

플러그인 자동 발견 위치. `Stop` 이벤트에 커맨드 hook 하나:

```json
{
  "hooks": {
    "Stop": [
      { "hooks": [ { "type": "command", "command": "bun run \"${CLAUDE_PLUGIN_ROOT}/src/hooks/log-turn.ts\"" } ] }
    ]
  }
}
```

### 2. `src/hooks/log-turn.ts` (+ `src/hooks/transcript.ts`) (신규)

Stop hook 엔트리. **절대 턴을 막지 않는다** — 전부 try/catch, 항상 `exit 0`.

1. stdin JSON 파싱: `{ transcript_path, cwd, ... }`.
2. **gating**: `ROCKY_WORKLOG_AUTO_CAPTURE` (env, `0`/`false`/`off`/`no` 면 비활성) > `rocky.json`
   `worklog.autoCapture` (기본 **true**). 비활성이면 즉시 `exit 0`.
3. `transcript_path` (JSONL) → 마지막 turn 추출 (`extractTurn`, 순수 함수):
   - `req` = 마지막 real user prompt 텍스트 (truncate `captureMaxChars`, 기본 800)
   - `tools` = 그 이후 assistant `tool_use` 이름 (중복 `이름(×N)`, 최대 20)
   - `did` = 마지막 assistant text 블록 (truncate 800)
4. `buildTurnContent` → `content = "req: … | tools: … | did: …"`.
5. `createWorklogFromEnv(config.worklog).append({ kind:"turn", content, tags:["turn"] })`.
6. 추출 실패 / user 메시지 없음 → skip (`exit 0`).

순수 추출 함수(`extractTurn` / `buildTurnContent`)는 `src/hooks/transcript.ts` 로 분리해 IO 없이 테스트.

### 3. `commands/recall.md` (신규, 기존 `curate.md` 대체)

`/recall [주제 힌트]` — 워크로그를 앵커 히스토리 다이제스트로 정리·보고.

1. `worklog_status` → totalEntries / lastDigestAt.
2. 마지막 watermark: `worklog_read { kind:"digest", limit:1 }` → 있으면 그 timestamp, 없으면 첫 실행.
3. `worklog_read { since:<watermark>, limit:500 }` 로 새 항목 수집 (digest 제외). 힌트 있으면
   `worklog_search` 보강. 새 항목 0 → no-op 종료 (watermark 안 남김).
4. **적응적 모델**: 새 항목 `n ≤ worklog.digestThreshold`(기본 40) → `haiku`, 초과 → `sonnet`.
   `Task` 로 서브에이전트 dispatch.
5. 서브에이전트가 **앵커 다이제스트** 생성 — 의미 있는 순간만, 각 앵커에 원본 엔트리 `id`(+ts).
6. `worklog_append { kind:"digest", content:<다이제스트>, tags:["digest"] }` — 이 엔트리가 watermark.
7. 다이제스트를 한국어로 보고 (드릴다운 id 포함). 장문 금지.

**앵커 포맷**:
```markdown
## digest — <n> entries, <first ts> … <last ts>
- <결정/전환 요약> → id:<entry-id> (<ts>)
```

### 4. `src/core/worklog.ts` 등 (개명 + wikiDir 제거)

기존 `journal.ts` 를 `worklog.ts` 로 개명하며 모든 심볼을 worklog 네이밍으로. 동시에
`wikiDir` / `WorklogWikiDirSource` / status 의 wiki 필드 제거, watermark `curate`→`digest`
(status 의 `lastCurateAt`→`lastDigestAt`).

### 5. `src/core/rocky-config.ts` + `rocky.schema.json` (lockstep)

`journal` 키 → `worklog` 키. 필드: `dir?`, `autoCapture?`(기본 true), `captureMaxChars?`(기본 800),
`digestThreshold?`(기본 40). `wikiDir` 제거.

### 6. 문서 / 매니페스트 동기화

`plugin.json`, `FEATURES.md`, `AGENTS.md`, `README.md` — worklog 개명 + Stop hook 자동 기록 +
`/recall` 반영, `/curate`·wikiDir 제거. `index.test.ts` 의 `JOURNAL_TOOLS`→`WORKLOG_TOOLS`.

## 데이터 모델

- `kind:"turn"` — Stop hook 자동 워크로그. `content = "req: … | tools: … | did: …"`.
- `kind:"digest"` — `/recall` 앵커 다이제스트. 원본 id 참조. timestamp 가 다음 watermark.
- 기존 `decision`/`blocker`/`answer`/`note` 수동 엔트리 공존 — 모두 `/recall` 정리 대상.

## 에러 처리 / 안전

- hook 은 턴을 막지 않는다 (try/catch, 항상 `exit 0`, off 면 즉시 종료).
- 기록은 로컬 전용. truncation + opt-out kill switch(`ROCKY_WORKLOG_AUTO_CAPTURE=0`)로 완화.
- worklog 는 append-only 불변 — hook/recall 모두 기존 줄 수정/삭제 안 함.

## 테스트

- `src/hooks/transcript.test.ts`: `extractTurn`/`buildTurnContent` (추출·truncate·tool 집계·null).
- `src/hooks/log-turn.test.ts`: `shouldCapture` gating (env/config).
- `worklog.test.ts`: 개명 반영, wikiDir 케이스 제거, `lastDigestAt` 추가.
- `rocky-config.test.ts`: `worklog.*` 검증, `wikiDir`/`journal` 키 reject.
- `index.test.ts`: 새 MCP 도구 없음(4개 그대로, 이름만 worklog_*) → surface 카운트 불변,
  `WORKLOG_TOOLS` 로 갱신.

## 스코프 정합성 (AGENTS.md)

- 자동 **기록**(Stop hook) ≠ 자동 **정리**(`/recall` 은 호출식) → "auto-curate Out" 과 무충돌.
- **플러그인에 hooks 표면이 처음 생김** + **journal→worklog 개명** → AGENTS.md *Project in one
  line* / *Layout* / *MVP scope*, `plugin.json`, FEATURES.md 대폭 갱신 필요.
- 새 런타임 prod-dep 없음.

## 변경 체크리스트 (구현 후)

1. `bun run check` / `typecheck` / `test` 통과
2. surface 변경(개명·hooks·커맨드·config) → FEATURES.md + AGENTS.md + README + plugin.json 동기화
3. 새 env(`ROCKY_WORKLOG_*`) → FEATURES.md env 표 + 소비 지점 갱신
4. `rocky.json` shape 변경 → `rocky.schema.json` ↔ `rocky-config.ts` lockstep
5. `/reload-plugins` 로 로컬 검증 (hook 발화 + `/recall` 왕복)
