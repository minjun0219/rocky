---
name: pull-request
description: Use when a session opens, watches, or reacts to its own GitHub pull requests through rocky — subscribing a PR so the daemon reports on it ("PR 감시해줘", "이 PR 구독해"), or receiving a rocky inbox/hook message about a PR (머지 후보 · 충돌 · CI 실패 · 리뷰가 붙음 · 머지됨). Explains what each message means and what to do next, that only the subscribing session is notified, that the daemon only reads GitHub, and the hard rules — never merge, never resolve threads or comment on your own, never run your own polling loop.
---

# rocky PR 알림 — 구독하고, 오는 메시지에 대응한다

rocky 데몬이 **이 세션이 구독한 PR** 을 몇 분마다 보고, 사람이 움직일 일이 생기면 이 세션에 알린다. 데몬은 GitHub 을
**읽기만** 한다 — 머지·코멘트·resolve 는 하지 않는다. 그 셋은 이 세션도 하지 않는다(사용자 몫).

## 구독

- PR 을 만들었으면 구독한다: `rocky pr subscribe <번호>`(`/rocky:review-request` 는 알아서 한다). 구독하지 않은 PR 은 데몬이
  보지 않는다 — 알림도 보드의 PR 상태도 없다.
- 조건으로 묶어 구독: `rocky pr subscribe --filter "repo:owner/name author:@me"` — 걸리는 열린 PR 이 이 세션 구독으로 들어온다.
- 확인 `rocky pr subscriptions`, 해지 `rocky pr unsubscribe <번호>`. 머지·닫힘에서 저절로 풀린다.
- 다른 세션이 맡던 PR 을 넘겨받으려면 여기서 다시 구독한다 — 알림은 구독한 세션에만 간다.

## 오는 메시지와 할 일

메시지는 받은편지함("rocky: owner/name #N …")이나 다음 턴의 훅 주입("# rocky: PR 상태 변화")으로 온다. **이 세션이 그 PR 을
만든 곳이 아니면 사용자에게 알리기만 한다.**

| 메시지 | 뜻 | 할 일 |
| --- | --- | --- |
| 머지 후보(ready) | CI 녹색·처리 안 된 스레드 없음·충돌 없음 — **기계 판정** | 바로 알리지 않는다. `/rocky:review-fix` 8단계대로 판단(요청된 리뷰어 응답, 봇 리뷰가 필수인 레포면 현재 커밋의 봇 판정, 방금 푸시, 작업 중 표시)한 뒤 걸리는 게 없을 때만 사용자에게 한 줄 |
| 충돌(conflict) | base 와 충돌 | 내 PR 이면 base 를 합쳐 풀고 게이트 → 푸시(이미 올라간 브랜치는 리베이스·강제 푸시하지 않는다) |
| 리뷰가 붙음 | 보드의 리뷰 반영(`reviewFix`)이 켜진 레포만 | `/rocky:review-fix` 한 번(명백한 오류만 고치고 🚀/👀 리액션, 결정 필요한 건 채팅으로 묻는다) |
| CI 실패 | 이 커밋에서 CI 가 실패로 바뀜 | 로그로 원인을 가른다 — 테스트 전에 죽었으면 실패 잡만 한 번 재실행, 코드가 실패면 고쳐 푸시(`/rocky:review-fix` 12단계). "flake" 는 원인이 아니다 |
| 머지됨 | 사용자가 머지했다 | 다시 알리지 않는다. 정리 한 번 — 최신 main, 끝난 브랜치(`-d`), 릴리스 PR·스택 다음 PR·머지 뒤 리뷰 확인(11단계, 긴 세션이면 `rocky:merge-cleanup` 에 맡긴다) |

다른 일을 하는 중이거나 대화가 길어졌으면 충돌·명백한 리뷰 수정·CI 는 `rocky:quick-fix` 에 맡긴다(13단계).

## PR ↔ 할 일

보드 할 일에서 시작한 PR 이면 그 할 일의 `links` 에 PR 주소를 더한다(있던 링크를 지우지 않게 통째로 다시 쓴다). 머지되면
데몬이 그 할 일을 완료하고 댓글을 남긴다 — 세션이 `done` 하지 않는다. 링크한 PR 이 여럿이면 감시 중인 마지막 PR 이 머지될 때 완료.

## 하지 않는 것

- 머지(`gh pr merge`), 리뷰 스레드 resolve, 코멘트 — 사용자가 채팅으로 시킬 때만.
- PR 상태를 보려고 `gh pr checks --watch`·Monitor 루프를 따로 돌리지 않는다 — 데몬이 본다. 예외: 봇 리뷰가 필수인 레포에서 첫 봇
  판정까지 한 번 기다리는 것(`/rocky:review-fix` 9단계).
- 강제 푸시, `--no-verify`.

## 안 올 때

`curl -sf http://127.0.0.1:8636/api/health | jq .prWatch` — `available: false` 면 `reason`(대개 `gh` 인증)을 사용자에게 알린다.
`GET /api/deliveries`(로컬)로 이 세션에 무엇이 갔고 못 갔는지(`reason`) 본다.
