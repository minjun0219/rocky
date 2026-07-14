# Design — journal worklog 자동 기록 + `/recall` 히스토리 다이제스트

> Status: 설계 확정 (승인 대기) · Date: 2026-07-14 · Scope: rocky journal 도메인

## 배경 / 문제

rocky journal 은 "기록(record) 레이어"를 표방하지만 **자동 캡처 트리거가 없다** —
`journal_append` 를 명시적으로 호출할 때만 한 줄이 쌓인다. 그래서 실사용에서 저널이
아예 비어 있고(디렉터리 미생성), "메모리가 안 쌓인다"는 증상이 나온다.

동시에 journal 이 스스로 내세운 정체성(네이티브 메모리와 구분되는 **완전한·결정론적
로그**)과 실제 동작(에이전트가 판단해 남기는 **큐레이션**)이 모순이다. 자동 캡처가 없으면
journal 은 "네이티브 메모리인데 컨텍스트 자동 로딩도 안 되는 열등한 중복"이 된다.

## 목표

journal 을 **블랙박스/워크로그(logbook)** 로 만든다:

1. **기계적으로 쌓는다** — 매 응답 종료 시 hook 이 자동으로 워크로그 한 줄을 남긴다
   (LLM 개입 없음, 사용자가 아무것도 안 해도 누적).
2. **필요할 때 정리해 앵커로 쓴다** — 히스토리 파악이 필요한 시점에 `/recall` 이
   워크로그를 요약해 **앵커(드릴다운 가능한 히스토리 색인)** 를 만들고 보고한다.
   위키(별도 지식 문서) 로 증류하지 않는다.

## 비목표 (Out)

- **위키 증류 제거** — 기존 `/curate` → Obsidian wiki 파이프라인과 `wikiDir` 개념 전면 삭제.
- **자동 read-back 없음** — 다음 세션에 워크로그/다이제스트를 자동 주입하지 않는다.
  히스토리는 `/recall` / `journal_read` / `journal_search` 로 on-demand 조회만.
- **자동 정리(폴링/임계 자동 발화) 없음** — `/recall` 은 여전히 호출식(반자동: 스킬이
  필요 시점에 부른다). 백그라운드 데몬 아님. (AGENTS.md "auto-curate Out" 과 정합.)
- 네이티브 메모리로의 승격/동기화 없음.

## 아키텍처 (2층, 위키 없음)

```
[매 응답 종료]  Stop hook → src/hooks/log-turn.ts
                  createJournalFromEnv().append(kind:"turn")   ← 기계적, LLM 0
                        │
                        ▼
                  journal.jsonl  ── 기록 레이어 (worklog + 수동 decision/blocker)
                        ▲
                        │  journal_read(since 마지막 digest)
[스킬이 히스토리 필요]  /recall  → 적응적 Haiku/Sonnet 서브에이전트
                                    → 앵커 다이제스트 생성 (id 참조 포함)
                                    → append(kind:"digest")  ← 다음 recall 의 watermark
                                    → 앵커 요약 보고
```

두 레이어 모두 기존 `AgentJournal` / `journal.jsonl` 를 재사용한다. hook 은 MCP 를
우회해 `createJournalFromEnv()` 로 같은 파일에 직접 append 한다 (rocky MCP 서버 기동
불필요).

## 컴포넌트

### 1. `hooks/hooks.json` (신규)

플러그인 자동 발견 위치. `Stop` 이벤트에 커맨드 hook 하나를 건다:

```json
{
  "hooks": {
    "Stop": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "bun run ${CLAUDE_PLUGIN_ROOT}/src/hooks/log-turn.ts"
          }
        ]
      }
    ]
  }
}
```

> 구현 시 `rocky:writing-cc-plugin` 스킬로 hooks.json 스키마/발견 규칙 최종 확인.

### 2. `src/hooks/log-turn.ts` (신규)

Stop hook 엔트리. **절대 턴을 막지 않는다** — 전부 try/catch, 항상 `exit 0`.

