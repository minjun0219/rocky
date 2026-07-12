---
description: 저널(journal_*)에 쌓인 기록을 읽어 설정된 wiki 위치(Obsidian vault 등)로 정리(整理)한다 — 프로젝트별 하위 폴더에, ownership marker 를 붙여, 마지막 curate 이후 항목만 증분 증류한다. 개인 노트는 절대 건드리지 않고, 저널도 지우지 않는다.
argument-hint: "[정리 주제/힌트] (생략 가능)"
allowed-tools: mcp__plugin_rocky_rocky__journal_status, mcp__plugin_rocky_rocky__journal_read, mcp__plugin_rocky_rocky__journal_search, mcp__plugin_rocky_rocky__journal_append, Read, Write, Glob, Grep
---

# curate — 기록(journal) → 정리(wiki) 증류

rocky 의 journal 은 **기록(記錄)** 레이어다 — append-only 원시 이벤트 로그. 이 커맨드는
그 기록을 읽어 **정리(整理)** 레이어인 wiki (설정된 Obsidian 호환 markdown vault) 로
증류한다. `$ARGUMENTS` 는 이번 정리에서 집중할 주제 힌트(있으면).

## 원칙

1. **rocky 는 기록만, 정리는 이 커맨드가.** journal 도구는 저장·조회만 한다. 실제
   증류(어떤 페이지에 무엇을 묶을지)는 호스트 LLM 인 네가 한다.
2. **프로젝트별 하위 폴더에만 쓴다.** `journal_status` 가 주는 `wikiDir` + `projectKey`
   로 **정리 루트 = `<wikiDir>/<projectKey>/`** 를 정하고, 그 안에서만 파일을 만든다.
   이 폴더 밖(다른 프로젝트 폴더 / vault 의 개인 노트 영역)으로는 **절대** 쓰지 않는다.
   `wikiDir` 미설정이면 정리하지 않고 멈춘다.
3. **Ownership marker — 개인 노트 안전장치.** curate 가 만든 페이지는 전부 frontmatter 에
   `source: rocky-curate` 를 붙인다. 병합/덮어쓰기는 **이 마커가 있는 파일에만** 한다.
   마커가 없는(= 사람이 쓴) 동명 파일은 **절대 덮어쓰지 않고** 다른 이름으로 쓰고 경고한다.
   → vault 를 개인 노트와 공유해도 안전하다.
4. **증분.** 마지막 `curate` watermark 이후 항목만 처리한다 — 매번 전체를 다시 쓰지 않는다.
5. **저널은 불변.** journal 파일을 지우거나 편집하지 않는다 (append-only). 정리가 끝나면
   `journal_append` 로 watermark 한 줄만 남긴다.
6. **네이티브 메모리와 별개.** 이 wiki 는 프로젝트 세컨드브레인이다. Claude Code 의
   글로벌 메모리(`~/.claude/.../memory/`)를 건드리지 않는다.

## 절차

### 1. 상태 확인 → 정리 루트 결정

```
journal_status
```

- `wikiDir` 가 없으면(정리 대상 미설정): `rocky.json` 의 `journal.wikiDir` 또는 env
  `ROCKY_JOURNAL_WIKI_DIR` 로 Obsidian vault 하위 경로를 지정하라고 안내하고 **멈춘다.**
- **정리 루트 = `<wikiDir>/<projectKey>/`.** (`projectKey` 도 status 가 준다.) 이 폴더가
  없으면 생성 여부를 확인하고 승인 시에만 만든다. 이후 모든 쓰기는 이 폴더 하위에만 한다.
- `lastCurateAt`(마지막 watermark)을 기억한다.
- `totalEntries` 가 0 이면 "정리할 기록 없음" 후 종료.

### 2. 새 기록 수집 (증분)

- `lastCurateAt` 이 있으면 그 이후만:

  ```
  journal_read { since: <lastCurateAt>, limit: 200 }
  ```

  없으면(첫 정리) 전체를 최근부터 넉넉한 limit 으로 읽는다.
- `$ARGUMENTS` 힌트가 있으면 `journal_search` 로 관련 항목을 보강한다.
- `kind:"curate"` 항목은 watermark 이므로 정리 대상에서 제외한다.
- 수집 결과가 비면 "정리할 새 기록 없음" 후 종료.

### 3. 증류 → wiki 페이지 작성 (정리 루트 안)

- 수집한 decision / blocker / answer / note 를 **주제별로 묶어** 정리 루트 안의 markdown
  페이지로 만든다. 주제 분해는 내용 기반(기능/모듈/의사결정 흐름)으로 하되, `tags` 와
  `pageId` 를 힌트로 쓴다.
- 각 페이지는 **Obsidian 친화 markdown** + **ownership marker**:

  ```markdown
  ---
  source: rocky-curate
  project: <projectKey>
  tags: [rocky, <주제-태그>]
  updated: <YYYY-MM-DD>
  ---

  # <제목>

  … 관련 주제는 [[다른-페이지-제목]] 위키링크로 연결 …
  ```

  원시 기록의 결정/근거를 사람이 읽을 서사로 재구성한다 (raw 로그 나열이 아니라 정리).
- **쓰기 전 충돌 검사 (필수):** 목표 파일 경로가 이미 있으면 먼저 `Read` 로 연다.
  - frontmatter 에 `source: rocky-curate` **가 있으면** → 기존 rocky 페이지. 새 사실만
    반영해 병합(중복 방지)한다.
  - 마커가 **없으면** → 사람이 쓴 노트일 수 있다. **덮어쓰지 말고** `-rocky` 접미사 등
    다른 이름으로 쓰고, 충돌 사실을 마지막 요약에 경고로 남긴다.
- 위키링크 `[[…]]` 는 정리 루트 안의 페이지를 가리키게 한다 (개인 노트 제목과 우연히
  겹치지 않도록 rocky 페이지 제목에 프로젝트 맥락을 담는 것을 권장).

### 4. watermark 기록

정리를 마치면 저널에 증분 기준점을 남긴다:

```
journal_append {
  kind: "curate",
  content: "<이번에 정리/갱신한 페이지 목록 + 처리한 마지막 항목 시각 요약>",
  tags: ["curate"]
}
```

- 이 항목의 timestamp 가 다음 `/rocky:curate` 의 `since` 기준점이 된다.

### 5. 마무리

- 생성/갱신한 wiki 페이지 경로(정리 루트 기준)와 한 줄 요약을 한국어로 출력한다.
- 마커 없는 파일과 충돌해 접미사로 우회한 경우가 있으면 함께 알린다.
- 장문 리포트 금지 — 무엇을 어디에 정리했는지만.

## 예외 처리

- `wikiDir` 미설정 → 정리 없이 설정 방법만 안내하고 멈춘다.
- 정리 루트(`<wikiDir>/<projectKey>/`)가 없으면 → 생성 여부 확인 후 승인 시에만 만든다.
- 마커 없는 동명 파일과 충돌 → 덮어쓰기 금지, 다른 이름 + 경고.
- 수집 항목 0 → no-op 로 종료 (watermark 도 남기지 않는다).
