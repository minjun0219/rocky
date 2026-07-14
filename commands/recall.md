---
description: 워크로그(worklog 의 kind:"turn" + 수동 decision/blocker)를 읽어 앵커 히스토리 다이제스트로 정리한다 — 마지막 digest 이후 항목만 증분 요약해 kind:"digest" 엔트리로 남기고, 각 앵커는 원본 엔트리 id 로 드릴다운 가능하게 한다. 배치 크기에 따라 Haiku/Sonnet 서브에이전트를 고른다.
argument-hint: "[집중할 주제/힌트] (생략 가능)"
allowed-tools: mcp__plugin_rocky_rocky__worklog_status, mcp__plugin_rocky_rocky__worklog_read, mcp__plugin_rocky_rocky__worklog_search, mcp__plugin_rocky_rocky__worklog_append, Task
---

# recall — 워크로그 → 앵커 히스토리 다이제스트

rocky 의 worklog 는 **기록(logbook)** 레이어다 — `Stop` hook 이 매 턴 `kind:"turn"` 워크로그를
자동으로 쌓는다(수동 `decision`/`blocker`/`answer`/`note` 공존). 이 커맨드는 그 워크로그를 읽어
**히스토리 다이제스트**로 정리한다 — 별도 위키 문서가 아니라, 워크로그로 **찾아 들어갈 수 있는
앵커**(각 항목이 원본 엔트리 id 를 가리킴)로. `$ARGUMENTS` 는 집중할 주제 힌트(있으면).

## 원칙

1. **rocky 는 기록만, 정리는 이 커맨드가.** 요약(어떤 순간을 앵커로 남길지)은 호스트 LLM /
   서브에이전트가 한다.
2. **증분.** 마지막 `kind:"digest"` watermark 이후 항목만 처리한다.
3. **worklog 는 불변.** 기존 줄을 지우거나 편집하지 않는다. 다이제스트도 `worklog_append` 한 줄.
4. **앵커는 드릴다운용.** 각 앵커는 요약 + 원본 엔트리 `id`(+timestamp) 를 담아, 읽는 쪽이
   필요하면 `worklog_read` 로 원문을 찾아가게 한다.
5. **네이티브 메모리와 별개.** 이 다이제스트는 worklog 안에 산다. Claude Code 글로벌 메모리를
   건드리지 않는다.

## 절차

### 1. 상태 확인 → watermark

```
worklog_status
```
- `totalEntries` 가 0 이면 "정리할 워크로그 없음" 후 종료.
- 마지막 watermark: `worklog_read { kind: "digest", limit: 1 }` → 있으면 그 `timestamp`, 없으면 첫 실행.

### 2. 새 워크로그 수집 (증분)

- watermark 이후만: `worklog_read { since: <watermark>, limit: 500 }`. 없으면 전체를 최근부터.
- `$ARGUMENTS` 힌트가 있으면 `worklog_search` 로 보강.
- `kind:"digest"` 항목은 제외. 새 항목 수 `n` 을 센다. `n == 0` → no-op 종료 (watermark 안 남김).

### 3. 적응적 모델로 서브에이전트 dispatch

- `worklog.digestThreshold`(기본 40) 기준: `n <= 40` → **Haiku**, `n > 40` → **Sonnet**.
- `Task` 로 서브에이전트를 띄운다 (model 을 위 규칙대로). 수집 항목(각 `id`/`timestamp`/`kind`/
  `content`)을 넘기고 아래 **앵커 다이제스트**를 만들게 한다:
  - raw 나열 금지 — 의미 있는 순간(결정/전환/blocker/사용자 답변)만.
  - 각 앵커 끝에 원본 `id:<id> (<ts>)`.
  - 포맷:
    ```markdown
    ## digest — <n> entries, <first ts> … <last ts>
    - <결정/전환 요약> → id:<entry-id> (<ts>)
    - <blocker 해결> → id:<entry-id> (<ts>)
    ```

### 4. 다이제스트 append (watermark 겸용)

```
worklog_append { kind: "digest", content: "<앵커 다이제스트>", tags: ["digest"] }
```
- 이 엔트리 timestamp 가 다음 `/recall` 의 `since` 기준점.

### 5. 마무리

- 만든 다이제스트(앵커 목록)를 한국어로 보고 — 드릴다운 id 포함. 장문 금지.

## 예외 처리

- `totalEntries == 0` 또는 새 항목 0 → no-op 종료 (watermark 안 남김).
- 서브에이전트 실패 → 다이제스트 append 하지 말고 실패만 알린다 (watermark 오염 방지).