절차:
1. stdin JSON 파싱: `{ transcript_path, session_id, cwd, ... }`.
2. **gating**: `ROCKY_JOURNAL_AUTO_CAPTURE` (env, `0`/`false` 면 비활성) > `rocky.json`
   `journal.autoCapture` (기본 **true**). 비활성이면 즉시 `exit 0` (I/O 없음).
3. `transcript_path` (JSONL) 를 읽어 **마지막 turn** 추출 — 마지막 `user` 메시지 이후 ~ 끝:
   - `req` = 그 user 메시지의 첫 text 블록 (truncate `captureMaxChars`, 기본 800)
   - `tools` = 그 이후 assistant 메시지들의 `tool_use` 이름 (중복은 `이름(×N)`, 최대 20개)
   - `did` = 마지막 assistant 메시지의 text 블록 (truncate 800)
4. `content = "req: <…> | tools: <…> | did: <…>"` 조립.
5. `createJournalFromEnv(config).append({ kind: "turn", content, tags: ["turn"] })`.
6. 추출 실패 / user 메시지 없음 / 빈 턴 → skip (`exit 0`).

**테스트 가능성**: 추출은 순수 함수로 분리해 export —
`extractTurn(transcriptText: string): { req: string; tools: string[]; did: string } | null`,
`buildTurnContent(parts, maxChars): string`. hook 본체는 stdin/env/파일 IO 만 담당.

### 3. `commands/recall.md` (신규, 기존 `curate.md` 대체)

`/recall [주제 힌트]` — 워크로그를 앵커 히스토리 다이제스트로 정리하고 보고.

절차:
1. `journal_status` → path / totalEntries / lastEntryAt.
2. 마지막 watermark 조회: `journal_read { kind: "digest", limit: 1 }` → 있으면 그
   timestamp 가 watermark, 없으면 첫 recall.
3. `journal_read { since: <watermark>, limit: 500 }` 로 새 항목 수집 (digest 항목은 제외).
   주제 힌트가 있으면 `journal_search` 로 보강. 새 항목 0 → no-op 보고 후 종료
   (watermark 안 남김).
4. **적응적 모델**: 새 항목 수 `n` 에 대해 `n ≤ journal.digestThreshold` (기본 40) 이면
   `haiku`, 초과면 `sonnet`. `Agent` 도구로 서브에이전트 dispatch.
5. 서브에이전트가 새 항목을 **앵커 다이제스트** 로 요약 — 각 앵커는 원본 워크로그
   엔트리 `id`(+timestamp)를 가리켜 드릴다운 가능하게. raw 나열이 아니라 의미 있는
   순간(결정/전환/blocker/답변)만.
6. `journal_append { kind: "digest", content: <다이제스트>, tags: ["digest"] }` — 이
   엔트리 자체가 다음 recall 의 watermark.
7. 다이제스트를 한국어로 보고 (드릴다운 id 포함). 장문 리포트 금지.

**다이제스트 앵커 포맷** (예):

```markdown
## digest — N turns, <first ts> … <last ts>
- <결정/전환 요약> → id:<entry-id> (<ts>)
- <blocker 해결> → id:<entry-id> (<ts>)
```

### 4. `src/core/rocky-config.ts` + `rocky.schema.json` (수정, lockstep)

`journal` 키:
- **추가**: `autoCapture?: boolean` (기본 `true`), `captureMaxChars?: number` (기본 800),
  `digestThreshold?: number` (기본 40).
- **제거**: `wikiDir`.

### 5. `src/core/journal.ts` 등 (수정 — wikiDir 제거)

`wikiDir` / `JournalWikiDirSource` / `AgentJournalOptions.wikiDir` / status 의 `wikiDir` ·
`wikiDirSource` 필드, `createJournalFromEnv` 의 `ROCKY_JOURNAL_WIKI_DIR` · `config.wikiDir`
처리, `journal-handlers` 의 status 노출을 전부 제거. `kind` 유니온의 `curate` →
`digest` 로 대체 (또는 둘 다 자유문자열이므로 문서만 갱신).

