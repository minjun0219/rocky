---
"@minjun0219/rocky": minor
---

`openapi_*` 7종 · `seo_validate` · `notion_*` 4종과 단독 CLI `openapi-mcp` 를 제거한다.
39개 레포 5,216 턴의 워크로그를 세어 보니 호출이 0회였다 (같은 기간 `worklog_read` 는 78회).
4,145 LOC 와 런타임 의존 6개(`@apidevtools/swagger-parser` · `swagger2openapi` · `js-yaml` ·
`openapi-types` · `pino` · `ogpeek`)가 같이 빠지고, MCP 표면은 `worklog_*` 4개만 남는다.
`rocky.json` 의 `openapi` / `seo` 블록은 이제 알 수 없는 키로 거부되니 옛 설정 파일에
남아 있으면 지운다. 전부 git 히스토리에서 꺼낼 수 있다.
