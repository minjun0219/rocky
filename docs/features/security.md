# 보안 경계 — 로컬 요청·cross-site·tailscale

> rocky 를 **고치는** 에이전트용 개발 문서. 근거는 [`docs/daemon.md`](../daemon.md).

## 규칙

- **로컬 요청 전용 동작.** 이슈 생성, 세션 띄우기, 검증 다시 돌리기, 보드의 `path` / `repo` / `reviewFix` / `prAuthors` 변경, 보드 수집함 설정
  (`/api/inbox/adapters`·`sources` 쓰기 — 값이 실행 인자가 된다), PR 구독 쓰기, 전달 조회·"보내지 않기" 는 `is_local_request` 가
  필요하다: 루프백 peer **이고** 프록시 헤더(`x-forwarded-*`, `forwarded`, `tailscale-user-*`, `cf-*`)가 없어야 한다. peer 주소가
  없으면 거부(fail-closed). *EN: Anything that writes to GitHub, spawns processes or steers sessions is local-only: loopback peer
  and no proxy headers; fail closed.*
- **cross-site 변경은 라우팅 전에 끊는다**(`is_cross_site_request`): `Sec-Fetch-Site: cross-site`(없으면 `Origin` 으로 판단)인 변경
  메서드는 REST 와 `/mcp` 에서 403. 헤더가 둘 다 없으면 비브라우저 클라이언트로 보고 통과시킨다. **읽기는 막지 않는다.** 웹소켓
  핸드셰이크(`GET /api/ws`)도 같은 가드를 탄다. *EN: Block cross-site mutations before routing (`Sec-Fetch-Site` first); never
  block reads.*
- **tailscale serve 자동 확보는 남의 노출을 빼앗지 않는다.** `decide_serve_action`: `claim`(빈 자리), `keep`(내 것),
  `yield`(살아 있는 다른 rocky 데몬), `reclaim`(죽은 포트). 수동 `rocky tailscale on` 은 가드하지 않는다.
- **Access 이메일은 화면 힌트다.** `access_user_email` 이 읽는 `cf-access-authenticated-user-email` 은 웹이 로그아웃 링크를
  그릴지만 정한다(`/api/health` 의 `accessUser`). 헤더는 위조로 "있게" 만들 수 있으므로 권한 판정에 쓰지 않는다.
- 새로 만드는 쓰기·실행 라우트는 위 둘 중 어디에 드는지 먼저 정한다 — 프로세스를 띄우거나 실행 인자를 받는 것은 로컬 전용.

## 코드

| 무엇 | 어디 |
| --- | --- |
| 판정(`is_local_request`·`is_cross_site_request`) | `crates/rocky-core/src/local_request.rs` |
| 적용 지점(라우팅 전 가드, 라우트별 로컬 검사) | `crates/rockyd/src/server.rs`, 웹소켓 `crates/rockyd/src/ws.rs` |
| tailscale serve | `crates/rockyd/src/tailscale.rs`, 수동 명령 `crates/rocky-cli/src/system.rs` |

테스트: `crates/rocky-core/tests/it/local_request_test.rs`, `crates/rockyd/tests/it/tailscale_test.rs`, 라우트별 로컬 검사는
`crates/rockyd/tests/it/server_*_test.rs`.
