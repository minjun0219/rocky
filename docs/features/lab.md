# lab — Claude Code function hooks 실험

> rocky 를 **고치는** 에이전트용 개발 문서. 켜는 법은 `README.md` "설정"의 `lab` 줄.

Claude Code 의 function hooks(셸 명령 대신 TS 모듈이 훅이 되는 early access 표면 — 플러그인 `hooks/hooks.json` 의 `modules`)로
데몬을 세션 화면에 얹어 본다. 무엇이 남을지 써 보고 정하려는 실험이라 기본은 꺼져 있다.

## 규칙

- **사용자 rocky.json 의 `lab` 블록이 있을 때만 켜진다**(`ROCKY_CONFIG` > `~/.config/rocky/rocky.json`, 프로젝트 설정은 안 본다 —
  `rc` 와 같은 기기별 스위치). 블록이 없거나 `enabled: false` 면 `session.start` 에서 아무것도 등록하지 않고 모든 훅이 그냥
  넘긴다. 칸(`toast`·`band`·`limits`)은 `false` 일 때만 끈다. **세션을 다시 열어야 적용된다** — `/reload-plugins` 는 바뀐
  모듈의 `session.start` 만 다시 돌리므로 rocky.json 만 바꿨으면 설정을 다시 읽지 않는다.
- **읽는 쪽은 TS `parseLabConfig`**(`plugin/hooks/lab/lib.ts`) 하나다. Rust `load_lab_block`(`crates/rocky-core/src/config.rs`)은
  부르는 곳 없이 스키마와 함께 모양을 고정하는 짝이다 — 규칙을 바꾸면 둘과 두 테스트(`config_test.rs` · `lib.test.ts`)를 같이.
- **데몬을 읽어 그리기만 한다.** 쓰기·턴 열기·판정 로직은 넣지 않는다 — 그건 `rocky_core` 와 데몬 몫이다(언어 경계: 이 모듈은
  웹 UI 와 같은 가장자리 클라이언트). *EN: read-only edge client of the daemon — no writes, no turns, no rules.*
- **주기 폴링을 두지 않는다.** 데몬의 REST 는 health·statusline·events·usage 말고 전부 사용 로그에 남는다 — 세션마다 5초
  폴링이면 하루 수만 줄이 쌓인다. band 는 `session.start`·**메인 대화의** `turn.complete`(서브에이전트 턴은 거른다)·rocky
  메시지가 왔을 때만 `GET /api/summary?cached=true`·`GET /api/deliveries` 를 읽고, 읽는 중에 또 요청이 오면 끝난 뒤 한 번
  더 돈다. `cached` 는 `true` 여야 한다(데몬의 플래그는 `true` 만 받는다 — `1` 이면 수집함 어댑터를 기다린다).
- **lab 의 요청은 라우트가 아니라 `hook lab` 으로 센다.** 요청에 `x-rocky-client: claude-code-lab`(`usage::LAB_CLIENT`)을
  붙이고, 데몬은 그 요청을 `rest_surface` 로 Hook 표면 `hook lab` 하나에 적는다 — 턴마다 읽는 요청이 `GET /api/deliveries`
  같은 라우트를 쓰이는 것처럼 부풀리지 않고, lab 이 쓰이는지는 `rocky usage` 에 남는다(`KNOWN_SURFACES`).
- **toast 는 `session.receive` 로**: 데몬이 받은편지함 소켓에 쓴 메시지(`rocky: …` — `rocky_core::peer_inbox`, `# rocky: …` —
  `handoff::build_handoff_poke`)의 첫 줄을 화면에도 띄운다. 본문은 그대로 넘기고(`next(e)` 먼저), 아래 체인이 가져간
  (`consumed`) 배달은 받은 것으로 치지 않는다. 서브에이전트 몫(`agentId`)은 건드리지 않는다. `toast: false` 여도 band 의
  마지막 메시지 줄은 갱신한다. 머리는 `peer_inbox_test.rs::daemon_messages_open_with_the_head_lab_reads` 가 고정한다 — 바꾸면
  `rockyToast` 도.
- **턴을 열지 않는다.** 쉬는 세션을 깨우는 것은 받은편지함 소켓이 이미 한다 — `$.prompt.submit` 으로 한 번 더 열면 같은
  알림에 턴이 둘 생긴다.
