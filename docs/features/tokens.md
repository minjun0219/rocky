# 토큰 색인 — 모델·effort·토큰과 추천

> rocky 를 **고치는** 에이전트용 개발 문서. **쓰는** 법(모델 고르기)은 README "토큰 사용"과 플러그인 스킬 쪽.

## 규칙

- **훅을 걸지 않는다.** 모델·effort·토큰은 훅 입력에 없고(토큰은 아예 없고 모델은 `SessionStart` 에만) 트랜스크립트 줄마다 있다.
  [로그 색인](log-index.md) 스레드가 Claude Code 트랜스크립트(`~/.claude/projects/**/*.jsonl`, `tokens.dir`·`CLAUDE_CONFIG_DIR`)를
  `cc_*` 표로 증분 색인한다. 첫 바퀴가 과거 가져오기를 겸한다.
- **같은 `message.id` 가 content 블록마다 반복된다** — 메시지 id 로 한 번만 센다. 도구 호출은 `tool_use` 블록 id 로 센다. 같은
  메시지의 뒤 줄이 다른 바퀴에 읽혀 도구 호출이 늘어도 그 세션은 바뀐 것으로 친다.
- **턴 경계는 사람이 쓴 프롬프트**다 — `isMeta`·압축 요약·하네스 주입(`<task-notification>`·`<wake>`·로컬 커맨드 출력)은 경계가
  아니다(그 응답은 앞 턴에 합친다). 열린 턴은 `cc_cursors` 에 바이트 위치와 같은 트랜잭션으로 둔다(파일이 줄면 처음부터).
- **서브에이전트**(`isSidechain`, `<session>/subagents/*.jsonl`)는 토큰 합계엔 넣고 턴 수·추천에서는 뺀다(턴당 출력은
  `mainOutputTokens / turns`).
- 조회: `GET /api/tokens/{summary,current,sessions/:id,recommendation}`(`groupBy`·`group_by` 둘 다), MCP `token_summary`·
  `token_current_session`(`cwd` 또는 `sessionId`), `rocky tokens [here]`. "지금 세션"은 정확히 그 cwd 의 세션이 먼저다.
- **추천 규칙 v1** 세 가지(`tokens.recommend` 로 조정): 짧은 턴인데 effort xhigh/max → medium, effort 를 올린 뒤 길어졌으면 억제,
  Opus 인데 도구 0 + 짧은 턴 → Sonnet medium. 최소 턴 수 아래는 판단하지 않는다.
- **SSE 는 전역 `/api/events` 와 나눈다**(`GET /api/tokens/events`, `event: tokens.recommendation`) — 그쪽 구독자는 `data:` 마다
  보드를 다시 읽는다. **낸 규칙 집합이 바뀐** 세션만 민다. 첫 바퀴는 과거 가져오기라 기준선만.

## 코드

| 무엇 | 어디 |
| --- | --- |
| 파서·스키마·조회·추천(순수) | `crates/rocky-core/src/tokens.rs` |
| 증분 색인 | `crates/rocky-core/src/logindex.rs`(`ingest_transcripts`) |
| 추천 피드 | `crates/rockyd/src/logindex.rs`(`RecommendationFeed`) |
| REST·MCP·CLI | `crates/rockyd/src/server.rs`, `crates/rockyd/src/mcp.rs`, `crates/rocky-cli/src/tokens_cmd.rs` |
| 설정 | `crates/rocky-core/src/config.rs`(`load_tokens_block`) + `rocky.schema.json` |

테스트: `crates/rocky-core/tests/it/tokens_test.rs`, `crates/rockyd/tests/it/tokens_test.rs`, `crates/rocky-cli/tests/it/tokens_cmd_test.rs`.

## 함정

- 트랜스크립트 모양은 Claude Code 내부 형식이다 — 바뀌면 파서가 조용히 덜 센다. 실데이터로 대조할 때 `message.id` 반복 줄의 usage 가
  같은지부터 본다(2026-10-05 실측 3,262건 전부 같았다).
- 비용의 대부분은 출력이 아니라 **캐시 읽기**다(긴 세션은 요청마다 맥락 전체를 다시 읽는다). 같은 세션에서 모델을 바꾸면 캐시가
  모델별이라 맥락 전체를 다시 쓴다.
