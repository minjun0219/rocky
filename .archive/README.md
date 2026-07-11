# `.archive/`

이 저장소에서 걷어낸 코드를 **in-tree 로 박제**해 두는 곳. 빌드 / lint / typecheck / test 게이트에서 제외된다 (`tsconfig.json` 의 `include` 는 `src/**` 만, `biome.json` 은 `!.archive`, `bun test` 는 `./src` 만).

여기 있는 코드는 **더 이상 빌드 / 실행되지 않는다** — 참조용 스냅샷이다. 임포트 경로(`@minjun0219/openapi-core` 등)나 의존성은 당시 monorepo 기준이라 지금은 resolve 되지 않는다.

## 내용

- [`agent-toolkit-opencode/`](./agent-toolkit-opencode) — 구 opencode plugin (`@minjun0219/agent-toolkit-opencode`). 단일 Claude Code 플러그인으로 정리하며 제거됨. opencode host 를 다시 지원할 때 여기서 포팅한다.

> v0.2 까지의 journal / mysql / notion / spec-pact / pr-watch 도메인은 이 디렉터리가 아니라 별도 브랜치 [`archive/pre-openapi-only-slim`](https://github.com/minjun0219/agent-toolkit/tree/archive/pre-openapi-only-slim) 에 있다.
