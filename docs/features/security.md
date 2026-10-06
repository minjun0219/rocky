# 보안 경계 — 로컬 요청·cross-site·tailscale

> rocky 를 **고치는** 에이전트용 개발 문서. 근거는 [`docs/daemon.md`](../daemon.md).

## 규칙

- **로컬 요청 전용 동작.** 이슈 생성, 세션 띄우기, 검증 다시 돌리기, 보드의 `path` / `repo` / `reviewFix` / `prAuthors` 변경, 보드 수집함 설정
  (`/api/inbox/adapters`·`sources` 쓰기 — 값이 실행 인자가 된다), PR 구독 쓰기, 전달 조회·"보내지 않기", 세션에 메시지 보내기(`POST /api/sessions/message`)·세션 멈추기(`POST /api/sessions/stop`)는 `is_local_request` 가
  필요하다: 루프백 peer **이고** 프록시 헤더(`x-forwarded-*`, `forwarded`, `tailscale-user-*`, `cf-*`)가 없어야 한다. peer 주소가
  없으면 거부(fail-closed). *EN: Anything that writes to GitHub, spawns processes or steers sessions is local-only: loopback peer
  and no proxy headers; fail closed.*
- **예외 — Access 원격 제어**(`rocky.json` 의 `access` 블록, `remoteControl: true`): 원격 제어 탭의 라우트(`/api/rc/*` 중
  rc 서버 띄우기·재시작, 핸드오프·대상 밖 서버 닫기, 야간 실행 — **Antigravity 켜기·끄기, 야간 리허설, 최근 활동(`activity=1`)은
  아니다** — 앞의 것은 이 기계의 원격 접속 데몬을 바꾸고, 뒤의 둘은 부를 때마다 프로세스를 띄우는 읽기라 요청 범위 밖)는
  `local` 이거나 Access 로 들어온 허용 이메일이면 받는다(`remote_control`). 판정은 셋 다 맞아야 한다: peer 가 루프백(cloudflared),
  `Cf-Access-Jwt-Assertion` 이 팀 공개키로 RS256 서명 검증, `iss`·`aud`·`exp`(±60초)·`nbf`·허용 이메일 일치
  (`rocky_core::access::verify_token`). 헤더 이름만으로는 믿지 않는다 — tailscale serve·내부망으로 온 사람도 같은 헤더를 써 넣을 수
  있다. 공개키는 1시간 캐시, 모르는 `kid` 면 다시 받되 1분에 한 번까지(`rockyd::access::AccessGate`). 블록은 사용자 설정에서만
  읽고 `team`·`aud`·`emails` 중 하나라도 비면 꺼진다(fail-closed). 검증은 `/api/rc/*` 의 변경과 `/api/health`(`rcControlAllowed`)에서만
  돌고, 같은 이유의 실패는 한 번만 로그에 남긴다. **변경은 같은 출처일 때만**(`is_same_origin_request` — `Sec-Fetch-Site: same-origin`,
  없으면 `Origin` = `Host`): Access 는 쿠키로 인증되어 같은 등록 도메인의 다른 서브도메인 페이지(`same-site`)가 시킨 POST 도
  토큰을 달고 온다 — cross-site 가드만으로는 못 막는다.
  새 라우트를 이 등급에 넣는 것은 범위 결정이다 — 오너가 정한 것만. *EN: Access-verified owners may use the remote-control
  tab only; verify the JWT in the daemon, never trust header names.*
- **cross-site 변경은 라우팅 전에 끊는다**(`is_cross_site_request`): `Sec-Fetch-Site: cross-site`(없으면 `Origin` 으로 판단)인 변경
  메서드는 REST 와 `/mcp` 에서 403. 헤더가 둘 다 없으면 비브라우저 클라이언트로 보고 통과시킨다. **읽기는 막지 않는다.** 웹소켓
  핸드셰이크(`GET /api/ws`)도 같은 가드를 탄다. *EN: Block cross-site mutations before routing (`Sec-Fetch-Site` first); never
  block reads.*
- **tailscale serve 자동 확보는 남의 노출을 빼앗지 않는다.** `decide_serve_action`: `claim`(빈 자리), `keep`(내 것),
  `yield`(살아 있는 다른 rocky 데몬), `reclaim`(죽은 포트). 수동 `rocky tailscale on` 은 가드하지 않는다.
- **Access 이메일은 화면 힌트다.** `access_user_email` 이 읽는 `cf-access-authenticated-user-email` 은 웹이 로그아웃 링크를
  그릴지만 정한다(`/api/health` 의 `accessUser`). 헤더는 위조로 "있게" 만들 수 있으므로 권한 판정에 쓰지 않는다 — 권한은 위의
  검증된 JWT 로만.
- 새로 만드는 쓰기·실행 라우트는 위 둘 중 어디에 드는지 먼저 정한다 — 프로세스를 띄우거나 실행 인자를 받는 것은 로컬 전용.

## 코드

| 무엇 | 어디 |
| --- | --- |
| 판정(`is_local_request`·`is_cross_site_request`) | `crates/rocky-core/src/local_request.rs` |
| Access 토큰 검증(순수) · 설정 | `crates/rocky-core/src/access.rs`, `config.rs`(`load_access_block`) + `rocky.schema.json` |
| 공개키 캐시 · 라우트 판정(`access_remote_control`) | `crates/rockyd/src/access.rs`, `crates/rockyd/src/server.rs` |
| 적용 지점(라우팅 전 가드, 라우트별 로컬 검사) | `crates/rockyd/src/server.rs`, 웹소켓 `crates/rockyd/src/ws.rs` |
| tailscale serve | `crates/rockyd/src/tailscale.rs`, 수동 명령 `crates/rocky-cli/src/system.rs` |

테스트: `crates/rocky-core/tests/it/local_request_test.rs`, `access_test.rs`(테스트 전용 키 `fixtures/access-test-rsa.pk8`),
`crates/rockyd/tests/it/server_access_test.rs`, `crates/rockyd/tests/it/tailscale_test.rs`, 라우트별 로컬 검사는
`crates/rockyd/tests/it/server_*_test.rs`.
