---
description: /rocky:opencode 가 백그라운드로 띄운 opencode 위임 잡을 조회·회수·취소한다. status(진행 중/최근) / result(최종 출력) / cancel(프로세스 그룹 중단). 잡을 새로 만들지는 않는다.
argument-hint: "[status|result|cancel] [job-id]"
allowed-tools: Bash(bun:*)
---

# opencode-jobs — 위임 잡 수명주기

`/rocky:opencode --background` 로 띄운 잡을 다룬다. `$ARGUMENTS` 의 첫 토큰이 하위 동작,
두 번째가 잡 id(생략 가능, prefix 만 줘도 된다). 출력은 **한국어**.

동작이 없으면 `status` 로 본다.

## 실행

```bash
bun run "${CLAUDE_PLUGIN_ROOT}/src/opencode-companion.ts" <status|result|cancel> [job-id]
```

- `status [id]` — 진행 중 잡(로그 최근 3줄 포함) + 최근 종료 잡.
- `result [id]` — **종료된** 잡의 최종 출력. 아직 진행 중이면 결과 대신 그 사실을 알린다.
- `cancel [id]` — 진행 중 잡의 워커 그룹과 opencode 그룹을 모두 SIGTERM 으로 끊고 `cancelled` 로 기록.

## 규칙

1. **출력을 그대로 제시한다.** 요약하거나 재구성하지 않는다 — 위임 결과를 내가 고쳐 쓰면
   사용자가 opencode 가 실제로 뭘 했는지 알 수 없게 된다. 필요하면 출력 **뒤에** 한 줄 논평만 붙인다.
2. **잡 id 는 prefix 로 충분하다.** 여러 잡에 걸리면 companion 이 모호하다고 에러를 낸다 —
   그때는 후보를 그대로 보여주고 사용자에게 고르게 한다. 임의로 하나를 고르지 않는다.
3. **세션 격리.** 기본적으로 이 Claude 세션이 만든 잡만 보인다. 다른 세션 것까지 보려면
   `--all` 을 붙인다(단, 취소는 남의 세션 잡에 대해 함부로 하지 않는다).
4. **`result` 는 병합이 아니다.** 잡이 끝났다고 worktree 변경이 병합되는 게 아니다 —
   병합 판정은 `/rocky:opencode` 의 감시 절차(게이트/표면/스코프)를 거쳐야 한다.
   `result` 로 성공을 확인했으면 그 다음은 `/rocky:opencode` 의 3~4단계를 이어서 수행한다.

## 예외 처리

- 잡이 하나도 없음 → 그 사실만 알리고 끝낸다.
- `result` 인데 아직 진행 중 → 경과 시간과 phase 를 전하고, 나중에 다시 보라고 안내한다.
- `cancel` 인데 이미 종료됨 → 이미 끝났다고 알리고 아무것도 하지 않는다.