`status()` 의 watermark 계산도 갱신: 현재 `kind === "curate"` 로 찾는 `lastCurateAt` 을
`kind === "digest"` 기준 `lastDigestAt` 으로 변경 (또는 `/recall` 이 `journal_read
{kind:"digest"}` 로 직접 찾으므로 status 필드에서 제거). `JournalStatus` 타입과
`journal-handlers` 노출을 lockstep 으로 반영.

### 6. 문서 / 매니페스트 동기화

- `plugin.json`: description 에서 `/curate`·wiki 문구 제거, `/recall` + hooks 반영.
- `FEATURES.md`(한국어): tool/config/커맨드 표 갱신 — autoCapture/digestThreshold env·config,
  `/recall`, Stop hook, wikiDir 삭제.
- `AGENTS.md`(영어): *Project in one line* + *Layout* 갱신 — `hooks/`, `src/hooks/log-turn.ts`,
  `commands/recall.md` 추가, `commands/curate.md`·wikiDir 제거, journal 이 자동 캡처로
  진화했음을 반영.
- `README.md`: surface 카운트/설명 갱신.

## 데이터 모델

- `kind:"turn"` — Stop hook 자동 워크로그. `content = "req: … | tools: … | did: …"`,
  `tags:["turn"]`.
- `kind:"digest"` — `/recall` 산출 앵커 다이제스트. 원본 엔트리 id 참조. timestamp 가
  다음 recall 의 증분 watermark 로 겸용.
- 기존 `decision`/`blocker`/`answer`/`note` 수동 엔트리와 공존 — 셋 다 `/recall` 정리 대상.

## 에러 처리 / 안전

- **hook 은 턴을 막지 않는다**: 모든 경로 try/catch, 항상 `exit 0`. 트랜스크립트
  파싱 실패·파일 부재·JSON 오류 → 조용히 skip.
- `autoCapture` off → hook 첫 단계에서 즉시 종료, 파일 접근 없음.
- 기록은 로컬 전용(기존 journal 과 동일 신뢰경계). 프롬프트/응답 truncation + opt-out
  kill switch 로 민감정보·용량 완화.
- 저널은 append-only 불변 — hook/recall 모두 기존 줄을 수정/삭제하지 않는다.

## 테스트

- `src/hooks/log-turn.test.ts`: fixture 트랜스크립트 JSONL →
  - `extractTurn` 이 req/tools/did 를 정확히 뽑고 truncate 하는지
  - tool 중복 집계(`이름(×N)`)·20개 cap
  - user 메시지 없음/빈 턴 → null
  - gating: autoCapture off 시 append 안 함
- `rocky-config` 테스트: `autoCapture`/`captureMaxChars`/`digestThreshold` 파싱 기본값,
  `wikiDir` 제거 회귀.
- `journal` 테스트: wikiDir 관련 케이스 제거, status 필드 축소 반영.
- `index.test.ts`: 새 MCP 도구 없음 → surface 카운트/`JOURNAL_TOOLS` 불변 재확인.
- `commands/recall.md` 는 산문 → 유닛테스트 없음, 수동 확인.

## 스코프 정합성 (AGENTS.md)

- 자동 **기록**(Stop hook) ≠ 자동 **정리**(`/recall` 은 호출식) → "auto-curate(threshold/
  polling) Out" 조항과 무충돌.
- **플러그인에 hooks 라는 표면이 처음 생긴다** → AGENTS.md *Layout* / *Project in one line*,
  `plugin.json`, FEATURES.md 갱신 필수.
- 새 런타임 prod-dep 없음 (hook 은 Bun 내장 + 기존 `AgentJournal`).

## 변경 체크리스트 (구현 후)

1. `bun run check` / `typecheck` / `test` 통과
2. surface 변경(hooks·커맨드·config) → FEATURES.md + AGENTS.md + README + plugin.json 동기화
3. 새 env(`ROCKY_JOURNAL_AUTO_CAPTURE`) → FEATURES.md env 표 + 소비 지점 갱신
4. `rocky.json` shape 변경 → `rocky.schema.json` ↔ `rocky-config.ts` lockstep
5. `/reload-plugins` 로 로컬 검증 (hook 발화 + `/recall` 왕복)
