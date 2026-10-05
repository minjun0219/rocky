---
name: token-usage
description: Use when choosing or second-guessing the Claude model and effort for a piece of work, or when the user asks how many tokens a session or task used ("모델 뭐 쓸까", "effort 낮춰도 돼?", "지난 턴에 오푸스가 적절했어?", "토큰 얼마나 썼어"). Covers rocky's transcript-based token index — the token_summary / token_current_session MCP tools and the `rocky tokens` CLI — how to read its rule-based recommendation and evidence, and the cost lesson that cache reads, not output, dominate long sessions, so switching to a lighter model belongs at a task boundary or in a subagent, not mid-session.
---

# 토큰 사용 — 모델·effort 고르기

rocky 데몬이 Claude Code 세션 트랜스크립트를 1분마다 색인해 모델·effort·토큰·도구 호출을 쌓는다. 훅은 없다 — 쓰면 그냥 남는다.

## 보는 법

| 무엇 | 도구 |
| --- | --- |
| 기간 합계(모델 × effort 기본, 세션·브랜치로도) | MCP `token_summary { days, groupBy }` · `rocky tokens [--since 7d] [--by model,effort\|session\|branch]` |
| 지금 세션의 턴별 기록 + 추천 | MCP `token_current_session { cwd }`(또는 `sessionId`) · `rocky tokens here` |

- **턴**은 사람이 쓴 프롬프트 하나부터 다음 프롬프트 전까지다. 백그라운드 알림·데몬이 깨운 메시지는 턴이 아니다.
- 서브에이전트 토큰은 합계엔 들어가고 턴 수·추천에선 빠진다.
- 색인은 1분 늦다 — 방금 끝난 턴이 아직 없을 수 있다.

## 추천 읽기

`recommendation.suggestions` 가 비어 있으면 지금 설정이 맞아 보인다는 뜻이다. 규칙은 셋이다(최근 15턴, 5턴 미만이면 판단하지 않음).

- `lower-effort` — 턴 평균 출력이 짧은데 effort 가 xhigh/max → medium 고려.
- `switch-to-sonnet` — Opus 인데 도구 호출이 0 이고 출력이 짧다 → Sonnet medium 고려.
- `held` — 판단을 보류한 이유(턴 부족, effort 를 올린 뒤 길어짐 = 이미 조정함).

추천은 근거 수치(`evidence`: 턴 수·평균 출력·도구 호출·모델·effort)와 같이 사용자에게 전한다. 모델·effort 를 바꾸는 건 사용자다.

## 판단할 때 알아 둘 것

- **긴 세션의 비용은 출력보다 캐시 읽기**다 — 요청마다 대화 전체를 다시 읽는다(실측: 한 세션에서 출력 16만 토큰, 캐시 읽기 4,470만).
- **같은 세션에서 모델을 바꾸면** 프롬프트 캐시가 모델별이라 그 맥락 전체를 새 모델 캐시에 다시 쓴다 — 오히려 손해일 수 있다.
- 그래서 가벼운 모델로 넘어가는 자리는 **작업 경계**다: 기계적인 후속(리뷰 반영·머지 뒤 정리)은 새 세션을 Sonnet 으로 열거나,
  가벼운 모델을 지정한 서브에이전트(`rocky:quick-fix` Sonnet, `rocky:merge-cleanup` Haiku)에 맡긴다.
- 설계·여러 파일에 걸친 판단·디버깅은 무거운 모델이 제값을 하는 자리다.
