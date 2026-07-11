# @minjun0219/agent-toolkit-opencode

agent-toolkit 의 opencode plugin. opencode 의 plugin 키에 등록하면 **7 개 `openapi_*` 도구** 를 노출 — OpenAPI / Swagger spec 캐시 우선 fetch, endpoint 점수화 검색, tag list. 같은 7-tool surface 를 [Claude Code plugin](../agent-toolkit-claude-code) / [standalone `openapi-mcp` CLI](../openapi-mcp) 도 노출한다.

> v0.3 부터 toolkit 은 OpenAPI 도메인만 다룬다. 이전 journal / mysql / notion / spec-pact / pr-watch 도메인은 [`archive/pre-openapi-only-slim`](https://github.com/minjun0219/agent-toolkit/tree/archive/pre-openapi-only-slim) 브랜치에 박제되어 있고, 활용 패턴이 잡히면 ROADMAP 의 phase 단위로 재추가된다.

설치 절차는 [`INSTALL.md`](./INSTALL.md), 도구 카탈로그는 루트 [`FEATURES.md`](../../FEATURES.md), 모노레포 구조는 루트 [`AGENTS.md`](../../AGENTS.md).
