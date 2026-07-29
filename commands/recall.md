---
description: 워크로그(worklog 의 kind:"turn" + 수동 decision/blocker)를 읽어 앵커 히스토리 다이제스트로 정리한다 — 마지막 digest 이후 항목만 증분 요약해 kind:"digest" 엔트리로 남기고, 각 앵커는 원본 엔트리 id 로 드릴다운 가능하게 한다. 배치 크기에 따라 저렴한 실행 주체(예: Haiku/Sonnet)를 고른다.
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
6. **한 실행은 한 프로젝트만 건드린다.** worklog 도구가 쓰는 키는 **MCP 서버 프로세스의
   `process.cwd()`** 에서 나온다(`defaultProjectKey`). rocky MCP 서버는 프로젝트마다 따로
   뜨므로, 세션이 붙은 인스턴스가 바뀌면 **한 실행 안에서도 읽는 프로젝트와 쓰는 프로젝트가
   갈릴 수 있다.** 그래서 `projectKey` 를 실행 내내 고정값으로 취급하지 말고 append 직전에
   다시 확인한다 (2026-07-29 실제 사고: A 를 읽고 B 에 써서 B 의 watermark 를 오염시켰다).

## 절차

### 1. 상태 확인 → watermark

```
worklog_status
```
- `totalEntries` 가 0 이면 "정리할 워크로그 없음" 후 종료.
- **응답의 `projectKey` 를 적어둔다** — 4단계에서 이 값과 대조한다 (원칙 6).
- 마지막 watermark: `worklog_read { kind: "digest", limit: 1 }` → 있으면 그 `timestamp`, 없으면 첫 실행.

### 2. 새 워크로그 수집 (증분)

- watermark 이후만: `worklog_read { since: <watermark>, limit: 500 }`. 없으면 전체를 최근부터.
- `$ARGUMENTS` 힌트가 있으면 `worklog_search` 로 보강.
- `kind:"digest"` 항목은 제외. 새 항목 수 `n` 을 센다. `n == 0` → no-op 종료 (watermark 안 남김).

### 3. 적응적 등급으로 서브에이전트 dispatch

- 다이제스트 요약은 저비용 작업이다 — 배치 크기로 **등급**만 고른다.
  `worklog.digestThreshold`(기본 40) 기준: `n <= 40` → 더 저렴한 쪽(예: **Haiku**),
  `n > 40` → 더 큰 컨텍스트/품질(예: **Sonnet**). 특정 모델 강제가 아니라 그 급의
  저비용 실행 주체면 된다 — 세션 환경이 다른 저비용 백엔드(예: 로컬 모델 위임)를
  제공하면 그쪽을 써도 좋다.
- `Task` 로 서브에이전트를 띄운다 (등급을 위 규칙대로). 수집 항목(각 `id`/`timestamp`/`kind`/
  `content`)을 넘기고 아래 **앵커 다이제스트**를 만들게 한다:
  - raw 나열 금지 — 의미 있는 순간(결정/전환/blocker/사용자 답변)만.
  - 각 앵커 끝에 원본 `id:<id> (<ts>)`.
  - 포맷:
    ```markdown
    ## digest — <n> entries, <first ts> … <last ts>
    - <결정/전환 요약> → id:<entry-id> (<ts>)
    - <blocker 해결> → id:<entry-id> (<ts>)
    ```

### 4. 프로젝트 확인 → 다이제스트 append (watermark 겸용)

**append 하기 직전에 `worklog_status` 를 한 번 더 부른다.**

```
worklog_status        # → projectKey 가 1단계와 같은가?
```
- **다르면 append 하지 말고 중단**한다. 읽은 프로젝트와 쓸 프로젝트가 갈렸다는 뜻이라, 그대로
  쓰면 엉뚱한 워크로그에 남의 히스토리를 박고 그쪽 watermark 까지 오염시킨다(원칙 6). 두 키를
  모두 밝혀 사용자에게 보고하고 판단을 받는다.
- 같으면 append 한다:

```
worklog_append { kind: "digest", content: "<앵커 다이제스트>", tags: ["digest"] }
```
- 이 엔트리 timestamp 가 다음 `/rocky:recall` 의 `since` 기준점.

### 5. 마무리

- 만든 다이제스트(앵커 목록)를 한국어로 보고 — 드릴다운 id 포함. 장문 금지.

## 예외 처리

- `totalEntries == 0` 또는 새 항목 0 → no-op 종료 (watermark 안 남김).
- 서브에이전트 실패 → 다이제스트 append 하지 말고 실패만 알린다 (watermark 오염 방지).
- **4단계의 `projectKey` 가 1단계와 불일치** → append 하지 말고 중단. 두 키를 밝혀 보고한다.
  이미 오염이 일어난 뒤라면 워크로그는 불변이므로 지우지 말고, ① 오배송 항목의 id 를 짚는
  `kind:"note"` 정정을 남기고 ② 올바른 범위로 다시 만든 digest 를 append 해 watermark 를
  덮는다 — 그 프로젝트의 digest 가 오염된 것 하나뿐이면 증분이 아니라 **전체 범위**로 다시
  만들어야 그 이전 항목이 영구히 건너뛰어지지 않는다.