- **`$` 를 다른 함수에 넘길 때 그 함수는 파일 최상위에 선언돼 있어야 한다.** 엔진이 소스를 읽어 모듈이 무엇을 부르는지
  정한다 — `register` 안에 선언한 헬퍼에 넘기면 로드가 거절된다(`claude plugin validate` 가 잡는다). 훅 안의 화살표 함수에서
  최상위 함수를 부르는 것(`() => void refresh($, lab)`)은 괜찮다.

## 코드

| 무엇 | 어디 |
| --- | --- |
| 배선(훅·band 그리기·`/rocky-lab` 진단) | `plugin/hooks/lab/register.tsx` |
| 순수 판정(설정·toast 줄·한도 줄·요약 칸) | `plugin/hooks/lab/lib.ts` |
| lab 요청을 세는 자리 | `rocky_core::usage::rest_surface`, `rockyd::server` 의 사용 기록 |
| band 상태 계약(`$.state` — `rocky.labBand`) | `plugin/types/index.d.ts`(plugin.json `types`) |
| 모듈 등록 | `plugin/hooks/hooks.json` 의 `modules` |
| 설정 블록 | `crates/rocky-core/src/config.rs`(`LabConfig`·`load_lab_block`), `rocky.schema.json` 의 `lab` |

## 검증 — CI 밖

CI 에는 claude CLI 가 없어서 이 모듈의 검증은 손으로 돈다.

- 순수 판정은 `plugin/hooks/lab/lib.test.ts`(bun) — `bun run test` 가 돌아 **CI 에 든다**. 루트 `tsconfig.json` 도 `lib.ts`·
  `lib.test.ts` 를 본다.
- `bun run test:lab` — `claude plugin validate` + `claude plugin test`(엔진 테스트 키트, `register.test.tsx`). 엔진 테스트는
  **`*.test.tsx`** 로 둔다: `claude plugin test` 는 폴더의 `*.test.ts`·`*.test.tsx` 를 전부 집어 bun 테스트까지 돌리므로(실패한다)
  스크립트가 lab 파일만 임시 폴더에 옮기며 `*.test.ts` 를 빼고, `bun run test` 의 `test:unit` 은 `*.test.tsx` 를 거른다.
- 타입: `claude --plugin-dir plugin` 으로 한 번 띄우면 엔진이 `plugin/.claude-plugin/types/` 에 그 빌드의 선언을 깐다(자체
  `.gitignore`). 그 뒤 `bunx tsc -p plugin` — `plugin/tsconfig.json` 이 그 폴더를 extends 하고 bun 테스트(`*.test.ts`)는 뺀다.
  루트 `bun run typecheck` 는 `lib.ts` 만 보고 엔진을 쓰는 파일은 보지 않는다(`claude-code` 모듈이 없다).
- `rocky.json` 규칙: `crates/rocky-core/tests/it/config_test.rs::lab_block_turns_on_only_when_present`.

## 함정

- **early access** — 이벤트·`$`·엘리먼트 모양이 릴리스마다 바뀔 수 있다. Claude Code 를 올린 뒤 `bun run test:lab` 을 먼저 돈다.
- 설치된 플러그인의 function hooks 는 엔진이 막을 수 있다 — 공식 플러그인 문서는 2.1.280 에서 `CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1`
  을 요구했고, 2.1.289 바이너리는 서버 플래그로 막는 것으로 보인다(확인 안 함). 막혀도 `modules` 항목만 빠지고 command 훅은
  그대로인 구조로 보인다. `--plugin-dir` 로는 2.1.289 에서 변수 없이 로드됐고, 마켓 설치 경로에서는 재 보지 않았다.
- **플러그인 하나에 모듈 하나** — 엔진이 `modules` 의 둘째 항목을 거절한다. rocky 의 자리는 lab 이 쓰고 있으니, 다른 function
  hooks 기능은 이 모듈에 더한다.
- env `CLAUDE_CODE_SESSION_ID` 는 부모 세션에서 물려받았거나 `/clear`·resume 뒤면 엔진의 세션 id 와 다르다. 이 모듈은
  `$.session.id()` 만 쓴다(`/rocky-lab` 이 둘을 나란히 보인다).
